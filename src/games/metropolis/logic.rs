//! Pure-function game logic.  No I/O, no rendering — safe to call millions
//! of times from `simulator.rs`.

use super::ai::{decide, AiAction};
use super::state::*;

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

/// Decrement every Construction tile; promote to Built when finished.
fn advance_construction(city: &mut City) {
    let mut completions: Vec<(usize, usize, Building)> = Vec::new();
    for y in 0..GRID_H {
        for x in 0..GRID_W {
            let tile = &mut city.grid[y][x];
            if let Tile::Construction {
                target,
                ticks_remaining,
            } = tile
            {
                if *ticks_remaining <= 1 {
                    let kind = *target;
                    *tile = Tile::Built(kind);
                    city.buildings_finished += 1;
                    completions.push((x, y, kind));
                } else {
                    *ticks_remaining -= 1;
                }
            }
        }
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

/// Tech 戦略時の建設速度ブースト。20% 短縮 → ticks * 4 / 5。
/// `state.strategy` の唯一の "副作用" であり、副作用の集約点として `logic.rs`
/// に置く (state/render は読取専用)。
fn build_ticks_for(city: &City, kind: Building) -> u32 {
    let base = kind.build_ticks();
    if matches!(city.strategy, Strategy::Tech) {
        (base as u64 * 4).div_ceil(5) as u32 // ceil-div で 0 化を防ぐ
    } else {
        base
    }
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
    city.push_event(format!(
        "▷ {} ({},{}) 着工 -${}",
        building_name(kind),
        x,
        y,
        cost
    ));
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
///   • Each finished House contributes $0.5 (rounded down per tick batch
///     via the tick-loop integer accumulator) — folded in below as cents.
///   • Each finished Shop contributes $2.0 *if* it has at least one road
///     neighbor AND a customer base (a House within Manhattan distance 3).
///
/// We work in whole dollars and accept the rounding; the simulator reports
/// large numbers so the loss from int truncation is negligible.
pub fn compute_income_per_sec(city: &City) -> i64 {
    let mut income: i64 = 0;

    // Houses → flat residential tax.  We use ceiling division so that
    // even 1 house produces $1/sec; otherwise an unlucky early game
    // can leave the city with 1 house and income==0 (death spiral —
    // the simulator catches this on seed=0xDEADBEEF without this).
    //
    // 設計判断: HouseTier は描画レイヤーで「街が育つ感」を表現する派生値として
    // 使い、収入計算には反映しない。理由:
    //   - 戦略の特化 (simulator::tier4_strategies_specialize) を保つには
    //     人口×Tier 比例の収入は Tech 戦略に有利すぎる (Tech は道路+人口優位)
    //   - 経済発展感は次フェーズの Workshop / Shop チェーン拡張で実現する
    //     方が分業として綺麗 (Phase A continuation, DESIGN.md §5)
    let houses = city.count_built(Building::House) as i64;
    income += (houses + 1) / 2;

    // Workshops → 労働力 (隣接 House) + Road 接続が必要。$1/s。
    // House より早期に建てられる「最初の職場」として、戦略の幅を増やす。
    for y in 0..GRID_H {
        for x in 0..GRID_W {
            if matches!(city.tile(x, y), Tile::Built(Building::Workshop))
                && workshop_is_active(city, x, y)
            {
                income += 1;
            }
        }
    }

    // Shops → check connectivity + customer base.
    for y in 0..GRID_H {
        for x in 0..GRID_W {
            if matches!(city.tile(x, y), Tile::Built(Building::Shop))
                && shop_is_active(city, x, y)
            {
                income += 2;
            }
        }
    }

    // Tech 戦略の収入ペナルティ: -20% (整数で *4/5)。
    // 0 化の死スパイラルを避けるため `1` を下限に。
    if matches!(city.strategy, Strategy::Tech) && income > 0 {
        income = ((income * 4) / 5).max(1);
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
///   現状 Workshop 建物は未実装なので常に 0。実装後に効いてくる。
/// - `n_shop_within_5`: Manhattan 距離 5 以内の Shop 数。
/// - `n_house_within_3`: Manhattan 距離 3 以内の House 数 (自身は除く)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HouseNeighborhood {
    pub n_road_adj: u32,
    pub n_workshop_within_5: u32,
    pub n_shop_within_5: u32,
    pub n_house_within_3: u32,
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
                _ => {}
            }
        }
    }

    HouseNeighborhood {
        n_road_adj,
        n_workshop_within_5,
        n_shop_within_5,
        n_house_within_3,
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
    let economic_density = stats.n_workshop_within_5 + stats.n_shop_within_5;

    // Highrise: 商工業が回っている成熟ゾーン。
    if stats.n_road_adj >= 2 && economic_density >= 2 && stats.n_house_within_3 >= 3 {
        return HouseTier::Highrise;
    }

    // Apartment: 商業が来ている街区。Road + 経済施設 (Shop/Workshop) が必須。
    if stats.n_road_adj >= 1 && economic_density >= 1 {
        return HouseTier::Apartment;
    }

    HouseTier::Cottage
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
        }
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
    fn empty_city_earns_nothing() {
        let city = City::new();
        assert_eq!(compute_income_per_sec(&city), 0);
    }

    #[test]
    fn finished_houses_earn_residential_tax() {
        let mut city = City::new();
        city.set_tile(0, 0, Tile::Built(Building::House));
        // 1 house → 1 cash/sec (ceil(1/2) — survival floor, no stall)
        assert_eq!(compute_income_per_sec(&city), 1);
        city.set_tile(1, 0, Tile::Built(Building::House));
        // 2 houses → still 1 cash/sec (ceil(2/2))
        assert_eq!(compute_income_per_sec(&city), 1);
        city.set_tile(2, 0, Tile::Built(Building::House));
        // 3 houses → 2 cash/sec (ceil(3/2))
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
        // House (5,6) は Road 隣接 0 なので Cottage = $1。
        // Workshop は House 隣接無しで inactive → $0。
        assert_eq!(compute_income_per_sec(&city), 0);

        // 隣接 House を追加 → Workshop activate。
        city.set_tile(5, 6, Tile::Built(Building::House));
        // House (5,6): n_road_adj=0, Cottage = $1
        // Workshop: active → $1
        // Total: $2
        assert_eq!(compute_income_per_sec(&city), 2);
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
        // Shop ($2) + 1 house ceil(1/2)=1 → $3
        assert_eq!(compute_income_per_sec(&city), 3);
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
        let ok = start_construction(&mut city, 0, 0, Building::Road);
        assert!(ok);
        assert!(matches!(
            city.tile(0, 0),
            Tile::Construction { .. }
        ));
        // Run road's full build duration.
        tick(&mut city, Building::Road.build_ticks());
        assert!(matches!(city.tile(0, 0), Tile::Built(Building::Road)));
    }

    #[test]
    fn cant_afford_means_no_construction() {
        let mut city = City::new();
        city.cash = 5; // less than any building
        assert!(!start_construction(&mut city, 0, 0, Building::House));
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

    #[test]
    fn hire_worker_blocks_at_max() {
        let mut city = City::new();
        city.cash = 1_000_000;
        city.workers = MAX_WORKERS;
        assert!(!hire_worker(&mut city));
        assert_eq!(city.workers, MAX_WORKERS);
        assert_eq!(city.cash, 1_000_000);
    }
}
