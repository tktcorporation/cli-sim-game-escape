//! AI brains.  Each Tier is a separate function with the same signature so
//! we can swap them via `decide()` and benchmark them independently.

use super::state::*;

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
        AiTier::RoadPlanner => tier2_greedy(city), // TODO: real impl
        AiTier::DemandAware => tier2_greedy(city), // TODO: real impl
    }
}

/// Tier 1 — Random Bot.
///
/// Intentionally dumb: picks a random empty cell and a random building.
/// The only safety net is "can I actually afford it?"; without that the
/// simulator showed money draining to zero before any income started.
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

    // Affordability gate (the *one* thing even the dumbest AI knows).
    if city.cash < kind.cost() {
        return AiAction::Idle;
    }

    // Try up to 30 random cells; if none are empty, idle this tick.
    for _ in 0..30 {
        let r = city.next_rand();
        let x = (r as usize) % GRID_W;
        let y = ((r >> 32) as usize) % GRID_H;
        if matches!(city.tile(x, y), Tile::Empty) {
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

    // Collect candidate empties adjacent to a built/under-construction tile.
    let mut candidates: Vec<(usize, usize)> = Vec::new();
    for y in 0..GRID_H {
        for x in 0..GRID_W {
            if !matches!(city.tile(x, y), Tile::Empty) {
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
    let dirs: [(i32, i32); 4] = [(-1, 0), (1, 0), (0, -1), (0, 1)];
    for (dx, dy) in dirs {
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
