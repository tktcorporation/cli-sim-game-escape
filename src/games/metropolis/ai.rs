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
pub fn decide(city: &mut City) -> AiAction {
    match city.ai_tier {
        AiTier::Random => tier1_random(city),
        AiTier::Greedy => tier2_greedy(city),
        AiTier::RoadPlanner => tier3_road_planner(city),
        AiTier::DemandAware => tier4_demand_aware(city),
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
    for _ in 0..30 {
        let r = city.next_rand();
        let x = (r as usize) % GRID_W;
        let y = ((r >> 32) as usize) % GRID_H;
        if matches!(city.tile(x, y), Tile::Empty) && city.terrain_at(x, y).buildable() {
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
    // 地形が建設不可 (Water) のセルは候補外。
    let mut candidates: Vec<(usize, usize)> = Vec::new();
    for y in 0..GRID_H {
        for x in 0..GRID_W {
            if !matches!(city.tile(x, y), Tile::Empty) {
                continue;
            }
            if !city.terrain_at(x, y).buildable() {
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
    if !matches!(city.tile(x, y), Tile::Empty) {
        return false;
    }
    if !city.terrain_at(x, y).buildable() {
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
    if !matches!(city.tile(x, y), Tile::Empty) {
        return false;
    }
    if !city.terrain_at(x, y).buildable() {
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
/// The expected outcome: shops activate sooner because they tend to
/// land beside an actual road, instead of needing the random Tier-2
/// placement to stumble onto a road neighbour.
fn tier3_road_planner(city: &mut City) -> AiAction {
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
    if !matches!(kind, Building::House) && city.cash < Building::House.cost() {
        return AiAction::Idle;
    }
    if matches!(kind, Building::Shop) && city.count_built(Building::House) < 3 {
        return AiAction::Idle;
    }

    let candidates: Vec<(usize, usize)> = match kind {
        // Buildings prefer to live next to roads.
        Building::House | Building::Shop | Building::Workshop => (0..GRID_H)
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

/// Tier 4 — Demand Aware.
///
/// Reads `city.strategy` to weight the build-kind roll, then places
/// shops only on cells that *will actually activate* (road neighbour
/// AND a house within Manhattan-3).  This is the difference that lets
/// Tier 4 reliably outproduce Tier 3 even with the same building counts.
///
/// Per-strategy roll weights (House% / Road% / Workshop% / Shop%):
///   - Growth   →  70 / 20 / 0  / 10  (population first; Workshop に興味なし)
///   - Tech     →  35 / 50 / 0  / 15  (road network first; Workshop 不採用)
///   - Income   →  30 / 22 / 13 / 35  (cash first, lots of shops + workshops)
///
/// **Workshop は Income 戦略の独自要素** — Tier 4 の Income プレイヤーが
/// Shop 一択ではなく「Workshop で Apartment 化を促し、その上に Shop を載せる」
/// 経済チェーン構築を試みる、というキャラ付け。Growth/Tech は旧来の重みを
/// 維持して戦略特化を保つ (simulator::tier4_strategies_specialize の不変条件)。
fn tier4_demand_aware(city: &mut City) -> AiAction {
    let (house_pct, road_pct, workshop_pct) = match city.strategy {
        Strategy::Growth => (70, 20, 0),
        Strategy::Tech => (35, 50, 0),
        Strategy::Income => (30, 22, 13),
    };

    let roll = (city.next_rand() % 100) as u32;
    let kind = if roll < house_pct {
        Building::House
    } else if roll < house_pct + road_pct {
        Building::Road
    } else if roll < house_pct + road_pct + workshop_pct {
        Building::Workshop
    } else {
        Building::Shop
    };

    if city.cash < kind.cost() {
        return AiAction::Idle;
    }
    if !matches!(kind, Building::House) && city.cash < Building::House.cost() {
        return AiAction::Idle;
    }
    // Workshop / Shop は労働力 / 顧客となる House が最低限必要。
    if matches!(kind, Building::Shop) && city.count_built(Building::House) < 3 {
        return AiAction::Idle;
    }
    if matches!(kind, Building::Workshop) && city.count_built(Building::House) < 2 {
        return AiAction::Idle;
    }

    // Smartest placement: a shop only goes where it will earn from
    // turn 1 (road-adjacent + customer base in range).  Houses prefer
    // road-adjacent; roads prefer next-to-buildings.
    // Workshop は隣接 House と Road が両方必要なので、その条件を
    // 直接フィルタする (= 即時稼働する場所だけに置く)。
    let candidates: Vec<(usize, usize)> = match kind {
        Building::Shop => (0..GRID_H)
            .flat_map(|y| (0..GRID_W).map(move |x| (x, y)))
            .filter(|(x, y)| would_shop_activate_here(city, *x, *y))
            .collect(),
        Building::Workshop => (0..GRID_H)
            .flat_map(|y| (0..GRID_W).map(move |x| (x, y)))
            .filter(|(x, y)| would_workshop_activate_here(city, *x, *y))
            .collect(),
        Building::House => (0..GRID_H)
            .flat_map(|y| (0..GRID_W).map(move |x| (x, y)))
            .filter(|(x, y)| is_empty_next_to_road(city, *x, *y))
            .collect(),
        Building::Road => (0..GRID_H)
            .flat_map(|y| (0..GRID_W).map(move |x| (x, y)))
            .filter(|(x, y)| is_empty_next_to_building(city, *x, *y))
            .collect(),
    };

    if candidates.is_empty() {
        return tier3_road_planner(city);
    }
    let pick = (city.next_rand() as usize) % candidates.len();
    let (x, y) = candidates[pick];
    AiAction::Build { x, y, kind }
}

/// True iff placing a Workshop at (x, y) right now would have it earning
/// income from tick 1 (隣接 House (労働力) AND 隣接 Road が両方必要)。
fn would_workshop_activate_here(city: &City, wx: usize, wy: usize) -> bool {
    if !matches!(city.tile(wx, wy), Tile::Empty) {
        return false;
    }
    if !city.terrain_at(wx, wy).buildable() {
        return false;
    }
    let mut has_road = false;
    let mut has_house = false;
    for (dx, dy) in DIRS4 {
        let nx = wx as i32 + dx;
        let ny = wy as i32 + dy;
        if nx < 0 || ny < 0 || nx >= GRID_W as i32 || ny >= GRID_H as i32 {
            continue;
        }
        match city.tile(nx as usize, ny as usize) {
            Tile::Built(Building::Road) => has_road = true,
            Tile::Built(Building::House) => has_house = true,
            _ => {}
        }
        if has_road && has_house {
            return true;
        }
    }
    has_road && has_house
}

/// True iff placing a Shop at (x, y) right now would have it earning
/// income from tick 1 (road neighbour AND a House within Manhattan-3).
fn would_shop_activate_here(city: &City, sx: usize, sy: usize) -> bool {
    if !matches!(city.tile(sx, sy), Tile::Empty) {
        return false;
    }
    if !city.terrain_at(sx, sy).buildable() {
        return false;
    }
    // Road neighbour required.
    let mut has_road = false;
    for (dx, dy) in DIRS4 {
        let nx = sx as i32 + dx;
        let ny = sy as i32 + dy;
        if nx < 0 || ny < 0 || nx >= GRID_W as i32 || ny >= GRID_H as i32 {
            continue;
        }
        if matches!(
            city.tile(nx as usize, ny as usize),
            Tile::Built(Building::Road)
        ) {
            has_road = true;
            break;
        }
    }
    if !has_road {
        return false;
    }
    // House within Manhattan distance 3.
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
