//! Pure-function game logic.  No I/O, no rendering — safe to call millions
//! of times from `simulator.rs`.

use super::ai::{decide, AiAction};
use super::state::*;

// ── Day / night cycle (Phase C) ─────────────────────────────
//
// バナーの太陽 ◉ / 月 ◯ と各タイルの「夜だから窓が灯る」判定は、これまで
// 別々の `tick % N` 計算で行っていた。視覚同期が崩れて「太陽が見えてる
// のにビルの窓が灯る」みたいな違和感が出る。`day_phase` を Single Source
// of Truth にすることで、太陽位置・夜判定・夜間の bg 暗化がすべて 1 つの
// 周期に乗る。
//
// `make_sky_line` は `tick / 30` で位相を進めていたので、それと完全一致
// させる。1 サイクル = 2 * grid_width * 30 ticks。
// 半サイクルで昼夜が切り替わる。

/// 1 日の進行度合い。banner / tile bg / window light が共通参照する。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DayPhase {
    /// 朝〜昼 — 太陽が左から右へ。
    Day,
    /// 夕方 — 太陽が地平線近く。窓が点灯し始める。
    Dusk,
    /// 夜 — 月。Highrise の窓が明るく、Plain bg は暗化。
    Night,
}

impl DayPhase {
    /// 0..=255 の暗化係数。0 = 元の色、255 = 黒。
    pub fn dim_factor(self) -> u8 {
        match self {
            DayPhase::Day => 0,
            DayPhase::Dusk => 60,
            DayPhase::Night => 110,
        }
    }
}

/// 1 in-game day の長さ (ticks)。10 ticks/sec なので 60 秒 = 1 日。
/// バナー幅と切り離した固定周期にすることで、ウィンドウサイズに依らない
/// 視覚同期が保てる (banner は別途 phase % DAY_LENGTH の比率で太陽位置を出す)。
pub const DAY_LENGTH_TICKS: u64 = 600;

/// 現在の DayPhase を返す純関数。
///
/// 内訳:
///   - 0..240   (40%): Day
///   - 240..300 (10%): Dusk
///   - 300..540 (40%): Night
///   - 540..600 (10%): Dusk (Dawn を Dusk と同一視)
pub fn day_phase(tick: u64) -> DayPhase {
    let phase = tick % DAY_LENGTH_TICKS;
    match phase {
        0..240 => DayPhase::Day,
        240..300 => DayPhase::Dusk,
        300..540 => DayPhase::Night,
        _ => DayPhase::Dusk, // 540..600 (dawn)
    }
}

/// バナー用: DayPhase 内の 0.0..=1.0 進行度。太陽/月の水平位置に使う。
/// Day phase 中は 0..1 を線形に進む (太陽が左→右)、Night も同様 (月が左→右)。
/// Dusk 中は次の天体に切り替わる過渡なので 1.0 寄りに固定。
pub fn day_progress(tick: u64) -> f32 {
    let phase = tick % DAY_LENGTH_TICKS;
    match phase {
        0..240 => phase as f32 / 240.0,
        240..300 => 1.0,
        300..540 => (phase - 300) as f32 / 240.0,
        _ => 1.0,
    }
}

/// `Color::Rgb(r,g,b)` を `dim` 量だけ黒に近づける。
///
/// ratatui の `Color` enum は Named/Indexed/Rgb の 3 種があり、Named/Indexed
/// は端末側の解釈に任せるしかない。ここは Rgb 入力専用ヘルパーで、Plain や
/// Forest など我々が完全制御している bg だけを夜間に暗化する用途。
pub fn dim_rgb(r: u8, g: u8, b: u8, dim: u8) -> (u8, u8, u8) {
    let f = 255u16 - dim as u16;
    (
        ((r as u16 * f) / 255) as u8,
        ((g as u16 * f) / 255) as u8,
        ((b as u16 * f) / 255) as u8,
    )
}

/// Advance the simulation by `delta_ticks` ticks.
///
/// Each tick we:
///   1. tick down active constructions, finishing any that hit zero
///   2. ask the AI for an action while there's a free worker (capped per tick)
///   3. accrue cash income (every 10 ticks = 1 simulated second)
pub fn tick(city: &mut City, delta_ticks: u32) {
    for _ in 0..delta_ticks {
        step_one_tick(city);
    }
}

fn step_one_tick(city: &mut City) {
    advance_construction(city);
    // auto を drive_ai より先に動かす理由: drive_ai は free_workers をすべて
    // 埋めてしまうので、その後で auto を回すと Outpost 派遣の発火ウィンドウ
    // (`tick % period == 0` の単一 tick) がほぼ常に free_workers=0 で外れる。
    // 先に auto が 1 ワーカーを Outpost に予約しておくことで、戦略の意図
    // (拡張する / 撤去する) が確実に成立する。
    auto_strategy_actions(city);
    drive_ai(city);
    accrue_income(city);
    detect_tier_advance(city);
    city.tick = city.tick.wrapping_add(1);
}

/// ティア境界を跨いだら演出をトリガー。降格 (建物撤去等で人口減) は
/// 現状ありえない (撤去機能なし) ので、上昇のみ検出する。
fn detect_tier_advance(city: &mut City) {
    let now = city_tier_for(city.population());
    if now > city.last_observed_tier {
        city.tier_flash_until = city.tick + TIER_FLASH_TICKS;
        city.push_event(format!("🎊 街が「{}」に成長しました!", now.jp()));
        city.last_observed_tier = now;
    }
}

/// Decrement every Construction / Clearing tile; promote them when finished.
///
/// Clearing 完了時は地形を Plain に書き換え、タイルを Empty に戻す
/// (=「整地済み」を地形レイヤーに永続化する設計)。これで撤去機能が
/// 将来入っても、整地済みエリアは再露出しても Plain のままになる。
fn advance_construction(city: &mut City) {
    let mut completions: Vec<(usize, usize, Building)> = Vec::new();
    let mut clearings: Vec<(usize, usize)> = Vec::new();
    for y in 0..GRID_H {
        for x in 0..GRID_W {
            let tile = &mut city.grid[y][x];
            match tile {
                Tile::Construction {
                    target,
                    ticks_remaining,
                } => {
                    if *ticks_remaining <= 1 {
                        let kind = *target;
                        *tile = Tile::Built(kind);
                        city.buildings_finished += 1;
                        // 「築 0 の起点」を記録。aging_factor / effective_house_tier の
                        // 基準点になる。撤去/再建で常に上書きされるため、その建物の
                        // 経年と Tier 昇格 dwell time の唯一のソース。
                        city.built_at_tick[y][x] = city.tick;
                        completions.push((x, y, kind));
                    } else {
                        *ticks_remaining -= 1;
                    }
                }
                Tile::Clearing { ticks_remaining } => {
                    if *ticks_remaining <= 1 {
                        *tile = Tile::Empty;
                        clearings.push((x, y));
                    } else {
                        *ticks_remaining -= 1;
                    }
                }
                _ => {}
            }
        }
    }
    // 整地完了: 地形を Plain に書き換え、軽いログを出す。
    for (x, y) in clearings {
        city.terrain[y][x] = super::terrain::Terrain::Plain;
        city.completion_flash_until[y][x] = city.tick + COMPLETION_FLASH_TICKS;
        city.push_event(format!("⛏ ({},{}) 整地完了", x, y));
    }
    for (x, y, kind) in completions {
        city.completion_flash_until[y][x] = city.tick + COMPLETION_FLASH_TICKS;
        city.push_event(format!("✓ {} ({},{}) 完成", building_name(kind), x, y));
    }
}

fn building_name(b: Building) -> &'static str {
    match b {
        Building::Road => "道路",
        Building::House => "住宅",
        Building::Workshop => "工房",
        Building::Shop => "店舗",
        Building::Park => "公園",
        Building::Outpost => "開拓機材",
    }
}

fn terrain_name(t: super::terrain::Terrain) -> &'static str {
    use super::terrain::Terrain::*;
    match t {
        Plain => "平地",
        Forest => "森",
        Wasteland => "荒地",
        Water => "湖",
        Rock => "岩盤",
    }
}

/// Let the AI place at most one new construction per tick per free worker.
/// We cap at `free_workers` per tick to avoid unrealistic burst placement.
fn drive_ai(city: &mut City) {
    let mut placements_left = city.free_workers();
    // Limit AI calls per tick so we don't loop forever if it keeps idling.
    let mut attempts = placements_left.saturating_mul(2).max(1);
    while placements_left > 0 && attempts > 0 {
        attempts -= 1;
        match decide(city) {
            AiAction::Build { x, y, kind } => {
                if start_construction(city, x, y, kind) {
                    placements_left -= 1;
                }
            }
            AiAction::Idle => break,
        }
    }
}

/// Tech 戦略時の建設速度ブースト。`strategy_info` 経由で取得することで
/// 「Strategy 副作用の唯一の集約点」を maintain。state/render は読取専用。
fn build_ticks_for(city: &City, kind: Building) -> u32 {
    let base = kind.build_ticks();
    let bonus = strategy_info(city.strategy).speed_bonus_pct;
    if bonus == 0 {
        return base;
    }
    // bonus = +20 (建設時間 -20%) → factor 80/100。
    // bonus = -10 (建設時間 +10%) → factor 110/100。
    let factor_num = (100 - bonus).max(10) as u64; // 下限 10 で安全側
    (base as u64 * factor_num).div_ceil(100) as u32
}

// ── Strategy metadata (Single Source of Truth) ─────────────
//
// Strategy の意味 (重み・速度ボーナス・収入ペナルティ・説明文・思考動詞) を
// 1 か所に集約。AI (ai.rs)・状態タブ・マネージャータブ・イベントログ・
// 建設速度補正 (build_ticks_for) はすべてここを参照する。

/// Strategy の全方位プロファイル。AI の重みも player への説明文も同居。
#[derive(Clone, Copy, Debug)]
pub struct StrategyInfo {
    /// 短いラベル ("成長重視" など)。
    pub label: &'static str,
    /// 1 行の意図説明。Manager タブのボタン下にこのまま出す。
    pub tagline: &'static str,
    /// AI が建物種別を引く時の重み (合計 100 を厳守)。
    pub house_pct: u32,
    pub road_pct: u32,
    pub workshop_pct: u32,
    pub shop_pct: u32,
    /// 公園の重み。0 だと AI は Park を建てない。Eco 戦略のみ非ゼロ。
    pub park_pct: u32,
    /// 建設速度ボーナス (%)。+20 = 建設 20% 短縮、-10 = 10% 延長。
    pub speed_bonus_pct: i32,
    /// 収入ペナルティ (%)。-20 = 収入 20% 減、0 = 通常。
    pub income_penalty_pct: i32,
}

/// 各戦略のプロファイル。重みは tier4_demand_aware と一致させる
/// (ai.rs はこの構造体を直接読む)。
pub fn strategy_info(s: Strategy) -> StrategyInfo {
    match s {
        Strategy::Growth => StrategyInfo {
            label: "成長重視",
            tagline: "人口を伸ばし街のティア進化を急ぐ",
            house_pct: 70,
            road_pct: 20,
            workshop_pct: 0,
            shop_pct: 10,
            park_pct: 0,
            speed_bonus_pct: 0,
            income_penalty_pct: 0,
        },
        Strategy::Income => StrategyInfo {
            label: "収入重視",
            tagline: "工房と店舗で経済を回し現金を稼ぐ",
            house_pct: 30,
            road_pct: 22,
            workshop_pct: 13,
            shop_pct: 35,
            park_pct: 0,
            speed_bonus_pct: 0,
            income_penalty_pct: 0,
        },
        Strategy::Tech => StrategyInfo {
            label: "技術投資",
            tagline: "道路網を急拡大 (建設+20% / 収入-20%)",
            house_pct: 35,
            road_pct: 50,
            workshop_pct: 0,
            shop_pct: 15,
            park_pct: 0,
            speed_bonus_pct: 20,
            income_penalty_pct: -20,
        },
        Strategy::Eco => StrategyInfo {
            label: "環境配慮",
            tagline: "森を残し公園で街を彩る (建設-10% / 収入+5%)",
            house_pct: 40,
            road_pct: 25,
            workshop_pct: 0,
            shop_pct: 25,
            // Eco 戦略の固有要素: Park を 10% 確率で建てる。
            // 公園が増えると周囲の House が Apartment/Highrise に育ちやすくなり、
            // 「森を残しても街は育つ」というキャラ付けが完成する。
            park_pct: 10,
            // 副作用は「ゆっくり育てる」を表現する負の建設速度と僅かな収入ボーナス。
            // ボーナスは正の `income_penalty_pct = +5` として扱う (関数側で 100+5)。
            speed_bonus_pct: -10,
            income_penalty_pct: 5,
        },
    }
}

/// AI のイベントログに出す「思考動詞」を Strategy × Building で返す。
/// マネージャー視点で「CPU が今この戦略でこの建物を建てた → だからこういう
/// 意図」を体感できるようにする。
pub fn strategy_thought_verb(s: Strategy, kind: Building) -> &'static str {
    match (s, kind) {
        (Strategy::Growth, Building::House) => "住宅地を拡張",
        (Strategy::Growth, Building::Road) => "生活道路を整備",
        (Strategy::Growth, Building::Shop) => "近所の店舗を出店",
        (Strategy::Growth, Building::Workshop) => "近隣の工房を整備",

        (Strategy::Income, Building::House) => "労働者用住宅を建設",
        (Strategy::Income, Building::Road) => "商業道路を整備",
        (Strategy::Income, Building::Shop) => "商業地を育てる",
        (Strategy::Income, Building::Workshop) => "工房で雇用を創出",

        (Strategy::Tech, Building::House) => "ベッドタウンを増設",
        (Strategy::Tech, Building::Road) => "道路網を伸ばす",
        (Strategy::Tech, Building::Shop) => "幹線沿いに出店",
        (Strategy::Tech, Building::Workshop) => "工業地区を試験設置",

        (Strategy::Eco, Building::House) => "緑に囲まれた住宅を整備",
        (Strategy::Eco, Building::Road) => "並木道を敷設",
        (Strategy::Eco, Building::Shop) => "地域密着の店舗を出店",
        (Strategy::Eco, Building::Workshop) => "森に配慮した工房を整備",

        // Park: 戦略によって公園の意味付けが変わる。
        (Strategy::Growth, Building::Park) => "中央公園を整備",
        (Strategy::Income, Building::Park) => "高級住宅街向け緑地を確保",
        (Strategy::Tech, Building::Park) => "幹線沿いに緑地帯を配置",
        (Strategy::Eco, Building::Park) => "森を残し公園として開放",

        // Outpost はプレイヤー操作専用 — AI は建てない (= 思考動詞は出ない)。
        // ただし match 網羅性のため共通文言を入れる (debug 中などに発火した場合)。
        (_, Building::Outpost) => "開拓機材を設置",
    }
}

// ── 自動運用ポリシー (Phase A 撤去 / 開拓の完全自動化) ─────────────
//
// プレイヤーが選んだ Strategy に応じて、CPU が「いつ Outpost を派遣するか /
// いつ撤去判断を走らせるか / どこまで現金を予備に取っておくか」を自動決定する。
// 撤去・開拓の手動ボタンは廃止され、すべてここを起点に tick 駆動で発火する。
//
// ## 数値の意味
//
// - `outpost_dispatch_period_ticks` (None = 自動拡張しない):
//   この間隔で `dispatch_outpost` を試行。Outpost 自体の建設時間が 60 sec
//   (= 600 ticks) なので、これより短くすると同時建設が積み上がる。
//
// - `auto_demolish_period_ticks` (None = 自動撤去しない):
//   この間隔で `auto_demolish` を試行。短くすると無駄な建物を即排除する
//   「効率最大化」キャラ、長くすると「のんびり放置」キャラ。
//
// - `min_cash_reserve`:
//   この額より下になる行動 (Outpost 派遣 / 撤去) は控える。drive_ai が建物を
//   建てる原資を残すためのガード。Outpost コスト ($600) + 余裕 = 戦略次第。
//
// 数値はシミュレーター (simulator.rs `automation_*` 群) で 30min ベンチを
// 走らせて「成長する / cash が貯まる / Rock が解禁される」を観測しながら
// 確定させた。変更時は同テストで回帰を確認すること。

/// 自動運用の方針 (Strategy ごとの周期と予算ガード)。
#[derive(Clone, Copy, Debug)]
pub struct AutomationPolicy {
    pub outpost_dispatch_period_ticks: Option<u32>,
    pub auto_demolish_period_ticks: Option<u32>,
    pub min_cash_reserve: i64,
}

/// 戦略ごとの自動運用設定。`auto_strategy_actions` が毎 tick 参照する。
///
/// シミュレータ (`automation_drives_outposts_and_demolitions`) で 4-worker
/// DemandAware で 30 min 動かして「全戦略が拡張可能戦略では Outpost を
/// 1 基以上派遣する」を担保しながら値を確定した。
///
/// `min_cash_reserve` は **drive_ai が建物を建て終えた後の余裕**。
/// drive_ai が貪欲にキャッシュを消費するため、リザーブを大きくしすぎると
/// 自動派遣が永遠に発火しない。$200-500 程度が実機テストでバランス良い。
pub fn automation_policy(s: Strategy) -> AutomationPolicy {
    match s {
        // 成長: 人口拡張のために土地が要る → 拡張は最速ペース、撤去は控えめ。
        // 戦略の主役 (人口) を支えるため、4 戦略中で最も短い拡張周期にする。
        Strategy::Growth => AutomationPolicy {
            outpost_dispatch_period_ticks: Some(600),
            auto_demolish_period_ticks: Some(900),
            min_cash_reserve: 300,
        },
        // 収入: 効率最大化 = 無駄な建物を積極排除。拡張は控えめ。
        Strategy::Income => AutomationPolicy {
            outpost_dispatch_period_ticks: Some(1500),
            auto_demolish_period_ticks: Some(750),
            min_cash_reserve: 500,
        },
        // 技術投資: 建設+20% を活かす + 道路網重視。拡張は中庸 (= 道路を
        // 既存域内で太く張ることに比重を置く)。Growth より遅い周期にして
        // 「Growth は土地で攻める / Tech は道路網で攻める」差別化を担保。
        Strategy::Tech => AutomationPolicy {
            outpost_dispatch_period_ticks: Some(1000),
            auto_demolish_period_ticks: Some(750),
            min_cash_reserve: 400,
        },
        // 環境配慮: 緑も岩も残す。自動拡張・撤去とも無し。
        // 「緑地を保護し、既存の市域で完結する」キャラクター。
        // プレイヤーが Eco を選ぶ = 「広げない、潰さない、ゆっくり育てる」を
        // 全面に押し出すための明確な選択肢にする。
        Strategy::Eco => AutomationPolicy {
            outpost_dispatch_period_ticks: None,
            auto_demolish_period_ticks: None,
            min_cash_reserve: 200,
        },
    }
}

/// `step_one_tick` から毎 tick 呼ばれる自動運用ハブ。
///
/// **クールダウン方式**: 「最後に成功した tick + period」を過ぎていれば毎 tick
/// 試行する。`tick % period == 0` の単一 tick 発火だと、その瞬間に
/// `free_workers == 0` だった場合に取り逃がす (DemandAware は drive_ai が
/// 直前にワーカーを埋めるため発生しがち)。クールダウン方式なら、条件が
/// 揃った最初の tick で発火するので戦略の意図が必ず実行される。
pub fn auto_strategy_actions(city: &mut City) {
    if city.tick == 0 {
        return;
    }
    let policy = automation_policy(city.strategy);

    if let Some(period) = policy.outpost_dispatch_period_ticks {
        let due = city.tick.saturating_sub(city.last_outpost_dispatch_tick) >= period as u64
            || city.last_outpost_dispatch_tick == 0;
        // 同時に複数の Outpost を建てない (= 1 基ずつ確実に建てて Rock を
        // 解禁してから次へ進める)。
        let outpost_in_progress = city.cells().any(|(_, _, t)| {
            matches!(t, Tile::Construction { target: Building::Outpost, .. })
        });
        // Outpost 建設は 60 sec ワーカー占有 = 1-worker プレイヤーは時間の
        // 半分以上を Outpost に取られて家が建たなくなる。雇用 (>= 2 worker)
        // が「自動拡張アンロック」の役割を持つ設計。プレイヤーが [W] で
        // ワーカーを増やすことに具体的な意味が生まれる。
        if due
            && city.workers >= 2
            && !outpost_in_progress
            && city.free_workers() > 0
            && city.cash >= Building::Outpost.cost() + policy.min_cash_reserve
            && dispatch_outpost(city)
        {
            city.last_outpost_dispatch_tick = city.tick;
        }
    }
    if let Some(period) = policy.auto_demolish_period_ticks {
        let due = city.tick.saturating_sub(city.last_auto_demolish_tick) >= period as u64
            || city.last_auto_demolish_tick == 0;
        if due {
            if let Some((tx, ty, _score)) = auto_demolish_target(city) {
                if city.cash >= demolish_cost(tx, ty) + policy.min_cash_reserve
                    && auto_demolish(city)
                {
                    city.last_auto_demolish_tick = city.tick;
                }
            }
        }
    }
}

/// セルが「AI が即着工できる」状態か。
///
/// 条件: Empty かつ buildable かつ (Rock なら隣接 Outpost あり)。
/// AI のタイル候補フィルタの一元化用。各 tier の placement filter から呼ぶ。
pub fn ai_can_break_ground(city: &City, x: usize, y: usize) -> bool {
    if !matches!(city.tile(x, y), Tile::Empty) {
        return false;
    }
    let t = city.terrain_at(x, y);
    if !t.buildable() {
        return false;
    }
    if t.needs_outpost() && !has_outpost_neighbor(city, x, y) {
        return false;
    }
    true
}

/// 4-近傍 (上下左右) に Outpost が建っているか。Rock 整地のゲート判定。
/// 建設中 Outpost は対象外 (まだ機材として稼働していない)。
pub fn has_outpost_neighbor(city: &City, x: usize, y: usize) -> bool {
    for (dx, dy) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
        let nx = x as i32 + dx;
        let ny = y as i32 + dy;
        if nx < 0 || ny < 0 || nx >= GRID_W as i32 || ny >= GRID_H as i32 {
            continue;
        }
        if matches!(
            city.tile(nx as usize, ny as usize),
            Tile::Built(Building::Outpost)
        ) {
            return true;
        }
    }
    false
}

/// Spend cash and turn an Empty cell into a Construction tile.
/// Returns false if the cell is non-empty, terrain forbids it, or we can't
/// afford it.
pub fn start_construction(city: &mut City, x: usize, y: usize, kind: Building) -> bool {
    if x >= GRID_W || y >= GRID_H {
        return false;
    }
    if !matches!(city.grid[y][x], Tile::Empty) {
        return false;
    }
    // 地形の建設可否 (湖には建てられない)。
    if !city.terrain_at(x, y).buildable() {
        return false;
    }
    // 要整地の地形 (Forest/Wasteland/Rock) はまず整地工程を発生させる。
    // Rock のみ追加で「隣接 Outpost 必須」のゲートが入る。
    // 整地中は Tile::Clearing になり worker を 1 占有する。完了後は Empty に
    // 戻り、AI が次の tick で改めて建物を建てに来る (= 関数を 2 回通る)。
    let terrain = city.terrain_at(x, y);
    if terrain.needs_outpost() && !has_outpost_neighbor(city, x, y) {
        // Outpost が隣に無いと Rock は破砕できない。AI には事前にこのチェックを
        // させたいので、ここで早期 return する (cash も消費しない)。
        return false;
    }
    if terrain.needs_clearing() {
        let clearing_cost = terrain.clearing_cost();
        if city.cash < clearing_cost {
            return false;
        }
        city.cash -= clearing_cost;
        city.cash_spent_total += clearing_cost;
        city.grid[y][x] = Tile::Clearing {
            ticks_remaining: terrain.clearing_ticks(),
        };
        city.push_event(format!(
            "⛏ ({},{}) 整地着工 ({}) -${}",
            x,
            y,
            terrain_name(terrain),
            clearing_cost
        ));
        return true;
    }
    let cost = kind.cost();
    if city.cash < cost {
        return false;
    }
    city.cash -= cost;
    city.cash_spent_total += cost;
    let ticks = build_ticks_for(city, kind);
    city.grid[y][x] = Tile::Construction {
        target: kind,
        ticks_remaining: ticks,
    };
    city.buildings_started += 1;
    // Tier 4 (DemandAware) のみ Strategy に基づく動詞を表示。
    // 低 Tier は戦略を読まない設計なので、汎用の「着工」を出す方が誠実。
    // この差自体が「上位 AI ほど目的を持って動いている」演出にもなる。
    if matches!(city.ai_tier, AiTier::DemandAware) {
        city.push_event(format!(
            "▷ {} ({},{}) — {} -${}",
            building_name(kind),
            x,
            y,
            strategy_thought_verb(city.strategy, kind),
            cost
        ));
    } else {
        city.push_event(format!(
            "▷ {} ({},{}) 着工 -${}",
            building_name(kind),
            x,
            y,
            cost
        ));
    }
    true
}

/// Earn cash once per simulated second (every 10 ticks).
fn accrue_income(city: &mut City) {
    if !city.tick.is_multiple_of(TICKS_PER_SEC as u64) {
        return;
    }
    let income = compute_income_per_sec(city);
    city.cash += income;
    city.cash_earned_total += income;
    if income > 0 {
        city.last_payout_amount = income;
        city.last_payout_tick = city.tick;
    }
    let mut flash_targets: Vec<(usize, usize)> = Vec::new();
    for y in 0..GRID_H {
        for x in 0..GRID_W {
            if matches!(city.tile(x, y), Tile::Built(Building::Shop)) && shop_is_active(city, x, y)
            {
                flash_targets.push((x, y));
            }
        }
    }
    for (x, y) in flash_targets {
        city.payout_flash_until[y][x] = city.tick + PAYOUT_FLASH_TICKS;
    }
}

/// Compute total cash/sec.  Pure function over the grid — easy to unit-test.
///
/// **Tier × 老朽化** の二軸で建物個別に収入を出す:
///   - House: 実効 Tier (Cottage/Apartment/Highrise) 別の基本値
///   - Workshop / Shop: 活性条件を満たすと固定値
///   - すべての建物に **aging_factor** を掛ける (Tier ごとの寿命差を反映)
///
/// 整数演算で確定的に計算するため、内部では「セント単位」で集計し、最後に
/// 100 で割って円ドル単位の i64 に戻す。`aging_factor_per_mille` は ‰ なので
/// `cents * factor / 1000` で老朽化込み。
///
/// **Tier 別 House 収入** (`/sec`、aging 前):
///   - Cottage:   $0.5  ((houses+1)/2 の旧仕様と同等スケール)
///   - Apartment: $1.5  (Cottage の 3 倍)
///   - Highrise:  $3.0  (Cottage の 6 倍 — 育てきった街区の報酬)
///
/// 「Highrise は 6 倍」が本機能の主役。dwell time (5 min) と寿命 (4×) を考えると
/// 「育てた街区は長く高収入を出す」が成り立つ。
pub fn compute_income_per_sec(city: &City) -> i64 {
    let now = city.tick;
    let mut income_cents: i64 = 0;

    for y in 0..GRID_H {
        for x in 0..GRID_W {
            let kind = match city.tile(x, y) {
                Tile::Built(b) => *b,
                _ => continue,
            };
            // Tier (House のみ) を先に決める。aging の lifespan にも使う。
            let tier_opt = if matches!(kind, Building::House) {
                Some(effective_tier_at(city, x, y))
            } else {
                None
            };
            // 基本収入 (cents/sec)。
            let base_cents: i64 = match kind {
                Building::House => match tier_opt.expect("house has tier") {
                    HouseTier::Cottage => 50,
                    HouseTier::Apartment => 150,
                    HouseTier::Highrise => 300,
                },
                Building::Workshop if workshop_is_active(city, x, y) => 100,
                Building::Shop if shop_is_active(city, x, y) => 200,
                _ => 0,
            };
            if base_cents == 0 {
                continue;
            }
            // 築年数で aging を掛ける。built_at_tick が 0 = 起点未設定 = 老けない扱い。
            let aged = if city.built_at_tick[y][x] == 0 {
                base_cents
            } else {
                let age = now.saturating_sub(city.built_at_tick[y][x]);
                let factor = aging_factor_per_mille(age, lifespan_x100(kind, tier_opt)) as i64;
                (base_cents * factor) / 1000
            };
            income_cents += aged;
        }
    }

    // 死スパイラル防止: 1 軒でも House があれば最低 $1/s は保証する。
    // 旧仕様 `(houses + 1) / 2` の「1 軒で $1」を維持し、序盤の seed-RNG
    // 偶発で income==0 のままになるのを防ぐ (simulator::tier1_never_stalls)。
    let any_house = city.count_built(Building::House) > 0;
    let mut income = income_cents / 100;
    if any_house && income == 0 {
        income = 1;
    }

    // Strategy の収入修正 (Tech は -20%、Eco は +5% 等)。
    let modifier = strategy_info(city.strategy).income_penalty_pct;
    if modifier != 0 && income > 0 {
        let factor = (100 + modifier).max(10) as i64;
        income = ((income * factor) / 100).max(1);
    }
    income
}

/// 住宅の段階レベル (描画専用の派生値)。
///
/// 純関数 — 周辺の House 密度から計算する。state にフィールドを増やさず、
/// 描画時に毎回計算する設計。Cookie Factory と同じ Pure Logic Pattern。
///
/// **デザイン**: 隣接 (4-近傍) に House がいくつあるか:
///   - 0 → Low  (低層)   `▟▙`
///   - 1〜2 → Mid (中層) `▛▜`
///   - 3〜4 → High (高層) `█▌`
///
/// 都市計画のリアルさ: 周りに住宅クラスターがあると土地が高密度化する。
/// プレイヤーが「住宅は固めて配置すべき」と気付ける戦略レイヤー。
pub fn house_level(city: &City, x: usize, y: usize) -> HouseLevel {
    let mut neighbors = 0u32;
    for (dx, dy) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
        let nx = x as i32 + dx;
        let ny = y as i32 + dy;
        if nx < 0 || ny < 0 || nx >= GRID_W as i32 || ny >= GRID_H as i32 {
            continue;
        }
        if matches!(city.tile(nx as usize, ny as usize), Tile::Built(Building::House)) {
            neighbors += 1;
        }
    }
    match neighbors {
        0 => HouseLevel::Low,
        1 | 2 => HouseLevel::Mid,
        _ => HouseLevel::High,
    }
}

/// 住宅密度レベル。描画専用 — state には保持しない。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HouseLevel {
    Low,
    Mid,
    High,
}

// ── House evolution (DESIGN.md §2.3, §4) ────────────────────
//
// 「街が育つ感」の核となるルール群。すべて純関数で、state を増やさず
// 周囲のセルだけ見て派生値を計算する。Pure Logic Pattern。

/// 住宅の経済段階。Cottage → Apartment → Highrise と育つ。
///
/// `HouseLevel` (隣接 House 数による低/中/高層の見た目) とは別軸:
/// こちらは「経済が回って住宅が高層化する」段階で、Workshop / Shop / Road の
/// 充実度から決まる (DESIGN.md §2.3)。両者は最終的に統合する可能性があるが、
/// 一旦は別概念として並置し、render で組み合わせる。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum HouseTier {
    Cottage,   // 基本の住宅。インフラ未整備。
    Apartment, // 中層。Road + Workshop が近い。
    Highrise,  // 高層。Road + Workshop + Shop が揃った成熟ゾーン。
}

/// `house_tier_for` が見る周囲の充実度サマリ。
///
/// House 一軒分の周辺をスキャンして集計したもの。フィールドの意味:
/// - `n_road_adj`: 4-近傍にある Road タイル数 (0..=4)。0 だと未接続。
/// - `n_workshop_within_5`: Manhattan 距離 5 以内の Workshop 数。
/// - `n_shop_within_5`: Manhattan 距離 5 以内の Shop 数。
/// - `n_house_within_3`: Manhattan 距離 3 以内の House 数 (自身は除く)。
/// - `n_park_within_4`: Manhattan 距離 4 以内の Park 数。Workshop / Shop と
///   並ぶ「経済刺激源」として Tier 上昇に寄与する。緑地でも街が育つ。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HouseNeighborhood {
    pub n_road_adj: u32,
    pub n_workshop_within_5: u32,
    pub n_shop_within_5: u32,
    pub n_house_within_3: u32,
    pub n_park_within_4: u32,
}

/// 周囲をスキャンして `HouseNeighborhood` を組み立てる。
///
/// この関数は機械的な集計のみを担当する (純関数 / 副作用なし)。
/// 「どの数値で Tier を決めるか」というゲームデザイン判断は
/// `house_tier_for` 側に閉じる。
pub fn gather_house_neighborhood(city: &City, x: usize, y: usize) -> HouseNeighborhood {
    let mut n_road_adj = 0u32;
    for (dx, dy) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
        let nx = x as i32 + dx;
        let ny = y as i32 + dy;
        if nx < 0 || ny < 0 || nx >= GRID_W as i32 || ny >= GRID_H as i32 {
            continue;
        }
        if matches!(
            city.tile(nx as usize, ny as usize),
            Tile::Built(Building::Road)
        ) {
            n_road_adj += 1;
        }
    }

    let mut n_shop_within_5 = 0u32;
    let mut n_workshop_within_5 = 0u32;
    let mut n_house_within_3 = 0u32;
    let mut n_park_within_4 = 0u32;
    for cy in 0..GRID_H {
        for cx in 0..GRID_W {
            let dx = (cx as i32 - x as i32).abs();
            let dy = (cy as i32 - y as i32).abs();
            let manhattan = (dx + dy) as u32;
            match city.tile(cx, cy) {
                Tile::Built(Building::Shop) if manhattan <= 5 => n_shop_within_5 += 1,
                Tile::Built(Building::Workshop) if manhattan <= 5 => n_workshop_within_5 += 1,
                Tile::Built(Building::House) if manhattan <= 3 && (cx, cy) != (x, y) => {
                    n_house_within_3 += 1
                }
                Tile::Built(Building::Park) if manhattan <= 4 => n_park_within_4 += 1,
                _ => {}
            }
        }
    }

    HouseNeighborhood {
        n_road_adj,
        n_workshop_within_5,
        n_shop_within_5,
        n_house_within_3,
        n_park_within_4,
    }
}

/// House の経済段階を決定する純関数。**★ ゲーム体験の核**
///
/// この関数の中身が「街がどう育つか」を直接決める。詳細な設計指針は
/// `DESIGN.md §4.1` を参照。簡潔に言うと:
///
/// - Cottage は無条件 (デフォルト)
/// - Apartment は「インフラが届いている」感を出す条件にしたい
/// - Highrise は「商工業が回っている」感を出す条件にしたい
///
/// プレイヤーが街を眺めて「あ、ここは Apartment になりかけてる、Shop を
/// もう一つ近くに置けば育ちそう」と気付ける形が理想。
///
/// **TODO (User contribution)**: この関数を実装してください (5〜10 行)。
/// 現状の `todo!()` は呼び出されると panic するため、未統合の今は問題ない
/// ですが、render / income に組み込む際に必須になります。
///
/// テストは `tests::house_tier_for_*` を参照 — 期待する大まかな挙動を
/// アサートしているので、書いた式で `cargo test -p metropolis` が通れば OK。
///
/// **採用方針**: 多段ゲート方式 (DESIGN.md §4.1)。
///   - Cottage  : 既定。インフラ未到達 or 商業未到達。
///   - Apartment: Road 接続 + 経済刺激 (Workshop/Shop) が近い。
///   - Highrise : Road 2 本以上 + 経済の厚み ≥ 2 + 周囲 House ≥ 3。
///
/// `economic_density = n_workshop_within_5 + n_shop_within_5` を派生値として
/// 一段噛ませる。Workshop 未実装の現在は Shop だけで Highrise に到達でき、
/// Workshop 実装後は両方が寄与する設計 (career Tier 進化と同じ「複数経路」思想)。
///
/// **シムシティ的な性質**: 「家を固めて道路を引いただけ」では Apartment にならず、
/// **商業 (Shop / Workshop) が近くで動いて初めて街区がリッチ化する**。
/// この条件があるため、Tech 戦略 (道路重視) が単独で住宅を高層化することはなく、
/// 戦略の特化が崩れない (simulator::tier4_strategies_specialize の不変条件)。
pub fn house_tier_for(stats: HouseNeighborhood) -> HouseTier {
    // Park は商業ほど刺激は強くない (1 Park = 0.5 経済密度) という重み付け:
    // 公園 2 つで Workshop/Shop 1 つ相当。「緑地だけでも育つが、商業よりは
    // ゆっくり」という SimCity 的な感覚を再現する。
    //
    // 整数演算なので `n_park / 2` で 0.5 倍を表現。`(n_park + 1) / 2` だと
    // 公園 1 つでも 1 ポイント (= Apartment 化に十分) になる切り上げ動作。
    let park_density = stats.n_park_within_4.div_ceil(2);
    let economic_density =
        stats.n_workshop_within_5 + stats.n_shop_within_5 + park_density;

    // Highrise: 商工業 + 緑地が両立した成熟ゾーン。
    if stats.n_road_adj >= 2 && economic_density >= 2 && stats.n_house_within_3 >= 3 {
        return HouseTier::Highrise;
    }

    // Apartment: 商業 or 緑地が来ている街区。Road + 経済刺激源が最低 1 つ必須。
    if stats.n_road_adj >= 1 && economic_density >= 1 {
        return HouseTier::Apartment;
    }

    HouseTier::Cottage
}

// ── Tier 昇格 dwell time + 老朽化 (Phase D: 建物バリエーション) ─────────
//
// 「良い建物はたくさん時間がかかる」「一度置いても時間が経ったら価値が下がる」
// 「整理したくなるバランス」を 1 セットの純関数で表現する。state は
// `built_at_tick` 1 グリッドだけ追加し、Tier や老朽化はすべて派生計算。
//
// ## 全体像
//
// - **Tier 昇格 dwell time**: `house_tier_for` が「目標 Tier」を返し、
//   実効 Tier はその目標が **建物の築年数で許される** 範囲に制限される。
//   Apartment は築 60 sec、Highrise は築 5 min が必要。
// - **老朽化 (aging factor)**: 築 1 min まではフル出力、5 min かけて 50% に
//   落ちる。**ただし Tier が高い建物ほど寿命倍率が大きい** — 同じ年数でも
//   Cottage はぼろぼろ、Highrise はまだまだ働く。これが「Highrise を育てると
//   長く儲かる」体感の源。
// - **再建で寿命リセット**: 撤去 → 同セルに新築すると `built_at_tick` が
//   更新され、aging が 0 から再カウント。`auto_demolish` が老朽化を検知して
//   自動更新するため、idle ゲームでも「世代交代の波」が街を流れる。

/// House を `target` Tier まで昇格させるのに必要な築年数 (ticks)。
///
/// プレイヤーが「街区が育つには時間がかかる」を体感するための主要数値。
/// 短いと「建てたら即 Highrise」で深みがゼロになり、長いと 30 min ベンチで
/// Highrise が一棟も拝めない。ベンチで houses ≈ 50-90 が 30 min で建つので、
/// Highrise dwell は数分程度が妥当。
///
/// **採用値**:
/// - Cottage:   0 ticks (即時、デフォルト)
/// - Apartment: 600 ticks (= 60 sec) — 「家が建ってひと段落した頃」
/// - Highrise:  3000 ticks (= 5 min) — 「街区が成熟した頃」
///
/// 30 min ベンチ (1800 sec) では、最序盤に建てた家のうち条件を満たすものが
/// 終盤近くで Highrise 化し、見栄えとしては数棟確認できる想定。
pub fn tier_dwell_required_ticks(target: HouseTier) -> u64 {
    match target {
        HouseTier::Cottage => 0,
        HouseTier::Apartment => 600,
        HouseTier::Highrise => 3000,
    }
}

/// 周辺条件 (`target`) と築年数を合わせた **実効 Tier**。
///
/// 周辺条件を満たしていても、築年数が足りなければ一段下の Tier に留まる。
/// 「条件は揃った、あとは時間が必要」という SimCity 的な熟成感を作る。
///
/// **設計判断**: 「条件が連続維持されたか」の追跡はせず、**築年数のみ**で
/// 判定する。シンプルで純関数のままなのが利点。「条件が揺れても、家自体が
/// 古ければ昇格できる」副作用があるが、Idle ゲームの粒度では問題にならない。
pub fn effective_house_tier(target: HouseTier, age_ticks: u64) -> HouseTier {
    if matches!(target, HouseTier::Highrise)
        && age_ticks >= tier_dwell_required_ticks(HouseTier::Highrise)
    {
        return HouseTier::Highrise;
    }
    if !matches!(target, HouseTier::Cottage)
        && age_ticks >= tier_dwell_required_ticks(HouseTier::Apartment)
    {
        return HouseTier::Apartment;
    }
    HouseTier::Cottage
}

/// セル `(x, y)` の House 実効 Tier を返す convenience。
/// 描画と `should_show_aviation_light` から共通で使う。
pub fn effective_tier_at(city: &City, x: usize, y: usize) -> HouseTier {
    let target = house_tier_for(gather_house_neighborhood(city, x, y));
    let age = city.tick.saturating_sub(city.built_at_tick[y][x]);
    effective_house_tier(target, age)
}

/// 建物の寿命倍率 (×100 整数で表現)。1.0 = 標準カーブ、2.0 = 倍長持ち。
///
/// **u32::MAX = 老いない**。Park / Road は永続資産扱い (= スクラップビルドの
/// 対象外、街の骨格として残る)。Outpost は Rock 解禁が終わったら
/// `auto_demolish` が拾うので寿命は短くて構わない。
///
/// **HouseTier 別**:
/// - Cottage: 標準 (100) — 築 5 min で 50% に劣化
/// - Apartment: 2.5× (250) — 築 12.5 min で 50% に劣化
/// - Highrise: 4.0× (400) — 築 20 min で 50% に劣化
///
/// → 高 Tier は「育てるのに時間がかかる代わりに長く儲かる」。再建サイクルの
/// 周期が長くなり、終盤の街が落ち着いて見える効果も狙う。
pub fn lifespan_x100(building: Building, tier: Option<HouseTier>) -> u32 {
    match (building, tier) {
        (Building::House, Some(HouseTier::Highrise)) => 400,
        (Building::House, Some(HouseTier::Apartment)) => 250,
        (Building::House, _) => 100,
        // Workshop / Shop は Tier 概念がない。中庸の長寿で「街の骨格」感を保つ。
        (Building::Workshop, _) => 200,
        (Building::Shop, _) => 220,
        // インフラと緑地は不老。
        (Building::Park, _) => u32::MAX,
        (Building::Road, _) => u32::MAX,
        // Outpost は使い捨て (Rock 解禁後に auto_demolish される前提)。
        (Building::Outpost, _) => 100,
    }
}

/// 築年数による出力倍率を **‰ (千分率)** で返す。500 = 0.5 倍、1000 = 等倍。
///
/// **カーブ**:
/// - scaled_age < 600 (= 1 min): 1000 (フル出力)
/// - 600 <= scaled_age < 3000 (= 1〜5 min): 1000 → 500 に線形低下
/// - scaled_age >= 3000: 500 (下限、idle 健全性のため 0 にしない)
///
/// `scaled_age = age_ticks * 100 / lifespan_x100`。lifespan が大きいほど
/// scaled_age が小さくなり、老朽化が遅延する。
///
/// **例**: lifespan=400 (Highrise) で age=2400 (4 min) → scaled_age=600 → 1000。
/// つまり 4 分目までフル出力。10 min (6000 ticks) で scaled_age=1500 → 約 800。
///
/// `lifespan_x100 == u32::MAX` は不老建物 (Park/Road)。常に 1000 を返す。
pub fn aging_factor_per_mille(age_ticks: u64, lifespan_x100: u32) -> u32 {
    if lifespan_x100 == u32::MAX || lifespan_x100 == 0 {
        return 1000;
    }
    // age * 100 / lifespan で寿命補正後の年齢に。
    let scaled_age = (age_ticks.saturating_mul(100)) / (lifespan_x100 as u64);
    if scaled_age < 600 {
        return 1000;
    }
    if scaled_age >= 3000 {
        return 500;
    }
    // 600..3000 の線形補間。1000 → 500 に減少。
    let t = scaled_age - 600;
    let span: u64 = 3000 - 600;
    (1000 - (t * 500) / span) as u32
}


/// 航空標識: 高層ビル屋上の赤い点滅灯を出すか。純関数。
///
/// **意図**: rebels-in-the-sky 風の「都市感」最終スパイス。摩天楼が密集
/// した時だけ航空法上の障害灯を点滅させる演出で、Tier 4 まで育てきった
/// プレイヤーへの視覚報酬。
///
/// **条件**:
///   1. 夜間 (DayPhase::Night) のみ — Dusk は除外 (まだ日が残っている)
///   2. 周囲 4-近傍に Highrise が **2 軒以上** ある
///   3. 1.5 秒周期で点滅 (15 ticks に 1 度くらい)
///
/// 「2 軒以上」にしているのは、Highrise が単独だと「ビル建てただけ」感が
/// 強く演出が浮くため。クラスタ化して初めて都市的な絵面になる。
pub fn should_show_aviation_light(city: &City, x: usize, y: usize, tick: u64) -> bool {
    if !matches!(day_phase(tick), DayPhase::Night) {
        return false;
    }
    // 1.5 秒 (15 ticks) 周期、ON 区間は 0.5 秒 (5 ticks)。
    let blink_phase = tick % 15;
    if blink_phase >= 5 {
        return false;
    }
    // 周囲の Highrise 数を数える (4-近傍)。
    let mut highrise_neighbors = 0u32;
    for (dx, dy) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
        let nx = x as i32 + dx;
        let ny = y as i32 + dy;
        if nx < 0 || ny < 0 || nx >= GRID_W as i32 || ny >= GRID_H as i32 {
            continue;
        }
        let nx = nx as usize;
        let ny = ny as usize;
        if matches!(city.tile(nx, ny), Tile::Built(Building::House)) {
            let neighbor_tier = effective_tier_at(city, nx, ny);
            if matches!(neighbor_tier, HouseTier::Highrise) {
                highrise_neighbors += 1;
            }
        }
    }
    highrise_neighbors >= 2
}

/// 店舗の段階レベル — 隣接アクティブ House 数 + 道路接続で評価。
/// 賑わいの可視化用。アクティブで Mid 以上の住宅が近いとプレミアム。
pub fn shop_level(city: &City, x: usize, y: usize) -> ShopLevel {
    if !shop_is_active(city, x, y) {
        return ShopLevel::Idle;
    }
    let mut customers = 0u32;
    for cy in 0..GRID_H {
        for cx in 0..GRID_W {
            if matches!(city.tile(cx, cy), Tile::Built(Building::House)) {
                let dx = cx as i32 - x as i32;
                let dy = cy as i32 - y as i32;
                if dx.abs() + dy.abs() <= 3 {
                    customers += 1;
                }
            }
        }
    }
    match customers {
        0 => ShopLevel::Idle,
        1 | 2 => ShopLevel::Basic,
        3..=5 => ShopLevel::Busy,
        _ => ShopLevel::Premium,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShopLevel {
    Idle,    // 非アクティブ (灰)
    Basic,   // 道路はあるが客は少ない
    Busy,    // 標準的な賑わい
    Premium, // 大繁盛 (★付き表示)
}

/// Workshop は隣接 House (労働力) と Road 接続の両方が必要。
/// Shop と違って距離は隣接のみ — 「働き手は徒歩圏内から来る」感を出す。
pub(super) fn workshop_is_active(city: &City, wx: usize, wy: usize) -> bool {
    has_neighbor_kind(city, wx, wy, Building::Road)
        && has_neighbor_kind(city, wx, wy, Building::House)
}

/// A shop earns money if it has a road neighbor *and* a house within
/// Manhattan distance 3.  This makes Tier-1's random scattering punishable
/// without being unwinnable.
pub(super) fn shop_is_active(city: &City, sx: usize, sy: usize) -> bool {
    if !has_neighbor_kind(city, sx, sy, Building::Road) {
        return false;
    }
    for y in 0..GRID_H {
        for x in 0..GRID_W {
            if !matches!(city.tile(x, y), Tile::Built(Building::House)) {
                continue;
            }
            let dx = x as i32 - sx as i32;
            let dy = y as i32 - sy as i32;
            if dx.abs() + dy.abs() <= 3 {
                return true;
            }
        }
    }
    false
}

fn has_neighbor_kind(city: &City, x: usize, y: usize, kind: Building) -> bool {
    let dirs: [(i32, i32); 4] = [(-1, 0), (1, 0), (0, -1), (0, 1)];
    for (dx, dy) in dirs {
        let nx = x as i32 + dx;
        let ny = y as i32 + dy;
        if nx < 0 || ny < 0 || nx >= GRID_W as i32 || ny >= GRID_H as i32 {
            continue;
        }
        if let Tile::Built(b) = city.tile(nx as usize, ny as usize) {
            if *b == kind {
                return true;
            }
        }
    }
    false
}

/// 中央からの Manhattan 距離。撤去コスト計算と AI スコアリングで共通利用。
pub fn distance_from_center(x: usize, y: usize) -> u32 {
    let cx = (GRID_W / 2) as i32;
    let cy = (GRID_H / 2) as i32;
    let dx = (x as i32 - cx).abs();
    let dy = (y as i32 - cy).abs();
    (dx + dy) as u32
}

/// 撤去コスト (cash)。中央からの Manhattan 距離で 2 次関数的に上がる。
///
/// 公式: `50 + d² * 5`
/// - 中央 (d=0): $50  ← 街の中心は撤去しやすい
/// - コア端 (d=5): $50 + 125 = $175
/// - 中間 (d=10): $50 + 500 = $550
/// - 外周 (d=20): $50 + 2000 = $2050
///
/// d² 曲線にすることで、外側に建てた建物を「気軽に撤去」できなくなる。
/// プレイヤーは「市域拡張は慎重に」というプレッシャーを受ける。
pub fn demolish_cost(x: usize, y: usize) -> i64 {
    let d = distance_from_center(x, y) as i64;
    50 + d * d * 5
}

/// セル (x, y) の建物を撤去する。Built タイル限定 (Construction や Empty は false)。
/// コストは `demolish_cost(x, y)` で計算。地形 (terrain) は変更しない (= Plain 化済み
/// だった整地後の Rock セルは Plain のまま、再露出はしない)。
///
/// 戻り値: 撤去成功時 true。
///   - 撤去済み / 建設中 / 空セル: false
///   - 現金不足: false
pub fn demolish_at(city: &mut City, x: usize, y: usize) -> bool {
    if x >= GRID_W || y >= GRID_H {
        return false;
    }
    let kind = match city.tile(x, y) {
        Tile::Built(b) => *b,
        _ => return false,
    };
    let cost = demolish_cost(x, y);
    if city.cash < cost {
        city.push_event(format!(
            "❌ 撤去には ${} 必要 (現在 ${})",
            cost, city.cash
        ));
        return false;
    }
    city.cash -= cost;
    city.cash_spent_total += cost;
    city.grid[y][x] = Tile::Empty;
    // 既存フラッシュをリセット (古い完成フラッシュが残ると違和感)。
    city.completion_flash_until[y][x] = 0;
    city.payout_flash_until[y][x] = 0;
    // 築年数も初期化 — 同セルに新築が入ったら advance_construction で再設定される。
    city.built_at_tick[y][x] = 0;
    city.push_event(format!(
        "🗑 ({},{}) {} を撤去 -${}",
        x,
        y,
        building_name(kind),
        cost
    ));
    true
}

// ── AI 撤去判断 (Phase A: 完全自動化) ───────────────────────────
//
// Idle Metropolis の哲学「CPU が街を作る、プレイヤーは方向だけ」に沿って、
// 撤去判断も CPU 側に委譲する。`auto_strategy_actions` から戦略ごとの
// 周期 (`automation_policy(s).auto_demolish_period_ticks`) で発火し、
// **最も無駄な建物**を 1 つ選んで撤去する。手動撤去ボタン / モードは
// すべて廃止されたため、プレイヤーは戦略選択でしか撤去挙動を変えられない。
//
// ## 評価関数の設計
//
// 「無駄な建物」の定義は複数あり得る:
//   1. 機能不全 (inactive Shop / Workshop)
//   2. 役目を終えた (Outpost で周囲に Rock がもう無い)
//   3. 戦略的にミスマッチ (Cottage in core, でも income に貢献している…)
//
// すべて足し合わせた **wastefulness score** から **demolish_cost / 10** を引いた
// 値を「撤去で得をする度合い」とし、最大値の建物を選ぶ。負の値しか出ない場合
// (= どの撤去も損) は何もしない。
//
// この設計だと:
// - 中央のミスは積極的に撤去される (cost 安い)
// - 外周の建設ミスは滅多に撤去されない (cost 高い、d² 曲線が AI を萎縮させる)
//   → プレイヤーが「外側に建てる前に再考しろ」と誘導される

/// 撤去候補の優先度スコア (高いほど撤去すべき)。
///
/// **正の値** = wasted potential、**負の値** = useful asset。
/// `auto_demolish` が「最大スコア > 0」の建物を 1 つ選んで取り壊す。
///
/// 設計判断: スコアの絶対値は重要でなく、**相対順位**のみが意味を持つ。
/// バランス調整時はここの数字を弄る。
fn wastefulness_score(city: &City, x: usize, y: usize) -> Option<i64> {
    let kind = match city.tile(x, y) {
        Tile::Built(b) => *b,
        _ => return None,
    };

    let mut score: i64 = 0;
    match kind {
        Building::Shop => {
            // 非アクティブ Shop は最大の無駄。
            if !shop_is_active(city, x, y) {
                score += 250;
            }
        }
        Building::Workshop => {
            if !workshop_is_active(city, x, y) {
                score += 200;
            }
        }
        Building::Outpost => {
            // 役目を終えた Outpost (周囲 4-近傍に Rock が無い) は撤去候補。
            // 周囲に Rock があるならまだ仕事中なので score 0 (== 撤去しない)。
            if count_rock_neighbors(city, x, y) == 0 {
                score += 300;
            }
        }
        Building::House => {
            // House はほぼ常に有用 — 唯一例外は「孤立 Cottage」(Road 接続無し)。
            // ただし Cottage でも +1$/2 の収入源なので慎重に。スコア控えめ。
            let stats = gather_house_neighborhood(city, x, y);
            if stats.n_road_adj == 0 && stats.n_house_within_3 == 0 {
                score += 80;
            }
        }
        Building::Road => {
            // 行き止まりの孤立 Road (隣接 Built が 0)。実害は小さいが見た目が悪い。
            if !has_any_neighbor_built(city, x, y) {
                score += 60;
            }
        }
        Building::Park => {
            // Park は Manhattan 4 以内に House が無いと触媒として機能しない。
            if !has_house_within(city, x, y, 4) {
                score += 100;
            }
        }
    }

    // 老朽化ボーナス: 寿命が尽きた (= aging factor が下限の 500‰ 付近) 建物は
    // 「再建すれば収入が回復する」候補。とくに Tier が低いまま放置された
    // Cottage / inactive な Shop / Workshop は積極的に世代交代したい。
    // 不老建物 (Park/Road) は age 関係なくスコア加算しない (= 永続資産扱い)。
    if city.built_at_tick[y][x] != 0 {
        let age = city.tick.saturating_sub(city.built_at_tick[y][x]);
        let tier_opt = if matches!(kind, Building::House) {
            Some(effective_house_tier(
                house_tier_for(gather_house_neighborhood(city, x, y)),
                age,
            ))
        } else {
            None
        };
        let lifespan = lifespan_x100(kind, tier_opt);
        let factor = aging_factor_per_mille(age, lifespan);
        // 寿命下限 (=500‰) に達した建物は再建価値あり。
        // 750..1000 の中間域では加点しない (= 「まだ働ける」ので維持)。
        if factor <= 600 {
            score += 80;
        } else if factor <= 750 {
            score += 30;
        }
    }

    if score == 0 {
        return None;
    }

    // コスト割引: 撤去には金がかかる。コスト/10 をスコアから引く。
    // 中央 ($50) なら -5、外周 ($2050) なら -205。
    // → 同じ「inactive Shop」でも、中央なら net +245、外周なら net +45。
    //   外周は大幅にスコアが落ちるが、まだプラスなので最後の手段として撤去対象に
    //   なり得る。AI が外周を「やむなく撤去」する挙動が出る。
    let cost_penalty = demolish_cost(x, y) / 10;
    Some(score - cost_penalty)
}

fn has_any_neighbor_built(city: &City, x: usize, y: usize) -> bool {
    for (dx, dy) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
        let nx = x as i32 + dx;
        let ny = y as i32 + dy;
        if nx < 0 || ny < 0 || nx >= GRID_W as i32 || ny >= GRID_H as i32 {
            continue;
        }
        if matches!(city.tile(nx as usize, ny as usize), Tile::Built(_)) {
            return true;
        }
    }
    false
}

fn has_house_within(city: &City, x: usize, y: usize, dist: u32) -> bool {
    for hy in 0..GRID_H {
        for hx in 0..GRID_W {
            if !matches!(city.tile(hx, hy), Tile::Built(Building::House)) {
                continue;
            }
            let dx = (hx as i32 - x as i32).unsigned_abs();
            let dy = (hy as i32 - y as i32).unsigned_abs();
            if dx + dy <= dist {
                return true;
            }
        }
    }
    false
}

/// 全 Built タイルから最高スコアの撤去候補を返す。
///
/// 戻り値: `(x, y, score)`。何も無い時は None。
/// ここで返ってくる候補は「スコア > 0 かつコストを引いてもプラス」のもの限定。
pub fn auto_demolish_target(city: &City) -> Option<(usize, usize, i64)> {
    let mut best: Option<(usize, usize, i64)> = None;
    for y in 0..GRID_H {
        for x in 0..GRID_W {
            if let Some(score) = wastefulness_score(city, x, y) {
                if score <= 0 {
                    continue;
                }
                let better = match best {
                    None => true,
                    Some((_, _, prev)) => score > prev,
                };
                if better {
                    best = Some((x, y, score));
                }
            }
        }
    }
    best
}

/// AI に撤去判断を一任する。最高スコアの建物を 1 つ撤去する。
/// 候補が無い / 現金不足の時は **無音で false** を返す (tick 駆動の自動発火
/// から呼ばれるためログ spam を避ける)。成功時は `demolish_at` 内で
/// "🗑 ... を撤去" のイベントが既に記録される。
pub fn auto_demolish(city: &mut City) -> bool {
    let Some((x, y, _score)) = auto_demolish_target(city) else {
        return false;
    };
    let cost = demolish_cost(x, y);
    if city.cash < cost {
        return false;
    }
    demolish_at(city, x, y)
}

/// 開拓機材 (Outpost) を派遣する。
///
/// **配置ロジック (idle ゲームの哲学に沿って自動)**:
///   1. 既存の建物 (Built / Construction) に隣接する Empty セル
///   2. かつ自身の 4-近傍に Rock セルがある (= 設置すれば実際に Rock が解禁される)
///   3. のうち、最もスコアが高いものを選ぶ
///
/// スコア = 解禁できる Rock 数 (近傍の Rock 数)。
/// これで「街の境界の岩壁の前」に自動配置される。
///
/// プレイヤー視点: 「機材派遣ボタンを押す → AI が良い場所に置いてくれる」。
/// Idle Metropolis の「CPU が考える、プレイヤーは方向だけ」哲学に整合。
pub fn dispatch_outpost(city: &mut City) -> bool {
    let cost = Building::Outpost.cost();
    if city.cash < cost {
        return false;
    }

    let Some((bx, by)) = best_outpost_placement(city) else {
        return false;
    };

    if !start_construction(city, bx, by, Building::Outpost) {
        return false;
    }
    city.outposts_dispatched_total = city.outposts_dispatched_total.saturating_add(1);
    let unlocked = count_rock_neighbors(city, bx, by);
    city.push_event(format!(
        "🚧 ({},{}) 開拓機材を自動派遣 — 周囲 {} 岩盤を解禁予定",
        bx, by, unlocked
    ));
    true
}

/// `dispatch_outpost` の候補探索。最も多くの Rock を解禁できる Empty セルを返す。
///
/// 「街の連続性」要件は Manhattan 距離 2 (= 4-近傍 + その近傍) で見る。AI が
/// 4-近傍を埋め切るスピードに対し、距離 2 のバッファを持たせることで Empty
/// Plain な候補が安定して残る。距離 2 でも「街から数歩」の位置なので、Rock
/// 解禁後に AI が自然に拡張してくる位置として妥当。
fn best_outpost_placement(city: &City) -> Option<(usize, usize)> {
    let mut best: Option<(usize, usize, u32)> = None;
    for y in 0..GRID_H {
        for x in 0..GRID_W {
            if !matches!(city.tile(x, y), Tile::Empty) {
                continue;
            }
            // 機材は **整地不要セル (Plain)** にしか置かない。Forest/Wasteland を
            // 候補に入れると `dispatch_outpost` → `start_construction` のフローで
            // 整地だけ走って Outpost が建たない silent failure になる
            // (Codex review #100: P1)。
            let terrain = city.terrain_at(x, y);
            if !terrain.buildable() || terrain.needs_clearing() {
                continue;
            }
            // 既存建造物 (Built or Construction) から Manhattan 距離 2 以内に
            // 居ること = 街の周辺に居る。距離 1 (4-近傍) だと AI のフロンティア
            // 拡張で候補がすぐ消える。
            if !has_built_within_distance(city, x, y, 4) {
                continue;
            }
            // 近傍 Rock 数 = 解禁スコア。
            let unlocked = count_rock_neighbors(city, x, y);
            if unlocked == 0 {
                continue;
            }
            let better = match best {
                None => true,
                Some((_, _, prev)) => unlocked > prev,
            };
            if better {
                best = Some((x, y, unlocked));
            }
        }
    }
    best.map(|(x, y, _)| (x, y))
}

/// (x, y) から Manhattan 距離 dist 以内に Built/Construction セルが存在するか。
fn has_built_within_distance(city: &City, x: usize, y: usize, dist: i32) -> bool {
    let xi = x as i32;
    let yi = y as i32;
    for dy in -dist..=dist {
        for dx in -dist..=dist {
            if dx.abs() + dy.abs() > dist || (dx == 0 && dy == 0) {
                continue;
            }
            let nx = xi + dx;
            let ny = yi + dy;
            if nx < 0 || ny < 0 || nx >= GRID_W as i32 || ny >= GRID_H as i32 {
                continue;
            }
            if matches!(
                city.tile(nx as usize, ny as usize),
                Tile::Built(_) | Tile::Construction { .. }
            ) {
                return true;
            }
        }
    }
    false
}

fn count_rock_neighbors(city: &City, x: usize, y: usize) -> u32 {
    let mut n = 0;
    for (dx, dy) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
        let nx = x as i32 + dx;
        let ny = y as i32 + dy;
        if nx < 0 || ny < 0 || nx >= GRID_W as i32 || ny >= GRID_H as i32 {
            continue;
        }
        if matches!(
            city.terrain_at(nx as usize, ny as usize),
            super::terrain::Terrain::Rock
        ) {
            n += 1;
        }
    }
    n
}

/// Try to upgrade the AI brain.  Returns true on success.
pub fn upgrade_ai(city: &mut City) -> bool {
    let Some(next) = city.ai_tier.next() else {
        return false;
    };
    let cost = next.upgrade_cost();
    if city.cash < cost {
        return false;
    }
    city.cash -= cost;
    city.cash_spent_total += cost;
    city.ai_tier = next;
    city.push_event(format!("⚡ CPU進化 → {}", next.name()));
    true
}

/// Cost of hiring the next worker (i.e. promoting `workers` from N to N+1).
///
/// Returns `None` when the worker cap (`MAX_WORKERS`) is already reached or
/// the doubling cost would overflow `i64`.  Callers must treat `None` as
/// "this action is currently unavailable" rather than free.
///
/// The original `100 * (1 << (workers - 1))` form was unsafe in release
/// builds: corrupted state with `workers >= 64` would silently wrap to
/// zero/negative cost (Codex P2).  `checked_shl` returns `None` cleanly
/// for any out-of-range shift, so we propagate the failure instead of
/// computing a bogus price.
pub fn hire_worker_cost(workers: u32) -> Option<i64> {
    if workers == 0 || workers >= MAX_WORKERS {
        return None;
    }
    100i64.checked_shl(workers - 1)
}

/// Hire one more worker.  Cost doubles per current count, capped at
/// `MAX_WORKERS`.
pub fn hire_worker(city: &mut City) -> bool {
    let Some(cost) = hire_worker_cost(city.workers) else {
        return false;
    };
    if city.cash < cost {
        return false;
    }
    city.cash -= cost;
    city.cash_spent_total += cost;
    city.workers += 1;
    city.push_event(format!("➕ 作業員雇用 → {}人", city.workers));
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── House evolution rule (DESIGN.md §4) ────────────────
    //
    // user contribution が満たすべき大まかな性質をテストで明文化。
    // 中の数字は「正解」というより「方向性」のチェック。書いたルールで
    // 全部通ればまず妥当。

    fn nbh(n_road: u32, n_workshop: u32, n_shop: u32, n_house: u32) -> HouseNeighborhood {
        HouseNeighborhood {
            n_road_adj: n_road,
            n_workshop_within_5: n_workshop,
            n_shop_within_5: n_shop,
            n_house_within_3: n_house,
            n_park_within_4: 0,
        }
    }

    /// Park 寄与版: 既存テストとの互換性を保ちつつ Park だけ追加で渡す。
    fn nbh_with_park(
        n_road: u32,
        n_workshop: u32,
        n_shop: u32,
        n_house: u32,
        n_park: u32,
    ) -> HouseNeighborhood {
        HouseNeighborhood {
            n_road_adj: n_road,
            n_workshop_within_5: n_workshop,
            n_shop_within_5: n_shop,
            n_house_within_3: n_house,
            n_park_within_4: n_park,
        }
    }

    /// Park 単体でも Apartment まで育つ (Eco 戦略の核)。
    #[test]
    fn park_alone_can_lift_to_apartment() {
        // road=1, workshop=0, shop=0, house=1, park=1
        // park_density = ceil(1/2) = 1 → economic_density = 1 → Apartment OK
        assert_eq!(
            house_tier_for(nbh_with_park(1, 0, 0, 1, 1)),
            HouseTier::Apartment
        );
    }

    /// Park 2 つ + 商業 1 つで Highrise に届く (Eco × 高密度パスの保証)。
    #[test]
    fn park_plus_shop_can_reach_highrise() {
        // road=2, shop=1, house=3, park=2
        // park_density = 1, economic_density = 1+1=2 → Highrise OK
        assert_eq!(
            house_tier_for(nbh_with_park(2, 0, 1, 3, 2)),
            HouseTier::Highrise
        );
    }

    /// 完全孤立した House は Cottage のまま。
    /// (#[ignore] を外してから実装すると、書いた式で通るか確認できる)
    #[test]
    fn isolated_house_is_cottage() {
        assert_eq!(house_tier_for(nbh(0, 0, 0, 0)), HouseTier::Cottage);
    }

    /// インフラだけ届いている (Road あり、Shop / Workshop ゼロ) は
    /// Highrise にはならない — 「商業が回っていない」ため。
    #[test]
    fn road_only_does_not_reach_highrise() {
        assert_ne!(house_tier_for(nbh(2, 0, 0, 1)), HouseTier::Highrise);
    }

    /// Road + Workshop + Shop が揃い周囲に House もいる豊かなゾーンは
    /// Highrise に到達する。
    #[test]
    fn full_economy_reaches_highrise() {
        assert_eq!(house_tier_for(nbh(2, 2, 2, 4)), HouseTier::Highrise);
    }

    /// 単調性: 「条件が悪くなって Tier が上がる」のは想定外。
    /// 引数の各成分を増やしても Tier は下がらない (>= で良い)。
    #[test]
    fn tier_is_monotone() {
        let lo = house_tier_for(nbh(1, 0, 0, 1));
        let hi = house_tier_for(nbh(2, 1, 1, 3));
        assert!(hi >= lo, "richer neighborhood should not produce a worse tier");
    }

    #[test]
    fn day_phase_cycles_through_day_dusk_night() {
        // 1 日 = 600 ticks。0=Day 開始、240=Dusk 開始、300=Night 開始。
        assert_eq!(day_phase(0), DayPhase::Day);
        assert_eq!(day_phase(100), DayPhase::Day);
        assert_eq!(day_phase(240), DayPhase::Dusk);
        assert_eq!(day_phase(300), DayPhase::Night);
        assert_eq!(day_phase(450), DayPhase::Night);
        assert_eq!(day_phase(540), DayPhase::Dusk); // dawn
        // 次の周期の開始も同じ。
        assert_eq!(day_phase(600), DayPhase::Day);
        assert_eq!(day_phase(1200), DayPhase::Day);
    }

    #[test]
    fn day_progress_monotone_within_phase() {
        let p0 = day_progress(0);
        let p1 = day_progress(120);
        let p2 = day_progress(239);
        assert!(p0 < p1 && p1 < p2);
        assert!(p2 <= 1.0);
    }

    /// 暗化係数が単調 (Day=0 < Dusk < Night)。
    #[test]
    fn dim_factor_is_monotone() {
        assert!(DayPhase::Day.dim_factor() < DayPhase::Dusk.dim_factor());
        assert!(DayPhase::Dusk.dim_factor() < DayPhase::Night.dim_factor());
    }

    /// dim_rgb は 0 で恒等、255 で完全黒。
    #[test]
    fn dim_rgb_endpoints() {
        assert_eq!(dim_rgb(100, 200, 50, 0), (100, 200, 50));
        assert_eq!(dim_rgb(255, 255, 255, 255), (0, 0, 0));
    }

    #[test]
    fn empty_city_earns_nothing() {
        let city = City::new();
        assert_eq!(compute_income_per_sec(&city), 0);
    }

    #[test]
    fn finished_houses_earn_residential_tax() {
        let mut city = City::new();
        city.set_tile(0, 0, Tile::Built(Building::House));
        // 1 Cottage = 50¢/sec → $0 だが死スパイラル防止フォールバックで $1。
        assert_eq!(compute_income_per_sec(&city), 1);
        city.set_tile(1, 0, Tile::Built(Building::House));
        // 2 Cottages = 100¢ = $1。フォールバック不要。
        assert_eq!(compute_income_per_sec(&city), 1);
        city.set_tile(2, 0, Tile::Built(Building::House));
        // 3 Cottages = 150¢ = $1 (整数切り捨て、旧 ceil(3/2)=2 から変更)。
        // Tier-aware 収入は per-cent で正確に積算するので、半端は丸めで吸収する。
        assert_eq!(compute_income_per_sec(&city), 1);
        city.set_tile(3, 0, Tile::Built(Building::House));
        // 4 Cottages = 200¢ = $2。
        assert_eq!(compute_income_per_sec(&city), 2);
    }

    #[test]
    fn shop_without_road_earns_nothing() {
        let mut city = City::new();
        city.set_tile(5, 5, Tile::Built(Building::Shop));
        city.set_tile(5, 6, Tile::Built(Building::House));
        // Shop is inactive (no road neighbor) → only the house's $1 counts.
        assert_eq!(compute_income_per_sec(&city), 1);
    }

    /// Workshop は隣接 House と Road が両方必要。片方だけでは inactive。
    #[test]
    fn workshop_needs_road_and_house_neighbors() {
        let mut city = City::new();
        // (5,5) に Workshop。隣接 Road のみ → inactive。
        city.set_tile(5, 5, Tile::Built(Building::Workshop));
        city.set_tile(5, 4, Tile::Built(Building::Road));
        // House がないので fallback も働かず、Workshop も inactive で $0。
        assert_eq!(compute_income_per_sec(&city), 0);

        // 隣接 House を追加 → Workshop activate。
        city.set_tile(5, 6, Tile::Built(Building::House));
        // House (5,6): Cottage = 50¢
        // Workshop: active → 100¢
        // Total: 150¢ = $1 (整数切り捨て)。
        assert_eq!(compute_income_per_sec(&city), 1);
    }

    /// 要整地の地形 (Forest) に建てようとすると、まず整地工程が起きる。
    /// 整地完了後に terrain が Plain に書き換わる。
    #[test]
    fn forest_triggers_clearing_then_plain() {
        let mut city = City::new();
        city.cash = 1000;
        // (5,5) を強制的に Forest に。
        city.terrain[5][5] = super::super::terrain::Terrain::Forest;
        let ok = start_construction(&mut city, 5, 5, Building::House);
        assert!(ok, "start_construction should succeed (triggers clearing)");
        // 直後は Clearing タイル。
        assert!(matches!(city.tile(5, 5), Tile::Clearing { .. }));
        // 整地時間 (Forest = 60 ticks) を進めると Empty に戻り terrain が Plain に。
        tick(&mut city, 60);
        assert!(matches!(city.tile(5, 5), Tile::Empty));
        assert_eq!(
            city.terrain_at(5, 5),
            super::super::terrain::Terrain::Plain,
            "clearing should overwrite terrain to Plain"
        );
    }

    /// Eco 戦略は collection-time builder of Forest avoidance。
    /// strategy_info の `speed_bonus_pct` が負、`income_penalty_pct` が正。
    #[test]
    fn eco_strategy_has_negative_speed_and_positive_income() {
        let info = strategy_info(Strategy::Eco);
        assert!(info.speed_bonus_pct < 0, "Eco builds slower");
        assert!(info.income_penalty_pct > 0, "Eco earns slightly more");
    }

    /// Eco 戦略時、Tech と同じく定数倍が income に効く。+5% で 1 軒 → 1$/s が
    /// 維持される (床保護)。
    #[test]
    fn eco_income_bonus_does_not_break_floor() {
        let mut city = City::new();
        city.strategy = Strategy::Eco;
        city.set_tile(0, 0, Tile::Built(Building::House));
        // (1+1)/2 = 1, +5% = 1.05 → floor で 1。床保護で 1 を下回らない。
        assert!(compute_income_per_sec(&city) >= 1);
    }

    /// Wasteland の整地は Forest より速く安い (terrain.rs のバランスに合う)。
    #[test]
    fn wasteland_clearing_is_cheaper_and_faster() {
        use super::super::terrain::Terrain;
        assert!(Terrain::Wasteland.clearing_ticks() < Terrain::Forest.clearing_ticks());
        assert!(Terrain::Wasteland.clearing_cost() < Terrain::Forest.clearing_cost());
    }

    /// Workshop が近くにあると House は Apartment になる (Workshop が経済刺激源)。
    #[test]
    fn workshop_promotes_nearby_house_to_apartment() {
        let mut city = City::new();
        city.set_tile(1, 1, Tile::Built(Building::House));
        city.set_tile(0, 1, Tile::Built(Building::Road));
        // Workshop at (3,1): Manhattan distance 2 from (1,1)。
        city.set_tile(3, 1, Tile::Built(Building::Workshop));
        let tier = house_tier_for(gather_house_neighborhood(&city, 1, 1));
        assert_eq!(tier, HouseTier::Apartment);
    }

    #[test]
    fn shop_with_road_and_house_earns() {
        let mut city = City::new();
        city.set_tile(5, 5, Tile::Built(Building::Shop));
        city.set_tile(5, 4, Tile::Built(Building::Road));
        city.set_tile(5, 6, Tile::Built(Building::House));
        // Shop active = 200¢ + Cottage 50¢ = 250¢ = $2 (整数切り捨て、旧 $3 から変更)。
        assert_eq!(compute_income_per_sec(&city), 2);
    }

    /// HouseTier は描画専用 — gather → tier_for で派生値が取れる。
    /// 道路接続 + Shop が距離 5 以内なら Apartment になる (描画切替の根拠)。
    #[test]
    fn house_with_road_and_shop_renders_as_apartment() {
        let mut city = City::new();
        city.set_tile(1, 1, Tile::Built(Building::House));
        city.set_tile(0, 1, Tile::Built(Building::Road));
        city.set_tile(3, 1, Tile::Built(Building::Shop));
        let tier = house_tier_for(gather_house_neighborhood(&city, 1, 1));
        assert_eq!(tier, HouseTier::Apartment);
    }

    /// 道路 + 周囲 House だけでは Cottage のまま (商業が来ないとリッチ化しない)。
    #[test]
    fn road_and_houses_alone_stays_cottage_visually() {
        let mut city = City::new();
        city.set_tile(1, 1, Tile::Built(Building::House));
        city.set_tile(0, 1, Tile::Built(Building::Road));
        city.set_tile(2, 1, Tile::Built(Building::House));
        city.set_tile(1, 2, Tile::Built(Building::House));
        let tier = house_tier_for(gather_house_neighborhood(&city, 1, 1));
        assert_eq!(tier, HouseTier::Cottage);
    }

    #[test]
    fn construction_finishes_after_build_ticks() {
        let mut city = City::new();
        // 中央コア (d=0) を使う。デフォルト世界生成では (0,0) は外側で
        // ほぼ確実に Rock になるため、座標を中央に寄せる。
        let cx = GRID_W / 2;
        let cy = GRID_H / 2;
        // 中央セルの地形を強制 Plain にして、Forest/Wasteland 由来の Clearing が
        // 紛れ込まないようにする (テストは「Road が build_ticks で完成」が論点)。
        city.terrain[cy][cx] = super::super::terrain::Terrain::Plain;
        let ok = start_construction(&mut city, cx, cy, Building::Road);
        assert!(ok);
        assert!(matches!(
            city.tile(cx, cy),
            Tile::Construction { .. }
        ));
        tick(&mut city, Building::Road.build_ticks());
        assert!(matches!(city.tile(cx, cy), Tile::Built(Building::Road)));
    }

    #[test]
    fn cant_afford_means_no_construction() {
        let mut city = City::new();
        city.cash = 5; // less than any building
        // テスト中央セルを Plain に固定 (Rock だと needs_clearing で別経路に入る)。
        let cx = GRID_W / 2;
        let cy = GRID_H / 2;
        city.terrain[cy][cx] = super::super::terrain::Terrain::Plain;
        assert!(!start_construction(&mut city, cx, cy, Building::House));
        assert_eq!(city.cash, 5);
    }

    #[test]
    fn hire_worker_cost_doubles_per_step() {
        assert_eq!(hire_worker_cost(1), Some(100));
        assert_eq!(hire_worker_cost(2), Some(200));
        assert_eq!(hire_worker_cost(3), Some(400));
        assert_eq!(hire_worker_cost(7), Some(6_400));
    }

    /// Reaching the cap returns None (no further hire) — but doesn't panic.
    #[test]
    fn hire_worker_cost_caps_at_max() {
        assert_eq!(hire_worker_cost(MAX_WORKERS), None);
        assert_eq!(hire_worker_cost(MAX_WORKERS + 1), None);
    }

    /// Pathological state values must not panic or wrap.  Codex P2 (#93):
    /// `100 * (1 << (workers - 1))` would UB-shift at workers >= 65.
    #[test]
    fn hire_worker_cost_handles_pathological_input() {
        // workers == 0 is meaningless (we always start at 1) — should be None
        assert_eq!(hire_worker_cost(0), None);
        // Far above any reasonable game state — clamps via MAX_WORKERS gate
        assert_eq!(hire_worker_cost(1_000), None);
        assert_eq!(hire_worker_cost(u32::MAX), None);
    }

    /// ティアが上がる瞬間に flash と event が発火する。
    #[test]
    fn tier_advance_triggers_flash_and_event() {
        let mut city = City::new();
        // 50 pop = 10 軒の House で Town 到達。
        for i in 0..10 {
            city.set_tile(i, 0, Tile::Built(Building::House));
        }
        assert_eq!(city.last_observed_tier, CityTier::Village);
        // 1 tick 進めれば detect_tier_advance が走る。
        tick(&mut city, 1);
        assert_eq!(city.last_observed_tier, CityTier::Town);
        assert!(city.tier_flash_until > city.tick);
        // イベントログの先頭にティア進化メッセージ。
        assert!(
            city.events.first().is_some_and(|e| e.contains("町")),
            "first event should mention 町, got {:?}",
            city.events.first()
        );
    }

    /// 追加 House でも同じティア内なら再発火しない (ログ汚染防止)。
    #[test]
    fn tier_does_not_re_trigger_within_same_tier() {
        let mut city = City::new();
        for i in 0..10 {
            city.set_tile(i, 0, Tile::Built(Building::House));
        }
        tick(&mut city, 1);
        let event_count_after_tier_event = city.events.len();
        // もう 1 軒追加 (まだ Town 範囲内: 55 pop)。
        city.set_tile(11, 0, Tile::Built(Building::House));
        tick(&mut city, 5);
        assert_eq!(
            city.events.len(),
            event_count_after_tier_event,
            "re-tick within same tier should not push another tier event"
        );
    }

    /// 撤去コストは中央で最小、外周で最大。d² 曲線が効いていることを確認。
    #[test]
    fn demolish_cost_scales_with_distance() {
        let cx = GRID_W / 2;
        let cy = GRID_H / 2;
        let center = demolish_cost(cx, cy);
        let edge = demolish_cost(0, 0);
        let mid = demolish_cost(cx, 0); // 中央列の上端
        assert_eq!(center, 50);
        // 外周は中央の 10 倍以上 (急峻なペナルティ)。
        assert!(
            edge >= center * 10,
            "outer cost ({}) should be ≥ 10× inner ({})",
            edge,
            center
        );
        // 中間は中央 < mid < edge。
        assert!(center < mid && mid < edge);
    }

    /// 撤去成功で Built → Empty に戻り、cash が引かれる。
    #[test]
    fn demolish_removes_built_tile() {
        let mut city = City::new();
        city.cash = 10_000;
        let cx = GRID_W / 2;
        let cy = GRID_H / 2;
        city.set_tile(cx, cy, Tile::Built(Building::House));
        let before_cash = city.cash;
        let cost = demolish_cost(cx, cy);
        assert!(demolish_at(&mut city, cx, cy));
        assert!(matches!(city.tile(cx, cy), Tile::Empty));
        assert_eq!(city.cash, before_cash - cost);
    }

    /// 建設中タイルは撤去対象外 (Construction は別ロジックで cancel すべき)。
    #[test]
    fn demolish_rejects_construction_tile() {
        let mut city = City::new();
        city.cash = 10_000;
        let cx = GRID_W / 2;
        let cy = GRID_H / 2;
        city.terrain[cy][cx] = super::super::terrain::Terrain::Plain;
        // 着工中。
        assert!(start_construction(&mut city, cx, cy, Building::House));
        let cash_before_demolish = city.cash;
        assert!(!demolish_at(&mut city, cx, cy));
        // cash は変化しない。
        assert_eq!(city.cash, cash_before_demolish);
    }

    /// 現金不足だと撤去失敗 + cash 据え置き + ログ。
    #[test]
    fn demolish_fails_on_insufficient_cash() {
        let mut city = City::new();
        city.cash = 10;
        // 外周 (0,0) に House を強制配置 — コスト = 50 + 24² * 5 = $2930。
        city.set_tile(0, 0, Tile::Built(Building::House));
        assert!(!demolish_at(&mut city, 0, 0));
        assert_eq!(city.cash, 10);
        // House はそのまま。
        assert!(matches!(city.tile(0, 0), Tile::Built(Building::House)));
    }

    /// AI 撤去: 中央に置いた inactive Shop が最高スコアで撤去対象になる。
    #[test]
    fn auto_demolish_picks_inactive_shop_in_core() {
        let mut city = City::new();
        city.cash = 10_000;
        let cx = GRID_W / 2;
        let cy = GRID_H / 2;
        // 中央 Shop だけ置く (隣接 Road なし → inactive)。
        city.set_tile(cx, cy, Tile::Built(Building::Shop));
        let target = auto_demolish_target(&city);
        assert!(target.is_some(), "should find a candidate");
        let (tx, ty, score) = target.unwrap();
        assert_eq!((tx, ty), (cx, cy));
        assert!(score > 0);
    }

    /// AI 撤去: 全建物が active なら撤去対象なし。
    #[test]
    fn auto_demolish_returns_none_when_everything_active() {
        let mut city = City::new();
        city.cash = 10_000;
        let cx = GRID_W / 2;
        let cy = GRID_H / 2;
        // House を 1 軒、Road 接続あり (有用な Cottage)。
        city.set_tile(cx, cy, Tile::Built(Building::House));
        city.set_tile(cx + 1, cy, Tile::Built(Building::Road));
        // 撤去候補なし (孤立 House でもなく、Shop/Workshop でもなく、Outpost でもない)。
        // ただし Road は隣接 Built (House) があるので「行き止まり」判定にもならない。
        let target = auto_demolish_target(&city);
        assert!(
            target.is_none(),
            "no waste should mean no demolition target, got {:?}",
            target
        );
    }

    /// AI 撤去: 外周の inactive Shop はコスト負けして撤去されない。
    /// 中央の inactive Shop の方が優先される (= プレイヤーの「外周は重い」体感に整合)。
    #[test]
    fn auto_demolish_prefers_cheaper_targets() {
        let mut city = City::new();
        city.cash = 10_000;
        // 中央と外周の両方に inactive Shop を置く。
        let cx = GRID_W / 2;
        let cy = GRID_H / 2;
        city.set_tile(cx, cy, Tile::Built(Building::Shop));
        city.set_tile(0, 0, Tile::Built(Building::Shop));
        let (tx, ty, _) = auto_demolish_target(&city).expect("should find candidate");
        assert_eq!(
            (tx, ty),
            (cx, cy),
            "central inactive Shop should be picked over the outer one (cost penalty)"
        );
    }

    /// auto_demolish: 候補がある + 現金十分なら撤去成功。
    #[test]
    fn auto_demolish_runs_when_target_exists() {
        let mut city = City::new();
        city.cash = 10_000;
        let cx = GRID_W / 2;
        let cy = GRID_H / 2;
        city.set_tile(cx, cy, Tile::Built(Building::Shop));
        assert!(auto_demolish(&mut city));
        assert!(matches!(city.tile(cx, cy), Tile::Empty));
    }

    /// auto_demolish: 候補なしなら false。tick 駆動で頻繁に呼ばれる前提なので
    /// ログ spam を避けるために失敗時はサイレント (events にプッシュしない)。
    #[test]
    fn auto_demolish_no_candidate_returns_false() {
        let mut city = City::new();
        city.cash = 10_000;
        let events_before = city.events.len();
        assert!(!auto_demolish(&mut city));
        assert_eq!(
            city.events.len(),
            events_before,
            "auto_demolish failure must be silent"
        );
    }

    /// 役目を終えた Outpost (周囲 Rock が無い) は AI が撤去する。
    #[test]
    fn auto_demolish_picks_idle_outpost() {
        let mut city = City::new();
        city.cash = 10_000;
        let cx = GRID_W / 2;
        let cy = GRID_H / 2;
        // 中央コアは Rock が出ない (CORE_RADIUS=5)。Outpost を置けば「役目無し」状態。
        city.set_tile(cx, cy, Tile::Built(Building::Outpost));
        let (tx, ty, _) = auto_demolish_target(&city).expect("idle Outpost should be a candidate");
        assert_eq!((tx, ty), (cx, cy));
    }

    /// dispatch_outpost: 隣接 Rock がある建物境界を見つけて Outpost 着工する。
    /// Phase A: `best_outpost_placement` が Manhattan 距離 4 以内まで
    /// 候補を広げたため、特定セルへの厳密 assert はやめて「グリッド上の
    /// どこかに Outpost Construction が始まる」という不変条件のみ確認する。
    #[test]
    fn dispatch_outpost_places_at_boundary() {
        let mut city = City::new();
        city.cash = 1_000;
        let cx = GRID_W / 2;
        let cy = GRID_H / 2;
        city.terrain[cy][cx] = super::super::terrain::Terrain::Plain;
        city.set_tile(cx, cy, Tile::Built(Building::House));
        city.terrain[cy][cx + 1] = super::super::terrain::Terrain::Plain;
        city.terrain[cy][cx + 2] = super::super::terrain::Terrain::Rock;

        let ok = dispatch_outpost(&mut city);
        assert!(ok, "outpost dispatch should succeed");
        let any_outpost_construction = (0..GRID_H).any(|y| {
            (0..GRID_W).any(|x| {
                matches!(
                    city.tile(x, y),
                    Tile::Construction {
                        target: Building::Outpost,
                        ..
                    }
                )
            })
        });
        assert!(
            any_outpost_construction,
            "an Outpost Construction tile should exist after dispatch"
        );
        assert!(city.cash < 1_000, "cash should be spent");
    }

    /// 機材設置場所が無い時は false を返してサイレントに帰る。
    /// 旧仕様では失敗イベントログを出していたが、tick 駆動の自動発火に移行した
    /// ため log spam を避けてサイレント化した。
    #[test]
    fn dispatch_outpost_fails_when_no_boundary() {
        let mut city = City::new();
        city.cash = 1_000;
        let events_before = city.events.len();
        // 既存建物無し → has_any_built_neighbor の候補ゼロ
        let result = dispatch_outpost(&mut city);
        assert!(!result, "no boundary should fail dispatch");
        // cash は減っていない。
        assert_eq!(city.cash, 1_000);
        // 失敗時はイベントを増やさない (サイレント)。
        assert_eq!(city.events.len(), events_before);
    }

    /// Codex review #100 P1 回帰: Forest セルを Outpost 候補から除外する。
    ///
    /// 以前は Forest/Wasteland も候補に入っていたため、`start_construction` が
    /// 整地ブランチに入って $15 だけ消費し Outpost を建てない silent failure が
    /// 発生していた。修正後は Forest 隣接でも Plain 候補があればそちらを選ぶ。
    #[test]
    fn dispatch_outpost_skips_forest_candidates() {
        let mut city = City::new();
        city.cash = 10_000;
        let cx = GRID_W / 2;
        let cy = GRID_H / 2;
        // 中央 House → 隣 Plain (左) と隣 Forest (右) を候補に。両側に Rock を配置。
        city.terrain[cy][cx] = super::super::terrain::Terrain::Plain;
        city.set_tile(cx, cy, Tile::Built(Building::House));
        // 左: Plain + 隣 Rock
        city.terrain[cy][cx - 1] = super::super::terrain::Terrain::Plain;
        city.terrain[cy][cx - 2] = super::super::terrain::Terrain::Rock;
        // 右: Forest + 隣 Rock (旧バグでは候補になっていた)
        city.terrain[cy][cx + 1] = super::super::terrain::Terrain::Forest;
        city.terrain[cy][cx + 2] = super::super::terrain::Terrain::Rock;
        // Forest 上には更に Rock を増やしてスコアを上げ、旧バグで Forest が
        // 優先されることを再現しやすくする。
        if cy + 1 < GRID_H {
            city.terrain[cy + 1][cx + 1] = super::super::terrain::Terrain::Rock;
        }

        let placement = best_outpost_placement(&city).expect("should find a Plain candidate");
        // Forest セル (cx+1, cy) は候補に入れてはならない (silent failure 回避)。
        assert_ne!(
            placement,
            (cx + 1, cy),
            "Forest cell must not be selected for Outpost placement"
        );
        // 配置先の terrain は Plain であること (= Outpost が直接着工可能)。
        let (px, py) = placement;
        assert_eq!(
            city.terrain[py][px],
            super::super::terrain::Terrain::Plain,
            "placement {:?} should be on Plain terrain",
            placement
        );

        assert!(dispatch_outpost(&mut city));
        assert!(matches!(
            city.tile(px, py),
            Tile::Construction { target: Building::Outpost, .. }
        ));
    }

    /// Rock セルは隣接 Outpost が無いと start_construction が false を返す。
    #[test]
    fn rock_blocks_construction_without_outpost() {
        let mut city = City::new();
        city.cash = 1_000;
        let (rx, ry) = (GRID_W / 2 + 6, GRID_H / 2);
        // 強制的に Rock に。
        city.terrain[ry][rx] = super::super::terrain::Terrain::Rock;
        let ok = start_construction(&mut city, rx, ry, Building::House);
        assert!(!ok, "Rock cell without Outpost neighbor should reject construction");
        assert_eq!(city.cash, 1_000); // cash should not be spent
    }

    /// Outpost を隣に置くと Rock が整地できる。
    #[test]
    fn rock_with_outpost_neighbor_can_be_cleared() {
        let mut city = City::new();
        city.cash = 1_000;
        let (rx, ry) = (GRID_W / 2 + 6, GRID_H / 2);
        city.terrain[ry][rx] = super::super::terrain::Terrain::Rock;
        // 隣 (rx-1, ry) に Outpost を直接置く (テスト用 set_tile で完成状態に)。
        city.set_tile(rx - 1, ry, Tile::Built(Building::Outpost));
        // House を Rock の上に建てる試行 → 整地工程に入る (Tile::Clearing)。
        let ok = start_construction(&mut city, rx, ry, Building::House);
        assert!(ok, "Rock with Outpost neighbor should accept construction");
        assert!(matches!(city.tile(rx, ry), Tile::Clearing { .. }));
    }

    #[test]
    fn hire_worker_blocks_at_max() {
        let mut city = City::new();
        city.cash = 1_000_000;
        city.workers = MAX_WORKERS;
        assert!(!hire_worker(&mut city));
        assert_eq!(city.workers, MAX_WORKERS);
        assert_eq!(city.cash, 1_000_000);
    }

    // ── Phase D: 築年数 / Tier 昇格 dwell time / 老朽化 ─────────

    /// 周辺条件は Highrise 級でも、築年数が足りなければ Apartment 止まり。
    #[test]
    fn highrise_target_with_low_age_caps_at_apartment() {
        let target = HouseTier::Highrise;
        // 築 600 ticks (= 60 sec) → Apartment dwell は満たすが Highrise dwell は未満。
        assert_eq!(effective_house_tier(target, 600), HouseTier::Apartment);
        // 築 3000 ticks (= 5 min) → Highrise dwell 達成。
        assert_eq!(effective_house_tier(target, 3000), HouseTier::Highrise);
    }

    /// Apartment dwell 未満の家は築何年でも Cottage。
    #[test]
    fn fresh_house_is_cottage_regardless_of_target() {
        for target in [HouseTier::Cottage, HouseTier::Apartment, HouseTier::Highrise] {
            assert_eq!(effective_house_tier(target, 0), HouseTier::Cottage);
            assert_eq!(effective_house_tier(target, 599), HouseTier::Cottage);
        }
    }

    /// 周辺条件が Cottage なら age がいくつでも Cottage のまま (誤昇格しない)。
    #[test]
    fn cottage_target_never_promotes() {
        for age in [0, 600, 3000, 10_000] {
            assert_eq!(
                effective_house_tier(HouseTier::Cottage, age),
                HouseTier::Cottage
            );
        }
    }

    /// 老朽化曲線: 1 min まで full、5 min で 50% に達し下限。
    #[test]
    fn aging_curve_respects_lifespan_floor() {
        // Cottage (lifespan 100): 600 ticks まで full、3000 ticks で 500‰。
        assert_eq!(aging_factor_per_mille(0, 100), 1000);
        assert_eq!(aging_factor_per_mille(599, 100), 1000);
        assert_eq!(aging_factor_per_mille(600, 100), 1000);
        let mid = aging_factor_per_mille(1800, 100);
        assert!(
            mid > 500 && mid < 1000,
            "midway should be partial decay: got {}",
            mid
        );
        assert_eq!(aging_factor_per_mille(3000, 100), 500);
        assert_eq!(aging_factor_per_mille(10_000, 100), 500);
    }

    /// 高 Tier ほど寿命が長い: 同じ築年数でも Highrise は full、Cottage は減衰。
    #[test]
    fn higher_tier_ages_slower() {
        let age = 1500; // 2.5 min
        let cottage_factor = aging_factor_per_mille(age, lifespan_x100(Building::House, Some(HouseTier::Cottage)));
        let apt_factor = aging_factor_per_mille(age, lifespan_x100(Building::House, Some(HouseTier::Apartment)));
        let high_factor = aging_factor_per_mille(age, lifespan_x100(Building::House, Some(HouseTier::Highrise)));
        assert!(
            cottage_factor < apt_factor,
            "Cottage should decay faster than Apartment at age {}: C={}, A={}",
            age, cottage_factor, apt_factor
        );
        assert!(
            apt_factor <= high_factor,
            "Apartment should decay no faster than Highrise: A={}, H={}",
            apt_factor, high_factor
        );
        // Highrise は 2.5 min ではまだ full (寿命 4× = scaled_age 375 < 600)。
        assert_eq!(high_factor, 1000);
    }

    /// Park / Road は不老 (寿命 ∞)。
    #[test]
    fn parks_and_roads_never_age() {
        assert_eq!(
            aging_factor_per_mille(100_000, lifespan_x100(Building::Park, None)),
            1000
        );
        assert_eq!(
            aging_factor_per_mille(100_000, lifespan_x100(Building::Road, None)),
            1000
        );
    }

    /// 完成タイル (advance_construction 経由) は built_at_tick が記録される。
    #[test]
    fn construction_completion_stamps_built_at_tick() {
        let mut city = City::new();
        // 地形を Plain で固定してから建設 (seed によって forest/water になるのを避ける)。
        city.terrain[5][5] = super::super::terrain::Terrain::Plain;
        city.cash = 10_000;
        city.tick = 500;
        // 道路を建てる (build_ticks=30)。
        assert!(start_construction(&mut city, 5, 5, Building::Road));
        // 30 tick 進めると完成し、built_at_tick が記録される。
        tick(&mut city, 30);
        assert!(matches!(city.tile(5, 5), Tile::Built(Building::Road)));
        // 完成 tick = 500 + 30 = 530 (advance_construction が tick の頭で動く)。
        assert!(city.built_at_tick[5][5] >= 500 && city.built_at_tick[5][5] <= 530);
    }

    /// 撤去すると built_at_tick がリセットされる。
    #[test]
    fn demolish_clears_built_at_tick() {
        let mut city = City::new();
        city.cash = 10_000;
        city.set_tile(5, 5, Tile::Built(Building::House));
        city.built_at_tick[5][5] = 1000;
        assert!(demolish_at(&mut city, 5, 5));
        assert_eq!(city.built_at_tick[5][5], 0);
    }

    /// Cottage 1 軒だけでは fallback で $1 になる (死スパイラル防止)。
    #[test]
    fn one_cottage_uses_survival_fallback() {
        let mut city = City::new();
        city.set_tile(0, 0, Tile::Built(Building::House));
        // 50¢ → $0、fallback で $1。
        assert_eq!(compute_income_per_sec(&city), 1);
    }

    /// Tier 昇格で income が上がる。同じ街区が Cottage → Apartment になると約 3 倍。
    #[test]
    fn apartment_earns_more_than_cottage() {
        let mut city = City::new();
        // 4 軒の住宅 + 道路 + Shop で、(0,0) は Apartment 化条件を満たす。
        city.set_tile(0, 0, Tile::Built(Building::House));
        city.set_tile(0, 1, Tile::Built(Building::Road));
        city.set_tile(0, 2, Tile::Built(Building::Shop));
        // (0,0) の age 0 → Cottage 扱い。
        let cottage_income = compute_income_per_sec(&city);
        // age を 600 (Apartment dwell 達成) にして再計算。
        city.tick = 600;
        city.built_at_tick[0][0] = 0; // age = 600
        let apartment_income = compute_income_per_sec(&city);
        assert!(
            apartment_income > cottage_income,
            "Apartment should out-earn Cottage: cottage={} apt={}",
            cottage_income, apartment_income
        );
    }

    /// 老朽化した Cottage は wastefulness_score が加算され、撤去候補になりやすい。
    #[test]
    fn aged_cottage_becomes_demolish_candidate() {
        let mut city = City::new();
        // 中央付近に孤立 House を置いて built_at を古めに設定。
        city.set_tile(15, 8, Tile::Built(Building::House));
        // 何の経済刺激もない孤立 Cottage。
        city.tick = 5000;
        city.built_at_tick[8][15] = 0; // age = 5000、寿命下限到達
        let score = wastefulness_score(&city, 15, 8);
        assert!(
            score.is_some() && score.unwrap() > 0,
            "Aged isolated cottage should be a demolish candidate: {:?}",
            score
        );
    }
}
