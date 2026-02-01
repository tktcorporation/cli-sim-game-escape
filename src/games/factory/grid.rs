/// Grid types for the Tiny Factory game.

pub const GRID_W: usize = 10;
pub const GRID_H: usize = 8;

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
}

/// Items that flow through the factory.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ItemKind {
    IronOre,
    IronPlate,
    Gear,
}

impl ItemKind {
    pub fn symbol(&self) -> char {
        match self {
            ItemKind::IronOre => 'o',
            ItemKind::IronPlate => '=',
            ItemKind::Gear => '*',
        }
    }

    /// Display color for this item kind.
    pub fn color(&self) -> ratzilla::ratatui::style::Color {
        use ratzilla::ratatui::style::Color;
        match self {
            ItemKind::IronOre => Color::Cyan,
            ItemKind::IronPlate => Color::LightBlue,
            ItemKind::Gear => Color::Yellow,
        }
    }
}

/// Machine types.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MachineKind {
    /// Produces IronOre from nothing.
    Miner,
    /// Converts IronOre → IronPlate.
    Smelter,
    /// Converts IronPlate → Gear.
    Assembler,
    /// Exports items for money.
    Exporter,
}

impl MachineKind {
    pub fn name(&self) -> &str {
        match self {
            MachineKind::Miner => "Miner",
            MachineKind::Smelter => "Smelter",
            MachineKind::Assembler => "Assembler",
            MachineKind::Exporter => "Exporter",
        }
    }

    pub fn symbol(&self) -> char {
        match self {
            MachineKind::Miner => 'M',
            MachineKind::Smelter => 'S',
            MachineKind::Assembler => 'A',
            MachineKind::Exporter => 'E',
        }
    }

    /// Cost to place this machine.
    pub fn cost(&self) -> u64 {
        match self {
            MachineKind::Miner => 10,
            MachineKind::Smelter => 25,
            MachineKind::Assembler => 50,
            MachineKind::Exporter => 15,
        }
    }

    /// Ticks to produce one output.
    pub fn recipe_time(&self) -> u32 {
        match self {
            MachineKind::Miner => 10,    // 1 per second
            MachineKind::Smelter => 15,  // 0.67 per second
            MachineKind::Assembler => 20, // 0.5 per second
            MachineKind::Exporter => 5,  // 2 per second
        }
    }

    /// Input required (None for Miner).
    pub fn input(&self) -> Option<ItemKind> {
        match self {
            MachineKind::Miner => None,
            MachineKind::Smelter => Some(ItemKind::IronOre),
            MachineKind::Assembler => Some(ItemKind::IronPlate),
            MachineKind::Exporter => None, // accepts any
        }
    }

    /// Output produced (None for Exporter).
    pub fn output(&self) -> Option<ItemKind> {
        match self {
            MachineKind::Miner => Some(ItemKind::IronOre),
            MachineKind::Smelter => Some(ItemKind::IronPlate),
            MachineKind::Assembler => Some(ItemKind::Gear),
            MachineKind::Exporter => None,
        }
    }

    /// Revenue per item exported.
    pub fn export_value(item: &ItemKind) -> u64 {
        match item {
            ItemKind::IronOre => 1,
            ItemKind::IronPlate => 5,
            ItemKind::Gear => 20,
        }
    }
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
}

impl Machine {
    pub fn new(kind: MachineKind) -> Self {
        Self {
            kind,
            input_buffer: Vec::new(),
            output_buffer: Vec::new(),
            progress: 0,
            max_buffer: 5,
        }
    }
}

/// A belt segment.
#[derive(Clone, Debug)]
pub struct Belt {
    pub direction: Direction,
    /// Item currently on this belt tile (at most one).
    pub item: Option<ItemKind>,
}

impl Belt {
    pub fn new(direction: Direction) -> Self {
        Self {
            direction,
            item: None,
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
        assert_eq!(MachineKind::Smelter.input(), Some(ItemKind::IronOre));
        assert_eq!(MachineKind::Smelter.output(), Some(ItemKind::IronPlate));
    }

    #[test]
    fn export_values() {
        assert_eq!(MachineKind::export_value(&ItemKind::IronOre), 1);
        assert_eq!(MachineKind::export_value(&ItemKind::IronPlate), 5);
        assert_eq!(MachineKind::export_value(&ItemKind::Gear), 20);
    }
}
