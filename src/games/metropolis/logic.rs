//! Pure-function game logic.  No I/O, no rendering — safe to call millions
//! of times from `simulator.rs`.

use std::collections::VecDeque;

use super::ai::{decide, AiAction};
use super::state::*;

// ── Edge connectivity (Phase 2: SimCity 風 物流接続) ───────────
//
// 「マップの外と繋がっている道路網」を BFS で判定する純関数。
// 戻り値の bool grid を AI / activation rule / income calc が共有して、
// 「外との物流が通っている街区」だけが本来の収入を出す挙動を作る。
//
// **ルール (ユーザー指定: ハイブリッド)**:
//   - Shop / Workshop: HARD — 隣接 Road が edge-connected で無いと inactive。
//     (= 食材・部品の運搬が物理的に届かない)
//   - House:           SOFT — Cottage は孤立街区でも住める (収入は半減)、
//     Apartment / Highrise は edge-connected が必須 (= 高層化には流通インフラ)。
//
// 計算量: BFS O(N) を各 tick 1 回だけ実行 (`recompute_edge_connectivity`)。
// `City::edge_connected` をキャッシュ的に保持すると更に速いが、現状は呼び側で
// 1 度生成して使い回す pattern を奨励 (`compute_income_per_sec` 等)。

/// 全 Road セルが「マップ端まで連続する道路網」に属するか判定した bool grid。
///
/// マップ端 (x=0, x=GRID_W-1, y=0, y=GRID_H-1) のいずれかにある Road セルから
/// 4-近傍 BFS を流す。**完成 Road のみ**を通過 (建設中 Road は含めない —
/// 物流は完成しないと走らない)。
#[allow(clippy::needless_range_loop)] // 端 seed は (x,0)/(x,GRID_H-1) の二重 index 参照が本質的
pub fn compute_edge_connected_roads(city: &City) -> Vec<Vec<bool>> {
    let mut connected = vec![vec![false; GRID_W]; GRID_H];
    let mut queue: VecDeque<(usize, usize)> = VecDeque::new();
    // Seed: マップ端の Road を全部キューに入れる。
    let is_road = |x: usize, y: usize, c: &City| -> bool {
        matches!(c.tile(x, y), Tile::Built(Building::Road))
    };
    for x in 0..GRID_W {
        if is_road(x, 0, city) && !connected[0][x] {
            connected[0][x] = true;
            queue.push_back((x, 0));
        }
        if GRID_H >= 1 && is_road(x, GRID_H - 1, city) && !connected[GRID_H - 1][x] {
            connected[GRID_H - 1][x] = true;
            queue.push_back((x, GRID_H - 1));
        }
    }
    for y in 0..GRID_H {
        if is_road(0, y, city) && !connected[y][0] {
            connected[y][0] = true;
            queue.push_back((0, y));
        }
        if GRID_W >= 1 && is_road(GRID_W - 1, y, city) && !connected[y][GRID_W - 1] {
            connected[y][GRID_W - 1] = true;
            queue.push_back((GRID_W - 1, y));
        }
    }
    // BFS: Road を辿って連結成分を塗る。
    while let Some((x, y)) = queue.pop_front() {
        for (dx, dy) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
            let nx = x as i32 + dx;
            let ny = y as i32 + dy;
            if nx < 0 || ny < 0 || nx >= GRID_W as i32 || ny >= GRID_H as i32 {
                continue;
            }
            let (nx, ny) = (nx as usize, ny as usize);
            if connected[ny][nx] {
                continue;
            }
            if is_road(nx, ny, city) {
                connected[ny][nx] = true;
                queue.push_back((nx, ny));
            }
        }
    }
    connected
}

/// 建物 (x, y) が edge-connected な Road に隣接 (4-近傍) しているか。
/// `connected` は `compute_edge_connected_roads` で取得した bool grid。
pub fn is_building_edge_connected(connected: &[Vec<bool>], x: usize, y: usize) -> bool {
    for (dx, dy) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
        let nx = x as i32 + dx;
        let ny = y as i32 + dy;
        if nx < 0 || ny < 0 || nx >= GRID_W as i32 || ny >= GRID_H as i32 {
            continue;
        }
        if connected[ny as usize][nx as usize] {
            return true;
        }
    }
    false
}


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
    // `auto_strategy_actions` は no-op (互換 stub)。撤去判断は AI が
    // `decide()` 経由で `placement_value` と `demolish_value` を比較して行う。
    auto_strategy_actions(city);
    drive_ai(city);
    accrue_income(city);
    detect_tier_advance(city);
    city.tick = city.tick.wrapping_add(1);
}

/// ティア境界を跨いだら演出をトリガー。AI 撤去で人口が一時的に減ることは
/// あるが、`detect_tier_advance` は上昇遷移時のみフラッシュを焚く。
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
        Building::Factory => "工場",
        Building::Shop => "店舗",
        Building::Mall => "商業ビル",
        Building::Office => "オフィス",
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
///
/// **Demolish action** も Build と同じく 1 worker を消費する。worker を
/// 消費しないと「Demolish が tick あたり最高評価のまま続く」状況で 1 tick
/// 内に attempts (= worker×2) 回の連続撤去が走り、cash が一気に枯渇する。
/// 1 worker 1 アクションに揃えることで「1 tick 1 撤去」を保つ。
fn drive_ai(city: &mut City) {
    let mut placements_left = city.free_workers();
    let mut attempts = placements_left.saturating_mul(2).max(1);
    while placements_left > 0 && attempts > 0 {
        attempts -= 1;
        match decide(city) {
            AiAction::Build { x, y, kind } => {
                if start_construction(city, x, y, kind) {
                    placements_left -= 1;
                }
            }
            AiAction::Demolish { x, y } => {
                if demolish_at(city, x, y) {
                    placements_left -= 1;
                } else {
                    // 失敗 (cash 不足等) なら break して busy-loop を防ぐ。
                    break;
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
    /// AI が建物種別を引く時の重み (合計 100 を厳守)。
    ///
    /// **注意**: Tier 4 以上は `placement_value` 評価ベースなのでこの重みは
    /// 直接参照しない。Tier 3 (RoadPlanner) と Status パネルの「戦略内訳」
    /// 表示でのみ使われる。Park の重みは「strategy_bias」側に統合済み
    /// (= 評価ベース AI が Eco の時に Park を選びやすくなる)。
    pub house_pct: u32,
    pub road_pct: u32,
    pub workshop_pct: u32,
    pub shop_pct: u32,
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
            // 副作用は「ゆっくり育てる」を表現する負の建設速度と僅かな収入ボーナス。
            // ボーナスは正の `income_penalty_pct = +5` として扱う (関数側で 100+5)。
            // Park を建てる傾向は `strategy_bias(Eco, Park) = +60` で
            // 評価ベース AI に統合済み。
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

        // 上位建物 (Factory / Mall / Office) は中後盤の主役。戦略ごとの意図を
        // 明示する文言を入れる。
        (Strategy::Growth, Building::Factory) => "工業団地を整備",
        (Strategy::Income, Building::Factory) => "重工業区を稼働",
        (Strategy::Tech, Building::Factory) => "先端工場を立ち上げ",
        (Strategy::Eco, Building::Factory) => "環境配慮型の工場を試験設置",

        (Strategy::Growth, Building::Mall) => "近隣商業施設を建設",
        (Strategy::Income, Building::Mall) => "大型商業ビルを開業",
        (Strategy::Tech, Building::Mall) => "幹線沿いに商業ビルを開業",
        (Strategy::Eco, Building::Mall) => "地域密着型商業ビルを建設",

        (Strategy::Growth, Building::Office) => "高層オフィスを整備",
        (Strategy::Income, Building::Office) => "オフィス区を稼働",
        (Strategy::Tech, Building::Office) => "テック企業の拠点を設置",
        (Strategy::Eco, Building::Office) => "緑化オフィスを建設",

        (_, Building::Outpost) => "開拓機材を設置",
    }
}

// ── 自動運用ポリシー (Strategy ごとの撤去 cash 余力) ───────────────
//
// 撤去判断は AI (`ai::decide`) が `placement_value` と `demolish_value` を
// 同じ天秤で行う。本セクションが提供するのは「撤去後 cash がこの予備金を
// 下回るなら撤去を見送る」というガードのみ。これがないと cash $50 →
// 中央のミス建物を撤去 → cash $0 → 次 tick の build を全て idle、の
// デフレ螺旋に陥り得る。
//
// Strategy ごとの reserve は「キャラ付け」を数字で表現する:
//   - 守備的な戦略 (Growth/Eco) は予備金を厚めに → cash 枯渇しにくい
//   - 攻撃的な戦略 (Income) はやや薄めに → 撤去再建をテンポ良く回す

/// 撤去判断時の cash 予備金ガード。AI が demolish action を出す前に
/// `cash >= demolish_cost(x, y) + min_cash_reserve` を満たすか確認する。
#[derive(Clone, Copy, Debug)]
pub struct AutomationPolicy {
    /// 撤去後に手元に残しておく cash 下限。これを割る撤去は見送られる。
    pub min_cash_reserve: i64,
}

/// 戦略ごとの撤去予備金ガード。AI と Manager タブの両方が参照する。
pub fn automation_policy(s: Strategy) -> AutomationPolicy {
    match s {
        // Growth: 人口拡張に資金を残したいので予備金厚め。
        Strategy::Growth => AutomationPolicy { min_cash_reserve: 250 },
        // Income: 撤去再建を積極的に回す = 予備金は薄め。
        Strategy::Income => AutomationPolicy { min_cash_reserve: 400 },
        // Tech: 建設+20% を活かしたいので House cost 程度を残す。
        Strategy::Tech => AutomationPolicy { min_cash_reserve: 350 },
        // Eco: 既存街区の更新がメインなので慎重に。
        Strategy::Eco => AutomationPolicy { min_cash_reserve: 200 },
    }
}

/// `step_one_tick` から毎 tick 呼ばれる no-op (互換性のため残置)。
///
/// 旧仕様では戦略ごとの周期で撤去を発火していたが、AI 自身が
/// `placement_value` と `demolish_value` を同じ天秤で比較するように
/// なったため不要。`step_one_tick` の呼び出し点を変えずに済むよう
/// 関数だけ残してある。次回大幅リファクタ時に呼び出し側ごと削除可。
pub fn auto_strategy_actions(_city: &mut City) {}

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
    // Outpost 派遣統計: AI が `placement_value` 経由で Outpost を選んだ時にも
    // カウントされるよう、start_construction でフックする (= 旧 dispatch_outpost
    // の責務を吸収)。
    if matches!(kind, Building::Outpost) {
        city.outposts_dispatched_total = city.outposts_dispatched_total.saturating_add(1);
    }
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
    // 商業施設 (Shop / Mall) はどちらも収入を出すので両方フラッシュさせる。
    let connected = compute_edge_connected_roads(city);
    let mut flash_targets: Vec<(usize, usize)> = Vec::new();
    for y in 0..GRID_H {
        for x in 0..GRID_W {
            if matches!(
                city.tile(x, y),
                Tile::Built(Building::Shop) | Tile::Built(Building::Mall)
            ) && shop_is_active_with(city, x, y, &connected)
            {
                flash_targets.push((x, y));
            }
        }
    }
    for (x, y) in flash_targets {
        city.payout_flash_until[y][x] = city.tick + PAYOUT_FLASH_TICKS;
    }
}

// ── 需給ベースの per-tile 収入計算 (Phase A: 需給システム) ────────────
//
// Shop / Mall / Workshop / Factory / Office の収入は **近隣人口の需要** を
// **キャパシティで按分** した値になる。ユーザー要望の「人口が増えても店舗が
// 足りないと…」を表現する核心。
//
// 計算式 (per-supplier):
//   total_demand = sum(local_population × per_capita_demand_cents)
//   total_capacity = sum(supplier.capacity_cents) for suppliers in radius
//   my_share = total_demand × my_capacity / total_capacity
//   my_income = min(my_share, my_capacity)
//
// これにより:
//   - 人口が少ない街区: Shop は上限未達で「客足が薄い」(ShopLevel = Basic)
//   - 人口が増える: Shop が満員 → さらに Mall を建てる動機が出る
//   - 過剰店舗: 同範囲に何軒も建てると 1 軒あたりの取り分が減る
//
// 集計範囲 = 半径 5 (Manhattan)。範囲外 House の客は来ない / 範囲外 Worker は
// 通えない、という直感に揃える。

/// `compute_income_per_sec` 内で 1 度だけ計算する per-tile 人口テーブル。
///
/// 各 Built House セルで Tier 込みの定員を埋め、それ以外は 0。これを
/// `population_within` に渡すと近隣人口の集計が radius² の単純ループで済む。
#[allow(clippy::needless_range_loop)] // (y,x) 両方で他の grid を index するため enumerate 化はしない
fn compute_population_map(city: &City, connected: &[Vec<bool>]) -> Vec<Vec<u32>> {
    let mut map = vec![vec![0u32; GRID_W]; GRID_H];
    for y in 0..GRID_H {
        for x in 0..GRID_W {
            if !matches!(city.tile(x, y), Tile::Built(Building::House)) {
                continue;
            }
            let target = house_tier_for(gather_house_neighborhood_with(city, x, y, connected));
            let age = city.tick.saturating_sub(city.built_at_tick[y][x]);
            let tier = effective_house_tier(target, age);
            map[y][x] = house_capacity(tier);
        }
    }
    map
}

/// 半径 `radius` 内の人口合計 (Manhattan 距離)。
fn population_within(pop_map: &[Vec<u32>], x: usize, y: usize, radius: i32) -> u32 {
    let mut total = 0u32;
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            if dx.abs() + dy.abs() > radius {
                continue;
            }
            let nx = x as i32 + dx;
            let ny = y as i32 + dy;
            if nx < 0 || ny < 0 || nx >= GRID_W as i32 || ny >= GRID_H as i32 {
                continue;
            }
            total += pop_map[ny as usize][nx as usize];
        }
    }
    total
}

/// 半径内の商業供給キャパシティ合計 (cents/sec 単位)。Shop = 200, Mall = 600。
fn commercial_capacity_within(city: &City, x: usize, y: usize, radius: i32) -> i64 {
    let mut total = 0i64;
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            if dx.abs() + dy.abs() > radius {
                continue;
            }
            let nx = x as i32 + dx;
            let ny = y as i32 + dy;
            if nx < 0 || ny < 0 || nx >= GRID_W as i32 || ny >= GRID_H as i32 {
                continue;
            }
            match city.tile(nx as usize, ny as usize) {
                Tile::Built(Building::Shop) => total += SHOP_CAPACITY_CENTS,
                Tile::Built(Building::Mall) => total += MALL_CAPACITY_CENTS,
                _ => {}
            }
        }
    }
    total
}

/// 半径内の雇用供給キャパシティ合計 (cents/sec 単位)。
fn employment_capacity_within(city: &City, x: usize, y: usize, radius: i32) -> i64 {
    let mut total = 0i64;
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            if dx.abs() + dy.abs() > radius {
                continue;
            }
            let nx = x as i32 + dx;
            let ny = y as i32 + dy;
            if nx < 0 || ny < 0 || nx >= GRID_W as i32 || ny >= GRID_H as i32 {
                continue;
            }
            match city.tile(nx as usize, ny as usize) {
                Tile::Built(Building::Workshop) => total += WORKSHOP_CAPACITY_CENTS,
                Tile::Built(Building::Factory) => total += FACTORY_CAPACITY_CENTS,
                Tile::Built(Building::Office) => total += OFFICE_CAPACITY_CENTS,
                _ => {}
            }
        }
    }
    total
}

/// 商業供給キャパシティ (cents/sec) — Shop / Mall の上限収入。
pub const SHOP_CAPACITY_CENTS: i64 = 200; // $2/sec
pub const MALL_CAPACITY_CENTS: i64 = 600; // $6/sec
/// 雇用供給キャパシティ (cents/sec) — Workshop / Factory / Office の上限収入。
pub const WORKSHOP_CAPACITY_CENTS: i64 = 100; // $1/sec
pub const FACTORY_CAPACITY_CENTS: i64 = 350; // $3.5/sec
pub const OFFICE_CAPACITY_CENTS: i64 = 250; // $2.5/sec

/// 1 人当たり購買力 (cents/sec)。商業需要の換算係数。
pub const PURCHASE_POWER_PER_CAPITA: i64 = 4;
/// 1 人当たり雇用需要 (cents/sec)。Workshop/Factory が吸収する。
pub const EMPLOYMENT_DEMAND_PER_CAPITA: i64 = 3;
/// 1 人当たりホワイトカラー需要 (cents/sec)。Office が吸収する。
pub const WHITE_COLLAR_DEMAND_PER_CAPITA: i64 = 2;

/// 商業建物 (Shop / Mall) の per-tile 収入 (cents/sec)。
///
/// 計算: 局所人口の購買力を商業キャパシティで按分し、自身のキャパに掛ける。
/// 自身が inactive (edge-connected & 半径 3 House あり) なら 0。
fn commercial_income_cents(
    city: &City,
    x: usize,
    y: usize,
    my_capacity: i64,
    pop_map: &[Vec<u32>],
    connected: &[Vec<bool>],
) -> i64 {
    if !shop_is_active_with(city, x, y, connected) {
        return 0;
    }
    let local_pop = population_within(pop_map, x, y, 5) as i64;
    let demand = local_pop * PURCHASE_POWER_PER_CAPITA;
    let total_capacity = commercial_capacity_within(city, x, y, 5);
    if total_capacity <= 0 {
        return 0;
    }
    let share = demand * my_capacity / total_capacity;
    share.min(my_capacity)
}

/// 雇用建物 (Workshop / Factory / Office) の per-tile 収入 (cents/sec)。
///
/// `demand_per_capita` は工業 (Workshop/Factory) なら `EMPLOYMENT_DEMAND_PER_CAPITA`、
/// オフィス (Office) なら `WHITE_COLLAR_DEMAND_PER_CAPITA`。
fn employment_income_cents(
    city: &City,
    x: usize,
    y: usize,
    my_capacity: i64,
    demand_per_capita: i64,
    pop_map: &[Vec<u32>],
    connected: &[Vec<bool>],
) -> i64 {
    if !workshop_is_active_with(city, x, y, connected) {
        return 0;
    }
    let local_pop = population_within(pop_map, x, y, 5) as i64;
    let demand = local_pop * demand_per_capita;
    let total_capacity = employment_capacity_within(city, x, y, 5);
    if total_capacity <= 0 {
        return 0;
    }
    let share = demand * my_capacity / total_capacity;
    share.min(my_capacity)
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
    let connected = compute_edge_connected_roads(city);
    // 全 House の Tier を一括スキャンして人口テーブルを作る。商業/雇用建物の
    // 需給按分 (commercial_income_cents / employment_income_cents) で参照。
    let pop_map = compute_population_map(city, &connected);

    for y in 0..GRID_H {
        for x in 0..GRID_W {
            let kind = match city.tile(x, y) {
                Tile::Built(b) => *b,
                _ => continue,
            };
            let tier_opt = if matches!(kind, Building::House) {
                let target =
                    house_tier_for(gather_house_neighborhood_with(city, x, y, &connected));
                let age = city.tick.saturating_sub(city.built_at_tick[y][x]);
                Some(effective_house_tier(target, age))
            } else {
                None
            };
            let base_cents: i64 = match kind {
                Building::House => {
                    let tier = tier_opt.expect("house has tier");
                    // House Tier ごとの基礎収入 (家賃)。Tier 連動の人口増加と
                    // 並行して、家賃そのものも Tier で 6 倍まで伸びる。
                    let raw = match tier {
                        HouseTier::Cottage => 50,
                        HouseTier::Apartment => 150,
                        HouseTier::Highrise => 300,
                    };
                    // House SOFT ルール: 未接続 Cottage は半減 ($0.25/sec)。
                    if !is_building_edge_connected(&connected, x, y) {
                        raw / 2
                    } else {
                        raw
                    }
                }
                Building::Workshop => employment_income_cents(
                    city,
                    x,
                    y,
                    WORKSHOP_CAPACITY_CENTS,
                    EMPLOYMENT_DEMAND_PER_CAPITA,
                    &pop_map,
                    &connected,
                ),
                Building::Factory => employment_income_cents(
                    city,
                    x,
                    y,
                    FACTORY_CAPACITY_CENTS,
                    EMPLOYMENT_DEMAND_PER_CAPITA,
                    &pop_map,
                    &connected,
                ),
                Building::Office => employment_income_cents(
                    city,
                    x,
                    y,
                    OFFICE_CAPACITY_CENTS,
                    WHITE_COLLAR_DEMAND_PER_CAPITA,
                    &pop_map,
                    &connected,
                ),
                Building::Shop => commercial_income_cents(
                    city,
                    x,
                    y,
                    SHOP_CAPACITY_CENTS,
                    &pop_map,
                    &connected,
                ),
                Building::Mall => commercial_income_cents(
                    city,
                    x,
                    y,
                    MALL_CAPACITY_CENTS,
                    &pop_map,
                    &connected,
                ),
                _ => 0,
            };
            if base_cents == 0 {
                continue;
            }
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

    // 死スパイラル防止: House があれば最低 $1/s 保証 (序盤の seed-RNG 偶発で
    // income==0 が続くのを防ぐ — simulator::tier1_never_stalls 等の不変条件)。
    let any_house = city.count_built(Building::House) > 0;
    let mut income = income_cents / 100;
    if any_house && income == 0 {
        income = 1;
    }

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

/// House Tier ごとの定員 (= 人口寄与)。
///
/// 街が育つ実感を「数字でも見せる」ための主要パラメータ。Tier が上がる時の
/// 倍率を 3x にすることで、Highrise 化が「街が爆発的に膨らむ瞬間」になる。
///
/// - Cottage:   4 人  (旧仕様の固定 5 を微調整 — 育てる旨味を作る)
/// - Apartment: 12 人 (Cottage の 3x)
/// - Highrise:  30 人 (Cottage の 7.5x)
pub fn house_capacity(tier: HouseTier) -> u32 {
    match tier {
        HouseTier::Cottage => 4,
        HouseTier::Apartment => 12,
        HouseTier::Highrise => 30,
    }
}

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
/// - `n_workshop_within_5`: 距離 5 以内の Workshop / Factory 合計数。
///   Factory は Workshop の 2 倍カウントする (= 規模換算)。
/// - `n_shop_within_5`: 距離 5 以内の Shop / Mall 合計数。
///   Mall は Shop の 2 倍カウントする。
/// - `n_office_within_5`: 距離 5 以内の Office 数。Highrise 化の触媒。
/// - `n_house_within_3`: 距離 3 以内の House 数 (自身は除く)。
/// - `n_park_within_4`: 距離 4 以内の Park 数。緑地でも街が育つ。
/// - `local_population`: 距離 5 以内の人口合計 (自身を除く)。需給ゲート用。
/// - `factory_smoke_penalty`: 隣接 (4-近傍) に Factory がある場合 true。
///   Tier を 1 段下げる「煙害」を表現。
/// - `edge_connected`: 隣接 Road が「マップ端まで繋がる幹線網」に属するか。
///   SOFT ルール: 未接続でも Cottage 暮らしは可。Apartment / Highrise には必須。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HouseNeighborhood {
    pub n_road_adj: u32,
    pub n_workshop_within_5: u32,
    pub n_shop_within_5: u32,
    pub n_office_within_5: u32,
    pub n_house_within_3: u32,
    pub n_park_within_4: u32,
    pub local_population: u32,
    pub factory_smoke_penalty: bool,
    pub edge_connected: bool,
}

/// 周囲をスキャンして `HouseNeighborhood` を組み立てる。
///
/// この関数は機械的な集計のみを担当する (純関数 / 副作用なし)。
/// 「どの数値で Tier を決めるか」というゲームデザイン判断は
/// `house_tier_for` 側に閉じる。
///
/// **オンデマンド版**: edge connectivity を都度 BFS する。複数 House を
/// 評価する場合は `gather_house_neighborhood_with` で BFS 結果を共有すること。
/// 現状はテストからのみ呼ばれる (production hot path は `_with` 経由)。
#[allow(dead_code)]
pub fn gather_house_neighborhood(city: &City, x: usize, y: usize) -> HouseNeighborhood {
    let connected = compute_edge_connected_roads(city);
    gather_house_neighborhood_with(city, x, y, &connected)
}

pub fn gather_house_neighborhood_with(
    city: &City,
    x: usize,
    y: usize,
    connected: &[Vec<bool>],
) -> HouseNeighborhood {
    let mut n_road_adj = 0u32;
    let mut factory_smoke_penalty = false;
    for (dx, dy) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
        let nx = x as i32 + dx;
        let ny = y as i32 + dy;
        if nx < 0 || ny < 0 || nx >= GRID_W as i32 || ny >= GRID_H as i32 {
            continue;
        }
        match city.tile(nx as usize, ny as usize) {
            Tile::Built(Building::Road) => n_road_adj += 1,
            Tile::Built(Building::Factory) => factory_smoke_penalty = true,
            _ => {}
        }
    }

    let mut n_shop_within_5 = 0u32;
    let mut n_workshop_within_5 = 0u32;
    let mut n_office_within_5 = 0u32;
    let mut n_house_within_3 = 0u32;
    let mut n_park_within_4 = 0u32;
    let mut local_population: u32 = 0;
    for cy in 0..GRID_H {
        for cx in 0..GRID_W {
            let dx = (cx as i32 - x as i32).abs();
            let dy = (cy as i32 - y as i32).abs();
            let manhattan = (dx + dy) as u32;
            match city.tile(cx, cy) {
                Tile::Built(Building::Shop) if manhattan <= 5 => n_shop_within_5 += 1,
                Tile::Built(Building::Mall) if manhattan <= 5 => n_shop_within_5 += 2,
                Tile::Built(Building::Workshop) if manhattan <= 5 => n_workshop_within_5 += 1,
                Tile::Built(Building::Factory) if manhattan <= 5 => n_workshop_within_5 += 2,
                Tile::Built(Building::Office) if manhattan <= 5 => n_office_within_5 += 1,
                Tile::Built(Building::House) if manhattan <= 5 && (cx, cy) != (x, y) => {
                    if manhattan <= 3 {
                        n_house_within_3 += 1;
                    }
                    // 需給ゲート用の local_population は **Cottage 定員固定** で
                    // 集計する。実効 Tier を呼ぶと「自身の Tier 計算が周囲の
                    // House の Tier に依存」する循環参照になるため。
                    // 「未成熟な街区の人口」を保守的に見積もるシンプルなモデルで、
                    // House が増えるほど需給ゲートが厳しくなる挙動は変わらない。
                    local_population += house_capacity(HouseTier::Cottage);
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
        n_office_within_5,
        n_house_within_3,
        n_park_within_4,
        local_population,
        factory_smoke_penalty,
        edge_connected: is_building_edge_connected(connected, x, y),
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
    // Park は商業ほど刺激は強くない (1 Park = 0.5 経済密度)。
    // Office は Highrise 化を促進する触媒として 1.5x 重み (整数演算で 3/2 計算)。
    let park_density = stats.n_park_within_4.div_ceil(2);
    let office_density = stats.n_office_within_5 * 3 / 2;
    let economic_density =
        stats.n_workshop_within_5 + stats.n_shop_within_5 + park_density + office_density;

    // **Phase 2 ハイブリッド連結性**: Apartment / Highrise はマップ外との
    // 物流接続が必要。Cottage は SOFT 制約。
    if !stats.edge_connected {
        return HouseTier::Cottage;
    }

    // **需給ゲート**: 街区の人口が増えるほど、必要な経済密度の閾値が上がる。
    //   local_pop 0..30:   閾値 = 1 (Apartment) / 2 (Highrise)
    //   local_pop 30..60:  閾値 = 2 / 3
    //   local_pop 60..90:  閾値 = 3 / 4
    //   local_pop 90+:     閾値 = 4 / 5
    // 「人口が伸びても店舗が足りないと住宅が育たない」を表現する核心ロジック。
    let demand_pressure = stats.local_population / 30; // 0, 1, 2, ...
    let apartment_required = 1 + demand_pressure;
    let highrise_required = 2 + demand_pressure;

    // Highrise: 商工業 + 緑地が両立した成熟ゾーン。Office があると Highrise 化
    // しやすい (= n_office_within_5 が経済密度に倍率付き加算済み)。
    if stats.n_road_adj >= 2
        && economic_density >= highrise_required
        && stats.n_house_within_3 >= 3
    {
        return apply_smoke_penalty(HouseTier::Highrise, stats.factory_smoke_penalty);
    }

    // Apartment: Road + 経済刺激源が最低 apartment_required 個必須。
    if stats.n_road_adj >= 1 && economic_density >= apartment_required {
        return apply_smoke_penalty(HouseTier::Apartment, stats.factory_smoke_penalty);
    }

    HouseTier::Cottage
}

/// Factory 隣接の煙害で Tier を 1 段下げる純関数。
///
/// 工業特化プレイヤーは「Factory を住宅街に近づけすぎると Highrise が育たない」
/// というジレンマに直面する。Factory 単体の高収入と Highrise 街区の高収入を
/// 両立させるには適切な距離配置が必要 — SimCity 的なゾーニング判断を作る。
fn apply_smoke_penalty(tier: HouseTier, penalized: bool) -> HouseTier {
    if !penalized {
        return tier;
    }
    match tier {
        HouseTier::Highrise => HouseTier::Apartment,
        HouseTier::Apartment => HouseTier::Cottage,
        HouseTier::Cottage => HouseTier::Cottage,
    }
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
///
/// **注意**: 内部で `compute_edge_connected_roads` を 1 回呼ぶため、
/// render の per-cell ループなど多数のセルを評価する場合は
/// `effective_tier_at_with` を使うこと。現状の production caller は
/// すべて `_with` 経由 (Codex review #103 P1 で hot path を移行済)。
/// この convenience 版は引き続きテスト・診断用に残す。
#[allow(dead_code)]
pub fn effective_tier_at(city: &City, x: usize, y: usize) -> HouseTier {
    let target = house_tier_for(gather_house_neighborhood(city, x, y));
    let age = city.tick.saturating_sub(city.built_at_tick[y][x]);
    effective_house_tier(target, age)
}

/// `effective_tier_at` の BFS 共有版。複数セルを評価する時に使う。
/// `should_show_aviation_light_with` と render hot path から呼ばれる。
pub fn effective_tier_at_with(
    city: &City,
    x: usize,
    y: usize,
    connected: &[Vec<bool>],
) -> HouseTier {
    let target = house_tier_for(gather_house_neighborhood_with(city, x, y, connected));
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
        (Building::Workshop, _) => 200,
        (Building::Shop, _) => 220,
        // 上位建物は基礎建物より長寿 — 大きな投資の元を取らせる。
        (Building::Factory, _) => 300,
        (Building::Mall, _) => 320,
        (Building::Office, _) => 280,
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
/// `should_show_aviation_light_with` の convenience 版。production caller は
/// すべて `_with` 経由 (Codex review #103 P1 移行)。診断・テスト用に保持。
#[allow(dead_code)]
pub fn should_show_aviation_light(city: &City, x: usize, y: usize, tick: u64) -> bool {
    let connected = compute_edge_connected_roads(city);
    should_show_aviation_light_with(city, x, y, tick, &connected)
}

/// `should_show_aviation_light` の BFS 共有版。
///
/// **重要**: render は per-cell ループでこの関数を呼ぶため、無印版だと
/// 4-近傍 × 1 BFS = 4 回の BFS が **タイルごと** に走り、64×32 マップで
/// 桁違いに重くなる (Codex review #103 P1 指摘)。`render_grid` で
/// 1 度だけ生成した `connected` を流して、隣接 Highrise 判定も BFS 共有版
/// (`effective_tier_at_with`) を使う。
pub fn should_show_aviation_light_with(
    city: &City,
    x: usize,
    y: usize,
    tick: u64,
    connected: &[Vec<bool>],
) -> bool {
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
            let neighbor_tier = effective_tier_at_with(city, nx, ny, connected);
            if matches!(neighbor_tier, HouseTier::Highrise) {
                highrise_neighbors += 1;
            }
        }
    }
    highrise_neighbors >= 2
}

/// 店舗の段階レベル — 隣接アクティブ House 数 + 道路接続で評価。
/// 賑わいの可視化用。アクティブで Mid 以上の住宅が近いとプレミアム。
///
/// **オンデマンド版**: 内部で BFS を 1 回実行する。render の per-cell ループ
/// など hot path では `shop_level_with` を使うこと (Codex review #103 P1)。
/// production caller はすべて `_with` 経由 — 診断・テスト用に保持。
#[allow(dead_code)]
pub fn shop_level(city: &City, x: usize, y: usize) -> ShopLevel {
    let connected = compute_edge_connected_roads(city);
    shop_level_with(city, x, y, &connected)
}

/// `shop_level` の BFS 共有版。render の per-tile 描画など複数セルを舐める
/// hot path で使う。`connected` は `compute_edge_connected_roads` で生成。
pub fn shop_level_with(
    city: &City,
    x: usize,
    y: usize,
    connected: &[Vec<bool>],
) -> ShopLevel {
    if !shop_is_active_with(city, x, y, connected) {
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

/// Workshop / Factory / Office は **隣接 House (= 労働力) + edge-connected Road**
/// の両方が必要。Workshop と Factory/Office も同じ活性条件 (働き手と物流)。
#[allow(dead_code)]
pub(super) fn workshop_is_active(city: &City, wx: usize, wy: usize) -> bool {
    if !has_neighbor_kind(city, wx, wy, Building::House) {
        return false;
    }
    let connected = compute_edge_connected_roads(city);
    is_building_edge_connected(&connected, wx, wy)
}

/// `workshop_is_active` の `connected` 持ち回し版。Workshop / Factory / Office
/// 共通で「労働力 (隣接 House) + 物流 (edge-connected Road)」を判定。
pub(super) fn workshop_is_active_with(
    city: &City,
    wx: usize,
    wy: usize,
    connected: &[Vec<bool>],
) -> bool {
    has_neighbor_kind(city, wx, wy, Building::House)
        && is_building_edge_connected(connected, wx, wy)
}

/// A shop earns money if it has a road neighbor *and* a house within
/// Manhattan distance 3.  This makes Tier-1's random scattering punishable
/// without being unwinnable.
///
/// **Phase 2: edge connectivity HARD ルール** — 隣接 Road が「マップ端まで
/// 繋がる幹線網」に属していないと inactive。商品 / 食材の搬入が外から
/// 届かないと店は回らない、という SimCity 的な制約。
///
/// production caller はすべて `_with` 経由 (Codex #103 P1)。診断用に保持。
#[allow(dead_code)]
pub(super) fn shop_is_active(city: &City, sx: usize, sy: usize) -> bool {
    let connected = compute_edge_connected_roads(city);
    shop_is_active_with(city, sx, sy, &connected)
}

pub(super) fn shop_is_active_with(
    city: &City,
    sx: usize,
    sy: usize,
    connected: &[Vec<bool>],
) -> bool {
    if !is_building_edge_connected(connected, sx, sy) {
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

/// (x, y) の 4-近傍に指定 terrain があるか。Outpost 候補絞り込みなど用。
pub(super) fn has_terrain_neighbor(
    city: &City,
    x: usize,
    y: usize,
    target: super::terrain::Terrain,
) -> bool {
    for (dx, dy) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
        let nx = x as i32 + dx;
        let ny = y as i32 + dy;
        if nx < 0 || ny < 0 || nx >= GRID_W as i32 || ny >= GRID_H as i32 {
            continue;
        }
        if city.terrain_at(nx as usize, ny as usize) == target {
            return true;
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

// ── 撤去評価関数 (AI が `placement_value` と並べて比較) ─────────────
//
// `demolish_value` が「撤去すれば街が改善する量」を返し、AI (`ai::decide`) が
// `placement_value` の最良候補と直接比較する。値の大小だけが意味を持つ
// 相対スコア (cents/sec とは厳密には揃わないが、cost_penalty を /10 にする
// ことで「中央のミスは即撤去 / 外周は滅多に撤去しない」という直感に
// 沿う挙動になる)。
//
// 「無駄な建物」の定義:
//   1. 機能不全 (inactive Shop / Workshop, Road 接続無し House)
//   2. 役目を終えた (Outpost で周囲に Rock が無い、孤立 Road, House 無し Park)
//   3. 老朽化 (寿命が尽きて income が大きく目減りしている)
//
// 中央 ($50) のミスは coast_penalty 5 で撤去価値が大きく残るが、外周 ($2050)
// では penalty 205 で機能不全 +250 でも余裕は +45 しかない。プレイヤーが
// 「外側に建てる前に再考しろ」と誘導される設計。

/// 撤去価値 (`auto_demolish_target` と AI 評価の両方が参照)。
///
/// **正の値** = 撤去で街の状態を改善できる量、
/// **i64::MIN** = 撤去対象外 (Empty / Construction セル等)。
/// 0 やマイナス = 撤去すべきでない (機能してる建物 / コスト負け)。
///
/// 設計: 各 building に「機能不全なら +X」の固定加点を載せ、老朽化建物には
/// aging recovery 加点を足し、最後に `demolish_cost / 10` を引く。
/// 単位は無次元のスコアだが、Tier 4/5 の AI は `placement_value` (cents/sec)
/// と直接比較して action を選ぶ。スケールが厳密に一致しないため、Build と
/// Demolish の選好は cost_penalty の係数とこの加点値の組み合わせで決まる。
///
/// **AI 統合**: Tier 4/5 はこの値を `placement_value` と並べて max 選択する。
/// 中央のミス (= cost_penalty が小さい inactive Shop など) は build を上回り
/// やすく、外周は coast_penalty が膨らむため build/idle が勝つ。
/// 全 Tier の AI が `decide()` 経由で参照 (Tier 1-3 はトリガー限定の撤去、
/// Tier 4/5 は build と同時比較)。`auto_demolish_target_with` がこの値の最大を選ぶ。
pub fn demolish_value(city: &City, x: usize, y: usize, connected: &[Vec<bool>]) -> i64 {
    let kind = match city.tile(x, y) {
        Tile::Built(b) => *b,
        _ => return i64::MIN,
    };

    let edge_ok = is_building_edge_connected(connected, x, y);

    let mut score: i64 = 0;
    match kind {
        Building::Shop => {
            if !shop_is_active_with(city, x, y, connected) {
                score += 250;
            }
        }
        Building::Mall => {
            // Mall は cost が高く、機能不全だと撤去価値も大きい。
            if !shop_is_active_with(city, x, y, connected) {
                score += 400;
            }
        }
        Building::Workshop => {
            if !workshop_is_active_with(city, x, y, connected) {
                score += 200;
            }
        }
        Building::Factory => {
            if !workshop_is_active_with(city, x, y, connected) {
                score += 350;
            }
        }
        Building::Office => {
            if !workshop_is_active_with(city, x, y, connected) {
                score += 280;
            }
        }
        Building::Outpost => {
            // 役目を終えた Outpost (周囲 4-近傍に Rock が無い) は撤去候補。
            if count_rock_neighbors(city, x, y) == 0 {
                score += 300;
            }
        }
        Building::House => {
            // edge 未接続 Cottage は収入半減のため撤去価値が上昇。
            // 完全孤立 (Road も House 隣接も無し) は更に追加。
            let stats = gather_house_neighborhood_with(city, x, y, connected);
            if !edge_ok {
                score += 60;
            }
            if stats.n_road_adj == 0 && stats.n_house_within_3 == 0 {
                score += 80;
            }
        }
        Building::Road => {
            // 行き止まりの孤立 Road (隣接 Built が 0)。
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

    // 老朽化ボーナス: 寿命が尽きた建物は「再建すれば収入が回復する」候補。
    // 不老建物 (Park/Road) は age 関係なく加点しない (= 永続資産扱い)。
    if city.built_at_tick[y][x] != 0 {
        let age = city.tick.saturating_sub(city.built_at_tick[y][x]);
        let tier_opt = if matches!(kind, Building::House) {
            Some(effective_house_tier(
                house_tier_for(gather_house_neighborhood_with(city, x, y, connected)),
                age,
            ))
        } else {
            None
        };
        let lifespan = lifespan_x100(kind, tier_opt);
        let factor = aging_factor_per_mille(age, lifespan);
        if factor <= 600 {
            score += 80;
        } else if factor <= 750 {
            score += 30;
        }
    }

    if score == 0 {
        return i64::MIN;
    }

    // コスト割引: 中央 ($50) → -5、外周 ($2050) → -205。
    // d² 曲線が外周を強くガードし、AI が「外周建物を気軽に撤去」しないようにする。
    let cost_penalty = demolish_cost(x, y) / 10;
    score - cost_penalty
}

/// `demolish_value` のオンデマンド版 (BFS 1 回)。
/// テスト互換性のため旧名を残す。
#[cfg(test)]
fn wastefulness_score(city: &City, x: usize, y: usize) -> Option<i64> {
    let connected = compute_edge_connected_roads(city);
    let v = demolish_value(city, x, y, &connected);
    if v == i64::MIN {
        None
    } else {
        Some(v)
    }
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

/// 全 Built タイルから最高 `demolish_value` の撤去候補を返す。
///
/// 戻り値: `(x, y, value)` (value = cents/sec 単位)。撤去価値プラスの候補が
/// 無い時は None。
pub fn auto_demolish_target(city: &City) -> Option<(usize, usize, i64)> {
    let connected = compute_edge_connected_roads(city);
    auto_demolish_target_with(city, &connected)
}

/// `auto_demolish_target` の BFS 共有版。AI 側が既に
/// `compute_edge_connected_roads` を計算済みの場合に再計算を避けて呼べる。
pub fn auto_demolish_target_with(
    city: &City,
    connected: &[Vec<bool>],
) -> Option<(usize, usize, i64)> {
    let mut best: Option<(usize, usize, i64)> = None;
    for y in 0..GRID_H {
        for x in 0..GRID_W {
            let v = demolish_value(city, x, y, connected);
            if v == i64::MIN || v <= 0 {
                continue;
            }
            let better = match best {
                None => true,
                Some((_, _, prev)) => v > prev,
            };
            if better {
                best = Some((x, y, v));
            }
        }
    }
    best
}

/// AI に撤去判断を一任する (テスト用)。最高スコアの建物を 1 つ撤去する。
/// 候補が無い / 現金不足の時は **無音で false** を返す。
///
/// 本番では `ai::decide` が `AiAction::Demolish` を返した時に `drive_ai` が
/// `demolish_at` を直接呼ぶため、本ヘルパーは経路に乗らない。テスト用に残す。
#[cfg(test)]
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

/// (x, y) から Manhattan 距離 dist 以内に Built/Construction セルが存在するか。
pub(super) fn has_built_within_distance(city: &City, x: usize, y: usize, dist: i32) -> bool {
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

// ── Placement Value (評価ベース AI の中核) ──────────────────────
//
// 「この候補に kind を建てると、シティ全体の income/sec がどれだけ増えそうか」
// を **cents 単位** で見積もる純関数。Tier 4 以上の AI はこの値を最大化する
// 候補を選ぶ。saturation 時に Outpost が高評価になり「自然に外の岩場を割る」
// という挙動は、この関数の論理から導かれる (= ハードコード分岐ではない)。
//
// 評価軸:
//   1. 直接稼働 income — その建物が tick 1 から稼ぐ cents/sec
//   2. シナジー — 周囲の既存 House を Tier 上げる (Apartment/Highrise 化) 影響
//   3. 将来潜在 — 道路/Outpost が解禁する Empty buildable cells の期待 income
//   4. 戦略バイアス — Strategy ごとの好みを軽く乗せる
//   5. ROI — kind.cost() を引いて「コスト見合いか」を反映
//
// 戻り値は cents (= 100 倍した日割り income)。10 は + $0.1/sec 相当の評価。
//
// **Outpost 自動派遣の自然発生**:
//   中央エリアが満杯 → House/Shop 候補は cost 高く synergy も既存 saturated で
//   value が小さい / 0。一方 Outpost は周囲 Rock 数 × 期待 House income の
//   future が乗るので相対的に高評価 → AI は Outpost を自分で選ぶ。
//   「拡張周期」のような外付けタイミングが不要になる。

/// Workshop / Shop / House の典型 income 期待値 (cents/sec)。
///
/// 将来潜在 (= 「ここに Empty buildable cell を作っておくと、後で何かが
/// 建つだろう」) の期待値計算に使うラフな mid-Tier 想定値。
///
/// **設計意図**: 将来潜在を「即時 income」と等価に扱うと AI が Outpost / Road を
/// 過剰選択する (= 30 min で 70 機材派遣して cash 枯渇) ため、controlled に discount。
/// Road は /3、Outpost は /4 で割って `direct + synergy` との拮抗を狙う。
const FUTURE_CELL_EXPECTATION_CENTS: i64 = 80; // 約 $0.8/sec — Cottage と Apartment の中間

/// 候補建設の収入評価 (cents/sec 単位)。**評価ベース AI の唯一の真実。**
///
/// `connected` は `compute_edge_connected_roads` の結果。
pub fn placement_value(
    city: &City,
    x: usize,
    y: usize,
    kind: Building,
    connected: &[Vec<bool>],
) -> i64 {
    if !matches!(city.tile(x, y), Tile::Empty) {
        return i64::MIN;
    }
    let terrain = city.terrain_at(x, y);
    if !terrain.buildable() {
        return i64::MIN;
    }
    if terrain.needs_outpost() && !has_outpost_neighbor(city, x, y) {
        return i64::MIN;
    }
    // 整地必要セルは「整地+建設で 2 倍時間がかかる」ペナルティ。
    // 候補としては残すが評価を下げる (= Plain 候補があればそちらを選ぶ)。
    let clearing_penalty: i64 = if terrain.needs_clearing() { -30 } else { 0 };

    let direct = direct_income_value(city, x, y, kind, connected);
    let synergy = synergy_income_value(city, x, y, kind, connected);
    let future = future_potential_value(city, x, y, kind, connected);
    let bias = strategy_bias(city.strategy, kind);

    // Eco 戦略は Forest セルを避けたいが、saturation 時に AI を完全 idle にすると
    // 「外を割れない、撤去もしない」二重死角になる (レビュー指摘 #1)。soft penalty
    // で「他に選択肢があれば森を残し、無ければ仕方なく切る」挙動に。
    let eco_forest_penalty = if matches!(city.strategy, Strategy::Eco)
        && matches!(terrain, super::terrain::Terrain::Forest)
    {
        -100
    } else {
        0
    };

    // ROI: cost を「秒単価」相当に換算して引く。cost $100 → -2 cents/sec
    // (= 50 秒で回収できると評価値ゼロ)。Outpost ($600) は -12、
    // House ($40) は -0.8 ≈ 0 に近い。
    let roi_penalty = kind.cost() / 50;

    direct + synergy + future + bias + clearing_penalty + eco_forest_penalty - roi_penalty
}

/// 1. 直接 income — 建物が tick 1 から稼ぐ cents/sec。
///
/// House: edge_connected なら 50 (Cottage)、未接続なら 25 (半減)。
///        ※ Apartment/Highrise への昇格は dwell time が必要なので即時は Cottage 想定。
/// Workshop: 隣接 House + edge-connected で 100。
/// Shop: edge-connected + 距離 3 以内 House で 200。
/// その他 (Road/Park/Outpost): 0。
fn direct_income_value(
    city: &City,
    x: usize,
    y: usize,
    kind: Building,
    connected: &[Vec<bool>],
) -> i64 {
    match kind {
        Building::House => {
            // House は配置の連結性で income が決まる。
            // (この時点では「将来 Apartment 化するか」は synergy 側で見る)
            let edge = is_building_edge_connected(connected, x, y);
            // House は隣接 Road が無いと活性化しない事実上の前提があるが、
            // SOFT ルールで Cottage も生きるので 25 を最低保証。
            if edge {
                50
            } else {
                25
            }
        }
        Building::Workshop => employment_demand_aware_value(
            city,
            x,
            y,
            connected,
            WORKSHOP_CAPACITY_CENTS,
            EMPLOYMENT_DEMAND_PER_CAPITA,
        ),
        Building::Factory => employment_demand_aware_value(
            city,
            x,
            y,
            connected,
            FACTORY_CAPACITY_CENTS,
            EMPLOYMENT_DEMAND_PER_CAPITA,
        ),
        Building::Office => employment_demand_aware_value(
            city,
            x,
            y,
            connected,
            OFFICE_CAPACITY_CENTS,
            WHITE_COLLAR_DEMAND_PER_CAPITA,
        ),
        Building::Shop => commercial_demand_aware_value(city, x, y, connected, SHOP_CAPACITY_CENTS),
        Building::Mall => commercial_demand_aware_value(city, x, y, connected, MALL_CAPACITY_CENTS),
        Building::Park | Building::Road | Building::Outpost => 0,
    }
}

/// 商業建物 (Shop / Mall) の direct income 評価 (cents/sec)。
///
/// 候補位置に置いた時の「需給按分後の収入」を概算する。AI Tier 4 が
/// 「人口が需要に対して足りない場所では Shop / Mall を建てない」と判断する核。
///
/// 計算量を抑えるため per-tile スキャンの簡易見積もり (実際の `commercial_income_cents`
/// より緩めの近似) — placement_value はランキング目的なので絶対値より相対順位が重要。
fn commercial_demand_aware_value(
    city: &City,
    x: usize,
    y: usize,
    connected: &[Vec<bool>],
    my_capacity: i64,
) -> i64 {
    let edge = is_building_edge_connected(connected, x, y);
    let near_house = has_house_within(city, x, y, 3);
    if !(edge && near_house) {
        return 0;
    }
    let local_pop = count_houses_within_radius_as_cottage(city, x, y, 5);
    let demand = local_pop * PURCHASE_POWER_PER_CAPITA;
    let total_capacity = commercial_capacity_within(city, x, y, 5) + my_capacity;
    let share = demand * my_capacity / total_capacity.max(1);
    share.min(my_capacity)
}

/// 雇用建物 (Workshop / Factory / Office) の direct income 評価。
fn employment_demand_aware_value(
    city: &City,
    x: usize,
    y: usize,
    connected: &[Vec<bool>],
    my_capacity: i64,
    demand_per_capita: i64,
) -> i64 {
    if !has_neighbor_kind(city, x, y, Building::House) {
        return 0;
    }
    if !is_building_edge_connected(connected, x, y) {
        return 0;
    }
    let local_pop = count_houses_within_radius_as_cottage(city, x, y, 5);
    let demand = local_pop * demand_per_capita;
    let total_capacity = employment_capacity_within(city, x, y, 5) + my_capacity;
    let share = demand * my_capacity / total_capacity.max(1);
    share.min(my_capacity)
}

/// AI 評価用の簡易局所人口集計。半径 R 内 House を全て Cottage 定員で数える。
///
/// `compute_population_map` 相当の精密計算は再帰参照になり重いため、AI が
/// 候補を順位付けする目的では Cottage 固定の下限値で十分 (相対比較が大事)。
/// bounded loop でマップ全走査を避ける。
fn count_houses_within_radius_as_cottage(city: &City, x: usize, y: usize, radius: i32) -> i64 {
    let mut local_pop = 0i64;
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            if dx.abs() + dy.abs() > radius {
                continue;
            }
            let nx = x as i32 + dx;
            let ny = y as i32 + dy;
            if nx < 0 || ny < 0 || nx >= GRID_W as i32 || ny >= GRID_H as i32 {
                continue;
            }
            if matches!(
                city.tile(nx as usize, ny as usize),
                Tile::Built(Building::House)
            ) {
                local_pop += house_capacity(HouseTier::Cottage) as i64;
            }
        }
    }
    local_pop
}

/// 2. シナジー — 周囲の既存 House の Tier が上昇しうる場合の income 増分。
///
/// `kind` が House/Workshop/Shop/Park/Road の時、それぞれの近傍 House の
/// `HouseNeighborhood` がどう変わり、Tier が変化するかを試算する。
/// ただし dwell time は無視 (= 「将来 Apartment になりうるなら +100」と楽観)。
fn synergy_income_value(
    city: &City,
    x: usize,
    y: usize,
    kind: Building,
    connected: &[Vec<bool>],
) -> i64 {
    let mut delta_cents = 0i64;
    // 影響範囲: kind ごとに Manhattan 距離。
    let radius: i32 = match kind {
        Building::Workshop | Building::Factory => 5,
        Building::Shop | Building::Mall => 5,
        Building::Office => 5,
        Building::Park => 4,
        Building::House => 3,
        Building::Road => 1,
        Building::Outpost => return 0,
    };
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            if dx.abs() + dy.abs() > radius {
                continue;
            }
            let nx = x as i32 + dx;
            let ny = y as i32 + dy;
            if nx < 0 || ny < 0 || nx >= GRID_W as i32 || ny >= GRID_H as i32 {
                continue;
            }
            let (nx, ny) = (nx as usize, ny as usize);
            if (nx, ny) == (x, y) {
                continue;
            }
            if !matches!(city.tile(nx, ny), Tile::Built(Building::House)) {
                continue;
            }
            // 既存 House (nx, ny) の現状 / 仮想 stats を比較。
            let cur = gather_house_neighborhood_with(city, nx, ny, connected);
            let mut after = cur;
            // kind を (x, y) に置いた時の stats 変化。
            match kind {
                Building::House => {
                    let manh = ((nx as i32 - x as i32).abs() + (ny as i32 - y as i32).abs()) as u32;
                    if manh <= 3 {
                        after.n_house_within_3 += 1;
                    }
                }
                Building::Workshop => after.n_workshop_within_5 += 1,
                Building::Factory => {
                    after.n_workshop_within_5 += 2;
                    let manh = ((nx as i32 - x as i32).abs() + (ny as i32 - y as i32).abs()) as u32;
                    if manh == 1 {
                        after.factory_smoke_penalty = true;
                    }
                }
                Building::Shop => after.n_shop_within_5 += 1,
                Building::Mall => after.n_shop_within_5 += 2,
                Building::Office => after.n_office_within_5 += 1,
                Building::Park => after.n_park_within_4 += 1,
                Building::Road => {
                    let manh = ((nx as i32 - x as i32).abs() + (ny as i32 - y as i32).abs()) as u32;
                    if manh == 1 {
                        after.n_road_adj += 1;
                    }
                }
                Building::Outpost => {}
            }
            let cur_tier = house_tier_for(cur);
            let new_tier = house_tier_for(after);
            if new_tier > cur_tier {
                let cur_inc = match cur_tier {
                    HouseTier::Cottage => 50,
                    HouseTier::Apartment => 150,
                    HouseTier::Highrise => 300,
                };
                let new_inc = match new_tier {
                    HouseTier::Cottage => 50,
                    HouseTier::Apartment => 150,
                    HouseTier::Highrise => 300,
                };
                delta_cents += new_inc - cur_inc;
            }
        }
    }
    delta_cents
}

/// 3. 将来潜在 — Road / Outpost が解禁する empty buildable cells の期待 income。
///
/// Road: 隣接 4-近傍の Empty buildable cells × 期待 cell income。
///       「道路を引くと、その隣に家を建てられるようになる」価値。
///       既に edge-connected ならボーナス。
/// Outpost: 4-近傍の Rock セル数 × 期待 cell income。Rock 解禁分。
fn future_potential_value(
    city: &City,
    x: usize,
    y: usize,
    kind: Building,
    connected: &[Vec<bool>],
) -> i64 {
    match kind {
        Building::Road => {
            // 既に edge-connected な道路に隣接していれば、新道路も即 edge-connected。
            // 隣接 Empty buildable cells の数 × 期待値の半分 (将来発生する income なので割引)。
            //
            // **マップ端の Road は edge-connected の seed**: 候補位置 (x, y) 自身が
            // マップ端にある場合は、建てれば即 BFS seed として機能する。
            // ループ不変条件なので事前に評価する (レビュー指摘 #5)。
            let at_map_edge = x == 0 || y == 0 || x == GRID_W - 1 || y == GRID_H - 1;
            let mut potential_cells = 0i64;
            let mut connects_to_edge =
                is_building_edge_connected(connected, x, y) || at_map_edge;
            for (dx, dy) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
                let nx = x as i32 + dx;
                let ny = y as i32 + dy;
                if nx < 0 || ny < 0 || nx >= GRID_W as i32 || ny >= GRID_H as i32 {
                    continue;
                }
                let (nx, ny) = (nx as usize, ny as usize);
                match city.tile(nx, ny) {
                    Tile::Empty => {
                        let t = city.terrain_at(nx, ny);
                        if t.buildable()
                            && (!t.needs_outpost() || has_outpost_neighbor(city, nx, ny))
                        {
                            potential_cells += 1;
                        }
                    }
                    Tile::Built(Building::Road) => {
                        if connected[ny][nx] {
                            connects_to_edge = true;
                        }
                    }
                    _ => {}
                }
            }
            // 孤立 Road (隣接 Built 0 + edge 未接続) は「将来何にも繋がらない」候補。
            // potential_cells = 0 なら下の式で 0、connects_to_edge = false で割引。
            // /3 ディスカウント: 将来潜在は割引しないと AI が道路を過剰選択する。
            let raw = potential_cells * FUTURE_CELL_EXPECTATION_CENTS / 3;
            if connects_to_edge {
                raw
            } else {
                // 未接続道路は半減。完全孤立 (0) なら 0。
                raw / 2
            }
        }
        Building::Outpost => {
            // Outpost は近傍 Rock を解禁する。Rock 解禁後はそのセルが建設可能になり、
            // 期待 income が乗る。Rock 数が多いほど評価高 = 飽和時に外周岩場が選ばれる。
            let n_rock = count_rock_neighbors(city, x, y) as i64;
            if n_rock == 0 {
                return 0;
            }
            // 「街と繋がっていない外周」だと建てても住人が来ない。
            // Manhattan 距離 4 以内に Built セルがある場所のみ評価。
            if !has_built_within_distance(city, x, y, 4) {
                return 0;
            }
            // 隣接 4 方向に edge-connected road があれば +20% (解禁後すぐ繋げられる)。
            let near_edge_road = [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)].iter().any(|&(dx, dy)| {
                let nx = x as i32 + dx;
                let ny = y as i32 + dy;
                if nx < 0 || ny < 0 || nx >= GRID_W as i32 || ny >= GRID_H as i32 {
                    return false;
                }
                connected[ny as usize][nx as usize]
            });
            // /4 ディスカウント: Outpost は cost $600 を回収するのに「Rock 解禁 →
            // 整地 → 建設」と 2-3 ステップ必要。即時 income と等価に評価すると
            // 30 min で 70 機材派遣して cash 枯渇する (実機ベンチで観測)。
            let base = n_rock * FUTURE_CELL_EXPECTATION_CENTS / 4;
            if near_edge_road {
                base * 12 / 10
            } else {
                base
            }
        }
        // House/Workshop/Shop/Park は future cell potential ゼロ (synergy / direct でカバー)。
        _ => 0,
    }
}

/// 4. 戦略バイアス — Strategy ボタンの「好み」を評価値に乗せる。
///
/// `strategy_info` の重み比率を直接使う代わりに、各 kind に「Strategy が
/// 優先するなら +N、嫌うなら -M」を返す形にすることで、評価ベース AI でも
/// 戦略の効果が出るようにする。
///
/// 効き方は弱め (合計で ±50 cents/sec 程度)。Strategy で建物選好が変わっても
/// 評価関数の主軸 (income/sec の真の予測) は崩さない。
fn strategy_bias(s: Strategy, kind: Building) -> i64 {
    match (s, kind) {
        // 成長: House 最優先、Office は Highrise 化に効くので軽く加点。
        (Strategy::Growth, Building::House) => 40,
        (Strategy::Growth, Building::Road) => 10,
        (Strategy::Growth, Building::Shop) => -20,
        (Strategy::Growth, Building::Office) => 15,
        // 収入: 商業 + 必要な住宅。Mall / Factory も積極的に。
        (Strategy::Income, Building::Shop) => 50,
        (Strategy::Income, Building::Mall) => 70,
        (Strategy::Income, Building::Workshop) => 25,
        (Strategy::Income, Building::Factory) => 45,
        (Strategy::Income, Building::Office) => 30,
        (Strategy::Income, Building::House) => 25,
        // 技術: 道路網拡大を最優先 + Office (テック企業のイメージ)。
        (Strategy::Tech, Building::Road) => 40,
        (Strategy::Tech, Building::House) => 10,
        (Strategy::Tech, Building::Office) => 25,
        // 環境: Park ボーナス + Factory は減点 (煙害)。
        (Strategy::Eco, Building::Park) => 60,
        (Strategy::Eco, Building::House) => 20,
        (Strategy::Eco, Building::Factory) => -50,
        _ => 0,
    }
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
            n_office_within_5: 0,
            n_house_within_3: n_house,
            n_park_within_4: 0,
            local_population: 0,
            factory_smoke_penalty: false,
            edge_connected: true,
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
            n_office_within_5: 0,
            n_house_within_3: n_house,
            n_park_within_4: n_park,
            local_population: 0,
            factory_smoke_penalty: false,
            edge_connected: true,
        }
    }

    /// Test helper: Road を (x, 0) に置いてマップ上端に「外と繋がる」起点を作る。
    /// Phase 2 の edge connectivity を満たさせるためのテスト用ユーティリティ。
    /// 何度呼んでも同じ Road を上書きするだけで副作用なし。
    fn place_edge_road(city: &mut City, x: usize) {
        city.set_tile(x, 0, Tile::Built(Building::Road));
    }

    /// Phase 2: edge_connected = false の場合は Cottage に縮退する (SOFT ルール)。
    #[test]
    fn unconnected_house_stays_cottage() {
        let stats = HouseNeighborhood {
            n_road_adj: 2,
            n_workshop_within_5: 2,
            n_shop_within_5: 2,
            n_office_within_5: 0,
            n_house_within_3: 4,
            n_park_within_4: 2,
            local_population: 0,
            factory_smoke_penalty: false,
            edge_connected: false,
        };
        assert_eq!(
            house_tier_for(stats),
            HouseTier::Cottage,
            "未接続街区は条件が揃っても Cottage 止まり"
        );
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
        // 上端に幹線道路を引いて全 House を edge-connected にする (Phase 2)。
        // (0..4, 1) に House、(0..4, 0) に Road、Road が上端 (y=0) なので edge-connected。
        for x in 0..4 {
            city.set_tile(x, 0, Tile::Built(Building::Road));
        }
        city.set_tile(0, 1, Tile::Built(Building::House));
        // 1 Cottage = 50¢/sec → $0 だが死スパイラル防止フォールバックで $1。
        assert_eq!(compute_income_per_sec(&city), 1);
        city.set_tile(1, 1, Tile::Built(Building::House));
        // 2 Cottages = 100¢ = $1。フォールバック不要。
        assert_eq!(compute_income_per_sec(&city), 1);
        city.set_tile(2, 1, Tile::Built(Building::House));
        // 3 Cottages = 150¢ = $1 (整数切り捨て、旧 ceil(3/2)=2 から変更)。
        // Tier-aware 収入は per-cent で正確に積算するので、半端は丸めで吸収する。
        assert_eq!(compute_income_per_sec(&city), 1);
        city.set_tile(3, 1, Tile::Built(Building::House));
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
    /// Phase 2: 隣接 Road が edge-connected であることも必須。
    #[test]
    fn workshop_needs_road_and_house_neighbors() {
        let mut city = City::new();
        // (5,1) に Workshop。隣接 Road (5,0) は上端で edge-connected。
        // 隣接 House はまだ無いので inactive。
        city.set_tile(5, 1, Tile::Built(Building::Workshop));
        place_edge_road(&mut city, 5);
        // House がないので fallback も働かず、Workshop も inactive で $0。
        assert_eq!(compute_income_per_sec(&city), 0);

        // 隣接 House を追加 → Workshop activate。
        city.set_tile(5, 2, Tile::Built(Building::House));
        // House (5,2): Cottage = 50¢ (edge-connected via Road at (5,0)... 待って、
        // House (5,2) の隣接 Road は (5,1) が Workshop なので Road 隣接 0。
        // よって edge_connected=false → Cottage で半減 25¢。
        // Workshop: active (隣接 Road (5,0) が edge-connected、隣接 House (5,2)) → 100¢
        // Total: 25¢ + 100¢ = 125¢ = $1。
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
    /// Phase 2: edge connectivity が必要なので、Road を上端 (y=0) と接続する。
    #[test]
    fn workshop_promotes_nearby_house_to_apartment() {
        let mut city = City::new();
        city.set_tile(1, 1, Tile::Built(Building::House));
        // (1,0) に Road を置いて House を edge-connected にする。
        place_edge_road(&mut city, 1);
        // House の隣接 (0,1) にも Road を置いて n_road_adj を稼ぐ。
        city.set_tile(0, 1, Tile::Built(Building::Road));
        // Workshop at (3,1): Manhattan distance 2 from (1,1)。
        city.set_tile(3, 1, Tile::Built(Building::Workshop));
        let tier = house_tier_for(gather_house_neighborhood(&city, 1, 1));
        assert_eq!(tier, HouseTier::Apartment);
    }

    #[test]
    fn shop_with_road_and_house_earns() {
        let mut city = City::new();
        // Shop の収入は需給連動 — 局所人口で決まる。
        // House を増やして商業需要を満たす検証に書き換える。
        city.set_tile(5, 5, Tile::Built(Building::Shop));
        for y in 0..=4 {
            city.set_tile(5, y, Tile::Built(Building::Road));
        }
        // (5,6) と (6,5) に House を置く。両方 Road 隣接で edge-connected。
        city.set_tile(5, 6, Tile::Built(Building::House));
        city.set_tile(6, 5, Tile::Built(Building::House));
        let income = compute_income_per_sec(&city);
        // Shop が活性 (Road接続 + 半径3 House) で income > 0 が成立。
        assert!(
            income > 0,
            "Shop with road + houses should produce positive income, got {}",
            income
        );
    }

    /// HouseTier は描画専用 — gather → tier_for で派生値が取れる。
    /// 道路接続 + Shop が距離 5 以内なら Apartment になる (描画切替の根拠)。
    /// Phase 2: edge connectivity を満たすため上端 Road を追加。
    #[test]
    fn house_with_road_and_shop_renders_as_apartment() {
        let mut city = City::new();
        city.set_tile(1, 1, Tile::Built(Building::House));
        // (1,0) Road が上端で edge-connected。
        place_edge_road(&mut city, 1);
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
        // Phase 3 で創設街路が中央列に置かれるので、+1 ずらして空セルを取る。
        let cx = GRID_W / 2 + 1;
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
        // Phase 3 で創設街路が cx 列にあるため +1 ずらす。
        let cx = GRID_W / 2 + 1;
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
        // Cottage 4 人定員のため、Town 閾値 50 を超えるには 13 軒以上必要。
        for i in 0..13 {
            city.set_tile(i, 0, Tile::Built(Building::House));
        }
        assert_eq!(city.last_observed_tier, CityTier::Village);
        tick(&mut city, 1);
        assert_eq!(city.last_observed_tier, CityTier::Town);
        assert!(city.tier_flash_until > city.tick);
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
        for i in 0..13 {
            city.set_tile(i, 0, Tile::Built(Building::House));
        }
        tick(&mut city, 1);
        let event_count_after_tier_event = city.events.len();
        // もう 1 軒追加 (まだ Town 範囲内: 14 軒 × 4 = 56 pop)。
        city.set_tile(14, 0, Tile::Built(Building::House));
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
        // Phase 3 創設街路を回避するため +1 ずらす。
        let cx = GRID_W / 2 + 1;
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
        // 外周 (0,0) に House を強制配置 — 64×32 マップでは d=48 でコスト
        // = 50 + 48² * 5 = $11,570 (旧 32×16 では $2,930 だった)。
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
    /// Phase 2: edge-connected 必須。中央 House を上端まで Road で繋ぐ。
    #[test]
    fn auto_demolish_returns_none_when_everything_active() {
        let mut city = City::new();
        city.cash = 10_000;
        let cx = GRID_W / 2;
        let cy = GRID_H / 2;
        // House を 1 軒、上端から中央まで Road で連結 (= edge-connected)。
        city.set_tile(cx, cy, Tile::Built(Building::House));
        for y in 0..cy {
            city.set_tile(cx + 1, y, Tile::Built(Building::Road));
        }
        city.set_tile(cx + 1, cy, Tile::Built(Building::Road));
        // 撤去候補なし (孤立 House でもなく、edge-connected で Cottage は半減 penalty も無く、
        // Shop/Workshop でもなく、Outpost でもない)。Road も Built (House) 隣接。
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
        // 中央コアは Rock が出ない (CORE_RADIUS=8)。Outpost を置けば「役目無し」状態。
        city.set_tile(cx, cy, Tile::Built(Building::Outpost));
        let (tx, ty, _) = auto_demolish_target(&city).expect("idle Outpost should be a candidate");
        assert_eq!((tx, ty), (cx, cy));
    }

    /// Outpost 派遣のテストは AI 評価関数 (`placement_value`) のテスト経由で
    /// 担保される (旧 dispatch_outpost / best_outpost_placement は AI 統合により廃止)。
    /// 「saturation 時に Outpost が高評価になる」性質は `placement_value_*` テストで確認。

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

    /// Tier 昇格で House の家賃収入が上がる (Cottage 50¢ → Apartment 150¢)。
    #[test]
    fn apartment_earns_more_than_cottage() {
        let mut city = City::new();
        // edge-connected な Road を上端 (y=0) に並べる。
        for x in 0..6 {
            place_edge_road(&mut city, x);
        }
        // (x, 1) に House を 5 軒並べる。Road 隣接 (上下) で edge-connected。
        for x in 0..5 {
            city.set_tile(x, 1, Tile::Built(Building::House));
        }
        // 半径 5 内に Shop を置いて Apartment 化条件を満たす。
        city.set_tile(2, 2, Tile::Built(Building::Shop));
        let cottage_income = compute_income_per_sec(&city);
        // 全 House の age を Apartment dwell 達成 (600 ticks) まで進める。
        city.tick = 600;
        for x in 0..5 {
            city.built_at_tick[1][x] = 0;
        }
        let apartment_income = compute_income_per_sec(&city);
        assert!(
            apartment_income > cottage_income,
            "Apartment should out-earn Cottage: cottage={} apt={}",
            cottage_income, apartment_income
        );
    }

    // ── 需給システム / 新建物 (Phase A 拡張) のテスト群 ────────────

    /// House Tier ごとの定員が単調増加 (Cottage < Apartment < Highrise)。
    #[test]
    fn house_capacity_is_monotone() {
        assert!(house_capacity(HouseTier::Cottage) < house_capacity(HouseTier::Apartment));
        assert!(house_capacity(HouseTier::Apartment) < house_capacity(HouseTier::Highrise));
    }

    /// City::population は Tier 連動で計算される。
    /// Cottage 4 + Apartment 12 + Highrise 30 が単純合算される。
    #[test]
    fn population_reflects_tier_capacity() {
        let mut city = City::new();
        // Cottage 1 軒のみ。Apartment 化条件を満たさず age=0。
        city.set_tile(0, 0, Tile::Built(Building::House));
        assert_eq!(city.population(), house_capacity(HouseTier::Cottage));
    }

    /// 需給ゲート: 局所人口が増えると Apartment 化に必要な経済密度が上がる。
    /// local_pop=0 では econ=1 で Apartment、local_pop=30 以上では econ=2 必要。
    #[test]
    fn demand_gate_blocks_apartment_when_supply_short() {
        // local_pop 30 (= Cottage 7-8 軒分) で economic_density 閾値が +1 上がる。
        let stats = HouseNeighborhood {
            n_road_adj: 1,
            n_workshop_within_5: 0,
            n_shop_within_5: 1,
            n_office_within_5: 0,
            n_house_within_3: 1,
            n_park_within_4: 0,
            local_population: 35, // ゲート発動
            factory_smoke_penalty: false,
            edge_connected: true,
        };
        // econ=1, 必要 2 → Apartment にならず Cottage に縮退。
        assert_eq!(house_tier_for(stats), HouseTier::Cottage);
    }

    /// 同条件で local_population が低い時は Apartment まで育つ (ゲート緩和)。
    #[test]
    fn low_demand_allows_apartment_with_minimal_supply() {
        let stats = HouseNeighborhood {
            n_road_adj: 1,
            n_workshop_within_5: 0,
            n_shop_within_5: 1,
            n_office_within_5: 0,
            n_house_within_3: 1,
            n_park_within_4: 0,
            local_population: 10, // ゲート未発動
            factory_smoke_penalty: false,
            edge_connected: true,
        };
        assert_eq!(house_tier_for(stats), HouseTier::Apartment);
    }

    /// Office は Highrise 化を促進する触媒。経済密度に 1.5x 寄与。
    #[test]
    fn office_promotes_to_highrise() {
        let stats = HouseNeighborhood {
            n_road_adj: 2,
            n_workshop_within_5: 0,
            n_shop_within_5: 1,
            n_office_within_5: 1, // 1 * 3 / 2 = 1 → econ=2
            n_house_within_3: 3,
            n_park_within_4: 0,
            local_population: 0,
            factory_smoke_penalty: false,
            edge_connected: true,
        };
        // economic_density = 1 (Shop) + 1 (Office) = 2 → Highrise 条件達成。
        assert_eq!(house_tier_for(stats), HouseTier::Highrise);
    }

    /// 隣接 Factory の煙害で Tier が 1 段下がる。
    #[test]
    fn factory_smoke_penalty_reduces_tier() {
        let stats = HouseNeighborhood {
            n_road_adj: 2,
            n_workshop_within_5: 0,
            n_shop_within_5: 2,
            n_office_within_5: 0,
            n_house_within_3: 4,
            n_park_within_4: 0,
            local_population: 0,
            factory_smoke_penalty: true, // 煙害 ON
            edge_connected: true,
        };
        // Highrise 条件を満たすが煙害で Apartment に降格。
        assert_eq!(house_tier_for(stats), HouseTier::Apartment);
    }

    /// Mall は Shop 2 つぶんの経済密度寄与 (n_shop_within_5 += 2)。
    /// 単独で Highrise 化条件を満たせる。
    #[test]
    fn mall_alone_drives_highrise() {
        let mut city = City::new();
        for x in 0..6 {
            place_edge_road(&mut city, x);
        }
        // (1,1) の n_road_adj=2 を確保: (1,0) Road + (0,1) Road の両方を用意。
        city.set_tile(0, 1, Tile::Built(Building::Road));
        city.set_tile(1, 1, Tile::Built(Building::House));
        city.set_tile(2, 1, Tile::Built(Building::House));
        city.set_tile(3, 1, Tile::Built(Building::House));
        city.set_tile(1, 2, Tile::Built(Building::House));
        city.set_tile(3, 2, Tile::Built(Building::Mall));
        let tier = house_tier_for(gather_house_neighborhood(&city, 1, 1));
        // n_road_adj=2, n_shop_within_5=2 (Mall x2), n_house_within_3>=3 → Highrise。
        assert_eq!(tier, HouseTier::Highrise);
    }

    /// Shop の収入は局所人口に応じて変化する: 人口少ない時は低収入、
    /// 人口増えると上限まで伸びる (需給連動の核)。
    #[test]
    fn shop_income_scales_with_population() {
        let mut city = City::new();
        for x in 0..10 {
            place_edge_road(&mut city, x);
        }
        // 縦の Road を引いて Shop の Road 隣接を確保。
        city.set_tile(0, 1, Tile::Built(Building::Road));
        city.set_tile(0, 2, Tile::Built(Building::Road));
        // House 1 軒 + Shop。
        city.set_tile(1, 1, Tile::Built(Building::House));
        city.set_tile(1, 2, Tile::Built(Building::Shop));
        let connected = compute_edge_connected_roads(&city);
        let pop_map = compute_population_map(&city, &connected);
        let small_income =
            commercial_income_cents(&city, 1, 2, SHOP_CAPACITY_CENTS, &pop_map, &connected);

        // House を増やして同じ Shop の収入を再計測。
        for x in 2..8 {
            city.set_tile(x, 1, Tile::Built(Building::House));
        }
        let connected = compute_edge_connected_roads(&city);
        let pop_map = compute_population_map(&city, &connected);
        let big_income =
            commercial_income_cents(&city, 1, 2, SHOP_CAPACITY_CENTS, &pop_map, &connected);
        assert!(
            big_income > small_income,
            "Shop income should scale with population: small={} big={}",
            small_income, big_income
        );
    }

    /// Mall は Shop の 3 倍のキャパシティ。Shop 上限を超える需要では Mall が稼ぐ。
    #[test]
    fn mall_outearns_shop_at_high_population() {
        let mut city = City::new();
        for x in 0..20 {
            place_edge_road(&mut city, x);
        }
        // (8, 1) を Road にして edge-connected な縦線を確保。
        city.set_tile(8, 1, Tile::Built(Building::Road));
        // House を y=1 と y=2 に大量配置 (Shop 上限超えの需要を作る)。
        for x in 1..16 {
            if x != 8 {
                city.set_tile(x, 1, Tile::Built(Building::House));
                city.set_tile(x, 2, Tile::Built(Building::House));
            }
        }
        // Shop / Mall を (8, 2) に。隣接 Road (8,1) edge-connected + 隣接 House (7,2)/(9,2) で active。
        city.set_tile(8, 2, Tile::Built(Building::Shop));
        let connected = compute_edge_connected_roads(&city);
        let pop_map = compute_population_map(&city, &connected);
        let shop_income =
            commercial_income_cents(&city, 8, 2, SHOP_CAPACITY_CENTS, &pop_map, &connected);
        city.set_tile(8, 2, Tile::Built(Building::Mall));
        let connected = compute_edge_connected_roads(&city);
        let pop_map = compute_population_map(&city, &connected);
        let mall_income =
            commercial_income_cents(&city, 8, 2, MALL_CAPACITY_CENTS, &pop_map, &connected);
        assert!(
            mall_income > shop_income,
            "Mall should out-earn Shop with high population: shop={} mall={}",
            shop_income, mall_income
        );
    }

    /// Factory は Workshop の 3 倍以上のキャパシティ。Workshop 上限を超える雇用需要で Factory が稼ぐ。
    #[test]
    fn factory_outearns_workshop_at_high_population() {
        let mut city = City::new();
        for x in 0..20 {
            place_edge_road(&mut city, x);
        }
        // (8, 1) を Road にして edge-connected な縦線を確保。
        city.set_tile(8, 1, Tile::Built(Building::Road));
        for x in 1..8 {
            city.set_tile(x, 1, Tile::Built(Building::House));
        }
        for x in 9..16 {
            city.set_tile(x, 1, Tile::Built(Building::House));
        }
        // Workshop / Factory の隣接 House を確保 (workshop_is_active 条件)。
        city.set_tile(7, 2, Tile::Built(Building::House));
        city.set_tile(8, 2, Tile::Built(Building::Workshop));
        let connected = compute_edge_connected_roads(&city);
        let pop_map = compute_population_map(&city, &connected);
        let workshop_income = employment_income_cents(
            &city,
            8,
            2,
            WORKSHOP_CAPACITY_CENTS,
            EMPLOYMENT_DEMAND_PER_CAPITA,
            &pop_map,
            &connected,
        );
        city.set_tile(8, 2, Tile::Built(Building::Factory));
        let connected = compute_edge_connected_roads(&city);
        let pop_map = compute_population_map(&city, &connected);
        let factory_income = employment_income_cents(
            &city,
            8,
            2,
            FACTORY_CAPACITY_CENTS,
            EMPLOYMENT_DEMAND_PER_CAPITA,
            &pop_map,
            &connected,
        );
        assert!(
            factory_income > workshop_income,
            "Factory should out-earn Workshop: workshop={} factory={}",
            workshop_income, factory_income
        );
    }

    /// 過剰店舗ペナルティ: 同範囲に Shop を増やすと 1 軒あたりの収入が減る。
    /// 需要が中庸 (Shop 上限未達) なシナリオで「按分が効いた」ことを strict に確認。
    #[test]
    fn excess_shops_split_demand() {
        let mut city = City::new();
        for x in 0..16 {
            place_edge_road(&mut city, x);
        }
        city.set_tile(0, 1, Tile::Built(Building::Road));
        city.set_tile(0, 2, Tile::Built(Building::Road));
        city.set_tile(0, 3, Tile::Built(Building::Road));
        // House 4 軒で local_pop ≈ 16 → demand 64 cents (Shop 上限 200 未達)。
        for x in 1..5 {
            city.set_tile(x, 1, Tile::Built(Building::House));
        }
        city.set_tile(1, 2, Tile::Built(Building::Shop));
        let connected = compute_edge_connected_roads(&city);
        let pop_map = compute_population_map(&city, &connected);
        let solo = commercial_income_cents(&city, 1, 2, SHOP_CAPACITY_CENTS, &pop_map, &connected);

        // 同範囲に Shop を追加すると total_capacity が倍 → 1 軒あたりの share が半減。
        city.set_tile(1, 3, Tile::Built(Building::Shop));
        let connected = compute_edge_connected_roads(&city);
        let pop_map = compute_population_map(&city, &connected);
        let with_competition =
            commercial_income_cents(&city, 1, 2, SHOP_CAPACITY_CENTS, &pop_map, &connected);
        assert!(
            with_competition < solo,
            "Adding competing shop should strictly reduce single shop income: solo={} comp={}",
            solo, with_competition
        );
    }

    /// 老朽化した Cottage は wastefulness_score が加算され、撤去候補になりやすい。
    /// Phase 3 創設街路 (中央列) を回避するため +5 ずらした孤立位置を使う
    /// (= !edge_ok で +60、孤立 House で +80、合計 140 が cost penalty を上回る)。
    #[test]
    fn aged_cottage_becomes_demolish_candidate() {
        let mut city = City::new();
        let cx = GRID_W / 2 + 5;
        let cy = GRID_H / 2;
        // 完全孤立した House を置く (隣接 Road も House も無い)。
        city.set_tile(cx, cy, Tile::Built(Building::House));
        city.tick = 5000;
        city.built_at_tick[cy][cx] = 0; // 老朽化計算はスキップ (built_at_tick == 0)
        let score = wastefulness_score(&city, cx, cy);
        assert!(
            score.is_some() && score.unwrap() > 0,
            "Aged isolated cottage should be a demolish candidate: {:?}",
            score
        );
    }
}
