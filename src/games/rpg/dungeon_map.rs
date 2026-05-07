//! Dungeon map generation — PMD-style rooms + corridors.
//!
//! Divides the map into a 3×3 grid of sections, places rectangular rooms
//! in most sections, then connects adjacent rooms with 1-wide corridors.

// Grid algorithms use index-based loops for clarity.
#![allow(clippy::needless_range_loop)]

use super::state::{
    elite_chance, enemy_affix_info, enemy_info, floor_enemies, vault_chance, CellType, DungeonMap,
    EnemyAffix, EnemyKind, Facing, MapCell, Monster, Room, Tile, VaultKind, ALL_ENEMY_AFFIXES,
};

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

    // Place special vault — curated room with hand-picked contents.
    // Done BEFORE place_events so vault cells (Treasure / Idol) are
    // already non-Corridor and the regular event distributor skips them.
    let vault = maybe_place_vault(
        &mut grid, &rooms, &room_section_map, floor, rng_seed,
        entrance_room_idx, stairs_room_idx,
    );

    // Place special events on RoomFloor tiles
    place_events(&mut grid, w, h, floor, rng_seed, start_x, start_y, stairs_x, stairs_y);

    // Spawn monster entities (separate from cell types). Skips the vault
    // room id so the curated guards aren't drowned by random spawns.
    let vault_room_id = vault.as_ref().map(|v| v.room_id);
    let mut monsters = spawn_monsters(
        &grid, w, h, floor, start_x, start_y, stairs_x, stairs_y, rng_seed, vault_room_id,
    );

    // Add vault guards now that the regular enemies are placed.
    if let Some(v) = &vault {
        spawn_vault_guards(&mut monsters, v, &rooms, floor, rng_seed);
    }

    DungeonMap {
        floor_num: floor,
        width: w,
        height: h,
        grid,
        player_x: start_x,
        player_y: start_y,
        last_dir: Facing::North,
        rooms,
        monsters,
        is_overworld: false,
    }
}

/// Spawn monster entities on walkable tiles (room floors, away from
/// entrance/stairs/event cells). The bottom floor (B10F) spawns the
/// Demon Lord at the stairs cell as a boss encounter.
///
/// `skip_room_id`: when set, no random monsters spawn inside the
/// matching room — used by the vault system so curated guards are
/// the only inhabitants.
#[allow(clippy::too_many_arguments)]
fn spawn_monsters(
    grid: &[Vec<MapCell>],
    w: usize,
    h: usize,
    floor: u32,
    entrance_x: usize,
    entrance_y: usize,
    stairs_x: usize,
    stairs_y: usize,
    rng_seed: &mut u64,
    skip_room_id: Option<u8>,
) -> Vec<Monster> {
    let mut monsters = Vec::new();

    // Boss floor: just the Demon Lord at the stairs tile.
    if floor >= super::state::MAX_FLOOR {
        let info = enemy_info(super::state::EnemyKind::DemonLord);
        monsters.push(Monster {
            kind: super::state::EnemyKind::DemonLord,
            x: stairs_x,
            y: stairs_y,
            hp: info.max_hp,
            max_hp: info.max_hp,
            awake: true,
            charging: false,
            affix: None,
        });
        return monsters;
    }

    // Collect candidate tiles: walkable, not on entrance/stairs, no event.
    let mut candidates: Vec<(usize, usize)> = Vec::new();
    for y in 0..h {
        for x in 0..w {
            let cell = &grid[y][x];
            if !cell.is_walkable() { continue; }
            if (x, y) == (entrance_x, entrance_y) { continue; }
            if (x, y) == (stairs_x, stairs_y) { continue; }
            if cell.cell_type != CellType::Corridor { continue; }
            if cell.tile != Tile::RoomFloor { continue; }
            if let (Some(skip), Some(rid)) = (skip_room_id, cell.room_id) {
                if skip == rid { continue; }
            }
            // Don't spawn within 4 tiles of entrance (give player breathing room)
            let dx = x as i32 - entrance_x as i32;
            let dy = y as i32 - entrance_y as i32;
            if dx * dx + dy * dy < 16 { continue; }
            candidates.push((x, y));
        }
    }

    // Shuffle
    for i in (1..candidates.len()).rev() {
        let j = rng_range(rng_seed, (i + 1) as u32) as usize;
        candidates.swap(i, j);
    }

    // Spawn count scales with floor
    let count = match floor {
        1..=2 => 4,
        3..=5 => 6,
        6..=9 => 8,
        _ => 10,
    };
    let pool = floor_enemies(floor);

    let elite_pct = elite_chance(floor);
    for &(x, y) in candidates.iter().take(count) {
        let kind = pool[rng_range(rng_seed, pool.len() as u32) as usize];
        let info = enemy_info(kind);
        // Elite affix roll. Boss (DemonLord) is excluded by elite_chance
        // returning 0 on the boss floor.
        let affix = if elite_pct > 0 && rng_range(rng_seed, 100) < elite_pct {
            let i = rng_range(rng_seed, ALL_ENEMY_AFFIXES.len() as u32) as usize;
            Some(ALL_ENEMY_AFFIXES[i])
        } else {
            None
        };
        let max_hp = match affix {
            Some(a) => (info.max_hp * enemy_affix_info(a).hp_pct / 100).max(1),
            None => info.max_hp,
        };
        monsters.push(Monster {
            kind,
            x,
            y,
            hp: max_hp,
            max_hp,
            awake: false,
            charging: false,
            affix,
        });
    }

    monsters
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
/// add corridors to link them. Re-runs BFS after each round of connections
/// to handle chained unreachable rooms.
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

    let start_room = &rooms[0];
    let sx = start_room.x + start_room.w / 2;
    let sy = start_room.y + start_room.h / 2;

    // Hard cap on outer iterations as defense in depth — every iteration
    // that makes progress reduces the unreachable set by at least one, so
    // `rooms.len()` rounds is plenty.
    for _ in 0..rooms.len().saturating_add(2) {
        // BFS from the first room center
        let distances = bfs_distances(grid, w, h, sx, sy);

        // Check which rooms are reachable
        let unreachable: Vec<usize> = rooms
            .iter()
            .enumerate()
            .filter_map(|(i, room)| {
                let cx = room.x + room.w / 2;
                let cy = room.y + room.h / 2;
                if distances[cy][cx] == 0 && (cx != sx || cy != sy) {
                    Some(i)
                } else {
                    None
                }
            })
            .collect();

        if unreachable.is_empty() {
            return;
        }

        // First, try to connect each unreachable room to a reachable
        // neighbor section (the cheap, "natural-looking" path).
        let mut progressed = false;
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
                    progressed = true;
                    break;
                }
            }
        }

        // If no unreachable room had a reachable neighbor section, the
        // section graph is disconnected (e.g. skip pattern {1,3} isolates
        // section 0). Force-connect the first unreachable room directly
        // to the start room with an L-shaped corridor — guaranteed to
        // make progress, so the next BFS round will see fewer unreachable
        // rooms.
        if !progressed {
            let target = &rooms[unreachable[0]];
            let tx = target.x + target.w / 2;
            let ty = target.y + target.h / 2;
            carve_l_corridor(grid, w, h, sx, sy, tx, ty, rng_seed);
        }
    }
}

/// Carve an L-shaped corridor between two arbitrary points, going through
/// any walls in between. Used as a last-resort fallback when adjacent-section
/// routing can't reach an isolated room.
#[allow(clippy::too_many_arguments)]
fn carve_l_corridor(
    grid: &mut [Vec<MapCell>],
    w: usize,
    h: usize,
    x1: usize,
    y1: usize,
    x2: usize,
    y2: usize,
    rng_seed: &mut u64,
) {
    if rng_range(rng_seed, 2) == 0 {
        carve_horizontal_segment(grid, w, h, x1, x2, y1);
        carve_vertical_segment(grid, w, h, x2, y1, y2);
    } else {
        carve_vertical_segment(grid, w, h, x1, y1, y2);
        carve_horizontal_segment(grid, w, h, x1, y2, x2);
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
    let dist = room_distribution(floor, total);

    let plan: [(usize, CellType); 11] = [
        (dist.treasures, CellType::Treasure),
        (dist.traps, CellType::Trap),
        (dist.springs, CellType::Spring),
        (dist.lores, CellType::Lore),
        (dist.npcs, CellType::Npc),
        // Issue #90: Elona-flavored encounters scattered across floors.
        (dist.fallen, CellType::FallenAdventurer),
        (dist.fruits, CellType::FruitTree),
        (dist.wells, CellType::Well),
        (dist.idols, CellType::Idol),
        (dist.peddlers, CellType::Peddler),
        (dist.eggs, CellType::MonsterEgg),
    ];

    let mut placed = 0usize;
    for &(count, cell_type) in &plan {
        for &(x, y) in candidates.iter().skip(placed).take(count) {
            grid[y][x].cell_type = cell_type;
        }
        placed += count;
        if placed >= candidates.len() {
            break;
        }
    }
}

struct RoomDist {
    treasures: usize,
    traps: usize,
    springs: usize,
    lores: usize,
    npcs: usize,
    fallen: usize,
    fruits: usize,
    wells: usize,
    idols: usize,
    peddlers: usize,
    eggs: usize,
}

/// Room distribution: how many of each event type based on floor and total cells.
/// Enemies are spawned separately as Monster entities.
///
/// Issue #90: density bumped from ~30% to ~40% by adding 6 new event types.
/// Each type has its own per-floor curve so the player encounters a varied
/// mix instead of the old "treasure/trap dominate" feel.
fn room_distribution(floor: u32, total: usize) -> RoomDist {
    let t = total as f32;
    // Per floor band: (treasure, trap, spring, lore, npc, fallen, fruit, well, idol, peddler, egg).
    let pct: [f32; 11] = match floor {
        1..=2 => [0.05, 0.02, 0.04, 0.04, 0.02, 0.03, 0.04, 0.03, 0.02, 0.02, 0.03],
        3..=5 => [0.05, 0.03, 0.03, 0.03, 0.02, 0.04, 0.03, 0.03, 0.03, 0.03, 0.03],
        6..=9 => [0.05, 0.04, 0.02, 0.02, 0.02, 0.04, 0.02, 0.03, 0.04, 0.04, 0.03],
        _ => [0.04, 0.05, 0.02, 0.02, 0.01, 0.05, 0.02, 0.03, 0.04, 0.03, 0.03],
    };

    let n = |p: f32| -> usize { (t * p).round() as usize };
    RoomDist {
        treasures: n(pct[0]),
        traps: n(pct[1]),
        springs: n(pct[2]),
        lores: n(pct[3]),
        npcs: n(pct[4]),
        fallen: n(pct[5]),
        fruits: n(pct[6]),
        wells: n(pct[7]),
        idols: n(pct[8]),
        peddlers: n(pct[9]),
        eggs: n(pct[10]),
    }
}

// ── Vault placement ───────────────────────────────────────────

/// Information about a placed vault, used for follow-up monster spawning.
pub(crate) struct PlacedVault {
    pub kind: VaultKind,
    pub room_id: u8,
    pub center_x: usize,
    pub center_y: usize,
}

/// Pick a non-entrance, non-stairs room (when one exists) and convert it
/// into a vault. Cells inside the room are repainted with vault contents
/// (Treasure / Idol) so the regular event distributor leaves them alone.
fn maybe_place_vault(
    grid: &mut [Vec<MapCell>],
    rooms: &[Room],
    room_section_map: &[usize; 9],
    floor: u32,
    rng_seed: &mut u64,
    entrance_room_idx: usize,
    stairs_room_idx: usize,
) -> Option<PlacedVault> {
    let chance = vault_chance(floor);
    if chance == 0 || rng_range(rng_seed, 100) >= chance {
        return None;
    }

    // Eligible rooms: any room that's neither the entrance nor the stairs
    // room. Walking-around lookups via room_section_map keep things in
    // bounds without re-scanning the grid.
    let mut candidates: Vec<usize> = (0..9)
        .filter_map(|sec| {
            let r = room_section_map[sec];
            if r == usize::MAX { return None; }
            if r == entrance_room_idx || r == stairs_room_idx { return None; }
            Some(r)
        })
        .collect();
    if candidates.is_empty() { return None; }

    // Shuffle and take the first.
    for i in (1..candidates.len()).rev() {
        let j = rng_range(rng_seed, (i + 1) as u32) as usize;
        candidates.swap(i, j);
    }
    let room_idx = candidates[0];
    let room = &rooms[room_idx];
    let cx = room.x + room.w / 2;
    let cy = room.y + room.h / 2;

    let kind = if rng_range(rng_seed, 2) == 0 {
        VaultKind::TreasureVault
    } else {
        VaultKind::AltarChamber
    };

    match kind {
        VaultKind::TreasureVault => {
            // Three Treasure cells along the central row.
            grid[cy][cx].cell_type = CellType::Treasure;
            // Try left and right neighbours within the room bounds.
            if cx > room.x {
                grid[cy][cx - 1].cell_type = CellType::Treasure;
            }
            if cx + 1 < room.x + room.w {
                grid[cy][cx + 1].cell_type = CellType::Treasure;
            }
        }
        VaultKind::AltarChamber => {
            // Single Idol at the center; the rest stays as room floor.
            grid[cy][cx].cell_type = CellType::Idol;
        }
    }

    Some(PlacedVault {
        kind,
        room_id: room_idx as u8,
        center_x: cx,
        center_y: cy,
    })
}

/// Spawn 1-2 elite-flavored guards inside the vault room. The vault
/// monsters always carry an `EnemyAffix` to stand out from regular mobs
/// even when the floor's elite roll wouldn't normally trigger.
fn spawn_vault_guards(
    monsters: &mut Vec<Monster>,
    vault: &PlacedVault,
    rooms: &[Room],
    floor: u32,
    rng_seed: &mut u64,
) {
    let room = &rooms[vault.room_id as usize];
    // Pick one of the floor's regular enemy kinds for thematic consistency.
    let pool = floor_enemies(floor);
    if pool.is_empty() { return; }

    let pick_affix = |seed: &mut u64| -> EnemyAffix {
        let i = rng_range(seed, ALL_ENEMY_AFFIXES.len() as u32) as usize;
        ALL_ENEMY_AFFIXES[i]
    };

    let count = match vault.kind {
        VaultKind::TreasureVault => 2,
        VaultKind::AltarChamber => 1,
    };

    // Place guards near the corners of the room (avoid stepping on the
    // central ritual / treasure cells).
    let corners = [
        (room.x + 1,           room.y + 1),
        (room.x + room.w - 2,  room.y + 1),
        (room.x + 1,           room.y + room.h - 2),
        (room.x + room.w - 2,  room.y + room.h - 2),
    ];

    // Vault guards pick the toughest mob in the floor pool so the room
    // feels distinctly more dangerous than the corridors leading to it.
    let guard_kind = pool
        .iter()
        .copied()
        .max_by_key(|k| {
            let i = enemy_info(*k);
            i.max_hp + i.atk * 3
        })
        .unwrap_or(EnemyKind::Slime);

    let mut placed = 0;
    for &(gx, gy) in &corners {
        if placed >= count { break; }
        if (gx, gy) == (vault.center_x, vault.center_y) { continue; }
        if monsters.iter().any(|m| m.x == gx && m.y == gy) { continue; }
        let info = enemy_info(guard_kind);
        let affix = pick_affix(rng_seed);
        let max_hp = (info.max_hp * enemy_affix_info(affix).hp_pct / 100).max(1);
        monsters.push(Monster {
            kind: guard_kind,
            x: gx,
            y: gy,
            hp: max_hp,
            max_hp,
            // Guards are always alert — this is their treasure.
            awake: true,
            charging: false,
            affix: Some(affix),
        });
        placed += 1;
    }
}

// ── Tests ──────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// When the section-skip RNG picks an isolating pattern (e.g. skipping
    /// sections {1,3} cuts off section 0; {1,5} cuts off section 2),
    /// `ensure_connectivity` used to spin forever because no unreachable room
    /// has a reachable adjacent section. This test sweeps many seeds across
    /// every floor and asserts both termination and full connectivity.
    #[test]
    fn generate_map_always_connects_all_rooms() {
        for floor in 1..=super::super::state::MAX_FLOOR {
            for seed_start in 0u64..500 {
                let mut seed = seed_start.wrapping_mul(0x9E3779B97F4A7C15);
                let map = generate_map(floor, &mut seed);
                let dist = bfs_distances(&map.grid, map.width, map.height, map.player_x, map.player_y);
                for room in &map.rooms {
                    let cx = room.x + room.w / 2;
                    let cy = room.y + room.h / 2;
                    assert!(
                        dist[cy][cx] > 0 || (cx == map.player_x && cy == map.player_y),
                        "floor={floor} seed_start={seed_start}: room at ({cx},{cy}) unreachable from entrance"
                    );
                }
            }
        }
    }

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
        assert!(types.contains(&CellType::Entrance));
        assert!(types.contains(&CellType::Stairs));
    }

    #[test]
    fn map_spawns_monsters() {
        let mut seed = 42u64;
        let map = generate_map(3, &mut seed);
        assert!(!map.monsters.is_empty(), "Floor 3 should spawn monsters");
    }

    #[test]
    fn boss_floor_spawns_demon_lord() {
        let mut seed = 42u64;
        let map = generate_map(10, &mut seed);
        assert!(map.monsters.iter().any(|m| m.kind == super::super::state::EnemyKind::DemonLord));
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
    fn vault_appears_on_mid_floors_within_50_seeds() {
        // F6 has vault_chance == 50, so across 50 seeds at least one
        // map should contain a vault. We assert both Treasure and Idol
        // counts, since either vault kind produces at least one such cell.
        let mut found = false;
        for seed in 0u64..50 {
            let mut s = seed;
            let map = generate_map(6, &mut s);
            let treasures = map.grid.iter().flatten()
                .filter(|c| c.cell_type == CellType::Treasure).count();
            let idols = map.grid.iter().flatten()
                .filter(|c| c.cell_type == CellType::Idol).count();
            // TreasureVault places 3 adjacent Treasure cells. Idol vault
            // adds at least one Idol. Either signature counts.
            if treasures >= 3 || idols >= 1 {
                // Also verify there are elite guards.
                let elite_count = map.monsters.iter()
                    .filter(|m| m.affix.is_some()).count();
                assert!(
                    elite_count >= 1,
                    "Vault floor seed {} should have at least one elite guard",
                    seed
                );
                found = true;
                break;
            }
        }
        assert!(found, "No vault appeared in 50 seeds on F6 (chance is 50%)");
    }

    #[test]
    fn vault_does_not_spawn_on_boss_floor() {
        for seed in 0u64..30 {
            let mut s = seed;
            let map = generate_map(super::super::state::MAX_FLOOR, &mut s);
            // Boss floor should contain only the Demon Lord — no elite affixes.
            assert!(
                map.monsters.iter().all(|m| m.affix.is_none()),
                "Boss floor must not have elite affixes (seed={})", seed
            );
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
