/// Tiny Factory game state.

use super::grid::{Cell, GRID_H, GRID_W};

/// What the player is placing.
#[derive(Clone, Debug, PartialEq)]
pub enum PlacementTool {
    None,
    Miner,
    Smelter,
    Assembler,
    Exporter,
    Belt, // uses current belt direction
    Delete,
}

/// Factory game state.
pub struct FactoryState {
    /// 2D grid [y][x].
    pub grid: Vec<Vec<Cell>>,
    /// Player's money.
    pub money: u64,
    /// Total items exported.
    pub total_exported: u64,
    /// Current cursor position.
    pub cursor_x: usize,
    pub cursor_y: usize,
    /// Current placement tool.
    pub tool: PlacementTool,
    /// Current belt direction (used when placing belts).
    pub belt_direction: super::grid::Direction,
    /// Stats: items produced per kind (for display).
    pub produced_count: [u64; 3], // IronOre, IronPlate, Gear
    /// Message log.
    pub log: Vec<String>,
    /// Animation frame counter.
    pub anim_frame: u32,
}

impl FactoryState {
    pub fn new() -> Self {
        let grid = vec![vec![Cell::Empty; GRID_W]; GRID_H];
        Self {
            grid,
            money: 50, // starting money
            total_exported: 0,
            cursor_x: 0,
            cursor_y: 0,
            tool: PlacementTool::None,
            belt_direction: super::grid::Direction::Right,
            produced_count: [0; 3],
            log: vec!["Tiny Factory へようこそ！".into()],
            anim_frame: 0,
        }
    }

    pub fn add_log(&mut self, text: &str) {
        self.log.push(text.to_string());
        if self.log.len() > 30 {
            self.log.remove(0);
        }
    }

    /// Move cursor, clamped to grid bounds.
    pub fn move_cursor(&mut self, dx: i32, dy: i32) {
        let nx = (self.cursor_x as i32 + dx).clamp(0, GRID_W as i32 - 1) as usize;
        let ny = (self.cursor_y as i32 + dy).clamp(0, GRID_H as i32 - 1) as usize;
        self.cursor_x = nx;
        self.cursor_y = ny;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_state() {
        let s = FactoryState::new();
        assert_eq!(s.money, 50);
        assert_eq!(s.grid.len(), GRID_H);
        assert_eq!(s.grid[0].len(), GRID_W);
        assert_eq!(s.cursor_x, 0);
        assert_eq!(s.cursor_y, 0);
    }

    #[test]
    fn move_cursor_clamp() {
        let mut s = FactoryState::new();
        s.move_cursor(-1, -1); // should stay at 0,0
        assert_eq!((s.cursor_x, s.cursor_y), (0, 0));

        s.move_cursor(100, 100); // clamp to max
        assert_eq!((s.cursor_x, s.cursor_y), (GRID_W - 1, GRID_H - 1));
    }

    #[test]
    fn log_truncation() {
        let mut s = FactoryState::new();
        for i in 0..40 {
            s.add_log(&format!("msg {}", i));
        }
        assert!(s.log.len() <= 30);
    }
}
