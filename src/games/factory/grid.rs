/// Grid types for the Tiny Factory game.

pub const GRID_W: usize = 40;
pub const GRID_H: usize = 30;

/// Viewport size (how many cells are visible at once).
pub const VIEW_W: usize = 20;
pub const VIEW_H: usize = 14;

/// Cardinal direction for belts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

impl Direction {
    /// Delta (dx, dy) for this direction.
    pub fn delta(&self) -> (i32, i32) {
        match self {
            Direction::Up => (0, -1),
            Direction::Down => (0, 1),
            Direction::Left => (-1, 0),
            Direction::Right => (1, 0),
        }
    }

    /// ASCII char for display.
    pub fn arrow(&self) -> char {
        match self {
            Direction::Up => '^',
            Direction::Down => 'v',
            Direction::Left => '<',
            Direction::Right => '>',
        }
    }

    /// Opposite direction.
    pub fn opposite(&self) -> Direction {
        match self {
            Direction::Up => Direction::Down,
            Direction::Down => Direction::Up,
            Direction::Left => Direction::Right,
            Direction::Right => Direction::Left,
        }
    }
}

/// Items that flow through the factory.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ItemKind {
    IronOre,
    IronPlate,
    Gear,
    CopperOre,
    CopperPlate,
    Circuit,
}

impl ItemKind {
    pub fn symbol(&self) -> char {
        match self {
            ItemKind::IronOre => 'o',
            ItemKind::IronPlate => '=',
            ItemKind::Gear => '*',
            ItemKind::CopperOre => 'c',
            ItemKind::CopperPlate => '~',
            ItemKind::Circuit => '#',
        }
    }

    /// Display color for this item kind.
    pub fn color(&self) -> ratzilla::ratatui::style::Color {
        use ratzilla::ratatui::style::Color;
        match self {
            ItemKind::IronOre => Color::Cyan,
            ItemKind::IronPlate => Color::LightBlue,
            ItemKind::Gear => Color::Yellow,
            ItemKind::CopperOre => Color::LightRed,
            ItemKind::CopperPlate => Color::Red,
            ItemKind::Circuit => Color::LightGreen,
        }
    }
}

/// Machine types.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MachineKind {
    /// Produces IronOre or CopperOre (depending on mode).
    Miner,
    /// Converts ore → plate (auto-detects input type).
    Smelter,
    /// Converts IronPlate → Gear.
    Assembler,
    /// Exports items for money.
    Exporter,
    /// Converts IronPlate + CopperPlate → Circuit (2-input).
    Fabricator,
}

impl MachineKind {
    pub fn name(&self) -> &str {
        match self {
            MachineKind::Miner => "Miner",
            MachineKind::Smelter => "Smelter",
            MachineKind::Assembler => "Assembler",
            MachineKind::Exporter => "Exporter",
            MachineKind::Fabricator => "Fabricator",
        }
    }

    pub fn symbol(&self) -> char {
        match self {
            MachineKind::Miner => 'M',
            MachineKind::Smelter => 'S',
            MachineKind::Assembler => 'A',
            MachineKind::Exporter => 'E',
            MachineKind::Fabricator => 'F',
        }
    }

    /// Cost to place this machine.
    pub fn cost(&self) -> u64 {
        match self {
            MachineKind::Miner => 10,
            MachineKind::Smelter => 25,
            MachineKind::Assembler => 50,
            MachineKind::Exporter => 15,
            MachineKind::Fabricator => 75,
        }
    }

    /// Ticks to produce one output.
    pub fn recipe_time(&self) -> u32 {
        match self {
            MachineKind::Miner => 10,     // 1 per second
            MachineKind::Smelter => 15,   // 0.67 per second
            MachineKind::Assembler => 20,  // 0.5 per second
            MachineKind::Exporter => 5,   // 2 per second
            MachineKind::Fabricator => 25, // 0.4 per second
        }
    }

    /// Primary input required (None for Miner, Exporter, Fabricator).
    /// Fabricator uses special 2-input logic handled in tick.
    pub fn input(&self) -> Option<ItemKind> {
        match self {
            MachineKind::Miner => None,
            MachineKind::Smelter => None, // polymorphic: accepts IronOre or CopperOre
            MachineKind::Assembler => Some(ItemKind::IronPlate),
            MachineKind::Exporter => None, // accepts any
            MachineKind::Fabricator => None, // 2-input: IronPlate + CopperPlate
        }
    }

    /// Output produced (None for Exporter).
    pub fn output(&self) -> Option<ItemKind> {
        match self {
            MachineKind::Miner => Some(ItemKind::IronOre),
            MachineKind::Smelter => Some(ItemKind::IronPlate), // default; actual output depends on input
            MachineKind::Assembler => Some(ItemKind::Gear),
            MachineKind::Exporter => None,
            MachineKind::Fabricator => Some(ItemKind::Circuit),
        }
    }

    /// Revenue per item exported.
    pub fn export_value(item: &ItemKind) -> u64 {
        match item {
            ItemKind::IronOre => 1,
            ItemKind::IronPlate => 5,
            ItemKind::Gear => 20,
            ItemKind::CopperOre => 2,
            ItemKind::CopperPlate => 8,
            ItemKind::Circuit => 50,
        }
    }
}

/// Miner production mode.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MinerMode {
    Iron,
    Copper,
}

/// A machine placed on the grid.
#[derive(Clone, Debug)]
pub struct Machine {
    pub kind: MachineKind,
    /// Input buffer (items waiting to be processed).
    pub input_buffer: Vec<ItemKind>,
    /// Output buffer (items produced, waiting to be taken by belt).
    pub output_buffer: Vec<ItemKind>,
    /// Progress toward producing next output (ticks).
    pub progress: u32,
    /// Maximum buffer size.
    pub max_buffer: usize,
    /// Miner mode (only relevant for Miner kind).
    pub mode: MinerMode,
    // ── Statistics ──
    /// Total items produced by this machine.
    pub stat_produced: u64,
    /// Total money earned (Exporter only).
    pub stat_revenue: u64,
    /// Ticks this machine was actively working (progress > 0 or producing).
    pub stat_active_ticks: u64,
    /// Total ticks since placement.
    pub stat_total_ticks: u64,
}

impl Machine {
    pub fn new(kind: MachineKind) -> Self {
        Self {
            kind,
            input_buffer: Vec::new(),
            output_buffer: Vec::new(),
            progress: 0,
            max_buffer: 5,
            mode: MinerMode::Iron,
            stat_produced: 0,
            stat_revenue: 0,
            stat_active_ticks: 0,
            stat_total_ticks: 0,
        }
    }

    /// Utilization rate (0.0 - 1.0).
    pub fn utilization(&self) -> f64 {
        if self.stat_total_ticks == 0 {
            0.0
        } else {
            self.stat_active_ticks as f64 / self.stat_total_ticks as f64
        }
    }
}

/// A belt segment (undirected; items auto-route away from their source).
#[derive(Clone, Debug)]
pub struct Belt {
    /// Item currently on this belt tile (at most one).
    pub item: Option<ItemKind>,
    /// Direction the current item entered from (to avoid backtracking).
    pub item_from: Option<Direction>,
}

impl Belt {
    pub fn new() -> Self {
        Self {
            item: None,
            item_from: None,
        }
    }
}

/// What's in a grid cell.
#[derive(Clone, Debug)]
pub enum Cell {
    Empty,
    Machine(Machine),
    /// Part of a 2×2 machine; the actual Machine data lives at the anchor cell.
    MachinePart { anchor_x: usize, anchor_y: usize },
    Belt(Belt),
}


/// Given any cell coordinate, return the anchor (top-left) position of the machine occupying it.
/// Returns None if the cell is not a machine or machine part.
pub fn anchor_of(grid: &[Vec<Cell>], x: usize, y: usize) -> Option<(usize, usize)> {
    match &grid[y][x] {
        Cell::Machine(_) => Some((x, y)),
        Cell::MachinePart { anchor_x, anchor_y } => Some((*anchor_x, *anchor_y)),
        _ => None,
    }
}

/// Get a reference to the Machine at the given anchor position.
pub fn machine_at(grid: &[Vec<Cell>], ax: usize, ay: usize) -> Option<&Machine> {
    match &grid[ay][ax] {
        Cell::Machine(m) => Some(m),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direction_deltas() {
        assert_eq!(Direction::Up.delta(), (0, -1));
        assert_eq!(Direction::Down.delta(), (0, 1));
        assert_eq!(Direction::Left.delta(), (-1, 0));
        assert_eq!(Direction::Right.delta(), (1, 0));
    }

    #[test]
    fn machine_new() {
        let m = Machine::new(MachineKind::Miner);
        assert_eq!(m.kind, MachineKind::Miner);
        assert!(m.input_buffer.is_empty());
        assert!(m.output_buffer.is_empty());
        assert_eq!(m.progress, 0);
    }

    #[test]
    fn machine_recipes() {
        assert_eq!(MachineKind::Miner.input(), None);
        assert_eq!(MachineKind::Miner.output(), Some(ItemKind::IronOre));
        assert_eq!(MachineKind::Smelter.input(), None); // polymorphic: accepts IronOre or CopperOre
        assert_eq!(MachineKind::Smelter.output(), Some(ItemKind::IronPlate));
    }

    #[test]
    fn export_values() {
        assert_eq!(MachineKind::export_value(&ItemKind::IronOre), 1);
        assert_eq!(MachineKind::export_value(&ItemKind::IronPlate), 5);
        assert_eq!(MachineKind::export_value(&ItemKind::Gear), 20);
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    fn arb_direction() -> impl Strategy<Value = Direction> {
        prop_oneof![
            Just(Direction::Up),
            Just(Direction::Down),
            Just(Direction::Left),
            Just(Direction::Right),
        ]
    }

    fn arb_item_kind() -> impl Strategy<Value = ItemKind> {
        prop_oneof![
            Just(ItemKind::IronOre),
            Just(ItemKind::IronPlate),
            Just(ItemKind::Gear),
            Just(ItemKind::CopperOre),
            Just(ItemKind::CopperPlate),
            Just(ItemKind::Circuit),
        ]
    }

    fn arb_machine_kind() -> impl Strategy<Value = MachineKind> {
        prop_oneof![
            Just(MachineKind::Miner),
            Just(MachineKind::Smelter),
            Just(MachineKind::Assembler),
            Just(MachineKind::Exporter),
            Just(MachineKind::Fabricator),
        ]
    }

    proptest! {
        #[test]
        fn prop_opposite_is_involution(dir in arb_direction()) {
            prop_assert_eq!(dir.opposite().opposite(), dir);
        }
    }

    proptest! {
        #[test]
        fn prop_opposite_has_negated_delta(dir in arb_direction()) {
            let (dx, dy) = dir.delta();
            let (ox, oy) = dir.opposite().delta();
            prop_assert_eq!((dx + ox, dy + oy), (0, 0));
        }
    }

    proptest! {
        #[test]
        fn prop_direction_delta_unit_length(dir in arb_direction()) {
            let (dx, dy) = dir.delta();
            prop_assert_eq!(dx.abs() + dy.abs(), 1,
                "delta should be a unit vector, got ({}, {})", dx, dy);
        }
    }

    proptest! {
        #[test]
        fn prop_direction_delta_is_axis_aligned(dir in arb_direction()) {
            let (dx, dy) = dir.delta();
            prop_assert!((dx == 0) != (dy == 0),
                "delta not axis-aligned: ({}, {})", dx, dy);
        }
    }

    proptest! {
        #[test]
        fn prop_export_value_positive(item in arb_item_kind()) {
            prop_assert!(MachineKind::export_value(&item) > 0);
        }
    }

    #[test]
    fn prop_processed_items_worth_more_than_raw() {
        let iron_ore = MachineKind::export_value(&ItemKind::IronOre);
        let iron_plate = MachineKind::export_value(&ItemKind::IronPlate);
        let gear = MachineKind::export_value(&ItemKind::Gear);
        let copper_ore = MachineKind::export_value(&ItemKind::CopperOre);
        let copper_plate = MachineKind::export_value(&ItemKind::CopperPlate);
        let circuit = MachineKind::export_value(&ItemKind::Circuit);

        assert!(iron_plate > iron_ore);
        assert!(gear > iron_plate);
        assert!(copper_plate > copper_ore);
        assert!(circuit > copper_plate);
    }

    proptest! {
        #[test]
        fn prop_machine_recipe_time_positive(kind in arb_machine_kind()) {
            prop_assert!(kind.recipe_time() > 0);
        }
    }

    proptest! {
        #[test]
        fn prop_machine_cost_positive(kind in arb_machine_kind()) {
            prop_assert!(kind.cost() > 0);
        }
    }

    proptest! {
        #[test]
        fn prop_machine_new_starts_empty(kind in arb_machine_kind()) {
            let m = Machine::new(kind);
            prop_assert!(m.input_buffer.is_empty());
            prop_assert!(m.output_buffer.is_empty());
            prop_assert_eq!(m.progress, 0);
        }
    }
}
