//! Dungeon map generation — grid-based maze with rooms.
//!
//! Uses a recursive backtracker (DFS) to carve corridors, then places
//! special rooms (treasure, enemies, traps, springs, lore, NPCs, stairs).

// Grid algorithms use index-based loops for clarity.
#![allow(clippy::needless_range_loop, clippy::too_many_arguments)]

use super::state::{CellType, DungeonMap, Facing, MapCell};

// ── RNG (same LCG as logic.rs) ──────────────────────────────

fn next_rng(seed: u64) -> u64 {
    seed.wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407)
}

fn rng_range(seed: &mut u64, max: u32) -> u32 {
    *seed = next_rng(*seed);
    ((*seed >> 33) % max as u64) as u32
}

// ── Map Size ──────────────────────────────────────────────────

pub fn map_size(floor: u32) -> (usize, usize) {
    match floor {
        1..=2 => (7, 7),
        3..=5 => (9, 9),
        6..=9 => (11, 11),
        _ => (9, 9), // F10: compact for final battle
    }
}

// ── Generation ────────────────────────────────────────────────

pub fn generate_map(floor: u32, rng_seed: &mut u64) -> DungeonMap {
    let (w, h) = map_size(floor);

    // Initialize grid with all walls
    let mut grid = vec![
        vec![
            MapCell {
                walls: [true; 4],
                cell_type: CellType::Corridor,
                visited: false,
                event_done: false,
            };
            w
        ];
        h
    ];

    // Entrance at bottom center
    let start_x = w / 2;
    let start_y = h - 1;

    // Carve maze using recursive backtracker (iterative stack)
    carve_maze(&mut grid, w, h, start_x, start_y, rng_seed);

    // Add some extra passages for loops (makes exploration less tedious)
    add_extra_passages(&mut grid, w, h, rng_seed, floor);

    // Place entrance
    grid[start_y][start_x].cell_type = CellType::Entrance;

    // Place stairs (far from entrance, top area)
    let stairs_pos = find_stairs_position(&grid, w, h, start_x, start_y, rng_seed);
    grid[stairs_pos.1][stairs_pos.0].cell_type = CellType::Stairs;

    // Place special rooms
    place_rooms(&mut grid, w, h, floor, rng_seed, start_x, start_y, stairs_pos);

    DungeonMap {
        floor_num: floor,
        width: w,
        height: h,
        grid,
        player_x: start_x,
        player_y: start_y,
        facing: Facing::North,
    }
}

/// Carve a maze using iterative DFS (recursive backtracker).
fn carve_maze(
    grid: &mut [Vec<MapCell>],
    w: usize,
    h: usize,
    start_x: usize,
    start_y: usize,
    rng_seed: &mut u64,
) {
    let mut visited = vec![vec![false; w]; h];
    let mut stack: Vec<(usize, usize)> = Vec::new();

    visited[start_y][start_x] = true;
    stack.push((start_x, start_y));

    while let Some(&(cx, cy)) = stack.last() {
        // Find unvisited neighbors
        let mut neighbors = Vec::new();
        // North
        if cy > 0 && !visited[cy - 1][cx] {
            neighbors.push((cx, cy - 1, Facing::North));
        }
        // East
        if cx + 1 < w && !visited[cy][cx + 1] {
            neighbors.push((cx + 1, cy, Facing::East));
        }
        // South
        if cy + 1 < h && !visited[cy + 1][cx] {
            neighbors.push((cx, cy + 1, Facing::South));
        }
        // West
        if cx > 0 && !visited[cy][cx - 1] {
            neighbors.push((cx - 1, cy, Facing::West));
        }

        if neighbors.is_empty() {
            stack.pop();
        } else {
            let idx = rng_range(rng_seed, neighbors.len() as u32) as usize;
            let (nx, ny, dir) = neighbors[idx];

            // Remove wall between current and neighbor
            grid[cy][cx].set_wall(dir, false);
            grid[ny][nx].set_wall(dir.reverse(), false);

            visited[ny][nx] = true;
            stack.push((nx, ny));
        }
    }
}

/// Add extra passages to create loops (less linear, more explorable).
fn add_extra_passages(
    grid: &mut [Vec<MapCell>],
    w: usize,
    h: usize,
    rng_seed: &mut u64,
    floor: u32,
) {
    // More passages on deeper floors (wider corridors feel less claustrophobic)
    let extra_count = match floor {
        1..=2 => 2,
        3..=5 => 4,
        6..=9 => 6,
        _ => 3,
    };

    for _ in 0..extra_count {
        let x = rng_range(rng_seed, w as u32) as usize;
        let y = rng_range(rng_seed, h as u32) as usize;
        let dir_idx = rng_range(rng_seed, 4);
        let dir = [Facing::North, Facing::East, Facing::South, Facing::West][dir_idx as usize];
        let nx = x as i32 + dir.dx();
        let ny = y as i32 + dir.dy();

        if nx >= 0 && ny >= 0 && (nx as usize) < w && (ny as usize) < h {
            grid[y][x].set_wall(dir, false);
            grid[ny as usize][nx as usize].set_wall(dir.reverse(), false);
        }
    }
}

/// Find a good stairs position — far from entrance, in the top half.
fn find_stairs_position(
    grid: &[Vec<MapCell>],
    w: usize,
    h: usize,
    start_x: usize,
    start_y: usize,
    rng_seed: &mut u64,
) -> (usize, usize) {
    // BFS from entrance to find the farthest reachable cell in the top quarter
    let distances = bfs_distances(grid, w, h, start_x, start_y);

    let mut best_pos = (w / 2, 0);
    let mut best_dist = 0;

    for y in 0..h / 3 + 1 {
        for x in 0..w {
            if distances[y][x] > best_dist {
                best_dist = distances[y][x];
                best_pos = (x, y);
            }
        }
    }

    // If BFS didn't find anything good, pick random top cell
    if best_dist == 0 {
        best_pos = (rng_range(rng_seed, w as u32) as usize, 0);
    }

    best_pos
}

/// BFS to compute distances from a start cell.
fn bfs_distances(
    grid: &[Vec<MapCell>],
    w: usize,
    h: usize,
    start_x: usize,
    start_y: usize,
) -> Vec<Vec<u32>> {
    let mut dist = vec![vec![0u32; w]; h];
    let mut visited = vec![vec![false; w]; h];
    let mut queue = std::collections::VecDeque::new();

    visited[start_y][start_x] = true;
    queue.push_back((start_x, start_y, 1u32));

    while let Some((cx, cy, d)) = queue.pop_front() {
        dist[cy][cx] = d;

        for &dir in &[Facing::North, Facing::East, Facing::South, Facing::West] {
            if !grid[cy][cx].wall(dir) {
                let nx = (cx as i32 + dir.dx()) as usize;
                let ny = (cy as i32 + dir.dy()) as usize;
                if nx < w && ny < h && !visited[ny][nx] {
                    visited[ny][nx] = true;
                    queue.push_back((nx, ny, d + 1));
                }
            }
        }
    }
    dist
}

/// Place special rooms (enemies, treasure, traps, springs, lore, NPCs).
fn place_rooms(
    grid: &mut [Vec<MapCell>],
    w: usize,
    h: usize,
    floor: u32,
    rng_seed: &mut u64,
    entrance_x: usize,
    entrance_y: usize,
    stairs_pos: (usize, usize),
) {
    // Collect all corridor cells (not entrance/stairs)
    let mut candidates: Vec<(usize, usize)> = Vec::new();
    for y in 0..h {
        for x in 0..w {
            if grid[y][x].cell_type == CellType::Corridor
                && (x, y) != (entrance_x, entrance_y)
                && (x, y) != stairs_pos
            {
                candidates.push((x, y));
            }
        }
    }

    // Shuffle candidates
    for i in (1..candidates.len()).rev() {
        let j = rng_range(rng_seed, (i + 1) as u32) as usize;
        candidates.swap(i, j);
    }

    // Determine room counts based on floor
    let total = candidates.len();
    let (enemies, treasures, traps, springs, lores, npcs) = room_distribution(floor, total);

    for (placed, &(x, y)) in candidates.iter().enumerate() {
        if placed < enemies {
            grid[y][x].cell_type = CellType::Enemy;
        } else if placed < enemies + treasures {
            grid[y][x].cell_type = CellType::Treasure;
        } else if placed < enemies + treasures + traps {
            grid[y][x].cell_type = CellType::Trap;
        } else if placed < enemies + treasures + traps + springs {
            grid[y][x].cell_type = CellType::Spring;
        } else if placed < enemies + treasures + traps + springs + lores {
            grid[y][x].cell_type = CellType::Lore;
        } else if placed < enemies + treasures + traps + springs + lores + npcs {
            grid[y][x].cell_type = CellType::Npc;
        } else {
            break;
        }
    }
}

/// Room distribution: how many of each type based on floor and total available cells.
fn room_distribution(floor: u32, total: usize) -> (usize, usize, usize, usize, usize, usize) {
    let t = total as f32;
    let (enemy_pct, treasure_pct, trap_pct, spring_pct, lore_pct, npc_pct) = match floor {
        1..=2 => (0.25, 0.15, 0.05, 0.10, 0.10, 0.05),
        3..=5 => (0.30, 0.12, 0.10, 0.08, 0.08, 0.05),
        6..=9 => (0.35, 0.10, 0.12, 0.06, 0.06, 0.03),
        _ => (0.40, 0.08, 0.10, 0.05, 0.05, 0.02),
    };

    (
        (t * enemy_pct).round() as usize,
        (t * treasure_pct).round() as usize,
        (t * trap_pct).round() as usize,
        (t * spring_pct).round() as usize,
        (t * lore_pct).round() as usize,
        (t * npc_pct).round() as usize,
    )
}

// ── Tests ──────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_valid_map() {
        let mut seed = 42u64;
        let map = generate_map(1, &mut seed);
        assert_eq!(map.width, 7);
        assert_eq!(map.height, 7);
        assert_eq!(map.player_x, 3);
        assert_eq!(map.player_y, 6);
        assert_eq!(map.facing, Facing::North);
        assert_eq!(map.cell(3, 6).cell_type, CellType::Entrance);
    }

    #[test]
    fn map_has_stairs() {
        let mut seed = 42u64;
        let map = generate_map(1, &mut seed);
        let has_stairs = map
            .grid
            .iter()
            .flatten()
            .any(|c| c.cell_type == CellType::Stairs);
        assert!(has_stairs);
    }

    #[test]
    fn entrance_reachable_to_stairs() {
        let mut seed = 42u64;
        let map = generate_map(1, &mut seed);
        let dist = bfs_distances(&map.grid, map.width, map.height, map.player_x, map.player_y);

        // Find stairs
        let mut stairs_found = false;
        for y in 0..map.height {
            for x in 0..map.width {
                if map.cell(x, y).cell_type == CellType::Stairs {
                    assert!(dist[y][x] > 0, "Stairs must be reachable from entrance");
                    stairs_found = true;
                }
            }
        }
        assert!(stairs_found);
    }

    #[test]
    fn map_has_special_rooms() {
        let mut seed = 42u64;
        let map = generate_map(3, &mut seed);
        let types: Vec<CellType> = map
            .grid
            .iter()
            .flatten()
            .map(|c| c.cell_type)
            .collect();
        assert!(types.contains(&CellType::Enemy));
        assert!(types.contains(&CellType::Entrance));
        assert!(types.contains(&CellType::Stairs));
    }

    #[test]
    fn deeper_floors_are_larger() {
        let (w1, h1) = map_size(1);
        let (w5, _h5) = map_size(5);
        let (w8, h8) = map_size(8);
        assert!(w5 >= w1);
        assert!(w8 >= w5);
        assert!(h8 >= h1);
    }

    #[test]
    fn walls_are_consistent() {
        let mut seed = 123u64;
        let map = generate_map(2, &mut seed);
        // If cell (x,y) has no wall East, then cell (x+1,y) should have no wall West
        for y in 0..map.height {
            for x in 0..map.width {
                if x + 1 < map.width {
                    let east = map.cell(x, y).wall(Facing::East);
                    let west = map.cell(x + 1, y).wall(Facing::West);
                    assert_eq!(east, west, "Walls inconsistent at ({},{}) east vs ({},{}) west", x, y, x+1, y);
                }
                if y + 1 < map.height {
                    let south = map.cell(x, y).wall(Facing::South);
                    let north = map.cell(x, y + 1).wall(Facing::North);
                    assert_eq!(south, north, "Walls inconsistent at ({},{}) south vs ({},{}) north", x, y, x, y+1);
                }
            }
        }
    }
}
