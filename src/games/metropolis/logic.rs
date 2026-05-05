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
    city.tick = city.tick.wrapping_add(1);
}

/// Decrement every Construction tile; promote to Built when finished.
fn advance_construction(city: &mut City) {
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
                } else {
                    *ticks_remaining -= 1;
                }
            }
        }
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

/// Spend cash and turn an Empty cell into a Construction tile.
/// Returns false if the cell is non-empty or we can't afford it.
pub fn start_construction(city: &mut City, x: usize, y: usize, kind: Building) -> bool {
    if x >= GRID_W || y >= GRID_H {
        return false;
    }
    if !matches!(city.grid[y][x], Tile::Empty) {
        return false;
    }
    let cost = kind.cost();
    if city.cash < cost {
        return false;
    }
    city.cash -= cost;
    city.cash_spent_total += cost;
    city.grid[y][x] = Tile::Construction {
        target: kind,
        ticks_remaining: kind.build_ticks(),
    };
    city.buildings_started += 1;
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
    let houses = city.count_built(Building::House) as i64;
    income += (houses + 1) / 2;

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
    income
}

/// A shop earns money if it has a road neighbor *and* a house within
/// Manhattan distance 3.  This makes Tier-1's random scattering punishable
/// without being unwinnable.
fn shop_is_active(city: &City, sx: usize, sy: usize) -> bool {
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
    true
}

/// Hire one more worker.  Cost grows with current count.
pub fn hire_worker(city: &mut City) -> bool {
    let cost: i64 = 100 * (1 << (city.workers - 1));
    if city.cash < cost {
        return false;
    }
    city.cash -= cost;
    city.cash_spent_total += cost;
    city.workers += 1;
    true
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn shop_with_road_and_house_earns() {
        let mut city = City::new();
        city.set_tile(5, 5, Tile::Built(Building::Shop));
        city.set_tile(5, 4, Tile::Built(Building::Road));
        city.set_tile(5, 6, Tile::Built(Building::House));
        // Shop ($2) + 1 house ceil(1/2)=1 → $3
        assert_eq!(compute_income_per_sec(&city), 3);
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
}
