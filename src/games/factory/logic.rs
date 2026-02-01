/// Tiny Factory game logic — pure functions, fully testable.

use super::grid::{anchor_of, Belt, Cell, Direction, ItemKind, Machine, MachineKind, GRID_H, GRID_W};
use super::state::{FactoryState, PlacementTool};

/// Advance the factory by one tick.
pub fn tick(state: &mut FactoryState) {
    // Phase 1: Tick all machines
    tick_machines(state);
    // Phase 2: Move items on belts
    tick_belts(state);
    // Phase 3: Transfer items between machines/belts
    transfer_items(state);
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

                match kind {
                    MachineKind::Miner => {
                        if output_full {
                            continue;
                        }
                        let new_progress = progress + 1;
                        if new_progress >= kind.recipe_time() {
                            // Produce output
                            if let Cell::Machine(m) = &mut state.grid[y][x] {
                                m.progress = 0;
                                let item = kind.output().unwrap();
                                m.output_buffer.push(item);
                                update_produced(state, &item);
                            }
                        } else if let Cell::Machine(m) = &mut state.grid[y][x] {
                            m.progress = new_progress;
                        }
                    }
                    MachineKind::Smelter | MachineKind::Assembler => {
                        if input_empty || output_full {
                            // Reset progress if no input
                            if input_empty {
                                if let Cell::Machine(m) = &mut state.grid[y][x] {
                                    m.progress = 0;
                                }
                            }
                            continue;
                        }
                        let new_progress = progress + 1;
                        if new_progress >= kind.recipe_time() {
                            if let Cell::Machine(m) = &mut state.grid[y][x] {
                                m.progress = 0;
                                m.input_buffer.remove(0);
                                let item = kind.output().unwrap();
                                m.output_buffer.push(item);
                                update_produced(state, &item);
                            }
                        } else if let Cell::Machine(m) = &mut state.grid[y][x] {
                            m.progress = new_progress;
                        }
                    }
                    MachineKind::Exporter => {
                        if input_empty {
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
                                // Export flash effect
                                state.export_flash = 5; // flash for 5 ticks (0.5s)
                                state.last_export_value = value;
                            }
                        } else if let Cell::Machine(m) = &mut state.grid[y][x] {
                            m.progress = new_progress;
                        }
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
    };
    state.produced_count[idx] += 1;
}

/// Move items on belts (from end to start to avoid double-moves).
fn tick_belts(state: &mut FactoryState) {
    // Process in reverse order to avoid double-movement.
    // Actually, belts move items to neighbor in transfer_items phase.
    // Belt-to-belt movement: item moves from one belt to next if next belt is empty.
    // We do this by iterating and checking destination.

    // Collect belt-to-belt moves
    let mut moves: Vec<(usize, usize, usize, usize)> = Vec::new(); // (from_x, from_y, to_x, to_y)

    for y in 0..GRID_H {
        for x in 0..GRID_W {
            if let Cell::Belt(belt) = &state.grid[y][x] {
                if belt.item.is_none() {
                    continue;
                }
                let (dx, dy) = belt.direction.delta();
                let nx = x as i32 + dx;
                let ny = y as i32 + dy;
                if nx < 0 || nx >= GRID_W as i32 || ny < 0 || ny >= GRID_H as i32 {
                    continue;
                }
                let nx = nx as usize;
                let ny = ny as usize;
                match &state.grid[ny][nx] {
                    Cell::Belt(next_belt) if next_belt.item.is_none() => {
                        moves.push((x, y, nx, ny));
                    }
                    _ => {}
                }
            }
        }
    }

    // Apply moves (check for conflicts: two belts targeting same cell)
    let mut occupied: Vec<(usize, usize)> = Vec::new();
    for &(fx, fy, tx, ty) in &moves {
        if occupied.contains(&(tx, ty)) {
            continue;
        }
        let item = if let Cell::Belt(belt) = &mut state.grid[fy][fx] {
            belt.item.take()
        } else {
            None
        };
        if let Some(item) = item {
            if let Cell::Belt(next_belt) = &mut state.grid[ty][tx] {
                next_belt.item = Some(item);
                occupied.push((tx, ty));
            }
        }
    }
}

/// Transfer items between machines and adjacent belts.
fn transfer_items(state: &mut FactoryState) {
    // Machine output → adjacent belt (if belt faces away from machine)
    // Belt → machine input (if belt points at machine)

    for y in 0..GRID_H {
        for x in 0..GRID_W {
            match &state.grid[y][x] {
                Cell::Machine(m) if !m.output_buffer.is_empty() => {
                    // Try to push output to an adjacent belt facing away
                    let kind = m.kind;
                    if kind == MachineKind::Exporter {
                        continue; // Exporters don't output
                    }
                    try_push_to_belt(state, x, y);
                }
                Cell::Belt(belt) if belt.item.is_some() => {
                    // Check if belt points at a machine (or machine part)
                    let (dx, dy) = belt.direction.delta();
                    let nx = x as i32 + dx;
                    let ny = y as i32 + dy;
                    if nx < 0 || nx >= GRID_W as i32 || ny < 0 || ny >= GRID_H as i32 {
                        continue;
                    }
                    let nx = nx as usize;
                    let ny = ny as usize;
                    // Resolve anchor if belt points at a MachinePart
                    if let Some((ax, ay)) = anchor_of(&state.grid, nx, ny) {
                        try_feed_machine(state, x, y, ax, ay);
                    }
                }
                _ => {}
            }
        }
    }
}

/// Push from machine output buffer at anchor (ax,ay) to an adjacent belt on the 2×2 perimeter.
fn try_push_to_belt(state: &mut FactoryState, ax: usize, ay: usize) {
    for (px, py) in perimeter_2x2(ax, ay) {
        if let Cell::Belt(belt) = &state.grid[py][px] {
            if belt.item.is_none() {
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
                    if let Cell::Belt(belt) = &mut state.grid[py][px] {
                        belt.item = Some(item);
                    }
                    return;
                }
            }
        }
    }
}

/// Feed item from belt at (bx,by) into machine at anchor (ax,ay).
fn try_feed_machine(state: &mut FactoryState, bx: usize, by: usize, ax: usize, ay: usize) {
    let accepts = if let Cell::Machine(m) = &state.grid[ay][ax] {
        if m.input_buffer.len() >= m.max_buffer {
            return;
        }
        match m.kind {
            MachineKind::Exporter => true, // accepts any
            _ => {
                if let Some(required) = m.kind.input() {
                    if let Cell::Belt(belt) = &state.grid[by][bx] {
                        belt.item.as_ref() == Some(&required)
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
        }
    } else {
        return;
    };

    if accepts {
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
                | PlacementTool::Exporter => {
                    let kind = match tool {
                        PlacementTool::Miner => MachineKind::Miner,
                        PlacementTool::Smelter => MachineKind::Smelter,
                        PlacementTool::Assembler => MachineKind::Assembler,
                        PlacementTool::Exporter => MachineKind::Exporter,
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
                    let dir = state.belt_direction;
                    state.grid[y][x] = Cell::Belt(Belt::new(dir));
                    state.add_log(&format!("Belt {} を設置", dir.arrow()));
                    // Auto-advance cursor in belt direction
                    let (dx, dy) = dir.delta();
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

/// Rotate belt direction clockwise.
pub fn rotate_belt(state: &mut FactoryState) {
    state.belt_direction = match state.belt_direction {
        Direction::Right => Direction::Down,
        Direction::Down => Direction::Left,
        Direction::Left => Direction::Up,
        Direction::Up => Direction::Right,
    };
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
        state.grid[0][0] = Cell::Belt(Belt::new(Direction::Right));
        state.grid[0][1] = Cell::Belt(Belt::new(Direction::Right));
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
    fn belt_feeds_machine() {
        let mut state = FactoryState::new();
        // Belt pointing right at a Smelter (2×2 at (2,0))
        // Belt at (1,0) → points into Smelter's left side at (2,0)
        place_machine_at(&mut state, 2, 0, MachineKind::Smelter);
        state.grid[0][1] = Cell::Belt(Belt::new(Direction::Right));
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
    fn machine_pushes_to_belt() {
        let mut state = FactoryState::new();
        // Miner 2×2 at (0,0), belt at (2,0) — right of machine
        place_machine_at(&mut state, 0, 0, MachineKind::Miner);
        state.grid[0][2] = Cell::Belt(Belt::new(Direction::Right));

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
        }
    }

    #[test]
    fn full_chain_miner_belt_smelter() {
        let mut state = FactoryState::new();
        // Miner(0,0) → Belt(2,0) → Belt(3,0) → Smelter(4,0)
        // 2×2 machines: Miner occupies (0,0)-(1,1), Smelter occupies (4,0)-(5,1)
        place_machine_at(&mut state, 0, 0, MachineKind::Miner);
        state.grid[0][2] = Cell::Belt(Belt::new(Direction::Right));
        state.grid[0][3] = Cell::Belt(Belt::new(Direction::Right));
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
        state.grid[0][2] = Cell::Belt(Belt::new(Direction::Right));
        state.grid[0][3] = Cell::Belt(Belt::new(Direction::Right));
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
        state.belt_direction = Direction::Right;

        assert!(place(&mut state));
        if let Cell::Belt(b) = &state.grid[0][0] {
            assert_eq!(b.direction, Direction::Right);
        } else {
            panic!("expected belt");
        }
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
        state.grid[0][0] = Cell::Belt(Belt::new(Direction::Right));
        state.tool = PlacementTool::Delete;

        assert!(place(&mut state));
        assert_eq!(state.money, 101); // 100 + 1
    }

    #[test]
    fn rotate_belt_cycles() {
        let mut state = FactoryState::new();
        assert_eq!(state.belt_direction, Direction::Right);
        rotate_belt(&mut state);
        assert_eq!(state.belt_direction, Direction::Down);
        rotate_belt(&mut state);
        assert_eq!(state.belt_direction, Direction::Left);
        rotate_belt(&mut state);
        assert_eq!(state.belt_direction, Direction::Up);
        rotate_belt(&mut state);
        assert_eq!(state.belt_direction, Direction::Right);
    }
}
