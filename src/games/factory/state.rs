/// Tiny Factory game state.

use super::grid::{Cell, GRID_H, GRID_W, VIEW_H, VIEW_W};

/// What the player is placing.
#[derive(Clone, Debug, PartialEq)]
pub enum PlacementTool {
    None,
    Miner,
    Smelter,
    Assembler,
    Exporter,
    Fabricator,
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
    /// [IronOre, IronPlate, Gear, CopperOre, CopperPlate, Circuit]
    pub produced_count: [u64; 6],
    /// Message log.
    pub log: Vec<String>,
    /// Animation frame counter.
    pub anim_frame: u32,
    /// Flash timer for export events (ticks remaining).
    pub export_flash: u32,
    /// Value of the last export (for display during flash).
    pub last_export_value: u64,
    /// Total money earned from exports (for income rate calculation).
    pub total_money_earned: u64,
    /// Tick counter for income rate calculation.
    pub total_ticks: u64,
    /// Viewport top-left corner (scroll offset).
    pub viewport_x: usize,
    pub viewport_y: usize,
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
            produced_count: [0; 6],
            log: vec!["Tiny Factory へようこそ！".into()],
            anim_frame: 0,
            export_flash: 0,
            last_export_value: 0,
            total_money_earned: 0,
            total_ticks: 0,
            viewport_x: 0,
            viewport_y: 0,
        }
    }

    pub fn add_log(&mut self, text: &str) {
        self.log.push(text.to_string());
        if self.log.len() > 30 {
            self.log.remove(0);
        }
    }

    /// Move cursor, clamped to grid bounds. Scrolls viewport to follow cursor.
    /// Also updates belt_direction to match movement direction.
    pub fn move_cursor(&mut self, dx: i32, dy: i32) {
        let nx = (self.cursor_x as i32 + dx).clamp(0, GRID_W as i32 - 1) as usize;
        let ny = (self.cursor_y as i32 + dy).clamp(0, GRID_H as i32 - 1) as usize;
        // Auto-set belt direction from cursor movement
        if dx == 1 && dy == 0 { self.belt_direction = super::grid::Direction::Right; }
        else if dx == -1 && dy == 0 { self.belt_direction = super::grid::Direction::Left; }
        else if dx == 0 && dy == 1 { self.belt_direction = super::grid::Direction::Down; }
        else if dx == 0 && dy == -1 { self.belt_direction = super::grid::Direction::Up; }
        self.cursor_x = nx;
        self.cursor_y = ny;
        self.scroll_to_cursor();
    }

    /// Adjust viewport so the cursor is visible, with 1-cell margin from edges.
    pub fn scroll_to_cursor(&mut self) {
        let margin = 1usize;
        // Horizontal
        if self.cursor_x < self.viewport_x + margin {
            self.viewport_x = self.cursor_x.saturating_sub(margin);
        } else if self.cursor_x >= self.viewport_x + VIEW_W - margin {
            self.viewport_x = (self.cursor_x + margin + 1).saturating_sub(VIEW_W);
        }
        // Vertical
        if self.cursor_y < self.viewport_y + margin {
            self.viewport_y = self.cursor_y.saturating_sub(margin);
        } else if self.cursor_y >= self.viewport_y + VIEW_H - margin {
            self.viewport_y = (self.cursor_y + margin + 1).saturating_sub(VIEW_H);
        }
        // Clamp viewport to grid bounds
        self.viewport_x = self.viewport_x.min(GRID_W.saturating_sub(VIEW_W));
        self.viewport_y = self.viewport_y.min(GRID_H.saturating_sub(VIEW_H));
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
    fn viewport_scrolls_with_cursor() {
        let mut s = FactoryState::new();
        assert_eq!(s.viewport_x, 0);
        assert_eq!(s.viewport_y, 0);

        // Move cursor to right edge of viewport — should start scrolling
        for _ in 0..VIEW_W {
            s.move_cursor(1, 0);
        }
        assert!(s.viewport_x > 0, "viewport should have scrolled right");
        assert!(s.cursor_x >= s.viewport_x, "cursor should be within viewport");
        assert!(s.cursor_x < s.viewport_x + VIEW_W, "cursor should be within viewport");
    }

    #[test]
    fn viewport_clamped_to_grid() {
        let mut s = FactoryState::new();
        // Move cursor to far bottom-right
        s.move_cursor(GRID_W as i32, GRID_H as i32);
        assert_eq!(s.cursor_x, GRID_W - 1);
        assert_eq!(s.cursor_y, GRID_H - 1);
        assert!(s.viewport_x + VIEW_W <= GRID_W);
        assert!(s.viewport_y + VIEW_H <= GRID_H);
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

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn prop_cursor_always_within_bounds(
            moves in proptest::collection::vec(
                prop_oneof![
                    Just((0i32, -1i32)),
                    Just((0, 1)),
                    Just((-1, 0)),
                    Just((1, 0)),
                ],
                1..50,
            ),
        ) {
            let mut state = FactoryState::new();
            for (dx, dy) in moves {
                state.move_cursor(dx, dy);
                prop_assert!(state.cursor_x < GRID_W, "cursor_x out of bounds: {}", state.cursor_x);
                prop_assert!(state.cursor_y < GRID_H, "cursor_y out of bounds: {}", state.cursor_y);
            }
        }

        #[test]
        fn prop_viewport_always_valid_after_scroll(
            moves in proptest::collection::vec(
                prop_oneof![
                    Just((0i32, -1i32)),
                    Just((0, 1)),
                    Just((-1, 0)),
                    Just((1, 0)),
                ],
                1..50,
            ),
        ) {
            let mut state = FactoryState::new();
            for (dx, dy) in moves {
                state.move_cursor(dx, dy);
                state.scroll_to_cursor();
                prop_assert!(state.viewport_x + VIEW_W <= GRID_W,
                    "viewport_x overflow: {} + {} > {}", state.viewport_x, VIEW_W, GRID_W);
                prop_assert!(state.viewport_y + VIEW_H <= GRID_H,
                    "viewport_y overflow: {} + {} > {}", state.viewport_y, VIEW_H, GRID_H);
            }
        }

        #[test]
        fn prop_cursor_visible_in_viewport(
            moves in proptest::collection::vec(
                prop_oneof![
                    Just((0i32, -1i32)),
                    Just((0, 1)),
                    Just((-1, 0)),
                    Just((1, 0)),
                ],
                1..50,
            ),
        ) {
            let mut state = FactoryState::new();
            for (dx, dy) in moves {
                state.move_cursor(dx, dy);
                state.scroll_to_cursor();
                prop_assert!(state.cursor_x >= state.viewport_x,
                    "cursor_x {} < viewport_x {}", state.cursor_x, state.viewport_x);
                prop_assert!(state.cursor_x < state.viewport_x + VIEW_W,
                    "cursor_x {} >= viewport_x + VIEW_W {}", state.cursor_x, state.viewport_x + VIEW_W);
                prop_assert!(state.cursor_y >= state.viewport_y,
                    "cursor_y {} < viewport_y {}", state.cursor_y, state.viewport_y);
                prop_assert!(state.cursor_y < state.viewport_y + VIEW_H,
                    "cursor_y {} >= viewport_y + VIEW_H {}", state.cursor_y, state.viewport_y + VIEW_H);
            }
        }
    }
}
