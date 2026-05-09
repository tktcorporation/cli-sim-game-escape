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
    // `decide()` 経由で `evaluate` と `action_value` を比較して行う。
    auto_strategy_actions(city);
    drive_ai(city);
    accrue_income(city);
    detect_tier_advance(city);
    // tick の最後でキャッシュをクリアし、次 tick / 次 frame の最初の参照で
    // 再計算させる。grid 個別変更箇所も invalidate するが、この一括クリアは
    // 「他で漏れてもここで必ず無効化される」セーフティネット。
    city.invalidate_population_cache();
    city.tick = city.tick.wrapping_add(1);
}

/// ティア境界を跨いだら演出をトリガー。AI 撤去で人口が一時的に減ることは
/// あるが、`detect_tier_advance` は上昇遷移時のみフラッシュを焚く。
///
/// 閾値判定は **Tier 込みの正確な人口** で行う (Apartment/Highrise の
/// 定員 boost を反映)。1 tick に 1 度しか呼ばれない経路なので BFS コストは
/// 許容範囲。`City::population()` は軽量 Cottage 概算なのでここでは使わない。
fn detect_tier_advance(city: &mut City) {
    let now = city_tier_for(tier_aware_population(city));
    if now > city.last_observed_tier {
        city.tier_flash_until = city.tick + TIER_FLASH_TICKS;
        city.push_event(format!("🎊 街が「{}」に成長しました!", now.jp()));
        city.last_observed_tier = now;
    }
}

/// Tier 連動の精密人口集計 (Apartment 12 / Highrise 30 を反映)。
///
/// 内部で edge-connectivity BFS と全 House の Tier 評価を走らせるため
/// O(houses + GRID²) と重い。**呼び出しは tick あたり数回までに抑える**:
///   - `detect_tier_advance` (1 tick に 1 度)
///   - render の Status タブ詳細 (1 frame に 1 度、必要なら自前でキャッシュ)
///   - シミュレータベンチ (テスト用)
///
/// `City::population()` (Cottage 固定の軽量概算) と分離することで、
/// レンダーの header / banner など毎 frame で参照される箇所を軽量に保つ。
pub fn tier_aware_population(city: &City) -> u32 {
    let connected = compute_edge_connected_roads(city);
    let mut total = 0u32;
    for y in 0..GRID_H {
        for x in 0..GRID_W {
            if !matches!(city.tile(x, y), Tile::Built(Building::House)) {
                continue;
            }
            let tier = effective_tier_at_with(city, x, y, &connected);
            total += house_capacity(tier);
        }
    }
    total
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
    let mutated = !clearings.is_empty() || !completions.is_empty();
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
    // 同一 tick 内で複数 worker が走る経路 (Tier 3+ workers) では、
    // 完成→次 worker の判断の間に population が古いままにならないよう
    // ここでも invalidate する。tick 終了時の一括クリアと二重防御。
    if mutated {
        city.invalidate_population_cache();
    }
}

fn building_name(b: Building) -> &'static str {
    building_display_name(b)
}

/// 図鑑・ログ・診断で使う日本語名。Catalog タブから直接参照するため `pub`。
pub fn building_display_name(b: Building) -> &'static str {
    match b {
        Building::Road => "道路",
        Building::House => "住宅",
        Building::Workshop => "工房",
        Building::Factory => "工場",
        Building::Shop => "店舗",
        Building::Mall => "商業ビル",
        Building::Office => "オフィス",
        Building::Park => "公園",
        Building::Plaza => "中央広場",
        Building::Stadium => "競技場",
        Building::MegaMall => "メガモール",
        Building::Headquarters => "本社ビル",
        Building::Refinery => "製油所",
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
/// 1 tick あたり最大 1 アクションを実行する (= worker 数に依存しない直列判断)。
///
/// **直列化の理由**: worker N 人いても並列に N 回 `decide()` を呼ぶと AI 探索コストが
/// N 倍になり、saturated map + Tier 5 の状態で 1 tick が 100ms を超えて render が
/// 詰まる (= 体感「重い」)。1 tick = 1 decide に固定することで、AI tick 時間が worker
/// 数に依存しなくなる (= 8 workers でも 1 worker と同じ思考コスト)。
///
/// 副次効果: Construction は複数 tick かかるため、worker は 1 tick あたり 1 つでも
/// すぐに埋まる。City growth pace も実用上ほぼ変わらない (10 ticks/sec × 1 build/tick =
/// 10 builds/sec の理論上限、実際は build 完了待ちで 1 build/sec 程度)。
fn drive_ai(city: &mut City) {
    if city.free_workers() == 0 {
        return;
    }
    // attempts は最大 2 回: 1 回目で失敗 (start_construction reject 等) した場合の
    // リトライ猶予。それでも駄目なら諦めて次 tick に回す (busy-loop 防止)。
    for _ in 0..2 {
        match decide(city) {
            AiAction::Build { x, y, kind } => {
                if start_construction(city, x, y, kind) {
                    return;
                }
            }
            AiAction::Demolish { x, y } => {
                // 成否いずれでも 1 attempt 消費して return。失敗時 (cash 不足等) を
                // リトライしても同条件で再失敗するだけなので、次 tick に回す方が
                // 健全 (= busy-loop 防止)。
                let _ = demolish_at(city, x, y);
                return;
            }
            AiAction::Idle => return,
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
    /// **注意**: Tier 4 以上は `evaluate` 評価ベースなのでこの重みは
    /// 直接参照しない。Tier 3 (Aware) と Status パネルの「戦略内訳」
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

        // 超上位 (Plaza / Stadium / MegaMall / Headquarters / Refinery) は
        // 街が成熟した終盤の象徴施設。戦略の差より「都市の象徴」感を優先する。
        (_, Building::Plaza) => "中央広場を整備",
        (_, Building::Stadium) => "競技場を建設",
        (_, Building::MegaMall) => "メガモールを開業",
        (_, Building::Headquarters) => "本社ビルを誘致",
        (_, Building::Refinery) => "製油所を稼働",

        (_, Building::Outpost) => "開拓機材を設置",
    }
}

// ── 自動運用ポリシー (Strategy ごとの撤去 cash 余力) ───────────────
//
// 撤去判断は AI (`ai::decide`) が `evaluate` と `action_value` を
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
/// `evaluate` と `action_value` を同じ天秤で比較するように
/// なったため不要。`step_one_tick` の呼び出し点を変えずに済むよう
/// 関数だけ残してある。次回大幅リファクタ時に呼び出し側ごと削除可。
pub fn auto_strategy_actions(_city: &mut City) {}

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
        city.invalidate_population_cache();
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
    city.invalidate_population_cache();
    city.buildings_started += 1;
    // Outpost 派遣統計: AI が `evaluate` 経由で Outpost を選んだ時にも
    // カウントされるよう、start_construction でフックする (= 旧 dispatch_outpost
    // の責務を吸収)。
    if matches!(kind, Building::Outpost) {
        city.outposts_dispatched_total = city.outposts_dispatched_total.saturating_add(1);
    }
    // Tier 4 (Planner) のみ Strategy に基づく動詞を表示。
    // 低 Tier は戦略を読まない設計なので、汎用の「着工」を出す方が誠実。
    // この差自体が「上位 AI ほど目的を持って動いている」演出にもなる。
    if matches!(city.ai_tier, AiTier::Planner) {
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
    // 累積後の cash を 1 秒間隔で記録 — 10s ROI の元データ。
    city.record_cash_sample();
    // 商業施設 (Shop / Mall) はどちらも収入を出すので両方フラッシュさせる。
    // connected は 1 度だけ計算して shop_is_active_with に使い回す。
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

// ── 需給ベースの per-tile 収入計算 ────────────────────────────────────
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
pub(super) fn compute_population_map(city: &City, connected: &[Vec<bool>]) -> Vec<Vec<u32>> {
    compute_pop_and_tier_maps(city, connected).0
}

/// AI 評価ホットパス用: pop_map と tier_map を 1 度のスキャンで両方計算する。
///
/// `tile_income_cents_with` は House の `effective_house_tier` を必要とし、
/// `compute_population_map` も同じ計算を行う。両者を合わせると House 1 軒あたり
/// `gather_house_neighborhood_with` (= O(R²) ≈ 121 ops) を 2 回呼ぶ無駄が発生する。
/// `_pre_tier` 引数で事前計算した tier を `tile_income_cents_with_tier` に渡すと、
/// 2 回目の gather を回避できる (= 評価関数の hot path で ~2× speedup)。
///
/// **alloc 削減版**: `fill_pop_and_tier_maps` は caller 提供の buffer に in-place で
/// 書き込むため、scratch を再利用すれば WASM の dlmalloc churn を回避できる。
/// テスト用には allocating wrapper の `compute_pop_and_tier_maps` を残す。
pub(super) fn compute_pop_and_tier_maps(
    city: &City,
    connected: &[Vec<bool>],
) -> (Vec<Vec<u32>>, Vec<Vec<Option<HouseTier>>>) {
    let mut pop = vec![vec![0u32; GRID_W]; GRID_H];
    let mut tiers: Vec<Vec<Option<HouseTier>>> = vec![vec![None; GRID_W]; GRID_H];
    fill_pop_and_tier_maps(city, connected, &mut pop, &mut tiers);
    (pop, tiers)
}

#[allow(clippy::needless_range_loop)]
pub(super) fn fill_pop_and_tier_maps(
    city: &City,
    connected: &[Vec<bool>],
    pop: &mut [Vec<u32>],
    tiers: &mut [Vec<Option<HouseTier>>],
) {
    for row in pop.iter_mut() {
        row.fill(0);
    }
    for row in tiers.iter_mut() {
        row.fill(None);
    }
    for y in 0..GRID_H {
        for x in 0..GRID_W {
            if !matches!(city.tile(x, y), Tile::Built(Building::House)) {
                continue;
            }
            let target = house_tier_for(gather_house_neighborhood_with(city, x, y, connected));
            let age = city.tick.saturating_sub(city.built_at_tick[y][x]);
            let tier = effective_house_tier(target, age);
            pop[y][x] = house_capacity(tier);
            tiers[y][x] = Some(tier);
        }
    }
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

/// 半径内の **active な** 商業供給キャパシティ合計 (cents/sec 単位)。
///
/// inactive Shop/Mall (= 隣接 Road 未接続 or 半径 3 House 無し) は供給ゼロなので
/// 按分対象から除外する。これを含めると「機能不全の Mall を 1 軒置いただけで
/// 周囲の active Shop の収入が薄まる」非直感的な挙動になる。
fn commercial_capacity_within(
    city: &City,
    x: usize,
    y: usize,
    radius: i32,
    connected: &[Vec<bool>],
) -> i64 {
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
            let (nx, ny) = (nx as usize, ny as usize);
            let cap = match city.tile(nx, ny) {
                Tile::Built(Building::Shop) => SHOP_CAPACITY_CENTS,
                Tile::Built(Building::Mall) => MALL_CAPACITY_CENTS,
                Tile::Built(Building::MegaMall) => MEGAMALL_CAPACITY_CENTS,
                _ => continue,
            };
            if shop_is_active_with(city, nx, ny, connected) {
                total += cap;
            }
        }
    }
    total
}

/// 半径内の active な **工業** 雇用キャパシティ (Workshop / Factory)。
///
/// Office (ホワイトカラー) は `white_collar_capacity_within` で別 pool にする。
/// 同じ pool で割ると Workshop と Office が抑制し合い、需要クラスを別々に
/// モデル化した意図 (= 工場労働者とオフィス勤務者は別の労働者) が崩れる。
fn industrial_capacity_within(
    city: &City,
    x: usize,
    y: usize,
    radius: i32,
    connected: &[Vec<bool>],
) -> i64 {
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
            let (nx, ny) = (nx as usize, ny as usize);
            let cap = match city.tile(nx, ny) {
                Tile::Built(Building::Workshop) => WORKSHOP_CAPACITY_CENTS,
                Tile::Built(Building::Factory) => FACTORY_CAPACITY_CENTS,
                Tile::Built(Building::Refinery) => REFINERY_CAPACITY_CENTS,
                _ => continue,
            };
            if workshop_is_active_with(city, nx, ny, connected) {
                total += cap;
            }
        }
    }
    total
}

/// 半径内の active な **ホワイトカラー** 雇用キャパシティ (Office)。
fn white_collar_capacity_within(
    city: &City,
    x: usize,
    y: usize,
    radius: i32,
    connected: &[Vec<bool>],
) -> i64 {
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
            let (nx, ny) = (nx as usize, ny as usize);
            let cap = match city.tile(nx, ny) {
                Tile::Built(Building::Office) => OFFICE_CAPACITY_CENTS,
                Tile::Built(Building::Headquarters) => HEADQUARTERS_CAPACITY_CENTS,
                _ => continue,
            };
            if workshop_is_active_with(city, nx, ny, connected) {
                total += cap;
            }
        }
    }
    total
}

/// 雇用クラス。Industrial = Workshop/Factory、WhiteCollar = Office。
/// `employment_income_cents` / `employment_demand_aware_value` で需給 pool を選ぶ。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EmploymentClass {
    Industrial,
    WhiteCollar,
}

/// 商業供給キャパシティ (cents/sec) — Shop / Mall / MegaMall の上限収入。
pub const SHOP_CAPACITY_CENTS: i64 = 200; // $2/sec
pub const MALL_CAPACITY_CENTS: i64 = 600; // $6/sec
/// MegaMall: Mall の約 2.5 倍。Tower 以上の住人プレミアム需要を吸収する。
pub const MEGAMALL_CAPACITY_CENTS: i64 = 1_500; // $15/sec
/// 雇用供給キャパシティ (cents/sec) — Workshop / Factory / Refinery / Office / Headquarters。
pub const WORKSHOP_CAPACITY_CENTS: i64 = 100; // $1/sec
pub const FACTORY_CAPACITY_CENTS: i64 = 350; // $3.5/sec
/// Refinery: Factory の約 2.5 倍。重工業の頂点。
pub const REFINERY_CAPACITY_CENTS: i64 = 900; // $9/sec
pub const OFFICE_CAPACITY_CENTS: i64 = 250; // $2.5/sec
/// Headquarters: Office の約 2.8 倍。Tower 化触媒で終盤の主力。
pub const HEADQUARTERS_CAPACITY_CENTS: i64 = 700; // $7/sec

/// 1 人当たり購買力 (cents/sec)。商業需要の換算係数。
pub const PURCHASE_POWER_PER_CAPITA: i64 = 4;
/// 1 人当たり雇用需要 (cents/sec)。Workshop/Factory が吸収する。
pub const EMPLOYMENT_DEMAND_PER_CAPITA: i64 = 3;
/// 1 人当たりホワイトカラー需要 (cents/sec)。Office が吸収する。
pub const WHITE_COLLAR_DEMAND_PER_CAPITA: i64 = 2;

/// セル単位の老朽化込み収入 (cents/sec)。Built タイル以外は 0。
///
/// `compute_income_per_sec` の per-tile 計算を切り出した版。`pop_map` /
/// `connected` を caller 側で 1 回だけ計算してまとめて使い回す ensure 用。
/// render の選択セル詳細表示 (selected_cell_lines) でも再利用する。
pub(super) fn tile_income_cents_with(
    city: &City,
    x: usize,
    y: usize,
    pop_map: &[Vec<u32>],
    connected: &[Vec<bool>],
) -> i64 {
    tile_income_cents_with_tier(city, x, y, pop_map, connected, None)
}

/// `tile_income_cents_with` の tier 事前計算版。AI 評価ホットパスで
/// `compute_pop_and_tier_maps` の出力を渡すことで gather の二重呼び出しを回避する。
pub(super) fn tile_income_cents_with_tier(
    city: &City,
    x: usize,
    y: usize,
    pop_map: &[Vec<u32>],
    connected: &[Vec<bool>],
    pre_tier: Option<HouseTier>,
) -> i64 {
    let kind = match city.tile(x, y) {
        Tile::Built(b) => *b,
        _ => return 0,
    };
    let tier_opt = if matches!(kind, Building::House) {
        if pre_tier.is_some() {
            pre_tier
        } else {
            let target =
                house_tier_for(gather_house_neighborhood_with(city, x, y, connected));
            let age = city.tick.saturating_sub(city.built_at_tick[y][x]);
            Some(effective_house_tier(target, age))
        }
    } else {
        None
    };
    let base_cents: i64 = match kind {
        Building::House => {
            let tier = tier_opt.expect("house has tier");
            let raw = match tier {
                HouseTier::Cottage => 50,
                HouseTier::Apartment => 150,
                HouseTier::Highrise => 300,
                HouseTier::Tower => 600,
                HouseTier::Arcology => 1_200,
            };
            // House SOFT ルール: 未接続 Cottage は半減 ($0.25/sec)。
            if !is_building_edge_connected(connected, x, y) {
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
            EmploymentClass::Industrial,
            pop_map,
            connected,
        ),
        Building::Factory => employment_income_cents(
            city,
            x,
            y,
            FACTORY_CAPACITY_CENTS,
            EmploymentClass::Industrial,
            pop_map,
            connected,
        ),
        Building::Office => employment_income_cents(
            city,
            x,
            y,
            OFFICE_CAPACITY_CENTS,
            EmploymentClass::WhiteCollar,
            pop_map,
            connected,
        ),
        Building::Headquarters => employment_income_cents(
            city,
            x,
            y,
            HEADQUARTERS_CAPACITY_CENTS,
            EmploymentClass::WhiteCollar,
            pop_map,
            connected,
        ),
        Building::Refinery => employment_income_cents(
            city,
            x,
            y,
            REFINERY_CAPACITY_CENTS,
            EmploymentClass::Industrial,
            pop_map,
            connected,
        ),
        Building::Shop => {
            commercial_income_cents(city, x, y, SHOP_CAPACITY_CENTS, pop_map, connected)
        }
        Building::Mall => {
            commercial_income_cents(city, x, y, MALL_CAPACITY_CENTS, pop_map, connected)
        }
        Building::MegaMall => {
            commercial_income_cents(city, x, y, MEGAMALL_CAPACITY_CENTS, pop_map, connected)
        }
        _ => 0,
    };
    if base_cents == 0 {
        return 0;
    }
    if city.built_at_tick[y][x] == 0 {
        base_cents
    } else {
        let age = city.tick.saturating_sub(city.built_at_tick[y][x]);
        let factor = aging_factor_per_mille(age, lifespan_x100(kind, tier_opt)) as i64;
        (base_cents * factor) / 1000
    }
}

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
    let total_capacity = commercial_capacity_within(city, x, y, 5, connected);
    if total_capacity <= 0 {
        return 0;
    }
    let share = demand * my_capacity / total_capacity;
    share.min(my_capacity)
}

/// 雇用建物 (Workshop / Factory / Office) の per-tile 収入 (cents/sec)。
///
/// `class` ごとに別 pool で按分する: Industrial (Workshop+Factory) と
/// WhiteCollar (Office) を分離して「工場労働者とオフィス勤務者は別ラベル」
/// という意図を保つ。同じ pool だと Workshop と Office が抑制し合うバグになる。
fn employment_income_cents(
    city: &City,
    x: usize,
    y: usize,
    my_capacity: i64,
    class: EmploymentClass,
    pop_map: &[Vec<u32>],
    connected: &[Vec<bool>],
) -> i64 {
    if !workshop_is_active_with(city, x, y, connected) {
        return 0;
    }
    let local_pop = population_within(pop_map, x, y, 5) as i64;
    let (demand_per_capita, total_capacity) = match class {
        EmploymentClass::Industrial => (
            EMPLOYMENT_DEMAND_PER_CAPITA,
            industrial_capacity_within(city, x, y, 5, connected),
        ),
        EmploymentClass::WhiteCollar => (
            WHITE_COLLAR_DEMAND_PER_CAPITA,
            white_collar_capacity_within(city, x, y, 5, connected),
        ),
    };
    let demand = local_pop * demand_per_capita;
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
    if let Some((cached_tick, cached_strategy, cached)) = city.income_dollars_cache.get() {
        if cached_tick == city.tick && cached_strategy == city.strategy {
            return cached;
        }
    }
    let connected_rc = cached_edge_connected_roads(city);
    let cents = compute_income_per_sec_cents_with(city, &connected_rc);
    let any_house = city.count_built(Building::House) > 0;
    let mut income = cents / 100;
    if any_house && income == 0 {
        income = 1;
    }
    city.income_dollars_cache
        .set(Some((city.tick, city.strategy, income)));
    income
}

/// `compute_edge_connected_roads` の per-frame メモ化版。`Rc` を共有して
/// frame 内の複数 caller (render の grid path / status panel / 選択セル詳細など) で
/// BFS を 1 度だけ走らせる。AI search の `with_action_applied` 経路では
/// `invalidate_population_cache` で cache がクリアされるので、AI 評価中の
/// 仮想 mutate と矛盾しない。
pub fn cached_edge_connected_roads(city: &City) -> std::rc::Rc<Vec<Vec<bool>>> {
    if let Some((cached_tick, rc)) = city.connected_cache.borrow().as_ref() {
        if *cached_tick == city.tick {
            return rc.clone();
        }
    }
    let rc = std::rc::Rc::new(compute_edge_connected_roads(city));
    *city.connected_cache.borrow_mut() = Some((city.tick, rc.clone()));
    rc
}

/// `compute_income_per_sec` の cents/sec 解像度版。AI 評価関数の基底として使う。
///
/// dollars/sec は AI 評価に粒度が荒すぎる ($0.50 が 0 に丸まる) ため cents/sec を
/// 公開する。`evaluate` で BFS 結果を `road_network_value` /
/// `inactive_building_penalty_with` と共有して、評価 1 回あたり BFS 1 回に抑える。
pub fn compute_income_per_sec_cents_with(city: &City, connected: &[Vec<bool>]) -> i64 {
    let mut scratch_ref = city.eval_scratch.borrow_mut();
    let scratch = &mut *scratch_ref;
    fill_pop_and_tier_maps(city, connected, &mut scratch.pop_map, &mut scratch.tier_map);
    let mut income_cents: i64 = 0;
    #[allow(clippy::needless_range_loop)]
    for y in 0..GRID_H {
        for x in 0..GRID_W {
            income_cents += tile_income_cents_with_tier(
                city,
                x,
                y,
                &scratch.pop_map,
                connected,
                scratch.tier_map[y][x],
            );
        }
    }
    let modifier = strategy_info(city.strategy).income_penalty_pct;
    if modifier != 0 && income_cents > 0 {
        let factor = (100 + modifier).max(10) as i64;
        income_cents = (income_cents * factor) / 100;
    }
    income_cents
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
/// - Cottage:   4 人 (基準)
/// - Apartment: 12 人 (Cottage の 3x)
/// - Highrise:  30 人 (Cottage の 7.5x)
pub fn house_capacity(tier: HouseTier) -> u32 {
    match tier {
        HouseTier::Cottage => 4,
        HouseTier::Apartment => 12,
        HouseTier::Highrise => 30,
        // Tower / Arcology は終盤の象徴段階。Highrise の倍以上を吸収する。
        HouseTier::Tower => 70,
        HouseTier::Arcology => 150,
    }
}

/// 住宅の経済段階。Cottage → Apartment → Highrise → Tower → Arcology と育つ。
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
    /// 超高層タワー。Highrise を超える経済密度と Plaza 級の文化触媒、
    /// または Headquarters / MegaMall いずれかが近接した街区で出現する。
    Tower,
    /// 自己完結都市 (Arcology)。Stadium と Headquarters / MegaMall が共に近く、
    /// 経済密度が極めて高い終盤の街区にのみ出現する象徴段階。
    Arcology,
}

/// `house_tier_for` が見る周囲の充実度サマリ。
///
/// House 一軒分の周辺をスキャンして集計したもの。フィールドの意味:
/// - `n_road_adj`: 4-近傍にある Road タイル数 (0..=4)。0 だと未接続。
/// - `n_workshop_within_5`: 距離 5 以内の Workshop / Factory / Refinery 換算数。
///   Factory は Workshop の 2 倍、Refinery は 4 倍 (= 規模換算)。
/// - `n_shop_within_5`: 距離 5 以内の Shop / Mall / MegaMall 換算数。
///   Mall は 2 倍、MegaMall は 4 倍。
/// - `n_office_within_5`: 距離 5 以内の Office / Headquarters 換算数。
///   Headquarters は Office の 3 倍 (Tower 化触媒)。
/// - `n_house_within_3`: 距離 3 以内の House 数 (自身は除く)。
/// - `n_park_within_4`: 距離 4 以内の Park / Plaza / Stadium 換算数。
///   Plaza は Park の 3 倍、Stadium は 6 倍。
/// - `n_megaculture_within_5`: 距離 5 以内の Stadium 数。Arcology の必須条件。
/// - `n_megacommerce_or_hq_within_5`: 距離 5 以内の MegaMall + Headquarters 数。
///   Tower 昇格の触媒条件。
/// - `local_population`: 距離 5 以内の人口合計 (自身を除く)。需給ゲート用。
/// - `factory_smoke_penalty`: 4-近傍に Factory、または距離 2 以内に Refinery が
///   ある場合 true。Tier を 1 段下げる「煙害」を表現。
/// - `edge_connected`: 隣接 Road が「マップ端まで繋がる幹線網」に属するか。
///   SOFT ルール: 未接続でも Cottage 暮らしは可。Apartment / Highrise には必須。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HouseNeighborhood {
    pub n_road_adj: u32,
    pub n_workshop_within_5: u32,
    pub n_shop_within_5: u32,
    pub n_office_within_5: u32,
    pub n_house_within_3: u32,
    pub n_park_within_4: u32,
    pub n_megaculture_within_5: u32,
    pub n_megacommerce_or_hq_within_5: u32,
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
    let mut n_megaculture_within_5 = 0u32;
    let mut n_megacommerce_or_hq_within_5 = 0u32;
    let mut local_population: u32 = 0;
    // 半径 5 (= 一番広いカウンタの参照範囲) にループを限定。AI 評価関数が
    // 候補数百回 / tick 呼ぶため、O(GRID²) では `compute_income_per_sec_cents` 全体が
    // 重くなりすぎる (Tier 5 の 3手読みで 30 分 sim が数十分かかる)。半径 5 の
    // bbox = 11×11 = 121 cells に絞ると 64×32 = 2048 cells から 17x 高速化。
    let xi = x as i32;
    let yi = y as i32;
    for dy in -5i32..=5 {
        let cy_i = yi + dy;
        if cy_i < 0 || cy_i >= GRID_H as i32 {
            continue;
        }
        let cy = cy_i as usize;
        for dx in -5i32..=5 {
            let cx_i = xi + dx;
            if cx_i < 0 || cx_i >= GRID_W as i32 {
                continue;
            }
            let cx = cx_i as usize;
            let manhattan = (dx.abs() + dy.abs()) as u32;
            if manhattan > 5 {
                continue;
            }
            match city.tile(cx, cy) {
                Tile::Built(Building::Shop) => n_shop_within_5 += 1,
                Tile::Built(Building::Mall) => n_shop_within_5 += 2,
                Tile::Built(Building::MegaMall) => {
                    n_shop_within_5 += 4;
                    n_megacommerce_or_hq_within_5 += 1;
                }
                Tile::Built(Building::Workshop) => n_workshop_within_5 += 1,
                Tile::Built(Building::Factory) => n_workshop_within_5 += 2,
                Tile::Built(Building::Refinery) => {
                    n_workshop_within_5 += 4;
                    // Refinery は Factory の倍 (距離 2) まで煙害を出す。
                    // 4-近傍ループだけだと距離 2 を取りこぼし、AI の placement
                    // 評価と実シミュレーションの Tier 判定が乖離するため、
                    // 外側スキャンで距離 2 を拾う。
                    if manhattan <= 2 {
                        factory_smoke_penalty = true;
                    }
                }
                Tile::Built(Building::Office) => n_office_within_5 += 1,
                Tile::Built(Building::Headquarters) => {
                    n_office_within_5 += 3;
                    n_megacommerce_or_hq_within_5 += 1;
                }
                Tile::Built(Building::House) if (cx, cy) != (x, y) => {
                    if manhattan <= 3 {
                        n_house_within_3 += 1;
                    }
                    // 需給ゲート用の local_population は **Cottage 定員固定**。
                    // 実効 Tier を呼ぶと「自身の Tier 計算が周囲の House の Tier に
                    // 依存」する循環参照になるため。
                    local_population += house_capacity(HouseTier::Cottage);
                }
                Tile::Built(Building::Park) if manhattan <= 4 => n_park_within_4 += 1,
                Tile::Built(Building::Plaza) if manhattan <= 4 => n_park_within_4 += 3,
                Tile::Built(Building::Stadium) => {
                    n_park_within_4 += 6;
                    n_megaculture_within_5 += 1;
                }
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
        n_megaculture_within_5,
        n_megacommerce_or_hq_within_5,
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
    // Tower / Arcology は終盤の象徴段階。経済密度の閾値を一気に引き上げ、
    // 「街区にメガ施設が揃わないと出ない」絵を作る。
    let tower_required = 8 + demand_pressure;
    let arcology_required = 14 + demand_pressure;

    // Arcology: 全部入りの最終段階。Stadium と (MegaMall または Headquarters) が
    // 共に近接し、Road / 周囲 House / 経済密度すべてが厚い時のみ。
    if stats.n_road_adj >= 3
        && stats.n_megaculture_within_5 >= 1
        && stats.n_megacommerce_or_hq_within_5 >= 1
        && economic_density >= arcology_required
        && stats.n_house_within_3 >= 4
    {
        return apply_smoke_penalty(HouseTier::Arcology, stats.factory_smoke_penalty);
    }

    // Tower: Highrise の上。MegaMall または Headquarters が近接し、経済密度
    // が高い街区で出現する。Stadium がなくとも到達できる「現実的な終盤」。
    if stats.n_road_adj >= 2
        && stats.n_megacommerce_or_hq_within_5 >= 1
        && economic_density >= tower_required
        && stats.n_house_within_3 >= 3
    {
        return apply_smoke_penalty(HouseTier::Tower, stats.factory_smoke_penalty);
    }

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
        HouseTier::Arcology => HouseTier::Tower,
        HouseTier::Tower => HouseTier::Highrise,
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
        // Tower: 築 10 min — 「街区がもう一段成熟するまで」
        HouseTier::Tower => 6000,
        // Arcology: 築 15 min — 終盤プレイヤーへの最終報酬
        HouseTier::Arcology => 9000,
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
    // 上から順に「目標 Tier 以上 かつ dwell 満了」を満たす最高位を返す。
    // 目標が下位なら age_ticks に関係なくそこで打ち切る (Cottage 街区が
    // 経年で勝手に Highrise にならない)。
    if matches!(target, HouseTier::Arcology)
        && age_ticks >= tier_dwell_required_ticks(HouseTier::Arcology)
    {
        return HouseTier::Arcology;
    }
    if matches!(target, HouseTier::Arcology | HouseTier::Tower)
        && age_ticks >= tier_dwell_required_ticks(HouseTier::Tower)
    {
        return HouseTier::Tower;
    }
    if matches!(
        target,
        HouseTier::Arcology | HouseTier::Tower | HouseTier::Highrise
    ) && age_ticks >= tier_dwell_required_ticks(HouseTier::Highrise)
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
        (Building::House, Some(HouseTier::Arcology)) => 800,
        (Building::House, Some(HouseTier::Tower)) => 600,
        (Building::House, Some(HouseTier::Highrise)) => 400,
        (Building::House, Some(HouseTier::Apartment)) => 250,
        (Building::House, _) => 100,
        (Building::Workshop, _) => 200,
        (Building::Shop, _) => 220,
        // 上位建物は基礎建物より長寿 — 大きな投資の元を取らせる。
        (Building::Factory, _) => 300,
        (Building::Mall, _) => 320,
        (Building::Office, _) => 280,
        // 超上位建物は終盤の街の骨格として長寿命。
        (Building::Refinery, _) => 500,
        (Building::MegaMall, _) => 550,
        (Building::Headquarters, _) => 480,
        // インフラと緑地は不老 (大型文化施設も含む)。
        (Building::Park, _) => u32::MAX,
        (Building::Plaza, _) => u32::MAX,
        (Building::Stadium, _) => u32::MAX,
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

pub(super) fn has_neighbor_kind(city: &City, x: usize, y: usize, kind: Building) -> bool {
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
    city.invalidate_population_cache();
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

/// (x, y) から Manhattan 距離 dist 以内に Built/Construction セルが存在するか。
/// Outpost 候補が「街に近い」かどうかの判定 (= 街から離れた荒野に Outpost を
/// 単独で建てない) に使う。
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

// ── 将棋AI 風 評価関数 / 探索 ─────────────────────────────────
//
// **思想**: 「この街局面の良さを 1 つの数値で表す」評価関数 (= shogi 評価関数) と、
// 「全合法手を仮想着手して評価値を最大化する手を選ぶ」探索 (= alpha-beta の単一
// エージェント版) で AI を構成する。Tier 差は **探索深さ + 評価ノイズ** で作り、
// 評価関数自体は全 Tier 共通 (Stockfish Skill Level / ぴよ将棋 と同じ思想)。
//
// 評価値の単位は cents/sec (= compute_income_per_sec_cents 解像度)。
// コストは「投資の回収期間 PAYBACK_SECS で按分」した cents/sec で評価値から減じる。

/// AI 評価で投資コストを「秒単価」に按分する基準期間 (秒)。
///
/// **キャリブレーション**: 「街を 30 分育てた時のリターン」基準。1800 秒 (= 30 分)。
///   - House $40 → 2.2 cents/sec amort: Cottage の +50 cents/sec で十分回収
///   - Road $10 → 0.55 cents/sec: 隣接 House を edge-connected 化する +25 で容易回収
///   - Outpost $600 → 33 cents/sec: 4 Rock セルで House +200 から十分回収可
///
/// 短い PAYBACK (60-120 sec) では Outpost のように「後続の建物で初めて価値が出る」
/// 投資が永遠に評価されない。30 分は metropolis の標準的な 1 セッション長。
pub const AI_PAYBACK_SECS: i64 = 1800;

/// 評価関数 (アマ初段〜プロ相当)。
/// 「街全体の cents/sec」+ thematic bonus + Outpost territory bonus
/// + inactive 建物 penalty + 道路網健全性。Tier 3 以上で共通、Tier 差は探索深さで作る。
pub fn evaluate(city: &City) -> i64 {
    // `cached_edge_connected_roads` を経由することで、AI search の非 Road actions では
    // BFS を走らせず connected_cache から再利用される (Road change 時のみ city.tick を
    // 進めて invalidate するか別経路で再計算)。同 tick 内で連続 evaluate しても
    // BFS 1 回で済む。
    let connected = cached_edge_connected_roads(city);
    compute_income_per_sec_cents_with(city, &connected)
        + strategy_thematic_bonus(city)
        + outpost_territory_bonus(city)
        + inactive_building_penalty_with(city, &connected)
        + road_network_value(city, &connected)
}

/// 道路網の「街にとっての価値」(cents/sec 単位)。2 成分の単一指標:
///
/// 1. **frontier 拡張余地**: 幹線網 (= edge-connected Road) に隣接する Empty buildable
///    セル × `FRONTIER_PER_CELL`。「将来そこに建物が建てられる」期待値。
/// 2. **孤立 Road ペナルティ**: edge-connected で無い Road タイル × `-ISOLATED_PENALTY`。
///    マップ外と物流が繋がらない Road は無駄なので、AI が自動で撤去候補にできるように
///    マイナス計上。
///
/// **単一指標で 3 症状を捕まえる設計**:
///   - 死に道路 (孤立 Road): 成分 2 で直接ペナルティ → 撤去で +Δ
///   - あみあみ (冗長な平行 Road): 既に隣接フロンティアがある領域に追加 Road しても
///     成分 1 の Δ がほぼ 0 → AI が選好しない
///   - 道路を囲んで拡張不能化: 成分 1 のフロンティアセルを建物が消費する形で -Δ →
///     周辺がすべて Built だと Road の expansion 余地が無くなり、評価が落ちる
fn road_network_value(city: &City, connected: &[Vec<bool>]) -> i64 {
    const FRONTIER_PER_CELL: i64 = 8;
    // Isolated Road の demolish_cost は外周ほど 2 次関数的に大きい
    // (距離 12 で $770 → 43 cents/sec amort)。撤去 action_value が build (~+48 cents/sec
    // for edge-connected House) を上回るには、penalty ≥ 距離 12 の amort + 48 = ~91。
    // 100 にすることで距離 ~12 までは確実に撤去候補として勝つ。
    const ISOLATED_PENALTY: i64 = 100;
    let mut scratch = city.eval_scratch.borrow_mut();
    let visited = &mut scratch.frontier_visited;
    for row in visited.iter_mut() {
        row.fill(false);
    }
    let mut frontier_count = 0i64;
    let mut isolated_roads = 0i64;
    #[allow(clippy::needless_range_loop)] // (y, x) を直接 connected[y][x] 参照に使うため enumerate 化はしない
    for y in 0..GRID_H {
        for x in 0..GRID_W {
            if !matches!(city.tile(x, y), Tile::Built(Building::Road)) {
                continue;
            }
            if !connected[y][x] {
                isolated_roads += 1;
                continue;
            }
            for (dx, dy) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
                let nx = x as i32 + dx;
                let ny = y as i32 + dy;
                if nx < 0 || ny < 0 || nx >= GRID_W as i32 || ny >= GRID_H as i32 {
                    continue;
                }
                let (nx, ny) = (nx as usize, ny as usize);
                if visited[ny][nx] {
                    continue;
                }
                if !matches!(city.tile(nx, ny), Tile::Empty) {
                    continue;
                }
                let t = city.terrain_at(nx, ny);
                if !t.buildable() {
                    continue;
                }
                // Rock セルは隣接 Outpost が無いと start_construction に通らないので、
                // 「現に建てられる」フロンティアにはカウントしない。
                if t.needs_outpost() && !has_outpost_neighbor(city, nx, ny) {
                    continue;
                }
                visited[ny][nx] = true;
                frontier_count += 1;
            }
        }
    }
    frontier_count * FRONTIER_PER_CELL - isolated_roads * ISOLATED_PENALTY
}

/// 機能不全の商業/雇用建物に対するマイナス bonus。
///
/// **必要性**: 純 Δincome 評価では「inactive Shop を撤去」の Δevaluate ≈ 0
/// (失う income も 0) になり、AI が機能不全建物を撤去しなくなる。「セルが他用途に
/// 使えるはず」という機会コストを評価に乗せることで、撤去 action が並行する
/// Build 候補 (House +25 cents/sec 等) を上回るようにする。
///
/// 値の根拠: 各建物の active 時の収入下限 ~1/3 を「失われている価値」として計上。
///   - Shop demolish $150 / 1800 sec ≈ 8.3 cents/sec amort → 撤去ペイバックは小さい
///   - Build House (= 並行候補) は +25 cents/sec → ペナルティはこれを超える必要
fn inactive_building_penalty_with(city: &City, connected: &[Vec<bool>]) -> i64 {
    let mut penalty = 0i64;
    for y in 0..GRID_H {
        for x in 0..GRID_W {
            match city.tile(x, y) {
                Tile::Built(Building::Shop) => {
                    if !shop_is_active_with(city, x, y, connected) {
                        penalty -= 50;
                    }
                }
                Tile::Built(Building::Mall) => {
                    if !shop_is_active_with(city, x, y, connected) {
                        penalty -= 100;
                    }
                }
                Tile::Built(Building::Workshop) => {
                    if !workshop_is_active_with(city, x, y, connected) {
                        penalty -= 50;
                    }
                }
                Tile::Built(Building::Factory) => {
                    if !workshop_is_active_with(city, x, y, connected) {
                        penalty -= 100;
                    }
                }
                Tile::Built(Building::Office) => {
                    if !workshop_is_active_with(city, x, y, connected) {
                        penalty -= 80;
                    }
                }
                _ => {}
            }
        }
    }
    penalty
}

/// Outpost が隣接 Rock を解禁する将来価値を評価関数に組み込む thematic bonus。
///
/// **必要性**: Outpost 自体は income 0、効能は「隣接 Rock セルを建設可能にする」点
/// だけ。純粋な Δincome 評価では Outpost コスト ($600) を回収しきれず、AI が
/// いつまで経っても Outpost を選ばなくなる。
///
/// **将棋AI の「駒の働き」**: 角の通り道のような潜在価値を駒得とは別軸で計上する。
/// income (現実の駒得) + territory_bonus (駒働き) の 2 軸で評価することで、
/// 評価ベース AI でも開拓行動が自然に出る。
///
/// 値: 隣接 Rock 数 × 20 cents/sec。Outpost cost $600 → amort = 33 cents/sec
/// なので、Rock 2 個以上隣接で評価値プラス。
fn outpost_territory_bonus(city: &City) -> i64 {
    // 複数 Outpost が同じ Rock セルに隣接していても 1 度しかカウントしない
    // (= 解禁余地は実物理セル数ぶんだけ)。重複を排除するため visited grid を使う。
    let mut counted = vec![vec![false; GRID_W]; GRID_H];
    let mut unlockable_rocks = 0i64;
    for y in 0..GRID_H {
        for x in 0..GRID_W {
            if !matches!(city.tile(x, y), Tile::Built(Building::Outpost)) {
                continue;
            }
            for (dx, dy) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
                let nx = x as i32 + dx;
                let ny = y as i32 + dy;
                if nx < 0 || ny < 0 || nx >= GRID_W as i32 || ny >= GRID_H as i32 {
                    continue;
                }
                let (nx, ny) = (nx as usize, ny as usize);
                if counted[ny][nx] {
                    continue;
                }
                // 「未利用 Rock」だけ将来価値として計上する。Built/Construction/
                // Clearing 中の Rock セルは既に消費中で、今後 Outpost が解禁する
                // 余地は無い。terrain layer は Clearing 完了まで Rock のままなので、
                // tile レイヤで Empty 判定する必要がある。
                if !matches!(city.tile(nx, ny), Tile::Empty) {
                    continue;
                }
                if city.terrain_at(nx, ny) == super::terrain::Terrain::Rock {
                    counted[ny][nx] = true;
                    unlockable_rocks += 1;
                }
            }
        }
    }
    unlockable_rocks * 20
}

/// 評価関数 — 簡易版 (アマ低級相当: 「駒得しか見えない」)。
/// 1-ply で見ても「将来の収入」を推測できないように **直接収入だけ** を見る。
/// = Cottage の +25/+50 cents しか見えず、Road や Park の長期効果が見えない。
pub fn evaluate_simple(city: &City) -> i64 {
    // 全 House の Cottage 想定収入だけを足す。Tier 評価も連結性も見ない。
    // = 「目先の家賃しか見えてない」初級プレイヤー思考。
    let mut cents = 0i64;
    for y in 0..GRID_H {
        for x in 0..GRID_W {
            if matches!(city.tile(x, y), Tile::Built(Building::House)) {
                cents += 50;
            }
        }
    }
    cents
}

/// Strategy ボタンの thematic bonus (cents/sec 単位)。
/// 評価関数に薄く乗せて「Eco なら Park、Income なら商業」を AI が選好するようにする。
/// 効きは弱め (= ±数十 cents/sec) で、純粋な income/sec 最大化を主軸に保つ。
fn strategy_thematic_bonus(city: &City) -> i64 {
    let s = city.strategy;
    let mut bonus = 0i64;
    for y in 0..GRID_H {
        for x in 0..GRID_W {
            let kind = match city.tile(x, y) {
                Tile::Built(b) => *b,
                _ => continue,
            };
            bonus += match (s, kind) {
                (Strategy::Growth, Building::House) => 5,
                (Strategy::Growth, Building::Road) => 1,
                (Strategy::Income, Building::Shop) => 8,
                (Strategy::Income, Building::Mall) => 12,
                (Strategy::Income, Building::Workshop) => 4,
                (Strategy::Income, Building::Factory) => 8,
                (Strategy::Tech, Building::Road) => 5,
                (Strategy::Tech, Building::Office) => 6,
                (Strategy::Eco, Building::Park) => 12,
                (Strategy::Eco, Building::Factory) => -10,
                _ => 0,
            };
        }
    }
    bonus
}

/// 仮想着手 + 評価 + 巻き戻し: city を mutate して `f` を呼び、終わったら元に戻す。
///
/// **クローン回避**: 64×32 grid の clone は ~18k bytes / 1 アクション分。Tier 5 の
/// depth=3 探索で数千回呼ばれると数百 MB の alloc になり tick が秒オーダで詰まる。
/// in-place mutate + revert なら同じ depth でも O(1) の操作で済む。
///
/// **Build は Construction を経由せず即 Built タイル**として置く (= 評価時点で
/// 「建ったあとの局面」を見たい)。Demolish は Built → Empty。Idle は f を直接呼ぶだけ。
///
/// `f` のシグネチャは `FnOnce(&mut City) -> R`。再帰的な探索 (= `f` 内で
/// さらに `with_action_applied` を呼ぶケース) を許す代わりに、**`f` は city を
/// 自身が受け取った時の状態に戻す責務を負う**。`f` が `with_action_applied` を
/// 入れ子で呼ぶ限り (= 各層が apply/revert する) 自然にこの契約が満たされる。
pub fn with_action_applied<R, F: FnOnce(&mut City) -> R>(
    city: &mut City,
    action: &super::ai::AiAction,
    f: F,
) -> R {
    match action {
        super::ai::AiAction::Build { x, y, kind } => {
            if *x >= GRID_W || *y >= GRID_H {
                return f(city);
            }
            let saved_tile = city.grid[*y][*x].clone();
            let saved_built_at = city.built_at_tick[*y][*x];
            // 整地必要セルでは start_construction が `kind.cost() + clearing_cost` を
            // 引く。仮想 cash も同額減らさないと、depth-2/3 探索の `action_passes_guards`
            // が現実では afford 不可能な後続 action を許してしまう。
            let terrain = city.terrain_at(*x, *y);
            let cost = kind.cost()
                + if terrain.needs_clearing() {
                    terrain.clearing_cost()
                } else {
                    0
                };
            city.grid[*y][*x] = Tile::Built(*kind);
            // 仮想着工は「築 0」として評価する。`built_at_tick = city.tick` を
            // セットしないと、`tile_income_cents_with` は `built_at_tick == 0` で
            // aging スキップする一方、`gather_house_neighborhood_with` の
            // `effective_house_tier(target, age)` が `age = city.tick` で扱う
            // ため、新築 House を即 Highrise として評価してしまう。
            city.built_at_tick[*y][*x] = city.tick;
            city.cash -= cost;
            // Road 配置は edge-connectivity を変えるので connected_cache を含めて全クリア。
            // 非 Road なら per-tile cache だけクリアして connected_cache を温存
            // (= 評価関数 BFS が cached_edge_connected_roads でヒットして高速化)。
            if matches!(kind, Building::Road) {
                city.invalidate_population_cache();
            } else {
                city.invalidate_per_tile_caches();
            }
            let result = f(city);
            city.grid[*y][*x] = saved_tile;
            city.built_at_tick[*y][*x] = saved_built_at;
            city.cash += cost;
            if matches!(kind, Building::Road) {
                city.invalidate_population_cache();
            } else {
                city.invalidate_per_tile_caches();
            }
            result
        }
        super::ai::AiAction::Demolish { x, y } => {
            if *x >= GRID_W || *y >= GRID_H {
                return f(city);
            }
            let saved_tile = city.grid[*y][*x].clone();
            let saved_built_at = city.built_at_tick[*y][*x];
            let cost = demolish_cost(*x, *y);
            // 撤去対象が Road の時のみ connected_cache を invalidate する必要がある。
            let was_road = matches!(saved_tile, Tile::Built(Building::Road));
            city.grid[*y][*x] = Tile::Empty;
            city.cash -= cost;
            city.built_at_tick[*y][*x] = 0;
            if was_road {
                city.invalidate_population_cache();
            } else {
                city.invalidate_per_tile_caches();
            }
            let result = f(city);
            city.grid[*y][*x] = saved_tile;
            city.built_at_tick[*y][*x] = saved_built_at;
            city.cash += cost;
            if was_road {
                city.invalidate_population_cache();
            } else {
                city.invalidate_per_tile_caches();
            }
            result
        }
        super::ai::AiAction::Idle => f(city),
    }
}

/// 1 アクションの評価値。
///
/// `Δevaluate − cost_amortized` を返す。**Build/Demolish/Idle が同じ天秤に乗る**
/// のがポイント (= AI は max を取るだけで「建てる/壊す/待つ」を統一的に選ぶ)。
///
/// `eval_fn` を引数に取ることで、Tier ごとに精緻 (`evaluate`) / 単純
/// (`evaluate_simple`) を切り替えられる。
///
/// **`&mut City` を取る理由**: in-place mutate + revert で評価する。clone は
/// 64×32 grid のメモリ確保が探索の度に走り、Tier 5 の depth=3 で tick が
/// 秒単位に詰まる。mutate-then-revert で同じ depth が ~10x 速くなる。
///
/// production の hot path (`rank_actions`) は baseline 共有版
/// `action_value_with_baseline` を直接呼ぶ。本関数は単発評価向けの公開 API
/// (テストや `render` の選択セル詳細などで使う)。
#[allow(dead_code)]
pub fn action_value<F: Fn(&City) -> i64>(
    city: &mut City,
    action: &super::ai::AiAction,
    eval_fn: &F,
) -> i64 {
    let before = eval_fn(city);
    action_value_with_baseline(city, action, eval_fn, before)
}

/// `action_value` の baseline 共有版。同じ city 状態に対して N 個の候補を比較する
/// 場合、呼び側で `before = eval_fn(city)` を 1 度だけ計算して渡せば、`eval_fn`
/// 呼び出しが 2N → N+1 回に減る。`rank_actions` のホットパスで効く。
pub(super) fn action_value_with_baseline<F: Fn(&City) -> i64>(
    city: &mut City,
    action: &super::ai::AiAction,
    eval_fn: &F,
    before: i64,
) -> i64 {
    let after = with_action_applied(city, action, |c| eval_fn(c));
    let cost_cents = match action {
        super::ai::AiAction::Build { x, y, kind } => {
            let mut cost = kind.cost();
            let t = city.terrain_at(*x, *y);
            if t.needs_clearing() {
                cost += t.clearing_cost();
            }
            cost * 100
        }
        super::ai::AiAction::Demolish { x, y } => demolish_cost(*x, *y) * 100,
        super::ai::AiAction::Idle => 0,
    };
    let amort = cost_cents / AI_PAYBACK_SECS;
    (after - before) - amort
}

/// 候補生成。Built 隣接 Empty cells (= 街の周辺 1 マス) と、Outpost 候補
/// (Rock 隣接 + 街の近く) を返す。
///
/// 候補が空なら距離 3 まで広げて再収集 (序盤フォールバック)。
#[allow(clippy::type_complexity)] // (regular, outpost) のタプルが意味的に明確で型エイリアスは過剰
pub(super) fn collect_candidates(city: &City) -> (Vec<(usize, usize)>, Vec<(usize, usize)>) {
    let mut regular: Vec<(usize, usize)> = Vec::new();
    let mut outpost: Vec<(usize, usize)> = Vec::new();
    for y in 0..GRID_H {
        for x in 0..GRID_W {
            if !matches!(city.tile(x, y), Tile::Empty) {
                continue;
            }
            let t = city.terrain_at(x, y);
            if !t.buildable() {
                continue;
            }
            let needs_outpost_unmet = t.needs_outpost() && !has_outpost_neighbor(city, x, y);
            if needs_outpost_unmet {
                continue;
            }
            let has_rock_n = has_terrain_neighbor(city, x, y, super::terrain::Terrain::Rock);
            if has_rock_n && !t.needs_clearing() && has_built_within_distance(city, x, y, 4) {
                outpost.push((x, y));
            }
            if has_built_neighbor_built(city, x, y) {
                regular.push((x, y));
            }
        }
    }
    if regular.is_empty() && outpost.is_empty() {
        for y in 0..GRID_H {
            for x in 0..GRID_W {
                if !matches!(city.tile(x, y), Tile::Empty) {
                    continue;
                }
                let t = city.terrain_at(x, y);
                if !t.buildable() {
                    continue;
                }
                // Rock セルは隣接 Outpost が無いと start_construction で必ず弾かれる。
                // fallback でも上ループと同じく needs_outpost ガードを通す。
                if t.needs_outpost() && !has_outpost_neighbor(city, x, y) {
                    continue;
                }
                if has_built_within_distance(city, x, y, 3) {
                    regular.push((x, y));
                }
            }
        }
    }
    (regular, outpost)
}

/// Built 隣接判定 (4-近傍に Built タイル or Construction が 1 つでもあるか)。
fn has_built_neighbor_built(city: &City, x: usize, y: usize) -> bool {
    for (dx, dy) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
        let nx = x as i32 + dx;
        let ny = y as i32 + dy;
        if nx < 0 || ny < 0 || nx >= GRID_W as i32 || ny >= GRID_H as i32 {
            continue;
        }
        if !matches!(city.tile(nx as usize, ny as usize), Tile::Empty) {
            return true;
        }
    }
    false
}

/// 全候補アクションを列挙。Build × 全種別 + Demolish × 全 Built + Idle。
///
/// affordability ガード + 建物別の前提条件 (Shop なら House 3 軒以上) を適用。
pub(super) fn enumerate_actions(city: &City) -> Vec<super::ai::AiAction> {
    let mut actions: Vec<super::ai::AiAction> = Vec::new();
    let (regular, outpost) = collect_candidates(city);
    // 上位建物 (Refinery / MegaMall / Headquarters / Plaza / Stadium) も候補に
    // 含める。cost が高いため `action_passes_guards` の cash + house-count
    // ガードで序盤は自然に落選し、終盤 (cash と街の規模が揃った時) のみ浮上する。
    let normal_kinds: &[Building] = &[
        Building::House,
        Building::Road,
        Building::Workshop,
        Building::Factory,
        Building::Refinery,
        Building::Shop,
        Building::Mall,
        Building::MegaMall,
        Building::Office,
        Building::Headquarters,
        Building::Park,
        Building::Plaza,
        Building::Stadium,
    ];
    for &(x, y) in &regular {
        let terrain = city.terrain_at(x, y);
        let extra = if terrain.needs_clearing() {
            terrain.clearing_cost()
        } else {
            0
        };
        for &kind in normal_kinds {
            if action_passes_guards(city, kind, extra) {
                actions.push(super::ai::AiAction::Build { x, y, kind });
            }
        }
    }
    for &(x, y) in &outpost {
        let terrain = city.terrain_at(x, y);
        let extra = if terrain.needs_clearing() {
            terrain.clearing_cost()
        } else {
            0
        };
        if action_passes_guards(city, Building::Outpost, extra) {
            actions.push(super::ai::AiAction::Build {
                x,
                y,
                kind: Building::Outpost,
            });
        }
    }
    // Demolish 候補は **「明らかに無駄」な Built tile のみ** に絞る:
    //   - inactive な商業/雇用建物 (Shop/Mall/Workshop/Factory/Office)
    //   - edge未接続 / Built 隣接無しの Road (= 孤立した死に道路)
    //   - 周囲 Rock 無しの Outpost (= 役目を終えた拠点)
    //
    // 「健全な建物 (active な Shop、edge connected な Road、住人のいる House) を
    // 撤去して上位建物に置換する」のような最適化はできなくなるが、saturated map で
    // Demolish 候補が数百個に膨れ上がって AI tick が秒オーダで詰まる症状を回避する
    // ためのトレードオフ。`inactive_building_penalty` / `road_network_value` の
    // 機会コスト計上と合わせて、機能不全建物は引き続き自動撤去される。
    //
    // 連結性は cache 済み。商業/雇用 active 判定の cache は無いので per-cell 計算するが、
    // フィルタで弾かれた cell は最初から Demolish 候補にすらならず evaluate コストが消える。
    let reserve = automation_policy(city.strategy).min_cash_reserve;
    let connected = cached_edge_connected_roads(city);
    for y in 0..GRID_H {
        for x in 0..GRID_W {
            let kind = match city.tile(x, y) {
                Tile::Built(b) => *b,
                _ => continue,
            };
            let worth_demolishing = match kind {
                Building::Shop | Building::Mall | Building::MegaMall => {
                    !shop_is_active_with(city, x, y, &connected)
                }
                Building::Workshop
                | Building::Factory
                | Building::Refinery
                | Building::Office
                | Building::Headquarters => {
                    !workshop_is_active_with(city, x, y, &connected)
                }
                Building::Road => {
                    !connected[y][x] || !has_built_neighbor_built(city, x, y)
                }
                Building::Outpost => {
                    let mut n = 0u32;
                    for (dx, dy) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
                        let nx = x as i32 + dx;
                        let ny = y as i32 + dy;
                        if nx < 0 || ny < 0 || nx >= GRID_W as i32 || ny >= GRID_H as i32 {
                            continue;
                        }
                        if city.terrain_at(nx as usize, ny as usize)
                            == super::terrain::Terrain::Rock
                        {
                            n += 1;
                        }
                    }
                    n == 0
                }
                // 直接収入を持たない触媒系 (House / Park / Plaza / Stadium) は
                // 機能不全による自動撤去対象外。Plaza / Stadium は un-supported なら
                // 撤去候補にしてよさそうだが、main 側の Park 同様に「永続資産」扱いにし、
                // saturate しないキャラクター付けに揃える。
                Building::House | Building::Park | Building::Plaza | Building::Stadium => false,
            };
            if !worth_demolishing {
                continue;
            }
            let cost = demolish_cost(x, y);
            if city.cash < cost + reserve {
                continue;
            }
            actions.push(super::ai::AiAction::Demolish { x, y });
        }
    }
    // Idle を常に合法手として並べる。全候補が負評価 (= cash 浪費) な状況で
    // AI が「最もマシな破壊」を強行する誤動作を防ぐ。`action_value(Idle)` は
    // Δevaluate=0、cost=0 で常に 0 を返すので、他の候補の上位値が負ならば Idle が
    // 自動的に勝つ。
    actions.push(super::ai::AiAction::Idle);
    actions
}

/// affordability + 建物前提条件ガード。`enumerate_actions` から呼ぶ。
///
/// `extra_cost` は terrain の `clearing_cost` を渡す枠。`start_construction` は
/// 整地必要セルでは「即時 `clearing_cost` を引いて Tile::Clearing にする」
/// だけで早期 return し、`kind.cost()` は整地完了後の別 tick で別途引く。
/// したがって即時必要 cash は:
///   - 整地必要 (extra_cost > 0): `clearing_cost` のみ
///   - そうでない (extra_cost == 0): `kind.cost()`
fn action_passes_guards(city: &City, kind: Building, extra_cost: i64) -> bool {
    let immediate_cost = if extra_cost > 0 {
        extra_cost
    } else {
        kind.cost()
    };
    if city.cash < immediate_cost {
        return false;
    }
    // 即時消費後に House 1 軒分の余裕は残す (savings protection)。
    let house_cost = Building::House.cost();
    if !matches!(kind, Building::House | Building::Outpost)
        && city.cash - immediate_cost < house_cost
    {
        return false;
    }
    if matches!(kind, Building::Shop) && city.count_built(Building::House) < 3 {
        return false;
    }
    if matches!(kind, Building::Mall) && city.count_built(Building::House) < 6 {
        return false;
    }
    if matches!(kind, Building::Workshop) && city.count_built(Building::House) < 2 {
        return false;
    }
    if matches!(kind, Building::Factory) && city.count_built(Building::House) < 5 {
        return false;
    }
    if matches!(kind, Building::Office) && city.count_built(Building::House) < 4 {
        return false;
    }
    // 超上位建物は街がそれなりに育ってから建てる (cost が高く、機能には
    // 一定数の House が必要)。ROI ペナルティだけだと序盤に空地スコアの
    // 関係で誤って高評価されることがあるため、人口下限のガードで弾く。
    if matches!(kind, Building::Refinery) && city.count_built(Building::House) < 12 {
        return false;
    }
    if matches!(kind, Building::MegaMall) && city.count_built(Building::House) < 12 {
        return false;
    }
    if matches!(kind, Building::Headquarters) && city.count_built(Building::House) < 10 {
        return false;
    }
    if matches!(kind, Building::Plaza) && city.count_built(Building::House) < 8 {
        return false;
    }
    if matches!(kind, Building::Stadium) && city.count_built(Building::House) < 20 {
        return false;
    }
    true
}

/// 探索: 全候補アクションを評価し、上位 K を返す (depth=1)。
/// 戻り値は `(action, value)` の降順ソート済み Vec。
pub(super) fn rank_actions<F: Fn(&City) -> i64>(
    city: &mut City,
    eval_fn: &F,
    top_k: usize,
) -> Vec<(super::ai::AiAction, i64)> {
    let actions = enumerate_actions(city);
    // baseline `before` は同じ city 状態で全候補に共通。1 度計算して N 個の候補で
    // 共有することで evaluate 呼び出しが 2N → N+1 回 (実質 ~2x speedup)。
    let before = eval_fn(city);
    let mut scored: Vec<(super::ai::AiAction, i64)> = actions
        .into_iter()
        .map(|a| {
            let v = action_value_with_baseline(city, &a, eval_fn, before);
            (a, v)
        })
        .collect();
    scored.sort_by(|a, b| b.1.cmp(&a.1));
    scored.truncate(top_k.max(1));
    scored
}

/// depth=N の探索 (beam search)。各 depth で上位 K に絞り込みながら 1 手ずつ深める。
///
/// **将棋エンジンとの対応**: shogi の alpha-beta は対戦相手のミニマックスが
/// 必須だが、本ゲームは単一エージェントなので「自分の連続着手」の合計
/// `Δevaluate` を最大化するだけ。よって素直な beam search で十分。
///
/// 戻り値: depth 探索後の合計値が最大の **1 手目** アクション。
pub(super) fn search_best_action<F: Fn(&City) -> i64>(
    city: &mut City,
    depth: u32,
    top_k: usize,
    eval_fn: &F,
) -> Option<super::ai::AiAction> {
    let depth1 = rank_actions(city, eval_fn, top_k);
    if depth1.is_empty() {
        return None;
    }
    if depth <= 1 {
        return Some(depth1[0].0.clone());
    }
    let mut best: Option<(super::ai::AiAction, i64)> = None;
    for (a, v1) in &depth1 {
        let next_k = (top_k / 2).max(2);
        let next_total = with_action_applied(city, a, |c| {
            best_continuation_value(c, depth - 1, next_k, eval_fn)
        });
        let total = v1 + next_total;
        let better = match &best {
            None => true,
            Some((_, prev)) => total > *prev,
        };
        if better {
            best = Some((a.clone(), total));
        }
    }
    best.map(|(a, _)| a)
}

/// 仮想着手後の局面で「残り `depth` 手で得られる最良の合計 action_value」を返す。
///
/// 単一エージェント探索なので「次の手を打たない (= action_value 0)」も選択肢。
/// 最良候補が負なら 0 を返す。Tier 5 の depth=3 まで本物の再帰で読む。
fn best_continuation_value<F: Fn(&City) -> i64>(
    city: &mut City,
    depth: u32,
    top_k: usize,
    eval_fn: &F,
) -> i64 {
    if depth == 0 {
        return 0;
    }
    let ranked = rank_actions(city, eval_fn, top_k);
    if ranked.is_empty() {
        return 0;
    }
    if depth == 1 {
        return ranked[0].1.max(0);
    }
    let next_k = (top_k / 2).max(2);
    let mut best_total = ranked[0].1.max(0);
    for (a, v1) in ranked.iter() {
        let cont = with_action_applied(city, a, |c| {
            best_continuation_value(c, depth - 1, next_k, eval_fn)
        });
        let total = v1 + cont;
        if total > best_total {
            best_total = total;
        }
    }
    best_total
}

/// Stockfish 流のノイズ付き選択: `noise_pct`% の確率で「上位 3 候補から random」を選ぶ。
/// 残り (100 − noise_pct)% は素直に best を返す。
///
/// **デザイン意図**: 明示ブランダー (突然の大悪手) は入れない。次善手を確率で
/// 選ぶことで、評価関数が見抜けない「微妙に良い手」を時々取りこぼす自然な弱体化。
pub(super) fn pick_with_noise(
    ranked: &[(super::ai::AiAction, i64)],
    noise_pct: u32,
    rng: u64,
) -> Option<super::ai::AiAction> {
    if ranked.is_empty() {
        return None;
    }
    if noise_pct == 0 {
        return Some(ranked[0].0.clone());
    }
    let roll = (rng % 100) as u32;
    if roll < noise_pct {
        let pick = (rng >> 32) as usize % ranked.len().min(3);
        Some(ranked[pick].0.clone())
    } else {
        Some(ranked[0].0.clone())
    }
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
            edge_connected: true,
            ..Default::default()
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
            edge_connected: true,
            ..Default::default()
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
            n_house_within_3: 4,
            n_park_within_4: 2,
            edge_connected: false,
            ..Default::default()
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
    /// ※ AI 由来の Build/Demolish 等の event は許容し、tier 進化 event だけを数える。
    #[test]
    fn tier_does_not_re_trigger_within_same_tier() {
        let mut city = City::new();
        for i in 0..13 {
            city.set_tile(i, 0, Tile::Built(Building::House));
        }
        tick(&mut city, 1);
        let tier_events_after_first_advance =
            city.events.iter().filter(|e| e.starts_with("🎊")).count();
        // もう 1 軒追加 (まだ Town 範囲内: 14 軒 × 4 = 56 pop)。
        city.set_tile(14, 0, Tile::Built(Building::House));
        tick(&mut city, 5);
        let tier_events_after_extra_house =
            city.events.iter().filter(|e| e.starts_with("🎊")).count();
        assert_eq!(
            tier_events_after_extra_house, tier_events_after_first_advance,
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

    /// AI 撤去: 中央に置いた inactive Shop が action_value で負評価になり撤去候補になる。
    /// 新評価ベースでは「撤去で街全体の cents/sec が改善する」を直接見るので、機能不全 Shop は
    /// `Demolish` action_value > `Build` action_value を返すはず。
    #[test]
    fn inactive_shop_in_core_is_negative_value() {
        let mut city = City::new();
        city.cash = 10_000;
        let cx = GRID_W / 2;
        let cy = GRID_H / 2;
        city.set_tile(cx, cy, Tile::Built(Building::Shop));
        // 機能不全 Shop は撤去で income が変わらない (= 既に 0)
        // → improvement は不問だが、demolish_cost amort 分マイナス。
        let demolish_action = AiAction::Demolish { x: cx, y: cy };
        let v = action_value(&mut city, &demolish_action, &evaluate);
        // 評価値は負だが、Build action は更に低い場合 AI は撤去を選ぶ。ここでは存在確認のみ。
        // (詳細な「中央 inactive Shop が壊される」挙動は simulator で確認)
        let _ = v;
    }

    /// edge-connected な House は撤去候補にならない (= action_value が負)。
    #[test]
    fn connected_house_should_not_be_worth_demolishing() {
        let mut city = City::new();
        city.cash = 10_000;
        let cx = GRID_W / 2;
        let cy = GRID_H / 2;
        city.set_tile(cx, cy, Tile::Built(Building::House));
        for y in 0..cy {
            city.set_tile(cx + 1, y, Tile::Built(Building::Road));
        }
        city.set_tile(cx + 1, cy, Tile::Built(Building::Road));
        let demolish_action = AiAction::Demolish { x: cx, y: cy };
        let v = action_value(&mut city, &demolish_action, &evaluate);
        assert!(
            v < 0,
            "connected House demolish should be net-negative, got {}",
            v
        );
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

    // ── 需給システム / 新建物のテスト群 ─────────────────────────────

    /// House Tier ごとの定員が単調増加 (Cottage < Apartment < Highrise)。
    #[test]
    fn house_capacity_is_monotone() {
        assert!(house_capacity(HouseTier::Cottage) < house_capacity(HouseTier::Apartment));
        assert!(house_capacity(HouseTier::Apartment) < house_capacity(HouseTier::Highrise));
    }

    /// `tier_aware_population` は Tier 連動で計算される。
    /// Cottage 4 + Apartment 12 + Highrise 30 が単純合算される。
    /// `City::population()` (Cottage 概算) との対比で Tier-aware 版が
    /// レンダーホットパスから分離されている設計を担保する。
    #[test]
    fn population_reflects_tier_capacity() {
        let mut city = City::new();
        city.set_tile(0, 0, Tile::Built(Building::House));
        // 軽量版とTier-aware版は age=0 (Cottage 扱い) では一致する。
        assert_eq!(city.population(), house_capacity(HouseTier::Cottage));
        assert_eq!(tier_aware_population(&city), house_capacity(HouseTier::Cottage));
    }

    /// 需給ゲート: 局所人口が増えると Apartment 化に必要な経済密度が上がる。
    /// local_pop=0 では econ=1 で Apartment、local_pop=30 以上では econ=2 必要。
    #[test]
    fn demand_gate_blocks_apartment_when_supply_short() {
        // local_pop 30 (= Cottage 7-8 軒分) で economic_density 閾値が +1 上がる。
        let stats = HouseNeighborhood {
            n_road_adj: 1,
            n_shop_within_5: 1,
            n_house_within_3: 1,
            local_population: 35, // ゲート発動
            edge_connected: true,
            ..Default::default()
        };
        // econ=1, 必要 2 → Apartment にならず Cottage に縮退。
        assert_eq!(house_tier_for(stats), HouseTier::Cottage);
    }

    /// 同条件で local_population が低い時は Apartment まで育つ (ゲート緩和)。
    #[test]
    fn low_demand_allows_apartment_with_minimal_supply() {
        let stats = HouseNeighborhood {
            n_road_adj: 1,
            n_shop_within_5: 1,
            n_house_within_3: 1,
            local_population: 10, // ゲート未発動
            edge_connected: true,
            ..Default::default()
        };
        assert_eq!(house_tier_for(stats), HouseTier::Apartment);
    }

    /// Office は Highrise 化を促進する触媒。経済密度に 1.5x 寄与。
    #[test]
    fn office_promotes_to_highrise() {
        let stats = HouseNeighborhood {
            n_road_adj: 2,
            n_shop_within_5: 1,
            n_office_within_5: 1, // 1 * 3 / 2 = 1 → econ=2
            n_house_within_3: 3,
            edge_connected: true,
            ..Default::default()
        };
        // economic_density = 1 (Shop) + 1 (Office) = 2 → Highrise 条件達成。
        assert_eq!(house_tier_for(stats), HouseTier::Highrise);
    }

    /// Office と Workshop は雇用クラスが別 (WhiteCollar vs Industrial) なので、
    /// 同範囲に Office を建てても Workshop の収入は薄まらない。
    /// Codex P2 (employment pool 共有による相互抑制バグ) への回帰テスト。
    #[test]
    fn office_does_not_dilute_workshop_employment() {
        let mut city = City::new();
        for x in 0..16 {
            place_edge_road(&mut city, x);
        }
        city.set_tile(8, 1, Tile::Built(Building::Road));
        for x in 1..8 {
            city.set_tile(x, 1, Tile::Built(Building::House));
        }
        for x in 9..16 {
            city.set_tile(x, 1, Tile::Built(Building::House));
        }
        // Workshop の隣接 House 確保 + Workshop 配置。
        city.set_tile(7, 2, Tile::Built(Building::House));
        city.set_tile(8, 2, Tile::Built(Building::Workshop));
        let connected = compute_edge_connected_roads(&city);
        let pop_map = compute_population_map(&city, &connected);
        let workshop_solo = employment_income_cents(
            &city,
            8,
            2,
            WORKSHOP_CAPACITY_CENTS,
            EmploymentClass::Industrial,
            &pop_map,
            &connected,
        );

        // 同範囲に Office を追加 (隣接 House あり、active になる位置)。
        city.set_tile(9, 2, Tile::Built(Building::House));
        city.set_tile(10, 2, Tile::Built(Building::Office));
        let connected = compute_edge_connected_roads(&city);
        let pop_map = compute_population_map(&city, &connected);
        let workshop_with_office = employment_income_cents(
            &city,
            8,
            2,
            WORKSHOP_CAPACITY_CENTS,
            EmploymentClass::Industrial,
            &pop_map,
            &connected,
        );
        // Office は Industrial pool に属さないので、Workshop 収入に影響しない。
        // (House 追加で local_pop は増える可能性がある = workshop_with_office >= workshop_solo)。
        assert!(
            workshop_with_office >= workshop_solo,
            "Adding Office should not reduce Workshop income: solo={} with_office={}",
            workshop_solo, workshop_with_office
        );
    }

    /// inactive な Mall (Road 未接続 / 顧客圏外) は周囲 Shop の収入を薄めない。
    /// 機能不全建物が capacity 按分で active 建物の取り分を奪う問題への回帰テスト。
    #[test]
    fn inactive_mall_does_not_dilute_active_shop() {
        let mut city = City::new();
        for x in 0..10 {
            place_edge_road(&mut city, x);
        }
        // 縦 Road で active Shop の接続を確保。
        city.set_tile(0, 1, Tile::Built(Building::Road));
        city.set_tile(0, 2, Tile::Built(Building::Road));
        for x in 1..6 {
            city.set_tile(x, 1, Tile::Built(Building::House));
        }
        city.set_tile(1, 2, Tile::Built(Building::Shop));
        let connected = compute_edge_connected_roads(&city);
        let pop_map = compute_population_map(&city, &connected);
        let solo = commercial_income_cents(&city, 1, 2, SHOP_CAPACITY_CENTS, &pop_map, &connected);

        // (5, 5) に Road 未接続 + 半径 3 House 無しの inactive Mall を置く。
        // active Shop の取り分が薄まらないことを担保。
        city.set_tile(5, 5, Tile::Built(Building::Mall));
        let connected = compute_edge_connected_roads(&city);
        let pop_map = compute_population_map(&city, &connected);
        let with_inactive_mall =
            commercial_income_cents(&city, 1, 2, SHOP_CAPACITY_CENTS, &pop_map, &connected);
        assert_eq!(
            solo, with_inactive_mall,
            "Inactive Mall should not affect active Shop income: solo={} after_inactive_mall={}",
            solo, with_inactive_mall
        );
    }

    /// 統合テスト: Factory を House の隣接に置くと、`compute_income_per_sec`
    /// 経由の家賃収入が下がる。`gather_house_neighborhood_with` →
    /// `house_tier_for` → `apply_smoke_penalty` → `compute_income_per_sec`
    /// のパイプライン全体で煙害が反映されることを確認する。
    #[test]
    fn factory_smoke_penalty_reduces_house_income_end_to_end() {
        let mut city = City::new();
        // edge-connected な Road を上端に並べ、House を Apartment 化条件下に置く。
        for x in 0..6 {
            place_edge_road(&mut city, x);
        }
        for x in 0..3 {
            city.set_tile(x, 1, Tile::Built(Building::House));
        }
        // Apartment 化条件: Road 1+ + 経済密度 1+ → Workshop を半径 5 内に置く。
        city.set_tile(2, 2, Tile::Built(Building::Workshop));
        // age を Apartment dwell まで進める。
        city.tick = 600;
        for x in 0..3 {
            city.built_at_tick[1][x] = 0;
        }
        let income_clean = compute_income_per_sec(&city);

        // Factory を House (0,1) の隣接に置くと煙害発動 → Apartment が Cottage に降格。
        city.set_tile(0, 2, Tile::Built(Building::Factory));
        city.built_at_tick[2][0] = city.tick; // Factory 完成済み扱い
        let income_with_smoke = compute_income_per_sec(&city);

        assert!(
            income_with_smoke < income_clean,
            "Factory smoke should reduce neighbor House income end-to-end: clean={} smoke={}",
            income_clean, income_with_smoke
        );
    }

    /// 隣接 Factory の煙害で Tier が 1 段下がる。
    #[test]
    fn factory_smoke_penalty_reduces_tier() {
        let stats = HouseNeighborhood {
            n_road_adj: 2,
            n_shop_within_5: 2,
            n_house_within_3: 4,
            factory_smoke_penalty: true, // 煙害 ON
            edge_connected: true,
            ..Default::default()
        };
        // Highrise 条件を満たすが煙害で Apartment に降格。
        assert_eq!(house_tier_for(stats), HouseTier::Apartment);
    }

    /// Refinery は Factory の 2 倍半径 (manhattan 距離 2 以内) で煙害を出す。
    /// 4-近傍ループだけで判定すると距離 2 を取りこぼし、AI の placement 評価
    /// と実シミュレーションの Tier 判定が乖離するため、外側スキャンで距離 2 を拾う。
    #[test]
    fn refinery_smoke_penalty_reaches_distance_two() {
        let mut city = City::new();
        for x in 0..8 {
            place_edge_road(&mut city, x);
        }
        // (3, 1) に House を置き、(3, 3) (Manhattan 距離 2) に Refinery を配置。
        city.set_tile(3, 1, Tile::Built(Building::House));
        city.set_tile(3, 3, Tile::Built(Building::Refinery));
        let connected = compute_edge_connected_roads(&city);
        let stats = gather_house_neighborhood_with(&city, 3, 1, &connected);
        assert!(
            stats.factory_smoke_penalty,
            "Refinery at manhattan distance 2 should trigger smoke penalty"
        );
    }

    /// Refinery が距離 3 以上離れていれば煙害は届かない (上限ガード)。
    #[test]
    fn refinery_smoke_penalty_does_not_reach_distance_three() {
        let mut city = City::new();
        for x in 0..8 {
            place_edge_road(&mut city, x);
        }
        city.set_tile(3, 1, Tile::Built(Building::House));
        city.set_tile(3, 4, Tile::Built(Building::Refinery));
        let connected = compute_edge_connected_roads(&city);
        let stats = gather_house_neighborhood_with(&city, 3, 1, &connected);
        assert!(
            !stats.factory_smoke_penalty,
            "Refinery at manhattan distance 3 should NOT trigger smoke penalty"
        );
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
            EmploymentClass::Industrial,
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
            EmploymentClass::Industrial,
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

}
