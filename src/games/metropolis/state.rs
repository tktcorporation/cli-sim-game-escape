//! City state for Idle Metropolis.
//!
//! AI-driven idle city builder.  The player only sets a strategy and buys
//! upgrades; the CPU does all placement.  At low Tiers the CPU is dumb on
//! purpose, so we need a balance simulator (see `simulator.rs`) to confirm
//! the game is still progressing.

use std::cell::Cell;
use std::collections::VecDeque;

/// **マップ全体**の幅 / 高さ (内部データの寸法)。
/// 表示は `VIEW_W × VIEW_H` の窓 (viewport) に切り取る。
/// 64×32 = 2048 セル — 32×16 (旧) の 4 倍の建設余地を持つ。
pub const GRID_W: usize = 64;
pub const GRID_H: usize = 32;

/// ビューポート (画面に同時表示するセル数)。`render` はこの寸法だけ描画し、
/// `City::cam_x` / `cam_y` で全マップを舐める。旧グリッドサイズ (32×16) と
/// 同一値にすることで、既存レイアウト幅 (`METROPOLIS_WIDE_MIN_WIDTH = 90`) を
/// 維持できる。
pub const VIEW_W: usize = 32;
pub const VIEW_H: usize = 16;

pub const TICKS_PER_SEC: u32 = 10;

/// Hard cap on parallel workers.  Doubling cost (`100 << (workers - 1)`)
/// stays comfortably within i64 well below this limit; the cap also keeps
/// gameplay bounded.
pub const MAX_WORKERS: u32 = 8;

/// What occupies a single map cell.
#[derive(Clone, Debug, PartialEq)]
pub enum Tile {
    Empty,
    /// 整地中。Wasteland / Forest の地形を Plain 化する工程。
    /// 完了すると下層の `terrain` が Plain に書き換わり、再び Empty タイルに戻る。
    /// 続けて何を建てるかは AI が次の tick で決める設計 (= 建物自由度を保つ)。
    Clearing { ticks_remaining: u32 },
    /// Construction in progress: target building, ticks remaining.
    Construction {
        target: Building,
        ticks_remaining: u32,
    },
    Built(Building),
}

/// Buildings the AI can place.
///
/// **経済チェーン (3 段)**:
///   Road (インフラ) → House (人口・需要源) → Workshop / Shop (供給源) →
///   Factory / Mall / Office (上位供給) → Park (文化触媒)
///
/// 需給システム上の役割:
///   - **Workshop / Factory**: 雇用供給。House の働き手 (人口/2) を吸収する。
///   - **Shop / Mall**: 商業供給。House の消費需要 (人口) を吸収する。
///   - **Office**: ホワイトカラー雇用供給。Highrise 化を促進する触媒。
///   - **Park**: 文化供給。直接収入はなく Highrise 化の最終条件を満たすため。
///
/// 上位建物 (Factory/Mall/Office) は基本建物よりコストが高く、供給キャパシティと
/// 収入上限が大きい。`logic::compute_supply` / `compute_demand_at` がここを参照。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Building {
    /// Connector: enables shops to be supplied.
    Road,
    /// Adds population.  Tier (Cottage/Apartment/Highrise) is a derived value
    /// computed from neighborhood + demand-supply ratio in `logic::house_tier_for`.
    House,
    /// 工房 (基礎雇用)。隣接 House (労働力) と Road 接続が必要。
    Workshop,
    /// 工場 (Workshop 上位)。雇用キャパシティが Workshop の約 3 倍。
    /// 隣接 House の Tier を 1 段下げる「煙害」デバフを持つため、純経済特化の
    /// プレイヤーが住宅地から離して配置する判断を強いる。
    Factory,
    /// 商店 (基礎商業)。Road 接続 + 距離 3 以内 House で活性。
    Shop,
    /// 大型商業 (Shop 上位)。商業キャパシティが Shop の約 3 倍で、
    /// Apartment / Highrise 住人にプレミアム需要を返す (= 高 Tier 街区で真価)。
    Mall,
    /// オフィスビル。ホワイトカラー雇用を供給し、周囲 House を Highrise 化
    /// する触媒。Park の経済版に近い役割で「成熟街区を Highrise に押し上げる」
    /// ための後半ピース。
    Office,
    /// 公園 (文化触媒)。直接収入なし。Highrise 化の最終条件である「文化需要
    /// 充足」を担う。道路接続不要。
    Park,
    /// **開拓機材** — 隣接する Rock セルの整地を可能にする特殊建物。
    /// AI (Tier 4/5) が `evaluate` で判定して自分で建てる。
    Outpost,
}

impl Building {
    /// One-time build cost in cash.
    ///
    /// バランス階段 (基礎 → 上位):
    ///   - Workshop $100 → Factory $300 (3x): 雇用キャパが 3 倍
    ///   - Shop $150 → Mall $400 (~2.7x): 商業キャパが 3 倍
    ///   - Office $250: 単独枠 (Highrise 化の触媒、Mall と Factory の中間価格)
    pub fn cost(self) -> i64 {
        match self {
            Building::Road => 10,
            Building::House => 40,
            Building::Park => 80,
            Building::Workshop => 100,
            Building::Shop => 150,
            Building::Office => 250,
            Building::Factory => 300,
            Building::Mall => 400,
            Building::Outpost => 600,
        }
    }

    /// Ticks needed to finish construction.
    ///
    /// 上位建物は Build 時間も比例して長く、$/sec の即時 ROI で見ると基礎建物が
    /// 序盤有利・上位建物が中盤以降に開く流れ。
    pub fn build_ticks(self) -> u32 {
        match self {
            Building::Road => 30,        // 3 sec
            Building::House => 100,      // 10 sec
            Building::Park => 80,        // 8 sec
            Building::Workshop => 150,   // 15 sec
            Building::Shop => 200,       // 20 sec
            Building::Office => 220,     // 22 sec — オフィスは内装に時間
            Building::Factory => 280,    // 28 sec — 重工業は搬入が長い
            Building::Mall => 320,       // 32 sec — 大型商業
            Building::Outpost => 600,    // 60 sec
        }
    }

}

/// 街の発展段階。人口で自動的に判定される (純関数)。
///
/// ティア進化はバナー表示と完成イベントログの主要な「自慢ポイント」。
/// プレイヤーが「次の段階まで pop X」を意識する見出しになる。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CityTier {
    Village,
    Town,
    City,
    Metropolis,
}

impl CityTier {
    pub fn name(self) -> &'static str {
        match self {
            CityTier::Village => "Village",
            CityTier::Town => "Town",
            CityTier::City => "City",
            CityTier::Metropolis => "Metropolis",
        }
    }
    pub fn jp(self) -> &'static str {
        match self {
            CityTier::Village => "村",
            CityTier::Town => "町",
            CityTier::City => "市",
            CityTier::Metropolis => "大都市",
        }
    }
}

/// 人口からティアを決定する純関数。
///
/// **TODO (バランス調整ポイント)**: 各閾値を確定する。これは
/// ゲーム体験を直接決める重要な数値で、シミュレーター結果から逆算する
/// と良い。30 分で T4 が pop ~600 に達するため、目安として:
///
///   - Village → Town:  ~50 pop  (序盤、最初の店舗が回り始める頃)
///   - Town → City:     ~250 pop (中盤、複数の住宅クラスター)
///   - City → Metropolis: ~600 pop (終盤、Tier 4 AI で十分到達可能)
///
/// 数字を変える時は、シミュレーターの 30min ベンチを実行して
/// 「ちょうど終盤直前で Metropolis に到達するか」を確認すること。
pub fn city_tier_for(population: u32) -> CityTier {
    if population >= 600 {
        CityTier::Metropolis
    } else if population >= 250 {
        CityTier::City
    } else if population >= 50 {
        CityTier::Town
    } else {
        CityTier::Village
    }
}

/// 次のティアまでの必要人口 (None = 既に Metropolis)。
pub fn next_tier_threshold(t: CityTier) -> Option<u32> {
    match t {
        CityTier::Village => Some(50),
        CityTier::Town => Some(250),
        CityTier::City => Some(600),
        CityTier::Metropolis => None,
    }
}

/// 右パネルのタブ。Status / Manager / Events / World が初期セット。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelTab {
    Status,
    Manager,
    Events,
    World,
}

impl PanelTab {
    pub fn label(self) -> &'static str {
        match self {
            PanelTab::Status => "状態",
            PanelTab::Manager => "操作",
            PanelTab::Events => "履歴",
            PanelTab::World => "世界",
        }
    }
}

/// Player's strategic preference.  Drives how Tier-2+ AI weights its choices;
/// Tier-1 ignores this field.
///
/// `Tech` は短期収入を犠牲にして建設速度と (将来の) 研究ポイントを稼ぐ路線。
/// `Eco` は森を残し荒地だけ整地する「環境配慮」型 — 整地メカニクスと組み合わせ。
/// `Balanced` は「中間値で意思決定が薄まる」ため削除し、各択に明確な
/// トレードオフを持たせる方針 (Plan #1)。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Strategy {
    Growth, // prefer Houses
    Income, // prefer Shops
    /// Tech: 建設速度 +20% / 収入 -20%。AI は道路を優先し展開を重視。
    Tech,
    /// Eco: 森を切らない (Forest 整地を AI が回避)。建設速度 -10% / 収入 +5%。
    /// 「ゆっくり丁寧に育てる」自然と共存する街づくり。
    Eco,
}

/// CPU intelligence tier — 将棋AI 風のレベル設計。
///
/// **強さの源泉**: 将棋エンジンと同じく **評価関数 × 探索深さ** の2軸。
/// 評価関数 (`logic::evaluate`) は Tier 3 以上で共通、Tier 差は探索深さ +
/// ノイズ量で作る (= Stockfish Skill Level / ぴよ将棋 と同じ思想)。
///
///   - Tier 1 (Random):  ランダム指し。15級相当。評価関数なし。
///   - Tier 2 (Greedy):  1手読み + 簡易評価 (駒得のみ) + 30%ノイズ。5級相当。
///   - Tier 3 (Aware):   1手読み + フル評価 + 5%ノイズ。初段相当。
///   - Tier 4 (Planner): 2手読み + フル評価。三段相当。
///   - Tier 5 (Master):  3手読み + フル評価。アマ高段相当。
///
/// 「自然な弱さ」設計: 弱い Tier は **視野を狭めることで自然に悪手が出る**。
/// 明示ブランダー (突然の大悪手) は入れない (=「バカにされた感」を避ける)。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum AiTier {
    /// 15級: 合法手から完全ランダム選択。評価関数なし。
    Random = 1,
    /// 5級: 1手読み + 簡易評価 (House 数だけ見る駒得) + 30% ノイズ。
    /// 「目先の家賃しか見えない」短視眼な弱さ。
    Greedy = 2,
    /// 初段: 1手読み + フル評価関数 (income/sec + Strategy bias) + 5% ノイズ。
    /// 「考えてるが先は読まない」レベル。
    Aware = 3,
    /// 三段: 2手読み + フル評価。「道路を引いて家を建てる」のような
    /// 1手目+2手目の連携を発見できる。
    Planner = 4,
    /// アマ高段: 3手読み + フル評価 + ワイド beam search。長期投資が見える。
    Master = 5,
}

impl AiTier {
    /// Cash price to upgrade *into* this tier.
    ///
    /// **2026-05 調整**: 旧価格は $500 / $5,000 / $50,000 と 10x ジャンプで
    /// 「$5,000 に詰まる」問題があった。段階を ~3x にすることで、
    /// 30 分プレイでも順次進化を実感できるカーブに調整。
    /// 累計 ~$50K は変えず、途中の $3K / $12K で頻繁に進化ボタンが効く。
    pub fn upgrade_cost(self) -> i64 {
        match self {
            AiTier::Random => 0, // starting tier
            AiTier::Greedy => 500,
            AiTier::Aware => 3_000,
            AiTier::Planner => 12_000,
            AiTier::Master => 35_000,
        }
    }

    pub fn next(self) -> Option<AiTier> {
        match self {
            AiTier::Random => Some(AiTier::Greedy),
            AiTier::Greedy => Some(AiTier::Aware),
            AiTier::Aware => Some(AiTier::Planner),
            AiTier::Planner => Some(AiTier::Master),
            AiTier::Master => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            AiTier::Random => "Random",
            AiTier::Greedy => "Greedy",
            AiTier::Aware => "Aware",
            AiTier::Planner => "Planner",
            AiTier::Master => "Master",
        }
    }
}

/// Whole-city snapshot.  Everything the simulator needs to step forward.
pub struct City {
    pub grid: Vec<Vec<Tile>>,
    /// Background terrain layer, generated once from the seed.
    /// 建物が建っても下に残るため、Water 隣接ボーナスなど将来の拡張に使える。
    pub terrain: super::terrain::TerrainLayer,
    /// 地形生成に使ったシード値 (UI で見せる、リセット時の再現用)。
    /// バナーに表示する予定 (CityTier 実装と一緒に使う)。
    #[allow(dead_code)]
    pub world_seed: u64,
    pub cash: i64,
    pub tick: u64,

    /// AI brain in use.
    pub ai_tier: AiTier,
    pub strategy: Strategy,

    /// 現在表示中の右パネルタブ。
    pub panel_tab: PanelTab,

    /// 直近 tick で観測したティア。次 tick で計算したティアと比較して、
    /// 進化したらフラッシュ + イベントログを発火。
    pub last_observed_tier: CityTier,
    /// ティア進化フラッシュが消える tick。`tick < value` の間バナーを光らせる。
    pub tier_flash_until: u64,

    /// 旧周期撤去のクールダウン用フィールド (現在 dead state)。値を読む
    /// production code は無いが、save schema v2 互換のため field と
    /// (de)serialize 経路は残す。次回 schema bump 時に削除候補。
    #[allow(dead_code)]
    pub last_outpost_dispatch_tick: u64,
    /// 旧周期撤去用フィールド (現在 dead state、save 互換のため残置)。
    /// 詳細は `last_outpost_dispatch_tick` の説明参照。
    #[allow(dead_code)]
    pub last_auto_demolish_tick: u64,
    /// Outpost の累計建設回数。`count_built(Outpost)` は撤去で減るので
    /// 「これまでに何基建てたか」の生涯統計はこちらに加算する。
    /// 戦略の挙動 (どのくらい拡張に投資したか) を測る統計用。永続化対象。
    pub outposts_dispatched_total: u64,

    /// Build queue: how many parallel constructions the AI can run.
    /// (Each Construction tile already counts toward this limit.)
    pub workers: u32,

    /// Deterministic PRNG state.  Seedable so simulator runs are reproducible.
    pub rng_state: u64,

    /// Rolling counters for diagnostics.
    pub buildings_started: u64,
    pub buildings_finished: u64,
    pub cash_earned_total: i64,
    pub cash_spent_total: i64,

    /// Most-recent AI activity for the on-screen "thought log".
    /// Newest first; older entries trimmed to `MAX_EVENTS`.
    pub events: Vec<String>,

    /// セルが完成フラッシュを保つ最終 tick (`tick < value` の間光る)。
    pub completion_flash_until: Vec<Vec<u64>>,
    /// アクティブ店舗が給料フラッシュを保つ最終 tick。
    pub payout_flash_until: Vec<Vec<u64>>,
    /// 直近の収入額 (status の "+$X" フラッシュ用)。
    pub last_payout_amount: i64,
    /// 直近の収入が発生した tick。
    pub last_payout_tick: u64,

    /// 各 Built タイルが完成した tick (= 築 0 の起点)。Empty / Construction の
    /// 間は 0。`logic::aging_factor` / `logic::effective_house_tier` が
    /// 「築年数」「Tier 昇格 dwell time」の判定に使う。
    ///
    /// **設計**: Tier 昇格時に上書きしない。一度建てた建物の経年は連続で進む
    /// (Highrise に育っても基盤の建物は同じ年代)、その代わり Tier ごとの
    /// `lifespan_multiplier` で「高 Tier ほど老けが遅い」を表現する。
    ///
    /// 永続化対象。旧セーブでは default 0 だが、`apply_save` 側で
    /// 「全建物を当該 tick 時点で新築扱い」にマイグレートする。
    pub built_at_tick: Vec<Vec<u64>>,

    /// ビューポートの左上セル座標 (カメラ位置)。
    /// `render` は `(cam_x..cam_x+VIEW_W, cam_y..cam_y+VIEW_H)` を描画する。
    /// プレイヤーの h/j/k/l (もしくは矢印キー) でスクロール。
    /// 永続化対象 (再ロード後も同じ視点を保つ)。
    pub cam_x: usize,
    pub cam_y: usize,

    /// 直近にプレイヤーがクリック / タップしたマップセル (絶対座標)。
    /// Status タブで「選択した施設の情報」を表示するために使う。
    /// 一時状態 (永続化しない、リロード後はリセット)。
    pub selected_cell: Option<(usize, usize)>,

    /// 右パネルタブ内コンテンツの縦スクロールオフセット (visual rows)。
    /// スマホ等の浅い縦幅で Manager 全行が入り切らない時に下まで届かせる。
    /// `&City` で渡る render 内で clamp する都合で `Cell` を採用。
    /// 一時状態 (永続化しない、タブ切替時にリセット)。
    pub panel_scroll: Cell<u16>,

    /// 直近の cash サンプル `(tick, cash)`。1 秒ごと (= 10 tick ごと) に push し、
    /// 12 サンプル (= 12 秒) 保持する。10 秒前との差分を取って「実 cash 増減レート」
    /// を算出する用途。撤去や建設のキャッシュ流出も含むため、`compute_income_per_sec`
    /// (理論値) との乖離が thrash の見える化になる。
    /// 一時状態 (永続化しない、リロード後はリセット)。
    pub cash_history: VecDeque<(u64, i64)>,

    /// `population()` の per-frame メモ化キャッシュ。Tier 連動の精密集計は
    /// O(houses + GRID²) と重いため、render が同 frame 内で複数回呼ぶケースで
    /// 再計算を避ける。`None` の時に算出 → 次回までキャッシュ。
    /// grid を変更する操作 (set_tile / 建設完成 / 撤去 / 整地完了 / save ロード)
    /// では invalidate して `None` に戻す。
    pub population_cache: Cell<Option<u32>>,

    /// `compute_edge_connected_roads` の per-frame メモ化キャッシュ。
    /// render は 60 FPS で複数の場所からこの BFS を呼ぶため、tick 境界毎に
    /// 1 回計算して frame 内で共有する。`(tick, Rc<grid>)` ペアで保持し、
    /// tick が進んだら自動で stale 判定 (= 直接 city.tick を書き換えるテスト経路でも
    /// 安全)。AI の `with_action_applied` で grid を仮想 mutate する経路は
    /// invalidate してから再計算する。
    #[allow(clippy::type_complexity)] // (tick, Rc<grid>) ペアが意味的に明確
    pub connected_cache: std::cell::RefCell<Option<(u64, std::rc::Rc<Vec<Vec<bool>>>)>>,

    /// `compute_income_per_sec` (dollars) の per-frame メモ化キャッシュ。
    /// 描画は header / status / Manager タブ等から複数回呼ぶ。`(tick, dollars)`
    /// ペアで保持。connected_cache と同じ寿命管理。
    pub income_dollars_cache: Cell<Option<(u64, i64)>>,

    /// タブ復帰時のオフライン進行ボーナス通知。`Some` の間は `render` が
    /// 中央モーダルを上書き描画し、`handle_input` が通常操作をブロックして
    /// 任意の入力で `None` に戻す。
    ///
    /// idle ゲームの「気付かないうちに増えた」を防ぐためのプレイヤー確認用
    /// 一時状態 (永続化しない)。Events ログにも同内容が残るので「閉じてしまうと
    /// 二度と確認できない」課題はそちらでカバーする。
    pub pending_offline_welcome: Option<PendingOfflineWelcome>,
}

/// オフライン進行ボーナス通知モーダルの表示データ。
///
/// `save::apply_offline_bonus` で `OfflineBonus` から構築される。`save` の型を
/// `state` に直接持たせると `state -> save` の逆向き依存になるため、表示に必要な
/// 値だけを切り出した独立 struct として持つ。
#[derive(Clone, Debug)]
pub struct PendingOfflineWelcome {
    pub elapsed_secs: u64,
    pub bonus_cash: i64,
    pub capped: bool,
}

pub const MAX_EVENTS: usize = 8;

/// 完成タイルが光り続けるtick数 (1.5秒)。
pub const COMPLETION_FLASH_TICKS: u64 = 15;

/// 店舗が給料発生時に光るtick数 (0.6秒)。
pub const PAYOUT_FLASH_TICKS: u64 = 6;

/// ティア進化時のバナー全体フラッシュ tick 数 (3 秒)。
pub const TIER_FLASH_TICKS: u64 = 30;

impl City {
    pub fn new() -> Self {
        Self::with_seed(0xC1A5_5EED)
    }

    pub fn with_seed(seed: u64) -> Self {
        let mut grid = Vec::with_capacity(GRID_H);
        let mut completion_flash_until = Vec::with_capacity(GRID_H);
        let mut payout_flash_until = Vec::with_capacity(GRID_H);
        let mut built_at_tick = Vec::with_capacity(GRID_H);
        for _ in 0..GRID_H {
            grid.push(vec![Tile::Empty; GRID_W]);
            completion_flash_until.push(vec![0u64; GRID_W]);
            payout_flash_until.push(vec![0u64; GRID_W]);
            built_at_tick.push(vec![0u64; GRID_W]);
        }
        let mut terrain = super::terrain::generate(seed);
        // **創設街路**: マップ上端 (y=0) から中央 (y=GRID_H/2) まで縦断する
        // 「幹線道路」を最初から敷いておく。Phase 2 の edge connectivity で
        // Shop/Workshop が活性化できる土地を保証し、低 Tier AI でも経済が
        // 立ち上がる (= 「街は外との物流から始まる」というシムシティ的な
        // 起点)。terrain は強制的に Plain にして、Forest/Wasteland 起因の
        // Clearing が紛れ込まないようにする。
        let cx = GRID_W / 2;
        for y in 0..=GRID_H / 2 {
            terrain[y][cx] = super::terrain::Terrain::Plain;
            grid[y][cx] = Tile::Built(Building::Road);
        }
        // カメラ初期位置: マップ中央が画面中央に来るように。
        // (GRID_W - VIEW_W) / 2 = (64-32)/2 = 16, (32-16)/2 = 8。
        let cam_x = (GRID_W.saturating_sub(VIEW_W)) / 2;
        let cam_y = (GRID_H.saturating_sub(VIEW_H)) / 2;
        Self {
            grid,
            terrain,
            world_seed: seed,
            cash: 200, // enough seed money for 5 houses or a shop
            tick: 0,
            ai_tier: AiTier::Random,
            strategy: Strategy::Growth,
            panel_tab: PanelTab::Manager,
            last_observed_tier: CityTier::Village,
            tier_flash_until: 0,
            last_outpost_dispatch_tick: 0,
            last_auto_demolish_tick: 0,
            outposts_dispatched_total: 0,
            workers: 1,
            rng_state: seed,
            buildings_started: 0,
            buildings_finished: 0,
            cash_earned_total: 0,
            cash_spent_total: 0,
            events: Vec::new(),
            completion_flash_until,
            payout_flash_until,
            last_payout_amount: 0,
            last_payout_tick: 0,
            built_at_tick,
            cam_x,
            cam_y,
            selected_cell: None,
            panel_scroll: Cell::new(0),
            cash_history: VecDeque::new(),
            population_cache: Cell::new(None),
            connected_cache: std::cell::RefCell::new(None),
            income_dollars_cache: Cell::new(None),
            pending_offline_welcome: None,
        }
    }

    /// grid 構造を変更した時に派生キャッシュを全て無効化する。
    /// `set_tile` / 建設完成 / 撤去 / 整地 / セーブロード / tick 境界 で呼ぶ。
    /// 関連: `population_cache` / `connected_cache` / `income_dollars_cache`。
    pub fn invalidate_population_cache(&self) {
        self.population_cache.set(None);
        self.connected_cache.borrow_mut().take();
        self.income_dollars_cache.set(None);
    }

    /// カメラを (dx, dy) だけ移動。`GRID_W - VIEW_W` を上限にクランプ。
    /// dx/dy は符号付きで、上下左右への移動を 1 メソッドで扱う。
    pub fn scroll_camera(&mut self, dx: i32, dy: i32) {
        let max_x = GRID_W.saturating_sub(VIEW_W);
        let max_y = GRID_H.saturating_sub(VIEW_H);
        let nx = (self.cam_x as i32 + dx).clamp(0, max_x as i32);
        let ny = (self.cam_y as i32 + dy).clamp(0, max_y as i32);
        self.cam_x = nx as usize;
        self.cam_y = ny as usize;
    }

    /// 右パネルを縦に動かす。`delta` は visual row 単位の符号付き量。
    /// 上限は render 内の `ScrollableTab` が content_h に合わせて再度
    /// クランプするため、ここでは下限 0 と u16 上限の saturate のみ保証する。
    pub fn scroll_panel(&mut self, delta: i32) {
        let cur = self.panel_scroll.get() as i32;
        let next = (cur + delta).clamp(0, u16::MAX as i32) as u16;
        self.panel_scroll.set(next);
    }

    /// Record a new AI activity entry, keeping the log bounded.
    pub fn push_event(&mut self, msg: impl Into<String>) {
        self.events.insert(0, msg.into());
        if self.events.len() > MAX_EVENTS {
            self.events.truncate(MAX_EVENTS);
        }
    }

    /// 1 秒ごとに現在の cash をサンプリングして履歴に積む。直近 12 秒分のみ保持。
    /// `tick % TICKS_PER_SEC == 0` のタイミングだけ呼ぶ前提
    /// (1 秒間隔より高頻度に積むとウィンドウ秒数とサンプル数が乖離する)。
    pub fn record_cash_sample(&mut self) {
        debug_assert!(
            self.tick.is_multiple_of(TICKS_PER_SEC as u64),
            "record_cash_sample must be called once per simulated second",
        );
        const KEEP_SAMPLES: usize = 12;
        self.cash_history.push_back((self.tick, self.cash));
        while self.cash_history.len() > KEEP_SAMPLES {
            self.cash_history.pop_front();
        }
    }

    /// 直近 `window_secs` 秒の cash 増減レート (cents/sec)。サンプル不足なら None。
    /// 撤去コストや建設コストも含む実効レート ─ 理論 income との乖離が thrash の指標。
    ///
    /// **戻り値は cents/sec**。$1/sec = 100。整数除算で sub-dollar の差が常に 0 に
    /// 丸められないよう cents 解像度で持つ。$ 表示は呼出側で /100 して整形する。
    ///
    /// `target_tick` (= `self.tick - window_secs * TICKS_PER_SEC`) 以降で最古の
    /// サンプルを基準に diff を取る。サンプルが target_tick より古いものしかない場合は
    /// その最古サンプルを使うので、起動直後でも「ある期間の平均」を返せる。
    pub fn cash_flow_per_sec_cents(&self, window_secs: u64) -> Option<i64> {
        let target_tick = self.tick.saturating_sub(window_secs * TICKS_PER_SEC as u64);
        let pivot = self
            .cash_history
            .iter()
            .find(|(t, _)| *t >= target_tick)
            .or_else(|| self.cash_history.front());
        let (t0, c0) = match pivot {
            Some(&(t, c)) => (t, c),
            None => return None,
        };
        let dt_ticks = self.tick.saturating_sub(t0) as i64;
        if dt_ticks <= 0 {
            return None;
        }
        // cents/sec = (Δ$ * 100) * TICKS_PER_SEC / Δticks
        // Δ$ は撤去・建設で減ると負になる。i64 セマンティクスで符号は保たれる。
        let cents_diff = self.cash.saturating_sub(c0).saturating_mul(100);
        Some(cents_diff.saturating_mul(TICKS_PER_SEC as i64) / dt_ticks)
    }

    /// xorshift64* — small, deterministic, good enough for placement noise.
    pub fn next_rand(&mut self) -> u64 {
        let mut x = self.rng_state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng_state = x;
        x
    }

    pub fn tile(&self, x: usize, y: usize) -> &Tile {
        &self.grid[y][x]
    }

    #[cfg(test)]
    pub fn set_tile(&mut self, x: usize, y: usize, t: Tile) {
        self.grid[y][x] = t;
        self.invalidate_population_cache();
    }

    /// How many constructions are currently active.
    pub fn active_constructions(&self) -> u32 {
        let mut n = 0;
        for row in &self.grid {
            for t in row {
                if matches!(t, Tile::Construction { .. }) {
                    n += 1;
                }
            }
        }
        n
    }

    /// Free worker slots = workers - active.
    pub fn free_workers(&self) -> u32 {
        self.workers.saturating_sub(self.active_constructions())
    }

    /// Iterate every cell with its (x, y).
    pub fn cells(&self) -> impl Iterator<Item = (usize, usize, &Tile)> {
        self.grid
            .iter()
            .enumerate()
            .flat_map(|(y, row)| row.iter().enumerate().map(move |(x, t)| (x, y, t)))
    }

    pub fn count_built(&self, kind: Building) -> u32 {
        let mut n = 0;
        for (_, _, t) in self.cells() {
            if let Tile::Built(b) = t {
                if *b == kind {
                    n += 1;
                }
            }
        }
        n
    }

    /// Tier 連動の正確な人口 (Apartment 12 / Highrise 30 を反映)。
    ///
    /// 内部で edge-connectivity BFS と全 House の Tier 評価を走らせる O(houses + GRID²)
    /// 計算を伴うが、`population_cache` で per-frame メモ化しているため
    /// 同 frame 内で複数回呼んでも 1 度しか走らない。grid を変更した後は
    /// `invalidate_population_cache()` を呼ぶ規約。
    ///
    /// UI のバナー / Status / 詳細表示と AI Tier 2/3 の人口ゲートはすべて
    /// この値を参照する。`detect_tier_advance` の閾値判定もここを通るため、
    /// 表示と進化判定が常に一致する。
    pub fn population(&self) -> u32 {
        if let Some(p) = self.population_cache.get() {
            return p;
        }
        let p = super::logic::tier_aware_population(self);
        self.population_cache.set(Some(p));
        p
    }

    /// Convenience: 指定セルの地形。境界外は Plain 扱い。
    pub fn terrain_at(&self, x: usize, y: usize) -> super::terrain::Terrain {
        if x >= GRID_W || y >= GRID_H {
            super::terrain::Terrain::Plain
        } else {
            self.terrain[y][x]
        }
    }
}

impl Default for City {
    fn default() -> Self {
        City::new()
    }
}
