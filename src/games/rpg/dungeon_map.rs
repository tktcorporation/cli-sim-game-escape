//! Dungeon map generation — PMD-style rooms + corridors.
//!
//! Divides the map into a 3×3 grid of sections, places rectangular rooms
//! in most sections, then connects adjacent rooms with 1-wide corridors.

// Grid algorithms use index-based loops for clarity.
#![allow(clippy::needless_range_loop)]

use super::state::{CellType, DungeonMap, Facing, MapCell, Room, Tile};

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
        1..=2 => (27, 27),
        3..=5 => (33, 33),
        6..=9 => (39, 39),
        _ => (27, 27), // F10: compact for final battle
    }
}

// ── Generation ────────────────────────────────────────────────

pub fn generate_map(floor: u32, rng_seed: &mut u64) -> DungeonMap {
    let (w, h) = map_size(floor);

    // Initialize grid with all walls
    let mut grid = vec![
        vec![
            MapCell {
                tile: Tile::Wall,
                cell_type: CellType::Corridor,
                visited: false,
                revealed: false,
                event_done: false,
                room_id: None,
            };
            w
        ];
        h
    ];

    // Section-based room placement (3×3 sections)
    let sec_w = w / 3;
    let sec_h = h / 3;

    // Decide which sections get rooms (7-8 out of 9).
    // Bottom-center (section index 7: row=2, col=1) MUST have a room (entrance).
    let mut has_room = [true; 9];
    {
        // Pick 1-2 sections to NOT have a room (but never section 7)
        let skip_count = 1 + rng_range(rng_seed, 2) as usize; // 1 or 2
        let mut skipped = 0;
        let mut attempts = 0;
        while skipped < skip_count && attempts < 30 {
            let idx = rng_range(rng_seed, 9) as usize;
            if idx != 7 && has_room[idx] {
                has_room[idx] = false;
                skipped += 1;
            }
            attempts += 1;
        }
    }

    let mut rooms: Vec<Room> = Vec::new();
    // room_section_map[section_idx] = index into rooms Vec, or usize::MAX if no room
    let mut room_section_map = [usize::MAX; 9];

    for sec_idx in 0..9 {
        if !has_room[sec_idx] {
            continue;
        }
        let sec_row = sec_idx / 3;
        let sec_col = sec_idx % 3;
        let sx = sec_col * sec_w;
        let sy = sec_row * sec_h;

        // Room size: 4-7 tiles, ensuring it fits in section with margin >= 1
        let max_rw = (sec_w - 2).min(7);
        let max_rh = (sec_h - 2).min(7);
        let min_rw = 4_usize.min(max_rw);
        let min_rh = 4_usize.min(max_rh);

        let rw = min_rw + rng_range(rng_seed, (max_rw - min_rw + 1) as u32) as usize;
        let rh = min_rh + rng_range(rng_seed, (max_rh - min_rh + 1) as u32) as usize;

        // Position within section (margin >= 1 from section edges)
        let max_rx = sec_w - rw - 1;
        let max_ry = sec_h - rh - 1;
        let rx = 1 + if max_rx > 1 {
            rng_range(rng_seed, (max_rx) as u32) as usize
        } else {
            0
        };
        let ry = 1 + if max_ry > 1 {
            rng_range(rng_seed, (max_ry) as u32) as usize
        } else {
            0
        };

        let room_x = sx + rx;
        let room_y = sy + ry;

        let room_id = rooms.len() as u8;
        room_section_map[sec_idx] = rooms.len();
        rooms.push(Room {
            x: room_x,
            y: room_y,
            w: rw,
            h: rh,
        });

        // Carve room
        for dy in 0..rh {
            for dx in 0..rw {
                let gx = room_x + dx;
                let gy = room_y + dy;
                if gx < w && gy < h {
                    grid[gy][gx].tile = Tile::RoomFloor;
                    grid[gy][gx].room_id = Some(room_id);
                }
            }
        }
    }

    // Connect adjacent rooms with corridors
    // Horizontal connections (col, col+1) for each row
    for row in 0..3 {
        for col in 0..2 {
            let left_idx = row * 3 + col;
            let right_idx = row * 3 + col + 1;
            if room_section_map[left_idx] != usize::MAX
                && room_section_map[right_idx] != usize::MAX
            {
                let left_room = &rooms[room_section_map[left_idx]];
                let right_room = &rooms[room_section_map[right_idx]];
                carve_horizontal_corridor(
                    &mut grid, w, h, left_room, right_room, sec_w, rng_seed,
                );
            }
        }
    }

    // Vertical connections (row, row+1) for each col
    for col in 0..3 {
        for row in 0..2 {
            let top_idx = row * 3 + col;
            let bot_idx = (row + 1) * 3 + col;
            if room_section_map[top_idx] != usize::MAX
                && room_section_map[bot_idx] != usize::MAX
            {
                let top_room = &rooms[room_section_map[top_idx]];
                let bot_room = &rooms[room_section_map[bot_idx]];
                carve_vertical_corridor(
                    &mut grid, w, h, top_room, bot_room, sec_h, rng_seed,
                );
            }
        }
    }

    // Ensure full connectivity via BFS; add extra corridors if needed
    ensure_connectivity(&mut grid, w, h, &rooms, &room_section_map, sec_w, sec_h, rng_seed);

    // Entrance: bottom-center section's room center
    let entrance_room_idx = room_section_map[7]; // section 7 = (row=2, col=1)
    let entrance_room = &rooms[entrance_room_idx];
    let start_x = entrance_room.x + entrance_room.w / 2;
    let start_y = entrance_room.y + entrance_room.h / 2;
    grid[start_y][start_x].cell_type = CellType::Entrance;

    // Stairs: farthest room from entrance by BFS
    let distances = bfs_distances(&grid, w, h, start_x, start_y);
    let stairs_room_idx = find_farthest_room(&rooms, &distances);
    let stairs_room = &rooms[stairs_room_idx];
    let stairs_x = stairs_room.x + stairs_room.w / 2;
    let stairs_y = stairs_room.y + stairs_room.h / 2;
    grid[stairs_y][stairs_x].cell_type = CellType::Stairs;

    // Place special events on RoomFloor tiles
    place_events(&mut grid, w, h, floor, rng_seed, start_x, start_y, stairs_x, stairs_y);

    DungeonMap {
        floor_num: floor,
        width: w,
        height: h,
        grid,
        player_x: start_x,
        player_y: start_y,
        last_dir: Facing::North,
        rooms,
    }
}

/// Carve a horizontal corridor connecting left_room and right_room.
fn carve_horizontal_corridor(
    grid: &mut [Vec<MapCell>],
    w: usize,
    h: usize,
    left_room: &Room,
    right_room: &Room,
    _sec_w: usize,
    rng_seed: &mut u64,
) {
    // Pick a Y within the overlap of the two rooms, or use their midpoints
    let left_mid_y = left_room.y + left_room.h / 2;
    let right_mid_y = right_room.y + right_room.h / 2;

    // Start X: right edge of left room
    let x_start = left_room.x + left_room.w;
    // End X: left edge of right room - 1
    let x_end = right_room.x.saturating_sub(1);

    // Choose a Y for the corridor (random between the two midpoints)
    let cy = if left_mid_y <= right_mid_y {
        left_mid_y + rng_range(rng_seed, (right_mid_y - left_mid_y + 1) as u32) as usize
    } else {
        right_mid_y + rng_range(rng_seed, (left_mid_y - right_mid_y + 1) as u32) as usize
    };
    let cy = cy.clamp(1, h - 2);

    // Carve from left room edge to the corridor Y, then horizontal, then to right room
    // Step 1: vertical from left_mid_y to cy at x_start-1
    let exit_x = (x_start).min(w - 1);
    carve_vertical_segment(grid, w, h, exit_x, left_mid_y, cy);

    // Step 2: horizontal from exit_x to entry_x at cy
    let entry_x = x_end.min(w - 1);
    carve_horizontal_segment(grid, w, h, exit_x, entry_x, cy);

    // Step 3: vertical from cy to right_mid_y at entry_x
    carve_vertical_segment(grid, w, h, entry_x, cy, right_mid_y);
}

/// Carve a vertical corridor connecting top_room and bot_room.
fn carve_vertical_corridor(
    grid: &mut [Vec<MapCell>],
    w: usize,
    h: usize,
    top_room: &Room,
    bot_room: &Room,
    _sec_h: usize,
    rng_seed: &mut u64,
) {
    let top_mid_x = top_room.x + top_room.w / 2;
    let bot_mid_x = bot_room.x + bot_room.w / 2;

    let y_start = top_room.y + top_room.h;
    let y_end = bot_room.y.saturating_sub(1);

    let cx = if top_mid_x <= bot_mid_x {
        top_mid_x + rng_range(rng_seed, (bot_mid_x - top_mid_x + 1) as u32) as usize
    } else {
        bot_mid_x + rng_range(rng_seed, (top_mid_x - bot_mid_x + 1) as u32) as usize
    };
    let cx = cx.clamp(1, w - 2);

    let exit_y = y_start.min(h - 1);
    carve_horizontal_segment(grid, w, h, top_mid_x, cx, exit_y);

    let entry_y = y_end.min(h - 1);
    carve_vertical_segment(grid, w, h, cx, exit_y, entry_y);

    carve_horizontal_segment(grid, w, h, cx, bot_mid_x, entry_y);
}

fn carve_horizontal_segment(
    grid: &mut [Vec<MapCell>],
    w: usize,
    _h: usize,
    x1: usize,
    x2: usize,
    y: usize,
) {
    let (start, end) = if x1 <= x2 { (x1, x2) } else { (x2, x1) };
    for x in start..=end.min(w - 1) {
        if grid[y][x].tile == Tile::Wall {
            grid[y][x].tile = Tile::Corridor;
        }
    }
}

fn carve_vertical_segment(
    grid: &mut [Vec<MapCell>],
    _w: usize,
    h: usize,
    x: usize,
    y1: usize,
    y2: usize,
) {
    let (start, end) = if y1 <= y2 { (y1, y2) } else { (y2, y1) };
    for y in start..=end.min(h - 1) {
        if grid[y][x].tile == Tile::Wall {
            grid[y][x].tile = Tile::Corridor;
        }
    }
}

/// Ensure all rooms are connected via BFS. If disconnected components exist,
/// add corridors to link them.
#[allow(clippy::too_many_arguments)]
fn ensure_connectivity(
    grid: &mut [Vec<MapCell>],
    w: usize,
    h: usize,
    rooms: &[Room],
    room_section_map: &[usize; 9],
    sec_w: usize,
    sec_h: usize,
    rng_seed: &mut u64,
) {
    if rooms.is_empty() {
        return;
    }

    // BFS from the first room center
    let start_room = &rooms[0];
    let sx = start_room.x + start_room.w / 2;
    let sy = start_room.y + start_room.h / 2;
    let distances = bfs_distances(grid, w, h, sx, sy);

    // Check which rooms are reachable
    let mut unreachable: Vec<usize> = Vec::new();
    for (i, room) in rooms.iter().enumerate() {
        let cx = room.x + room.w / 2;
        let cy = room.y + room.h / 2;
        if distances[cy][cx] == 0 && (cx != sx || cy != sy) {
            unreachable.push(i);
        }
    }

    // For each unreachable room, find its section neighbor and force a corridor
    for &room_idx in &unreachable {
        // Find which section this room belongs to
        let sec_idx = room_section_map
            .iter()
            .position(|&ri| ri == room_idx)
            .unwrap_or(0);

        let sec_row = sec_idx / 3;
        let sec_col = sec_idx % 3;

        // Try adjacent sections in all 4 directions
        let neighbors: [(i32, i32); 4] = [(0, -1), (1, 0), (0, 1), (-1, 0)];
        for &(dc, dr) in &neighbors {
            let nr = sec_row as i32 + dr;
            let nc = sec_col as i32 + dc;
            if !(0..3).contains(&nr) || !(0..3).contains(&nc) {
                continue;
            }
            let neighbor_sec = nr as usize * 3 + nc as usize;
            if room_section_map[neighbor_sec] == usize::MAX {
                continue;
            }
            let neighbor_room_idx = room_section_map[neighbor_sec];
            let neighbor_room = &rooms[neighbor_room_idx];
            let nr_cx = neighbor_room.x + neighbor_room.w / 2;
            let nr_cy = neighbor_room.y + neighbor_room.h / 2;

            // Check if neighbor is reachable from start
            if distances[nr_cy][nr_cx] > 0 || (nr_cx == sx && nr_cy == sy) {
                // Connect this room to the neighbor
                let this_room = &rooms[room_idx];
                if dc != 0 {
                    // Horizontal connection
                    let (left, right) = if dc > 0 {
                        (this_room, neighbor_room)
                    } else {
                        (neighbor_room, this_room)
                    };
                    carve_horizontal_corridor(grid, w, h, left, right, sec_w, rng_seed);
                } else {
                    // Vertical connection
                    let (top, bot) = if dr > 0 {
                        (this_room, neighbor_room)
                    } else {
                        (neighbor_room, this_room)
                    };
                    carve_vertical_corridor(grid, w, h, top, bot, sec_h, rng_seed);
                }
                break;
            }
        }
    }
}

/// BFS to compute distances from a start cell (only walks on walkable tiles).
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
            let nx = cx as i32 + dir.dx();
            let ny = cy as i32 + dir.dy();
            if nx >= 0 && ny >= 0 {
                let ux = nx as usize;
                let uy = ny as usize;
                if ux < w && uy < h && !visited[uy][ux] && grid[uy][ux].is_walkable() {
                    visited[uy][ux] = true;
                    queue.push_back((ux, uy, d + 1));
                }
            }
        }
    }
    dist
}

/// Find the room whose center is farthest from the entrance by BFS distance.
fn find_farthest_room(rooms: &[Room], distances: &[Vec<u32>]) -> usize {
    let mut best_idx = 0;
    let mut best_dist = 0;
    for (i, room) in rooms.iter().enumerate() {
        let cx = room.x + room.w / 2;
        let cy = room.y + room.h / 2;
        if distances[cy][cx] > best_dist {
            best_dist = distances[cy][cx];
            best_idx = i;
        }
    }
    best_idx
}

/// Place special events on RoomFloor tiles.
#[allow(clippy::too_many_arguments)]
fn place_events(
    grid: &mut [Vec<MapCell>],
    w: usize,
    h: usize,
    floor: u32,
    rng_seed: &mut u64,
    entrance_x: usize,
    entrance_y: usize,
    stairs_x: usize,
    stairs_y: usize,
) {
    // Collect all RoomFloor cells (not entrance/stairs position)
    let mut candidates: Vec<(usize, usize)> = Vec::new();
    for y in 0..h {
        for x in 0..w {
            if grid[y][x].tile == Tile::RoomFloor
                && grid[y][x].cell_type == CellType::Corridor
                && (x, y) != (entrance_x, entrance_y)
                && (x, y) != (stairs_x, stairs_y)
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

    // Determine event counts based on floor
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
    // Total event density ~30-40% so most steps are quiet exploration.
    let (enemy_pct, treasure_pct, trap_pct, spring_pct, lore_pct, npc_pct) = match floor {
        1..=2 => (0.12, 0.06, 0.02, 0.05, 0.04, 0.03), // ~32%
        3..=5 => (0.15, 0.05, 0.04, 0.04, 0.03, 0.02), // ~33%
        6..=9 => (0.18, 0.05, 0.05, 0.03, 0.02, 0.02), // ~35%
        _ => (0.20, 0.04, 0.06, 0.03, 0.02, 0.01),     // ~36%
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
        assert_eq!(map.width, 27);
        assert_eq!(map.height, 27);
        assert_eq!(map.last_dir, Facing::North);
        assert_eq!(
            map.cell(map.player_x, map.player_y).cell_type,
            CellType::Entrance
        );
        // Player is on a walkable tile
        assert!(map.player_cell().is_walkable());
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
    fn rooms_exist_and_connected() {
        for seed_base in [42u64, 100, 999, 12345] {
            let mut seed = seed_base;
            let map = generate_map(1, &mut seed);
            assert!(
                !map.rooms.is_empty(),
                "Map should have rooms (seed={})",
                seed_base
            );

            // All rooms reachable from entrance
            let dist = bfs_distances(
                &map.grid,
                map.width,
                map.height,
                map.player_x,
                map.player_y,
            );
            for (i, room) in map.rooms.iter().enumerate() {
                let cx = room.x + room.w / 2;
                let cy = room.y + room.h / 2;
                assert!(
                    dist[cy][cx] > 0 || (cx == map.player_x && cy == map.player_y),
                    "Room {} not reachable from entrance (seed={})",
                    i,
                    seed_base
                );
            }
        }
    }

    #[test]
    fn room_tiles_have_room_id() {
        let mut seed = 42u64;
        let map = generate_map(1, &mut seed);
        for (i, room) in map.rooms.iter().enumerate() {
            let cx = room.x + room.w / 2;
            let cy = room.y + room.h / 2;
            assert_eq!(
                map.cell(cx, cy).room_id,
                Some(i as u8),
                "Room center should have correct room_id"
            );
            assert_eq!(map.cell(cx, cy).tile, Tile::RoomFloor);
        }
    }

    #[test]
    fn all_floors_generate_valid_maps() {
        for floor in 1..=10 {
            let mut seed = 42u64 + floor as u64;
            let map = generate_map(floor, &mut seed);
            // Has entrance, stairs, rooms
            let has_entrance = map
                .grid
                .iter()
                .flatten()
                .any(|c| c.cell_type == CellType::Entrance);
            let has_stairs = map
                .grid
                .iter()
                .flatten()
                .any(|c| c.cell_type == CellType::Stairs);
            assert!(has_entrance, "Floor {} missing entrance", floor);
            assert!(has_stairs, "Floor {} missing stairs", floor);
            assert!(!map.rooms.is_empty(), "Floor {} has no rooms", floor);
        }
    }
}
