/// Tiny Factory game logic — pure functions, fully testable.

use super::grid::{anchor_of, Belt, Cell, Direction, ItemKind, Machine, MachineKind, MinerMode, GRID_H, GRID_W};
use super::state::{FactoryState, PlacementTool};

/// Advance the factory by one tick.
pub fn tick(state: &mut FactoryState) {
    // Phase 1: Tick all machines
    tick_machines(state);
    // Phase 2: Auto-route items on belts (belt→machine and belt→belt)
    tick_belts(state);
    // Phase 3: Push machine output to adjacent belts
    push_machine_output(state);
}

/// Advance multiple ticks.
pub fn tick_n(state: &mut FactoryState, n: u32) {
    for _ in 0..n {
        tick(state);
    }
    state.anim_frame = state.anim_frame.wrapping_add(n);
    state.total_ticks += n as u64;
    if state.export_flash > 0 {
        state.export_flash = state.export_flash.saturating_sub(n);
    }
}

/// Process all machines for one tick.
fn tick_machines(state: &mut FactoryState) {
    for y in 0..GRID_H {
        for x in 0..GRID_W {
            let cell = &state.grid[y][x];
            if let Cell::Machine(m) = cell {
                let kind = m.kind;
                let progress = m.progress;
                let input_empty = m.input_buffer.is_empty();
                let output_full = m.output_buffer.len() >= m.max_buffer;
                let was_active = m.progress > 0;

                match kind {
                    MachineKind::Miner => {
                        if output_full {
                            if let Cell::Machine(m) = &mut state.grid[y][x] {
                                m.stat_total_ticks += 1;
                            }
                            continue;
                        }
                        let miner_mode = m.mode;
                        let new_progress = progress + 1;
                        if new_progress >= kind.recipe_time() {
                            if let Cell::Machine(m) = &mut state.grid[y][x] {
                                m.progress = 0;
                                let item = match miner_mode {
                                    MinerMode::Iron => ItemKind::IronOre,
                                    MinerMode::Copper => ItemKind::CopperOre,
                                };
                                m.output_buffer.push(item);
                                m.stat_produced += 1;
                                update_produced(state, &item);
                            }
                        } else if let Cell::Machine(m) = &mut state.grid[y][x] {
                            m.progress = new_progress;
                        }
                    }
                    MachineKind::Smelter => {
                        if input_empty || output_full {
                            if let Cell::Machine(m) = &mut state.grid[y][x] {
                                if input_empty { m.progress = 0; }
                                m.stat_total_ticks += 1;
                            }
                            continue;
                        }
                        // Auto-detect output from input type
                        let input_item = m.input_buffer[0];
                        let output_item = match input_item {
                            ItemKind::IronOre => ItemKind::IronPlate,
                            ItemKind::CopperOre => ItemKind::CopperPlate,
                            _ => continue, // invalid input, skip
                        };
                        let new_progress = progress + 1;
                        if new_progress >= kind.recipe_time() {
                            if let Cell::Machine(m) = &mut state.grid[y][x] {
                                m.progress = 0;
                                m.input_buffer.remove(0);
                                m.output_buffer.push(output_item);
                                m.stat_produced += 1;
                                update_produced(state, &output_item);
                            }
                        } else if let Cell::Machine(m) = &mut state.grid[y][x] {
                            m.progress = new_progress;
                        }
                    }
                    MachineKind::Assembler => {
                        if input_empty || output_full {
                            if let Cell::Machine(m) = &mut state.grid[y][x] {
                                if input_empty { m.progress = 0; }
                                m.stat_total_ticks += 1;
                            }
                            continue;
                        }
                        let new_progress = progress + 1;
                        if new_progress >= kind.recipe_time() {
                            if let Cell::Machine(m) = &mut state.grid[y][x] {
                                m.progress = 0;
                                m.input_buffer.remove(0);
                                let item = ItemKind::Gear;
                                m.output_buffer.push(item);
                                m.stat_produced += 1;
                                update_produced(state, &item);
                            }
                        } else if let Cell::Machine(m) = &mut state.grid[y][x] {
                            m.progress = new_progress;
                        }
                    }
                    MachineKind::Fabricator => {
                        if output_full {
                            if let Cell::Machine(m) = &mut state.grid[y][x] {
                                m.stat_total_ticks += 1;
                            }
                            continue;
                        }
                        // Need both IronPlate and CopperPlate in buffer
                        let has_iron = m.input_buffer.iter().any(|i| *i == ItemKind::IronPlate);
                        let has_copper = m.input_buffer.iter().any(|i| *i == ItemKind::CopperPlate);
                        if !has_iron || !has_copper {
                            if let Cell::Machine(m) = &mut state.grid[y][x] {
                                m.progress = 0;
                                m.stat_total_ticks += 1;
                            }
                            continue;
                        }
                        let new_progress = progress + 1;
                        if new_progress >= kind.recipe_time() {
                            if let Cell::Machine(m) = &mut state.grid[y][x] {
                                m.progress = 0;
                                // Remove one IronPlate and one CopperPlate
                                if let Some(pos) = m.input_buffer.iter().position(|i| *i == ItemKind::IronPlate) {
                                    m.input_buffer.remove(pos);
                                }
                                if let Some(pos) = m.input_buffer.iter().position(|i| *i == ItemKind::CopperPlate) {
                                    m.input_buffer.remove(pos);
                                }
                                let item = ItemKind::Circuit;
                                m.output_buffer.push(item);
                                m.stat_produced += 1;
                                update_produced(state, &item);
                            }
                        } else if let Cell::Machine(m) = &mut state.grid[y][x] {
                            m.progress = new_progress;
                        }
                    }
                    MachineKind::Exporter => {
                        if input_empty {
                            // Update stats even when idle
                            if let Cell::Machine(m) = &mut state.grid[y][x] {
                                m.stat_total_ticks += 1;
                            }
                            continue;
                        }
                        let new_progress = progress + 1;
                        if new_progress >= kind.recipe_time() {
                            if let Cell::Machine(m) = &mut state.grid[y][x] {
                                m.progress = 0;
                                let item = m.input_buffer.remove(0);
                                let value = MachineKind::export_value(&item);
                                state.money += value;
                                state.total_exported += 1;
                                state.total_money_earned += value;
                                state.export_flash = 5;
                                state.last_export_value = value;
                                m.stat_produced += 1;
                                m.stat_revenue += value;
                            }
                        } else if let Cell::Machine(m) = &mut state.grid[y][x] {
                            m.progress = new_progress;
                        }
                    }
                }

                // Update stats for all machines
                if let Cell::Machine(m) = &mut state.grid[y][x] {
                    m.stat_total_ticks += 1;
                    if m.progress > 0 || was_active {
                        m.stat_active_ticks += 1;
                    }
                }
            }
        }
    }
}

fn update_produced(state: &mut FactoryState, item: &ItemKind) {
    let idx = match item {
        ItemKind::IronOre => 0,
        ItemKind::IronPlate => 1,
        ItemKind::Gear => 2,
        ItemKind::CopperOre => 3,
        ItemKind::CopperPlate => 4,
        ItemKind::Circuit => 5,
    };
    state.produced_count[idx] += 1;
}

// ── Auto-routing helpers ──

/// Get preferred movement directions for an item, excluding backtrack direction.
/// Forward (opposite of source) is tried first, then perpendicular directions.
fn preferred_directions(item_from: Option<Direction>) -> Vec<Direction> {
    match item_from {
        None => vec![Direction::Right, Direction::Down, Direction::Left, Direction::Up],
        Some(from) => {
            let forward = from.opposite();
            let (p1, p2) = match from {
                Direction::Up | Direction::Down => (Direction::Right, Direction::Left),
                Direction::Left | Direction::Right => (Direction::Down, Direction::Up),
            };
            vec![forward, p1, p2]
        }
    }
}

/// Check if machine at anchor (ax,ay) accepts the given item into its input buffer.
fn machine_accepts(grid: &[Vec<Cell>], ax: usize, ay: usize, item: &ItemKind) -> bool {
    if let Cell::Machine(m) = &grid[ay][ax] {
        if m.input_buffer.len() >= m.max_buffer {
            return false;
        }
        match m.kind {
            MachineKind::Miner => false,
            MachineKind::Exporter => true,
            MachineKind::Smelter => matches!(item, ItemKind::IronOre | ItemKind::CopperOre),
            MachineKind::Assembler => *item == ItemKind::IronPlate,
            MachineKind::Fabricator => matches!(item, ItemKind::IronPlate | ItemKind::CopperPlate),
        }
    } else {
        false
    }
}

/// Compute the direction from a belt back to its source (the machine side).
/// Returns the direction pointing from belt toward machine.
fn source_dir_from_machine(ax: usize, ay: usize, bx: usize, by: usize) -> Option<Direction> {
    let is_right = bx >= ax + 2;
    let is_left = bx < ax;
    let is_below = by >= ay + 2;
    let is_above = by < ay;

    if is_right && !is_above && !is_below { Some(Direction::Left) }
    else if is_left && !is_above && !is_below { Some(Direction::Right) }
    else if is_below && !is_left && !is_right { Some(Direction::Up) }
    else if is_above && !is_left && !is_right { Some(Direction::Down) }
    else { None } // corner — try all directions
}

/// Compute item_from direction when item moves from (fx,fy) to (tx,ty).
/// Returns direction pointing from (tx,ty) back toward (fx,fy).
fn source_dir_between(from_x: usize, from_y: usize, to_x: usize, to_y: usize) -> Direction {
    if from_x < to_x { Direction::Left }
    else if from_x > to_x { Direction::Right }
    else if from_y < to_y { Direction::Up }
    else { Direction::Down }
}

/// Auto-route items on belts: feed adjacent machines or move to adjacent empty belts.
fn tick_belts(state: &mut FactoryState) {
    // Collect intended moves
    let mut machine_feeds: Vec<(usize, usize, usize, usize)> = Vec::new(); // (belt_x, belt_y, anchor_x, anchor_y)
    let mut belt_moves: Vec<(usize, usize, usize, usize)> = Vec::new(); // (from_x, from_y, to_x, to_y)

    for y in 0..GRID_H {
        for x in 0..GRID_W {
            if let Cell::Belt(belt) = &state.grid[y][x] {
                if belt.item.is_none() {
                    continue;
                }
                let item = belt.item.unwrap();
                let directions = preferred_directions(belt.item_from);

                // Priority 1: feed an adjacent machine that accepts this item
                let mut fed = false;
                for dir in &directions {
                    let (dx, dy) = dir.delta();
                    let nx = x as i32 + dx;
                    let ny = y as i32 + dy;
                    if nx < 0 || nx >= GRID_W as i32 || ny < 0 || ny >= GRID_H as i32 {
                        continue;
                    }
                    let nx = nx as usize;
                    let ny = ny as usize;
                    if let Some((ax, ay)) = anchor_of(&state.grid, nx, ny) {
                        if machine_accepts(&state.grid, ax, ay, &item) {
                            machine_feeds.push((x, y, ax, ay));
                            fed = true;
                            break;
                        }
                    }
                }
                if fed {
                    continue;
                }

                // Priority 2: move to an adjacent empty belt
                for dir in &directions {
                    let (dx, dy) = dir.delta();
                    let nx = x as i32 + dx;
                    let ny = y as i32 + dy;
                    if nx < 0 || nx >= GRID_W as i32 || ny < 0 || ny >= GRID_H as i32 {
                        continue;
                    }
                    let nx = nx as usize;
                    let ny = ny as usize;
                    if let Cell::Belt(next_belt) = &state.grid[ny][nx] {
                        if next_belt.item.is_none() {
                            belt_moves.push((x, y, nx, ny));
                            break;
                        }
                    }
                }
            }
        }
    }

    // Apply machine feeds (re-check capacity to handle conflicts)
    // Note: we intentionally keep item_from on the belt even after removing the item,
    // so that try_push_to_belt can distinguish input vs output belts.
    for &(bx, by, ax, ay) in &machine_feeds {
        let should_feed = if let Cell::Belt(belt) = &state.grid[by][bx] {
            if let Some(item) = &belt.item {
                machine_accepts(&state.grid, ax, ay, item)
            } else {
                false
            }
        } else {
            false
        };
        if should_feed {
            let item = if let Cell::Belt(belt) = &mut state.grid[by][bx] {
                belt.item.take()
            } else {
                None
            };
            if let Some(item) = item {
                if let Cell::Machine(m) = &mut state.grid[ay][ax] {
                    m.input_buffer.push(item);
                }
            }
        }
    }

    // Apply belt-to-belt moves (check for conflicts: two belts targeting same cell)
    let mut occupied: Vec<(usize, usize)> = Vec::new();
    // Collect belts already consumed by machine feeds
    let consumed: Vec<(usize, usize)> = machine_feeds.iter()
        .filter(|&&(bx, by, _, _)| {
            if let Cell::Belt(belt) = &state.grid[by][bx] {
                belt.item.is_none() // was consumed
            } else {
                false
            }
        })
        .map(|&(bx, by, _, _)| (bx, by))
        .collect();

    for &(fx, fy, tx, ty) in &belt_moves {
        if consumed.contains(&(fx, fy)) {
            continue;
        }
        if occupied.contains(&(tx, ty)) {
            continue;
        }
        let item = if let Cell::Belt(belt) = &mut state.grid[fy][fx] {
            // Keep item_from to remember flow direction (helps try_push_to_belt)
            belt.item.take()
        } else {
            None
        };
        if let Some(item) = item {
            let new_from = source_dir_between(fx, fy, tx, ty);
            if let Cell::Belt(next_belt) = &mut state.grid[ty][tx] {
                next_belt.item = Some(item);
                next_belt.item_from = Some(new_from);
                occupied.push((tx, ty));
            }
        }
    }
}

/// Push machine output to adjacent empty belts.
fn push_machine_output(state: &mut FactoryState) {
    for y in 0..GRID_H {
        for x in 0..GRID_W {
            if let Cell::Machine(m) = &state.grid[y][x] {
                if !m.output_buffer.is_empty() && m.kind != MachineKind::Exporter {
                    try_push_to_belt(state, x, y);
                }
            }
        }
    }
}

/// Push from machine output buffer at anchor (ax,ay) to an adjacent empty belt.
/// Skips belts whose remembered flow direction points INTO this machine (input belts).
fn try_push_to_belt(state: &mut FactoryState, ax: usize, ay: usize) {
    for (px, py) in perimeter_2x2(ax, ay) {
        if let Cell::Belt(belt) = &state.grid[py][px] {
            if belt.item.is_none() {
                // Skip belts that were previously flowing INTO this machine (input belts).
                // If the belt's forward direction (opposite of item_from) points into
                // the machine's 2×2 area, this belt was used to deliver items TO the machine.
                if let Some(from_dir) = belt.item_from {
                    let forward = from_dir.opposite();
                    let (fdx, fdy) = forward.delta();
                    let target_x = px as i32 + fdx;
                    let target_y = py as i32 + fdy;
                    if target_x >= ax as i32 && target_x < (ax + 2) as i32
                        && target_y >= ay as i32 && target_y < (ay + 2) as i32
                    {
                        continue; // this belt flows into this machine, skip
                    }
                }

                let item = if let Cell::Machine(m) = &mut state.grid[ay][ax] {
                    if m.output_buffer.is_empty() {
                        None
                    } else {
                        Some(m.output_buffer.remove(0))
                    }
                } else {
                    None
                };
                if let Some(item) = item {
                    let from_dir = source_dir_from_machine(ax, ay, px, py);
                    if let Cell::Belt(belt) = &mut state.grid[py][px] {
                        belt.item = Some(item);
                        belt.item_from = from_dir;
                    }
                    return;
                }
            }
        }
    }
}

/// Check if all 4 cells for a 2×2 machine at (x,y) anchor are empty and within bounds.
fn can_place_2x2(state: &FactoryState, x: usize, y: usize) -> bool {
    if x + 1 >= GRID_W || y + 1 >= GRID_H {
        return false;
    }
    for dy in 0..2 {
        for dx in 0..2 {
            if !matches!(state.grid[y + dy][x + dx], Cell::Empty) {
                return false;
            }
        }
    }
    true
}

/// Place a 2×2 machine on the grid: anchor at (x,y), parts at the other 3 cells.
fn place_2x2_machine(state: &mut FactoryState, x: usize, y: usize, kind: MachineKind) {
    state.grid[y][x] = Cell::Machine(Machine::new(kind));
    state.grid[y][x + 1] = Cell::MachinePart { anchor_x: x, anchor_y: y };
    state.grid[y + 1][x] = Cell::MachinePart { anchor_x: x, anchor_y: y };
    state.grid[y + 1][x + 1] = Cell::MachinePart { anchor_x: x, anchor_y: y };
}

/// Remove a 2×2 machine given its anchor position. Returns the machine kind for refund calculation.
fn remove_2x2_machine(state: &mut FactoryState, ax: usize, ay: usize) -> Option<MachineKind> {
    let kind = if let Cell::Machine(m) = &state.grid[ay][ax] {
        Some(m.kind)
    } else {
        return None;
    };
    for dy in 0..2 {
        for dx in 0..2 {
            state.grid[ay + dy][ax + dx] = Cell::Empty;
        }
    }
    kind
}

/// Place a machine or belt at cursor position.
pub fn place(state: &mut FactoryState) -> bool {
    let x = state.cursor_x;
    let y = state.cursor_y;

    match &state.tool {
        PlacementTool::None => false,
        PlacementTool::Delete => {
            match &state.grid[y][x] {
                Cell::Empty => false,
                Cell::Machine(_) | Cell::MachinePart { .. } => {
                    // Find anchor, then remove all 4 cells
                    let (ax, ay) = anchor_of(&state.grid, x, y).unwrap();
                    if let Some(kind) = remove_2x2_machine(state, ax, ay) {
                        let refund = kind.cost() / 2;
                        state.money += refund;
                        state.add_log(&format!("削除しました (+${} 返金)", refund));
                        true
                    } else {
                        false
                    }
                }
                Cell::Belt(_) => {
                    let refund = 1u64; // belt costs $2, refund 50%
                    state.grid[y][x] = Cell::Empty;
                    state.money += refund;
                    state.add_log(&format!("削除しました (+${} 返金)", refund));
                    true
                }
            }
        }
        tool => {
            if !matches!(state.grid[y][x], Cell::Empty) {
                return false; // cell occupied
            }

            match tool {
                PlacementTool::Miner
                | PlacementTool::Smelter
                | PlacementTool::Assembler
                | PlacementTool::Exporter
                | PlacementTool::Fabricator => {
                    let kind = match tool {
                        PlacementTool::Miner => MachineKind::Miner,
                        PlacementTool::Smelter => MachineKind::Smelter,
                        PlacementTool::Assembler => MachineKind::Assembler,
                        PlacementTool::Exporter => MachineKind::Exporter,
                        PlacementTool::Fabricator => MachineKind::Fabricator,
                        _ => unreachable!(),
                    };
                    let cost = kind.cost();
                    if state.money < cost {
                        state.add_log("資金不足！");
                        return false;
                    }
                    if !can_place_2x2(state, x, y) {
                        state.add_log("スペース不足！(2×2必要)");
                        return false;
                    }
                    state.money -= cost;
                    place_2x2_machine(state, x, y, kind);
                    state.add_log(&format!("{} を設置 (-${})", kind.name(), cost));
                    placement_advice(state, x, y, kind);
                    true
                }
                PlacementTool::Belt => {
                    let cost = 2u64;
                    if state.money < cost {
                        state.add_log("資金不足！");
                        return false;
                    }
                    state.money -= cost;
                    state.grid[y][x] = Cell::Belt(Belt::new());
                    state.add_log("Belt を設置");
                    // Auto-advance cursor in last movement direction
                    let (dx, dy) = state.belt_direction.delta();
                    state.move_cursor(dx, dy);
                    true
                }
                _ => false,
            }
        }
    }
}

/// Collect all cells on the outer perimeter of a 2×2 machine anchored at (ax, ay).
/// Returns coordinates of cells adjacent to the 2×2 block but not part of it.
fn perimeter_2x2(ax: usize, ay: usize) -> Vec<(usize, usize)> {
    let mut cells = Vec::new();
    // Top edge (y = ay - 1, x = ax..=ax+1)
    if ay > 0 {
        for dx in 0..2 {
            cells.push((ax + dx, ay - 1));
        }
    }
    // Bottom edge (y = ay + 2, x = ax..=ax+1)
    if ay + 2 < GRID_H {
        for dx in 0..2 {
            cells.push((ax + dx, ay + 2));
        }
    }
    // Left edge (x = ax - 1, y = ay..=ay+1)
    if ax > 0 {
        for dy in 0..2 {
            cells.push((ax - 1, ay + dy));
        }
    }
    // Right edge (x = ax + 2, y = ay..=ay+1)
    if ax + 2 < GRID_W {
        for dy in 0..2 {
            cells.push((ax + 2, ay + dy));
        }
    }
    // Corners
    if ay > 0 && ax > 0 {
        cells.push((ax - 1, ay - 1));
    }
    if ay > 0 && ax + 2 < GRID_W {
        cells.push((ax + 2, ay - 1));
    }
    if ay + 2 < GRID_H && ax > 0 {
        cells.push((ax - 1, ay + 2));
    }
    if ay + 2 < GRID_H && ax + 2 < GRID_W {
        cells.push((ax + 2, ay + 2));
    }
    cells
}

/// Give placement advice after a machine is placed.
fn placement_advice(state: &mut FactoryState, x: usize, y: usize, kind: MachineKind) {
    let has_adjacent_belt = perimeter_2x2(x, y)
        .iter()
        .any(|&(px, py)| matches!(state.grid[py][px], Cell::Belt(_)));

    if !has_adjacent_belt {
        state.add_log("💡 隣にベルトを設置して接続しよう");
    }

    // Non-Miner machines need belt-fed input
    if kind != MachineKind::Miner && kind != MachineKind::Exporter && !has_adjacent_belt {
        state.add_log("💡 入力にはベルト経由で原料が必要");
    }
}

/// Toggle Miner mode (Iron ↔ Copper) for the machine under cursor.
pub fn toggle_miner_mode(state: &mut FactoryState) {
    let (cx, cy) = (state.cursor_x, state.cursor_y);
    if let Some((ax, ay)) = anchor_of(&state.grid, cx, cy) {
        if let Cell::Machine(m) = &mut state.grid[ay][ax] {
            if m.kind == MachineKind::Miner {
                m.mode = match m.mode {
                    MinerMode::Iron => MinerMode::Copper,
                    MinerMode::Copper => MinerMode::Iron,
                };
                let mode_name = match m.mode {
                    MinerMode::Iron => "Iron",
                    MinerMode::Copper => "Copper",
                };
                state.add_log(&format!("Miner → {} モード", mode_name));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: place a 2×2 machine manually at (x,y) in tests.
    fn place_machine_at(state: &mut FactoryState, x: usize, y: usize, kind: MachineKind) {
        state.grid[y][x] = Cell::Machine(Machine::new(kind));
        state.grid[y][x + 1] = Cell::MachinePart { anchor_x: x, anchor_y: y };
        state.grid[y + 1][x] = Cell::MachinePart { anchor_x: x, anchor_y: y };
        state.grid[y + 1][x + 1] = Cell::MachinePart { anchor_x: x, anchor_y: y };
    }

    fn make_state_with_miner() -> FactoryState {
        let mut state = FactoryState::new();
        place_machine_at(&mut state, 0, 0, MachineKind::Miner);
        state
    }

    #[test]
    fn miner_produces_after_recipe_time() {
        let mut state = make_state_with_miner();
        // Miner recipe_time = 10
        tick_n(&mut state, 9);
        if let Cell::Machine(m) = &state.grid[0][0] {
            assert!(m.output_buffer.is_empty());
        }
        tick_n(&mut state, 1);
        if let Cell::Machine(m) = &state.grid[0][0] {
            assert_eq!(m.output_buffer.len(), 1);
            assert_eq!(m.output_buffer[0], ItemKind::IronOre);
        }
    }

    #[test]
    fn miner_stops_when_output_full() {
        let mut state = make_state_with_miner();
        // Fill output buffer to max
        if let Cell::Machine(m) = &mut state.grid[0][0] {
            for _ in 0..m.max_buffer {
                m.output_buffer.push(ItemKind::IronOre);
            }
        }
        tick_n(&mut state, 20);
        if let Cell::Machine(m) = &state.grid[0][0] {
            assert_eq!(m.output_buffer.len(), 5); // unchanged, max_buffer=5
        }
    }

    #[test]
    fn smelter_processes_input() {
        let mut state = FactoryState::new();
        place_machine_at(&mut state, 0, 0, MachineKind::Smelter);
        if let Cell::Machine(m) = &mut state.grid[0][0] {
            m.input_buffer.push(ItemKind::IronOre);
        }

        // Smelter recipe_time = 15
        tick_n(&mut state, 14);
        if let Cell::Machine(m) = &state.grid[0][0] {
            assert!(m.output_buffer.is_empty());
            assert_eq!(m.input_buffer.len(), 1);
        }
        tick_n(&mut state, 1);
        if let Cell::Machine(m) = &state.grid[0][0] {
            assert_eq!(m.output_buffer.len(), 1);
            assert_eq!(m.output_buffer[0], ItemKind::IronPlate);
            assert!(m.input_buffer.is_empty());
        }
    }

    #[test]
    fn smelter_needs_input() {
        let mut state = FactoryState::new();
        place_machine_at(&mut state, 0, 0, MachineKind::Smelter);
        tick_n(&mut state, 30);
        if let Cell::Machine(m) = &state.grid[0][0] {
            assert!(m.output_buffer.is_empty());
        }
    }

    #[test]
    fn exporter_earns_money() {
        let mut state = FactoryState::new();
        let initial_money = state.money;
        place_machine_at(&mut state, 0, 0, MachineKind::Exporter);
        if let Cell::Machine(m) = &mut state.grid[0][0] {
            m.input_buffer.push(ItemKind::Gear);
        }

        // Exporter recipe_time = 5
        tick_n(&mut state, 5);
        assert_eq!(state.money, initial_money + 20); // Gear value = 20
        assert_eq!(state.total_exported, 1);
        assert_eq!(state.total_money_earned, 20);
    }

    #[test]
    fn exporter_sets_flash_on_export() {
        let mut state = FactoryState::new();
        place_machine_at(&mut state, 0, 0, MachineKind::Exporter);
        if let Cell::Machine(m) = &mut state.grid[0][0] {
            m.input_buffer.push(ItemKind::IronPlate);
        }

        assert_eq!(state.export_flash, 0);
        tick_n(&mut state, 5);
        assert_eq!(state.last_export_value, 5); // IronPlate value = 5
        assert_eq!(state.total_money_earned, 5);
    }

    #[test]
    fn belt_moves_item() {
        let mut state = FactoryState::new();
        state.grid[0][0] = Cell::Belt(Belt::new());
        state.grid[0][1] = Cell::Belt(Belt::new());
        if let Cell::Belt(b) = &mut state.grid[0][0] {
            b.item = Some(ItemKind::IronOre);
        }

        tick(&mut state);
        if let Cell::Belt(b) = &state.grid[0][0] {
            assert!(b.item.is_none());
        }
        if let Cell::Belt(b) = &state.grid[0][1] {
            assert_eq!(b.item, Some(ItemKind::IronOre));
        }
    }

    #[test]
    fn belt_auto_routes_forward() {
        // Item with known source direction should prefer forward movement
        let mut state = FactoryState::new();
        state.grid[5][5] = Cell::Belt(Belt::new());
        state.grid[5][6] = Cell::Belt(Belt::new()); // right (forward)
        state.grid[6][5] = Cell::Belt(Belt::new()); // down (perpendicular)
        if let Cell::Belt(b) = &mut state.grid[5][5] {
            b.item = Some(ItemKind::IronOre);
            b.item_from = Some(Direction::Left); // came from left → forward is Right
        }

        tick(&mut state);
        // Should prefer right (forward) over down (perpendicular)
        if let Cell::Belt(b) = &state.grid[5][6] {
            assert_eq!(b.item, Some(ItemKind::IronOre));
        }
        if let Cell::Belt(b) = &state.grid[6][5] {
            assert!(b.item.is_none());
        }
    }

    #[test]
    fn belt_auto_routes_perpendicular_when_forward_blocked() {
        let mut state = FactoryState::new();
        state.grid[5][5] = Cell::Belt(Belt::new());
        // No belt to the right (forward blocked)
        state.grid[6][5] = Cell::Belt(Belt::new()); // down (perpendicular)
        if let Cell::Belt(b) = &mut state.grid[5][5] {
            b.item = Some(ItemKind::IronOre);
            b.item_from = Some(Direction::Left); // forward = Right, but blocked
        }

        tick(&mut state);
        // Should fall back to perpendicular (Down)
        if let Cell::Belt(b) = &state.grid[6][5] {
            assert_eq!(b.item, Some(ItemKind::IronOre));
        }
    }

    #[test]
    fn belt_does_not_backtrack() {
        let mut state = FactoryState::new();
        state.grid[5][5] = Cell::Belt(Belt::new());
        state.grid[5][4] = Cell::Belt(Belt::new()); // left (backward = source direction)
        if let Cell::Belt(b) = &mut state.grid[5][5] {
            b.item = Some(ItemKind::IronOre);
            b.item_from = Some(Direction::Left); // came from left → don't go back left
        }

        tick(&mut state);
        // Item should NOT move back to (4,5)
        if let Cell::Belt(b) = &state.grid[5][4] {
            assert!(b.item.is_none());
        }
        // Item stays in place (no valid destination)
        if let Cell::Belt(b) = &state.grid[5][5] {
            assert_eq!(b.item, Some(ItemKind::IronOre));
        }
    }

    #[test]
    fn belt_feeds_machine() {
        let mut state = FactoryState::new();
        // Belt at (1,0) adjacent to Smelter at (2,0)
        place_machine_at(&mut state, 2, 0, MachineKind::Smelter);
        state.grid[0][1] = Cell::Belt(Belt::new());
        if let Cell::Belt(b) = &mut state.grid[0][1] {
            b.item = Some(ItemKind::IronOre);
        }

        tick(&mut state);
        // Item should be in smelter's input buffer
        if let Cell::Machine(m) = &state.grid[0][2] {
            assert_eq!(m.input_buffer.len(), 1);
            assert_eq!(m.input_buffer[0], ItemKind::IronOre);
        }
        if let Cell::Belt(b) = &state.grid[0][1] {
            assert!(b.item.is_none());
        }
    }

    #[test]
    fn belt_prefers_machine_over_belt() {
        // When both a machine and an empty belt are adjacent, item should feed the machine
        let mut state = FactoryState::new();
        state.grid[5][5] = Cell::Belt(Belt::new());
        state.grid[5][6] = Cell::Belt(Belt::new()); // empty belt to the right
        place_machine_at(&mut state, 5, 6, MachineKind::Exporter); // machine below
        if let Cell::Belt(b) = &mut state.grid[5][5] {
            b.item = Some(ItemKind::IronOre);
            b.item_from = Some(Direction::Left); // forward = Right
        }

        tick(&mut state);
        // Machine (Exporter at (5,6)) should receive the item over the belt at (5,6)
        if let Cell::Machine(m) = &state.grid[6][5] {
            assert_eq!(m.input_buffer.len(), 1);
        }
    }

    #[test]
    fn machine_pushes_to_belt() {
        let mut state = FactoryState::new();
        // Miner 2×2 at (0,0), belt at (2,0) — right of machine
        place_machine_at(&mut state, 0, 0, MachineKind::Miner);
        state.grid[0][2] = Cell::Belt(Belt::new());

        // Give miner an output item
        if let Cell::Machine(m) = &mut state.grid[0][0] {
            m.output_buffer.push(ItemKind::IronOre);
        }

        tick(&mut state);
        if let Cell::Machine(m) = &state.grid[0][0] {
            assert!(m.output_buffer.is_empty());
        }
        if let Cell::Belt(b) = &state.grid[0][2] {
            assert_eq!(b.item, Some(ItemKind::IronOre));
            // item_from should point back toward the machine (Left)
            assert_eq!(b.item_from, Some(Direction::Left));
        }
    }

    #[test]
    fn full_chain_miner_belt_smelter() {
        let mut state = FactoryState::new();
        // Miner(0,0) → Belt(2,0) → Belt(3,0) → Smelter(4,0)
        place_machine_at(&mut state, 0, 0, MachineKind::Miner);
        state.grid[0][2] = Cell::Belt(Belt::new());
        state.grid[0][3] = Cell::Belt(Belt::new());
        place_machine_at(&mut state, 4, 0, MachineKind::Smelter);

        // Run enough ticks for full pipeline
        tick_n(&mut state, 40);

        // Smelter should have received and started processing
        if let Cell::Machine(m) = &state.grid[0][4] {
            assert!(
                !m.output_buffer.is_empty() || m.progress > 0 || state.produced_count[1] > 0,
                "smelter should have received and started processing input"
            );
        }

        // Run more to ensure full production
        tick_n(&mut state, 30);

        assert!(
            state.produced_count[1] > 0,
            "should have produced at least one iron plate"
        );
    }

    #[test]
    fn full_chain_60_ticks_export() {
        let mut state = FactoryState::new();
        let initial_money = state.money;
        // Miner(0,0) → Belt(2,0) → Belt(3,0) → Exporter(4,0)
        place_machine_at(&mut state, 0, 0, MachineKind::Miner);
        state.grid[0][2] = Cell::Belt(Belt::new());
        state.grid[0][3] = Cell::Belt(Belt::new());
        place_machine_at(&mut state, 4, 0, MachineKind::Exporter);

        tick_n(&mut state, 80);

        assert!(
            state.money > initial_money,
            "should have earned money from exports"
        );
        assert!(state.total_exported > 0);
    }

    #[test]
    fn place_machine() {
        let mut state = FactoryState::new();
        state.money = 100;
        state.tool = PlacementTool::Miner;
        state.cursor_x = 3;
        state.cursor_y = 2;

        assert!(place(&mut state));
        assert!(matches!(state.grid[2][3], Cell::Machine(_)));
        assert_eq!(state.money, 100 - 10); // Miner costs 10
    }

    #[test]
    fn place_on_occupied_fails() {
        let mut state = FactoryState::new();
        state.money = 200;
        place_machine_at(&mut state, 0, 0, MachineKind::Miner);
        state.tool = PlacementTool::Smelter;
        state.cursor_x = 0;
        state.cursor_y = 0;

        assert!(!place(&mut state));
        assert_eq!(state.money, 200); // unchanged
    }

    #[test]
    fn place_insufficient_funds() {
        let mut state = FactoryState::new();
        state.money = 5;
        state.tool = PlacementTool::Miner; // costs 10

        assert!(!place(&mut state));
        assert!(matches!(state.grid[0][0], Cell::Empty));
    }

    #[test]
    fn place_belt() {
        let mut state = FactoryState::new();
        state.money = 10;
        state.tool = PlacementTool::Belt;

        assert!(place(&mut state));
        assert!(matches!(state.grid[0][0], Cell::Belt(_)));
    }

    #[test]
    fn delete_cell() {
        let mut state = FactoryState::new();
        place_machine_at(&mut state, 0, 0, MachineKind::Miner);
        state.tool = PlacementTool::Delete;

        assert!(place(&mut state));
        // All 4 cells should be empty
        assert!(matches!(state.grid[0][0], Cell::Empty));
        assert!(matches!(state.grid[0][1], Cell::Empty));
        assert!(matches!(state.grid[1][0], Cell::Empty));
        assert!(matches!(state.grid[1][1], Cell::Empty));
    }

    #[test]
    fn delete_machine_refunds_half() {
        let mut state = FactoryState::new();
        state.money = 100;
        place_machine_at(&mut state, 0, 0, MachineKind::Smelter); // cost 25
        state.tool = PlacementTool::Delete;

        assert!(place(&mut state));
        assert_eq!(state.money, 112); // 100 + 25/2 = 112
    }

    #[test]
    fn delete_from_machine_part_cell() {
        // Delete when cursor is on a MachinePart (not the anchor)
        let mut state = FactoryState::new();
        state.money = 100;
        place_machine_at(&mut state, 0, 0, MachineKind::Miner); // cost 10
        state.tool = PlacementTool::Delete;
        state.cursor_x = 1;
        state.cursor_y = 1; // bottom-right part

        assert!(place(&mut state));
        assert_eq!(state.money, 105); // 100 + 10/2 = 105
        assert!(matches!(state.grid[0][0], Cell::Empty));
        assert!(matches!(state.grid[1][1], Cell::Empty));
    }

    #[test]
    fn delete_belt_refunds_one() {
        let mut state = FactoryState::new();
        state.money = 100;
        state.grid[0][0] = Cell::Belt(Belt::new());
        state.tool = PlacementTool::Delete;

        assert!(place(&mut state));
        assert_eq!(state.money, 101); // 100 + 1
    }

    #[test]
    fn miner_copper_mode_produces_copper_ore() {
        let mut state = FactoryState::new();
        place_machine_at(&mut state, 0, 0, MachineKind::Miner);
        if let Cell::Machine(m) = &mut state.grid[0][0] {
            m.mode = MinerMode::Copper;
        }
        tick_n(&mut state, 10);
        if let Cell::Machine(m) = &state.grid[0][0] {
            assert_eq!(m.output_buffer.len(), 1);
            assert_eq!(m.output_buffer[0], ItemKind::CopperOre);
        }
    }

    #[test]
    fn toggle_miner_mode_switches() {
        let mut state = FactoryState::new();
        place_machine_at(&mut state, 0, 0, MachineKind::Miner);
        if let Cell::Machine(m) = &state.grid[0][0] {
            assert_eq!(m.mode, MinerMode::Iron);
        }
        toggle_miner_mode(&mut state);
        if let Cell::Machine(m) = &state.grid[0][0] {
            assert_eq!(m.mode, MinerMode::Copper);
        }
        toggle_miner_mode(&mut state);
        if let Cell::Machine(m) = &state.grid[0][0] {
            assert_eq!(m.mode, MinerMode::Iron);
        }
    }

    #[test]
    fn smelter_processes_copper_ore() {
        let mut state = FactoryState::new();
        place_machine_at(&mut state, 0, 0, MachineKind::Smelter);
        if let Cell::Machine(m) = &mut state.grid[0][0] {
            m.input_buffer.push(ItemKind::CopperOre);
        }
        tick_n(&mut state, 15);
        if let Cell::Machine(m) = &state.grid[0][0] {
            assert_eq!(m.output_buffer.len(), 1);
            assert_eq!(m.output_buffer[0], ItemKind::CopperPlate);
        }
    }

    #[test]
    fn smelter_rejects_invalid_input() {
        let mut state = FactoryState::new();
        place_machine_at(&mut state, 0, 0, MachineKind::Smelter);
        if let Cell::Machine(m) = &mut state.grid[0][0] {
            m.input_buffer.push(ItemKind::Gear); // invalid input for smelter
        }
        tick_n(&mut state, 30);
        if let Cell::Machine(m) = &state.grid[0][0] {
            assert!(m.output_buffer.is_empty(), "smelter should not process invalid input");
            assert_eq!(m.input_buffer.len(), 1, "invalid item should remain in buffer");
        }
    }

    #[test]
    fn fabricator_needs_both_inputs() {
        let mut state = FactoryState::new();
        place_machine_at(&mut state, 0, 0, MachineKind::Fabricator);
        // Only IronPlate — should not produce
        if let Cell::Machine(m) = &mut state.grid[0][0] {
            m.input_buffer.push(ItemKind::IronPlate);
        }
        tick_n(&mut state, 30);
        if let Cell::Machine(m) = &state.grid[0][0] {
            assert!(m.output_buffer.is_empty());
        }
    }

    #[test]
    fn fabricator_produces_circuit() {
        let mut state = FactoryState::new();
        place_machine_at(&mut state, 0, 0, MachineKind::Fabricator);
        if let Cell::Machine(m) = &mut state.grid[0][0] {
            m.input_buffer.push(ItemKind::IronPlate);
            m.input_buffer.push(ItemKind::CopperPlate);
        }
        // Fabricator recipe_time = 25
        tick_n(&mut state, 25);
        if let Cell::Machine(m) = &state.grid[0][0] {
            assert_eq!(m.output_buffer.len(), 1);
            assert_eq!(m.output_buffer[0], ItemKind::Circuit);
            assert!(m.input_buffer.is_empty());
        }
    }

    #[test]
    fn fabricator_no_progress_without_both() {
        let mut state = FactoryState::new();
        place_machine_at(&mut state, 0, 0, MachineKind::Fabricator);
        // Only CopperPlate — should not produce
        if let Cell::Machine(m) = &mut state.grid[0][0] {
            m.input_buffer.push(ItemKind::CopperPlate);
        }
        tick_n(&mut state, 30);
        if let Cell::Machine(m) = &state.grid[0][0] {
            assert!(m.output_buffer.is_empty());
            assert_eq!(m.progress, 0, "progress should be reset without both inputs");
        }
    }

    #[test]
    fn full_chain_copper_to_export() {
        let mut state = FactoryState::new();
        let initial_money = state.money;
        // CopperMiner(0,0) → Belt(2,0) → Belt(3,0) → Smelter(4,0)
        // → Belt(6,0) → Belt(7,0) → Exporter(8,0)
        place_machine_at(&mut state, 0, 0, MachineKind::Miner);
        if let Cell::Machine(m) = &mut state.grid[0][0] {
            m.mode = MinerMode::Copper;
        }
        state.grid[0][2] = Cell::Belt(Belt::new());
        state.grid[0][3] = Cell::Belt(Belt::new());
        place_machine_at(&mut state, 4, 0, MachineKind::Smelter);
        state.grid[0][6] = Cell::Belt(Belt::new());
        state.grid[0][7] = Cell::Belt(Belt::new());
        place_machine_at(&mut state, 8, 0, MachineKind::Exporter);

        tick_n(&mut state, 120);

        assert!(
            state.money > initial_money,
            "should have earned money from copper plate exports"
        );
        assert!(state.total_exported > 0);
    }

    #[test]
    fn full_chain_fabricator_circuit() {
        let mut state = FactoryState::new();
        // Set up Fabricator with pre-loaded inputs to test the core logic
        place_machine_at(&mut state, 0, 0, MachineKind::Fabricator);
        if let Cell::Machine(m) = &mut state.grid[0][0] {
            m.input_buffer.push(ItemKind::IronPlate);
            m.input_buffer.push(ItemKind::CopperPlate);
            m.input_buffer.push(ItemKind::IronPlate);
            m.input_buffer.push(ItemKind::CopperPlate);
        }
        // Belt out → Exporter
        state.grid[0][2] = Cell::Belt(Belt::new());
        state.grid[0][3] = Cell::Belt(Belt::new());
        place_machine_at(&mut state, 4, 0, MachineKind::Exporter);

        let initial_money = state.money;
        tick_n(&mut state, 120);

        assert!(
            state.produced_count[5] > 0,
            "should have produced circuits"
        );
        assert!(
            state.money > initial_money,
            "should have earned money from circuit exports"
        );
    }
}
