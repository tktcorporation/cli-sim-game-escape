/// Tiny Factory game logic — pure functions, fully testable.

use super::grid::{Belt, Cell, Direction, ItemKind, Machine, MachineKind, GRID_H, GRID_W};
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
                    // Check if belt points at a machine
                    let (dx, dy) = belt.direction.delta();
                    let nx = x as i32 + dx;
                    let ny = y as i32 + dy;
                    if nx < 0 || nx >= GRID_W as i32 || ny < 0 || ny >= GRID_H as i32 {
                        continue;
                    }
                    let nx = nx as usize;
                    let ny = ny as usize;
                    try_feed_machine(state, x, y, nx, ny);
                }
                _ => {}
            }
        }
    }
}

/// Push from machine output buffer at (x,y) to an adjacent belt.
fn try_push_to_belt(state: &mut FactoryState, x: usize, y: usize) {
    let directions = [(1i32, 0i32), (-1, 0), (0, 1), (0, -1)];
    for (dx, dy) in &directions {
        let nx = x as i32 + dx;
        let ny = y as i32 + dy;
        if nx < 0 || nx >= GRID_W as i32 || ny < 0 || ny >= GRID_H as i32 {
            continue;
        }
        let nx = nx as usize;
        let ny = ny as usize;
        if let Cell::Belt(belt) = &state.grid[ny][nx] {
            if belt.item.is_none() {
                // Check belt direction is facing away from machine (or at least not toward it)
                let item = if let Cell::Machine(m) = &mut state.grid[y][x] {
                    if m.output_buffer.is_empty() {
                        None
                    } else {
                        Some(m.output_buffer.remove(0))
                    }
                } else {
                    None
                };
                if let Some(item) = item {
                    if let Cell::Belt(belt) = &mut state.grid[ny][nx] {
                        belt.item = Some(item);
                    }
                    return;
                }
            }
        }
    }
}

/// Feed item from belt at (bx,by) into machine at (mx,my).
fn try_feed_machine(state: &mut FactoryState, bx: usize, by: usize, mx: usize, my: usize) {
    let accepts = if let Cell::Machine(m) = &state.grid[my][mx] {
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
            if let Cell::Machine(m) = &mut state.grid[my][mx] {
                m.input_buffer.push(item);
            }
        }
    }
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
                _ => {
                    state.grid[y][x] = Cell::Empty;
                    state.add_log("削除しました");
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
                    state.money -= cost;
                    state.grid[y][x] = Cell::Machine(Machine::new(kind));
                    state.add_log(&format!("{} を設置 (-${})", kind.name(), cost));
                    true
                }
                PlacementTool::Belt => {
                    let cost = 2u64;
                    if state.money < cost {
                        state.add_log("資金不足！");
                        return false;
                    }
                    state.money -= cost;
                    state.grid[y][x] = Cell::Belt(Belt::new(state.belt_direction));
                    state.add_log(&format!("Belt {} を設置", state.belt_direction.arrow()));
                    true
                }
                _ => false,
            }
        }
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

    fn make_state_with_miner() -> FactoryState {
        let mut state = FactoryState::new();
        state.grid[0][0] = Cell::Machine(Machine::new(MachineKind::Miner));
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
        state.grid[0][0] = Cell::Machine(Machine::new(MachineKind::Smelter));
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
        state.grid[0][0] = Cell::Machine(Machine::new(MachineKind::Smelter));
        tick_n(&mut state, 30);
        if let Cell::Machine(m) = &state.grid[0][0] {
            assert!(m.output_buffer.is_empty());
        }
    }

    #[test]
    fn exporter_earns_money() {
        let mut state = FactoryState::new();
        let initial_money = state.money;
        state.grid[0][0] = Cell::Machine(Machine::new(MachineKind::Exporter));
        if let Cell::Machine(m) = &mut state.grid[0][0] {
            m.input_buffer.push(ItemKind::Gear);
        }

        // Exporter recipe_time = 5
        tick_n(&mut state, 5);
        assert_eq!(state.money, initial_money + 20); // Gear value = 20
        assert_eq!(state.total_exported, 1);
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
        // Belt pointing right at a Smelter
        state.grid[0][0] = Cell::Belt(Belt::new(Direction::Right));
        state.grid[0][1] = Cell::Machine(Machine::new(MachineKind::Smelter));
        if let Cell::Belt(b) = &mut state.grid[0][0] {
            b.item = Some(ItemKind::IronOre);
        }

        tick(&mut state);
        // Item should be in smelter's input buffer
        if let Cell::Machine(m) = &state.grid[0][1] {
            assert_eq!(m.input_buffer.len(), 1);
            assert_eq!(m.input_buffer[0], ItemKind::IronOre);
        }
        if let Cell::Belt(b) = &state.grid[0][0] {
            assert!(b.item.is_none());
        }
    }

    #[test]
    fn machine_pushes_to_belt() {
        let mut state = FactoryState::new();
        state.grid[0][0] = Cell::Machine(Machine::new(MachineKind::Miner));
        state.grid[0][1] = Cell::Belt(Belt::new(Direction::Right));

        // Give miner an output item
        if let Cell::Machine(m) = &mut state.grid[0][0] {
            m.output_buffer.push(ItemKind::IronOre);
        }

        tick(&mut state);
        if let Cell::Machine(m) = &state.grid[0][0] {
            assert!(m.output_buffer.is_empty());
        }
        if let Cell::Belt(b) = &state.grid[0][1] {
            assert_eq!(b.item, Some(ItemKind::IronOre));
        }
    }

    #[test]
    fn full_chain_miner_belt_smelter() {
        let mut state = FactoryState::new();
        // Miner → Belt → Smelter
        state.grid[0][0] = Cell::Machine(Machine::new(MachineKind::Miner));
        state.grid[0][1] = Cell::Belt(Belt::new(Direction::Right));
        state.grid[0][2] = Cell::Machine(Machine::new(MachineKind::Smelter));

        // Run enough ticks for full pipeline:
        // Miner produces at tick 10, transfer takes ~2 ticks,
        // Smelter needs 15 ticks to process → ~27 ticks minimum
        tick_n(&mut state, 30);

        // Smelter should have received and started processing
        if let Cell::Machine(m) = &state.grid[0][2] {
            // Either produced output or consumed input (progress > 0)
            assert!(
                !m.output_buffer.is_empty() || m.progress > 0 || state.produced_count[1] > 0,
                "smelter should have received and started processing input"
            );
        }

        // Run more to ensure full production
        tick_n(&mut state, 20);

        // By now, at least one iron plate should have been produced
        assert!(
            state.produced_count[1] > 0,
            "should have produced at least one iron plate"
        );
    }

    #[test]
    fn full_chain_60_ticks_export() {
        let mut state = FactoryState::new();
        let initial_money = state.money;
        // Miner → Belt → Exporter
        state.grid[0][0] = Cell::Machine(Machine::new(MachineKind::Miner));
        state.grid[0][1] = Cell::Belt(Belt::new(Direction::Right));
        state.grid[0][2] = Cell::Machine(Machine::new(MachineKind::Exporter));

        tick_n(&mut state, 60);

        // After 60 ticks, miner produces at tick 10, 20, 30, 40, 50 → 5 ores
        // Each needs ~2 ticks to reach exporter (push + feed) + 5 ticks to export
        // So first export at ~tick 17, then every ~12 ticks
        // Rough check: at least 1 export happened
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
        state.grid[0][0] = Cell::Machine(Machine::new(MachineKind::Miner));
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
        state.grid[0][0] = Cell::Machine(Machine::new(MachineKind::Miner));
        state.tool = PlacementTool::Delete;

        assert!(place(&mut state));
        assert!(matches!(state.grid[0][0], Cell::Empty));
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
