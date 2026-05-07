//! AI brains.  Each Tier is a separate function with the same signature so
//! we can swap them via `decide()` and benchmark them independently.

use super::state::*;

/// 4-direction neighbour offsets shared by neighbour-checking helpers.
const DIRS4: [(i32, i32); 4] = [(-1, 0), (1, 0), (0, -1), (0, 1)];

/// What the AI wants the city to do this tick.
#[derive(Clone, Debug, PartialEq)]
pub enum AiAction {
    Build {
        x: usize,
        y: usize,
        kind: Building,
    },
    Idle,
}

/// Top-level dispatcher: routes to the active tier's brain.
///
/// **アーキテクチャ**: Tier 4 / 5 は `placement_value` ベースの評価 AI で、
/// 同じ評価関数を thinking depth (1 / 2 手先) で切替えるだけの差。
/// Tier 1-3 は historical なルールベースで残す (低 Tier の "dumb" さの担保)。
pub fn decide(city: &mut City) -> AiAction {
    match city.ai_tier {
        AiTier::Random => tier1_random(city),
        AiTier::Greedy => tier2_greedy(city),
        AiTier::RoadPlanner => tier3_road_planner(city),
        AiTier::DemandAware => tier4_value_search(city, 1),
        AiTier::DeepPlanner => tier4_value_search(city, 2),
    }
}

/// Tier 1 — Random Bot.
///
/// Intentionally dumb: picks a random empty cell and a random building.
/// Two safety nets:
///   1. "can I actually afford it?" — drops Idle when broke.
///   2. *savings protection*: with ~$2/sec income, cheap Roads ($10)
///      will drain funds before a House ($40) can ever be afforded.
///      Without this guard, the simulator observed "1 hour, 5 houses,
///      283 roads, grid full".  With this guard the AI saves up.
///
/// Distribution is biased 60% House / 25% Road / 15% Shop so the player
/// usually sees a population grow before shops appear, even though the
/// AI doesn't *understand* why.
fn tier1_random(city: &mut City) -> AiAction {
    // Pick building kind by weighted roll
    let roll = (city.next_rand() % 100) as u32;
    let kind = if roll < 60 {
        Building::House
    } else if roll < 85 {
        Building::Road
    } else {
        Building::Shop
    };

    // Affordability gate.
    if city.cash < kind.cost() {
        return AiAction::Idle;
    }

    // Savings protection: skip cheap Road/Shop rolls until we can also
    // afford a House.  Houses are the income source, so this prevents
    // the cheap-build death spiral.
    if !matches!(kind, Building::House) && city.cash < Building::House.cost() {
        return AiAction::Idle;
    }

    // No-customer guard: don't build a Shop with fewer than 3 houses
    // standing.  Without this, an unlucky early Shop roll can starve
    // the city of houses *and* leave the shop inactive (no customer
    // base in range), triggering an income==0 stall.  Even the dumbest
    // AI gets this one piece of "common sense".
    if matches!(kind, Building::Shop) && city.count_built(Building::House) < 3 {
        return AiAction::Idle;
    }

    // Try up to 30 random cells; if none are empty + buildable, idle.
    // Tier 1 はあえて愚直: 水セルに当たっても "haha 投げる" 回数で済ませる。
    // ただし Rock は機材必須なので、隣接 Outpost が無い Rock は除外する。
    // Tier 1 でも「機材ゲートは absolute rule」として尊重 — 戦略の話ではない。
    for _ in 0..30 {
        let r = city.next_rand();
        let x = (r as usize) % GRID_W;
        let y = ((r >> 32) as usize) % GRID_H;
        if super::logic::ai_can_break_ground(city, x, y) {
            return AiAction::Build { x, y, kind };
        }
    }
    AiAction::Idle
}

/// Tier 2 — Greedy.
///
/// Same building-kind roll, but picks an Empty cell that is **adjacent**
/// (4-connected) to an existing built tile.  Falls back to a random empty
/// cell if no adjacent option exists (early game).
fn tier2_greedy(city: &mut City) -> AiAction {
    let roll = (city.next_rand() % 100) as u32;
    let kind = if roll < 50 {
        Building::House
    } else if roll < 80 {
        Building::Road
    } else {
        Building::Shop
    };

    if city.cash < kind.cost() {
        return AiAction::Idle;
    }

    // Same savings + customer-base protections as Tier 1 — Tier 2 is
    // smarter about *where* it places things, not about *what economy*
    // to build.
    if !matches!(kind, Building::House) && city.cash < Building::House.cost() {
        return AiAction::Idle;
    }
    if matches!(kind, Building::Shop) && city.count_built(Building::House) < 3 {
        return AiAction::Idle;
    }

    // Collect candidate empties adjacent to a built/under-construction tile.
    // 地形が建設不可 (Water) や機材未到達の Rock は候補外。
    let mut candidates: Vec<(usize, usize)> = Vec::new();
    for y in 0..GRID_H {
        for x in 0..GRID_W {
            if !super::logic::ai_can_break_ground(city, x, y) {
                continue;
            }
            if has_built_neighbor(city, x, y) {
                candidates.push((x, y));
            }
        }
    }

    if candidates.is_empty() {
        return tier1_random(city);
    }
    let pick = (city.next_rand() as usize) % candidates.len();
    let (x, y) = candidates[pick];
    AiAction::Build { x, y, kind }
}

fn has_built_neighbor(city: &City, x: usize, y: usize) -> bool {
    for (dx, dy) in DIRS4 {
        let nx = x as i32 + dx;
        let ny = y as i32 + dy;
        if nx < 0 || ny < 0 || nx >= GRID_W as i32 || ny >= GRID_H as i32 {
            continue;
        }
        if !matches!(
            city.tile(nx as usize, ny as usize),
            Tile::Empty
        ) {
            return true;
        }
    }
    false
}

/// True if the cell at (x, y) is an Empty tile next to a finished or
/// under-construction Road.
fn is_empty_next_to_road(city: &City, x: usize, y: usize) -> bool {
    if !super::logic::ai_can_break_ground(city, x, y) {
        return false;
    }
    for (dx, dy) in DIRS4 {
        let nx = x as i32 + dx;
        let ny = y as i32 + dy;
        if nx < 0 || ny < 0 || nx >= GRID_W as i32 || ny >= GRID_H as i32 {
            continue;
        }
        match city.tile(nx as usize, ny as usize) {
            Tile::Built(Building::Road) => return true,
            Tile::Construction {
                target: Building::Road,
                ..
            } => return true,
            _ => {}
        }
    }
    false
}

/// True if (x, y) is an Empty cell adjacent to a House or Shop, useful
/// for "extend the road grid here so future buildings can connect".
fn is_empty_next_to_building(city: &City, x: usize, y: usize) -> bool {
    if !super::logic::ai_can_break_ground(city, x, y) {
        return false;
    }
    for (dx, dy) in DIRS4 {
        let nx = x as i32 + dx;
        let ny = y as i32 + dy;
        if nx < 0 || ny < 0 || nx >= GRID_W as i32 || ny >= GRID_H as i32 {
            continue;
        }
        if matches!(
            city.tile(nx as usize, ny as usize),
            Tile::Built(Building::House) | Tile::Built(Building::Shop)
        ) {
            return true;
        }
    }
    false
}

/// Tier 3 — Road Planner.
///
/// Same kind-roll as Tier 2, but placement obeys "the road network is
/// the backbone".  Concretely:
///   - Houses and Shops prefer cells adjacent to a Road.
///   - Roads prefer cells adjacent to a building (so we extend the
///     network *toward* development, not into the wilderness).
///   - On total dry spell (e.g., turn 1 with no roads), falls back to
///     Tier 2 placement so the city can still bootstrap.
///
/// **Strategy も「軽く」読む** (Tier 4 との階層差):
/// Tier 3 は Workshop を建てない (= 経済チェーンは Tier 4 専用) が、
/// House/Road/Shop の中で Strategy 寄りに `±10%` 揺らす。プレイヤーが
/// $5,000 で Tier 3 にアップグレードした時に「戦略ボタンが効く」実感が
/// 出る程度の弱い反映で、Tier 4 ($50,000) との価値差は維持する。
fn tier3_road_planner(city: &mut City) -> AiAction {
    // ベース 50/30/20 (House/Road/Shop) を Strategy で「気持ち程度」偏らせる。
    // Workshop は Tier 3 では建てない (= 0%) ので、Strategy::Income の
    // workshop 重みは無視。
    //
    // ±2% という非常に弱い反映: simulator::tier_ordering_holds_at_30min が
    // T3 < T4 を要求しているため、T3 が Strategy を強く読むと T4 を上回る。
    // 「ボタンを押した時にイベントログ + Status タブには変化が出るが、
    //  AI の挙動はほぼ変わらない」程度に留め、Tier 4 ($50,000) との価値差を
    //  保つ。±0 は寂しいので「気持ちだけ」反映。
    let info = super::logic::strategy_info(city.strategy);
    let house_pct = (50 + (info.house_pct as i32 - 35).clamp(-2, 2)).max(20) as u32;
    let road_pct = (30 + (info.road_pct as i32 - 30).clamp(-2, 2)).max(15) as u32;
    let shop_pct = 100u32.saturating_sub(house_pct + road_pct).max(5);
    let house_pct = 100 - road_pct - shop_pct; // 合計 100 を厳守

    let roll = (city.next_rand() % 100) as u32;
    let kind = if roll < house_pct {
        Building::House
    } else if roll < house_pct + road_pct {
        Building::Road
    } else {
        Building::Shop
    };

    if city.cash < kind.cost() {
        return AiAction::Idle;
    }
    if !matches!(kind, Building::House) && city.cash < Building::House.cost() {
        return AiAction::Idle;
    }
    if matches!(kind, Building::Shop) && city.count_built(Building::House) < 3 {
        return AiAction::Idle;
    }

    let candidates: Vec<(usize, usize)> = match kind {
        // Buildings prefer to live next to roads. Park は道路接続不要だが
        // Tier 3 が Park を選ぶことは無い (kind picker が House/Road/Shop のみ
        // — debug_assert で守る) — ただし match は網羅性が必要なので、
        // ここに来た場合は House と同じ扱いで safe fallback。
        // Outpost も AI は建てないが網羅性のため。
        Building::House
        | Building::Shop
        | Building::Workshop
        | Building::Park
        | Building::Outpost => (0..GRID_H)
            .flat_map(|y| (0..GRID_W).map(move |x| (x, y)))
            .filter(|(x, y)| is_empty_next_to_road(city, *x, *y))
            .collect(),
        // Roads prefer to extend toward existing buildings.
        Building::Road => (0..GRID_H)
            .flat_map(|y| (0..GRID_W).map(move |x| (x, y)))
            .filter(|(x, y)| is_empty_next_to_building(city, *x, *y))
            .collect(),
    };

    if candidates.is_empty() {
        // Bootstrapping early game (no roads yet) or stuck — fall back
        // to greedy adjacency so we don't deadlock.
        return tier2_greedy(city);
    }
    let pick = (city.next_rand() as usize) % candidates.len();
    let (x, y) = candidates[pick];
    AiAction::Build { x, y, kind }
}

/// Tier 4 / 5 共用の評価ベース placement search。
///
/// **アーキテクチャ**: `logic::placement_value` が「この候補で income/sec が
/// どれだけ増えるか」を cents 単位で返す純関数。AI はその値を最大化する
/// 候補を探すだけ。candidate set は **全建物種別** (Road / House / Workshop /
/// Shop / Park / **Outpost**) × **全 Empty cells** から評価する。
///
/// `depth = 1` (Tier 4): 各候補について placement_value を見て top-k から best 選択。
/// `depth = 2` (Tier 5): 各候補について 1 手目 value + 「2 手目に建てうる最良の
///   候補の value (= 1 手目を仮想的に着工した状態で再計算)」を加算した合計を評価。
///   これにより「道路を引いて、その隣に家を建てるシナリオ」が拾える。
///
/// **Outpost を candidate に含めることで saturation 解消が自然発生**:
///   中央満杯 → 既存 Empty cells の placement_value はほぼ 0 →
///   Outpost (隣接 Rock 数 × 期待 income) が相対的に勝つ → AI は外を割る。
///
/// 戻り値: AiAction::Build か AiAction::Idle。
fn tier4_value_search(city: &mut City, depth: u8) -> AiAction {
    use super::logic::placement_value;
    let connected = super::logic::compute_edge_connected_roads(city);

    let house_cost = Building::House.cost();
    let avoid_forest = matches!(city.strategy, Strategy::Eco);

    // 候補 cells を 2 群に分けて収集する (= O(N²) を避ける):
    //   `regular`  — 既存 Built に 4-近傍隣接する Empty cells (House/Road/etc 用)
    //   `outpost`  — Rock を 4-近傍に持ち、街から距離 4 以内の Plain Empty cells
    // 「Built 隣接」を distance 1 に絞ることで候補 cell 数が最大でも O(built * 4) ≈ 数百に。
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
            if avoid_forest && matches!(t, super::terrain::Terrain::Forest) {
                continue;
            }
            // Rock セルは Outpost が建てられないので Outpost candidate にもならない。
            // (Outpost は Plain にだけ建つ — Rock 上には建てない)
            let needs_outpost_unmet =
                t.needs_outpost() && !super::logic::has_outpost_neighbor(city, x, y);
            if needs_outpost_unmet {
                continue;
            }
            // Outpost 候補: Rock 隣接 + 街の近く + 整地不要 (Plain)
            let has_rock_n =
                super::logic::has_terrain_neighbor(city, x, y, super::terrain::Terrain::Rock);
            if has_rock_n
                && !t.needs_clearing()
                && super::logic::has_built_within_distance(city, x, y, 4)
            {
                outpost.push((x, y));
            }
            // 通常候補: Built 4-近傍隣接 (= 街の周辺 1 マス)
            if has_built_neighbor(city, x, y) {
                regular.push((x, y));
            }
        }
    }
    // 序盤フォールバック: 候補が枯渇していたら Built 距離 3 まで広げる。
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
                if avoid_forest && matches!(t, super::terrain::Terrain::Forest) {
                    continue;
                }
                if super::logic::has_built_within_distance(city, x, y, 3) {
                    regular.push((x, y));
                }
            }
        }
    }

    let mut best: Option<(usize, usize, Building, i64)> = None;
    let consider = |x: usize, y: usize, kind: Building, best: &mut Option<(usize, usize, Building, i64)>| {
        let cost = kind.cost();
        if city.cash < cost {
            return;
        }
        if matches!(kind, Building::Shop) && city.count_built(Building::House) < 3 {
            return;
        }
        if matches!(kind, Building::Workshop) && city.count_built(Building::House) < 2 {
            return;
        }
        // savings protection: 高コスト建物 (Workshop/Shop/Outpost) を建てたら House を
        // 建てる原資を割る場合は避ける。Outpost は飽和時専用なので例外。
        if !matches!(kind, Building::House | Building::Outpost)
            && (city.cash - cost) < house_cost
        {
            return;
        }

        let v1 = placement_value(city, x, y, kind, &connected);
        if v1 == i64::MIN {
            return;
        }
        let better = match *best {
            None => true,
            Some((_, _, _, prev)) => v1 > prev,
        };
        if better {
            *best = Some((x, y, kind, v1));
        }
    };

    // 通常 cells: 全 kinds (Outpost 除く) を評価
    let normal_kinds: &[Building] = &[
        Building::House,
        Building::Road,
        Building::Workshop,
        Building::Shop,
        Building::Park,
    ];
    for &(x, y) in &regular {
        for &kind in normal_kinds {
            consider(x, y, kind, &mut best);
        }
    }
    // Outpost cells: Outpost のみ評価 (= ほとんどのプレイで saturation 時のみ value 高)
    for &(x, y) in &outpost {
        consider(x, y, Building::Outpost, &mut best);
    }

    // depth=2 (Tier 5): top-1 の (x,y,kind) を採用する代わりに、上位 K (=12) を抽出して
    // それぞれに 2 手目評価を加算し、合計最大を選ぶ。
    if depth >= 2 {
        // 1 手目の上位 K 候補を集める。
        let mut top: Vec<(usize, usize, Building, i64)> = Vec::new();
        for &(x, y) in &regular {
            for &kind in normal_kinds {
                let cost = kind.cost();
                if city.cash < cost { continue; }
                if matches!(kind, Building::Shop) && city.count_built(Building::House) < 3 { continue; }
                if matches!(kind, Building::Workshop) && city.count_built(Building::House) < 2 { continue; }
                if !matches!(kind, Building::House | Building::Outpost)
                    && (city.cash - cost) < house_cost { continue; }
                let v = placement_value(city, x, y, kind, &connected);
                if v == i64::MIN { continue; }
                top.push((x, y, kind, v));
            }
        }
        for &(x, y) in &outpost {
            let cost = Building::Outpost.cost();
            if city.cash < cost { continue; }
            let v = placement_value(city, x, y, Building::Outpost, &connected);
            if v == i64::MIN { continue; }
            top.push((x, y, Building::Outpost, v));
        }
        top.sort_by(|a, b| b.3.cmp(&a.3));
        top.truncate(12);

        // 各 top 候補に 2 手目 value を加算
        let mut best2: Option<(usize, usize, Building, i64)> = None;
        for &(x, y, kind, v1) in &top {
            // 2 手目候補集合: 1 手目で建てた近傍 (距離 5) のみ。
            let mut nearby: Vec<(usize, usize)> = Vec::new();
            for &(rx, ry) in regular.iter().chain(outpost.iter()) {
                let manh = ((rx as i32 - x as i32).abs() + (ry as i32 - y as i32).abs()) as u32;
                if manh <= 5 && (rx, ry) != (x, y) {
                    nearby.push((rx, ry));
                }
            }
            let v2 = simulate_second_step_value(city, x, y, kind, &nearby, normal_kinds);
            let total = v1 + v2;
            let better = match best2 {
                None => true,
                Some((_, _, _, prev)) => total > prev,
            };
            if better {
                best2 = Some((x, y, kind, total));
            }
        }
        if let Some((x, y, kind, _)) = best2 {
            return AiAction::Build { x, y, kind };
        }
    }

    match best {
        Some((x, y, kind, _)) => AiAction::Build { x, y, kind },
        None => AiAction::Idle,
    }
}

/// 2 手目評価 — `(x, y, kind)` を仮想着工した状態で best 2 手目の placement_value
/// を返す。計算量抑制のため候補 cells は呼び側から渡す。
///
/// 仮想着工の実現方法: city を **clone せず**、評価関数側で「(x, y) に
/// kind が建っている」前提のラッパー city ビューを作るのが理想だが、
/// `placement_value` は `&City` を受け取るので、簡易に **clone** する。
/// これで logic を変えずに lookahead が成立する。
///
/// clone のコスト: City struct の grid + terrain + flash arrays で ~数 KB。
/// Tier 5 の AI 1 回 / tick = 10 回/秒 で数万 cells 走査するので無視できる範囲。
fn simulate_second_step_value(
    city: &City,
    x: usize,
    y: usize,
    kind: Building,
    cells: &[(usize, usize)],
    kinds: &[Building],
) -> i64 {
    // 仮想着工した city。建物を Built 状態として置く (Construction tick を読まない)。
    // Built として置くことで、direct_income_value も synergy も「建ってる前提」で評価される。
    let mut virt = clone_city_for_lookahead(city);
    virt.grid[y][x] = Tile::Built(kind);
    let connected2 = super::logic::compute_edge_connected_roads(&virt);

    let mut best2: i64 = 0; // 2 手目を打たない選択肢があるので 0 が下限。
    // top-K 絞り込み: 1 手目候補の周辺と Outpost 候補だけ。
    // 単純化のため cells をそのまま使うが、1 手目で建てた (x, y) は除外。
    // また、cash も 1 手目の cost を引いてシミュレート。
    let virtual_cash = city.cash - kind.cost();
    for &(cx, cy) in cells {
        if (cx, cy) == (x, y) {
            continue;
        }
        for &k2 in kinds {
            let c2 = k2.cost();
            if virtual_cash < c2 {
                continue;
            }
            let v = super::logic::placement_value(&virt, cx, cy, k2, &connected2);
            if v == i64::MIN {
                continue;
            }
            if v > best2 {
                best2 = v;
            }
        }
    }
    best2
}

/// 2 手目シミュレート用の軽量 City clone。grid と terrain だけ複製し、
/// 残りは default (= placement_value 内で参照しないフィールドはダミー値で OK)。
///
/// 完全 clone との差: `events` / `completion_flash_until` / `payout_flash_until`
/// は使わないため empty にして大幅に軽量化。`built_at_tick` は placement_value が
/// 読まないため empty で OK。
fn clone_city_for_lookahead(city: &City) -> City {
    City {
        grid: city.grid.clone(),
        terrain: city.terrain.clone(),
        world_seed: city.world_seed,
        cash: city.cash,
        tick: city.tick,
        ai_tier: city.ai_tier,
        strategy: city.strategy,
        panel_tab: city.panel_tab,
        last_observed_tier: city.last_observed_tier,
        tier_flash_until: 0,
        last_outpost_dispatch_tick: city.last_outpost_dispatch_tick,
        last_auto_demolish_tick: city.last_auto_demolish_tick,
        outposts_dispatched_total: city.outposts_dispatched_total,
        workers: city.workers,
        rng_state: city.rng_state,
        buildings_started: city.buildings_started,
        buildings_finished: city.buildings_finished,
        cash_earned_total: city.cash_earned_total,
        cash_spent_total: city.cash_spent_total,
        events: Vec::new(),
        completion_flash_until: vec![vec![0u64; GRID_W]; GRID_H],
        payout_flash_until: vec![vec![0u64; GRID_W]; GRID_H],
        last_payout_amount: 0,
        last_payout_tick: 0,
        built_at_tick: vec![vec![0u64; GRID_W]; GRID_H],
        cam_x: city.cam_x,
        cam_y: city.cam_y,
        selected_cell: None,
    }
}

