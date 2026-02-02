/// Shared input handling: coordinate conversion, click targets, and event types.
///
/// This module is game-agnostic. Each game implements its own input dispatch.

/// All possible input events, normalized from keyboard, mouse, and touch sources.
#[derive(Debug, Clone, PartialEq)]
pub enum InputEvent {
    /// A key press (from keyboard, mouse click, or touch tap).
    Key(char),
}

/// A region on screen that can be tapped/clicked to trigger an action.
#[derive(Debug, Clone)]
pub struct ClickTarget {
    pub row: u16,
    pub key: char,
    /// Optional column range for horizontal targeting within a row.
    /// If None, the target matches the entire row.
    pub col_range: Option<(u16, u16)>,
}

/// Shared state between the render loop and click handler.
pub struct ClickState {
    pub targets: Vec<ClickTarget>,
    pub terminal_cols: u16,
    pub terminal_rows: u16,
}

impl ClickState {
    pub fn new() -> Self {
        Self {
            targets: Vec::new(),
            terminal_cols: 0,
            terminal_rows: 0,
        }
    }

    pub fn clear_targets(&mut self) {
        self.targets.clear();
    }

    pub fn add_target(&mut self, row: u16, key: char) {
        self.targets.push(ClickTarget { row, key, col_range: None });
    }

    /// Add a target with a specific column range (col_start..col_end, exclusive).
    pub fn add_target_col(&mut self, row: u16, col_start: u16, col_end: u16, key: char) {
        self.targets.push(ClickTarget { row, key, col_range: Some((col_start, col_end)) });
    }

    /// Find the action key for a given terminal row (row-wide only, ignores column targets).
    #[cfg(test)]
    pub fn find_target_key(&self, row: u16) -> Option<char> {
        self.find_target_key_at(row, None)
    }

    /// Find the action key for a given terminal row and column.
    /// Column-specific targets are checked first, then row-wide targets.
    pub fn find_target_key_at(&self, row: u16, col: Option<u16>) -> Option<char> {
        // First, try column-specific targets
        if let Some(c) = col {
            if let Some(t) = self.targets.iter().find(|t| {
                t.row == row && t.col_range.map_or(false, |(start, end)| c >= start && c < end)
            }) {
                return Some(t.key);
            }
        }
        // Fall back to row-wide targets (col_range == None)
        self.targets.iter()
            .find(|t| t.row == row && t.col_range.is_none())
            .map(|t| t.key)
    }
}

/// Convert a pixel Y coordinate to a terminal row index.
///
/// `click_y` is relative to the grid container's top edge.
/// `grid_height` is the total pixel height of the grid container.
/// `terminal_rows` is the number of rows in the terminal.
///
/// Returns `None` if the click is outside the grid or inputs are invalid.
pub fn pixel_y_to_row(click_y: f64, grid_height: f64, terminal_rows: u16) -> Option<u16> {
    if grid_height <= 0.0 || terminal_rows == 0 || click_y < 0.0 {
        return None;
    }

    let cell_height = grid_height / terminal_rows as f64;
    let row = (click_y / cell_height) as u16;

    if row >= terminal_rows {
        return None;
    }

    Some(row)
}

/// Determine whether a screen width (in columns) should use narrow layout.
pub fn is_narrow_layout(width: u16) -> bool {
    width < 60
}

/// Resolve a tap row to a key using click targets, then wrap as a Key event.
/// Returns None if the tap didn't hit any target.
#[cfg(test)]
pub fn resolve_tap(row: u16, click_state: &ClickState) -> Option<InputEvent> {
    click_state.find_target_key(row).map(InputEvent::Key)
}

/// Convert a pixel X coordinate to a terminal column index.
pub fn pixel_x_to_col(click_x: f64, grid_width: f64, terminal_cols: u16) -> Option<u16> {
    if grid_width <= 0.0 || terminal_cols == 0 || click_x < 0.0 {
        return None;
    }
    let cell_width = grid_width / terminal_cols as f64;
    let col = (click_x / cell_width) as u16;
    if col >= terminal_cols { None } else { Some(col) }
}

/// Resolve a tap at (row, col) to a key, checking column-specific targets first.
pub fn resolve_tap_at(row: u16, col: u16, click_state: &ClickState) -> Option<InputEvent> {
    click_state.find_target_key_at(row, Some(col)).map(InputEvent::Key)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── resolve_tap tests ─────────────────────────────────────────────

    #[test]
    fn resolve_tap_finds_target() {
        let mut cs = ClickState::new();
        cs.add_target(10, '1');
        cs.add_target(11, '2');

        assert_eq!(resolve_tap(10, &cs), Some(InputEvent::Key('1')));
        assert_eq!(resolve_tap(11, &cs), Some(InputEvent::Key('2')));
    }

    #[test]
    fn resolve_tap_misses_returns_none() {
        let mut cs = ClickState::new();
        cs.add_target(10, '1');

        assert_eq!(resolve_tap(9, &cs), None);
        assert_eq!(resolve_tap(11, &cs), None);
    }

    // ── pixel_y_to_row tests ───────────────────────────────────────────

    #[test]
    fn pixel_to_row_basic() {
        assert_eq!(pixel_y_to_row(0.0, 450.0, 30), Some(0));
        assert_eq!(pixel_y_to_row(14.0, 450.0, 30), Some(0));
        assert_eq!(pixel_y_to_row(15.0, 450.0, 30), Some(1));
        assert_eq!(pixel_y_to_row(29.0, 450.0, 30), Some(1));
        assert_eq!(pixel_y_to_row(449.0, 450.0, 30), Some(29));
    }

    #[test]
    fn pixel_to_row_out_of_bounds() {
        assert_eq!(pixel_y_to_row(450.0, 450.0, 30), None);
        assert_eq!(pixel_y_to_row(500.0, 450.0, 30), None);
    }

    #[test]
    fn pixel_to_row_negative_y() {
        assert_eq!(pixel_y_to_row(-1.0, 450.0, 30), None);
    }

    #[test]
    fn pixel_to_row_zero_height() {
        assert_eq!(pixel_y_to_row(10.0, 0.0, 30), None);
    }

    #[test]
    fn pixel_to_row_zero_rows() {
        assert_eq!(pixel_y_to_row(10.0, 450.0, 0), None);
    }

    #[test]
    fn pixel_to_row_fractional_cell_height() {
        assert_eq!(pixel_y_to_row(0.0, 400.0, 24), Some(0));
        assert_eq!(pixel_y_to_row(16.0, 400.0, 24), Some(0));
        assert_eq!(pixel_y_to_row(17.0, 400.0, 24), Some(1));
        assert_eq!(pixel_y_to_row(399.0, 400.0, 24), Some(23));
    }

    // ── find_target_key tests ──────────────────────────────────────────

    #[test]
    fn find_target_key_matches() {
        let mut cs = ClickState::new();
        cs.add_target(5, '1');
        cs.add_target(6, '2');
        cs.add_target(7, 'n');

        assert_eq!(cs.find_target_key(5), Some('1'));
        assert_eq!(cs.find_target_key(6), Some('2'));
        assert_eq!(cs.find_target_key(7), Some('n'));
    }

    #[test]
    fn find_target_key_no_match() {
        let mut cs = ClickState::new();
        cs.add_target(5, '1');
        assert_eq!(cs.find_target_key(0), None);
        assert_eq!(cs.find_target_key(100), None);
    }

    #[test]
    fn find_target_key_empty() {
        let cs = ClickState::new();
        assert_eq!(cs.find_target_key(0), None);
    }

    #[test]
    fn find_target_key_duplicate_rows_returns_first() {
        let mut cs = ClickState::new();
        cs.add_target(5, 'a');
        cs.add_target(5, 'b');
        assert_eq!(cs.find_target_key(5), Some('a'));
    }

    // ── ClickState management tests ────────────────────────────────────

    #[test]
    fn click_state_clear() {
        let mut cs = ClickState::new();
        cs.add_target(1, 'x');
        cs.add_target(2, 'y');
        assert_eq!(cs.targets.len(), 2);

        cs.clear_targets();
        assert_eq!(cs.targets.len(), 0);
        assert_eq!(cs.find_target_key(1), None);
    }

    // ── Layout responsive tests ────────────────────────────────────────

    #[test]
    fn narrow_layout_threshold() {
        assert!(is_narrow_layout(30));
        assert!(is_narrow_layout(59));
        assert!(!is_narrow_layout(60));
        assert!(!is_narrow_layout(80));
    }

    // ── Integration-style pipeline tests ────────────────────────────────

    /// Helper: simulate tapping the center of a given target row.
    fn assert_tap_hits(cs: &ClickState, grid_height: f64, target_row: u16, expected_key: char) {
        let cell_height = grid_height / cs.terminal_rows as f64;
        let click_y = target_row as f64 * cell_height + cell_height / 2.0;
        let row = pixel_y_to_row(click_y, grid_height, cs.terminal_rows);
        assert_eq!(row, Some(target_row));
        assert_eq!(cs.find_target_key(target_row), Some(expected_key));
    }

    #[test]
    fn full_click_pipeline() {
        let mut cs = ClickState::new();
        cs.terminal_cols = 80;
        cs.terminal_rows = 30;

        cs.add_target(11, '1');
        cs.add_target(12, '2');
        cs.add_target(13, 'n');

        for row in 27..30 {
            cs.add_target(row, 'i');
        }

        let grid_height = 450.0;
        let cell_height = grid_height / 30.0;

        let click_y = 11.0 * cell_height + 7.0;
        let row = pixel_y_to_row(click_y, grid_height, cs.terminal_rows).unwrap();
        assert_eq!(row, 11);
        assert_eq!(cs.find_target_key(row), Some('1'));

        let click_y = 28.0 * cell_height + 10.0;
        let row = pixel_y_to_row(click_y, grid_height, cs.terminal_rows).unwrap();
        assert_eq!(cs.find_target_key(row), Some('i'));
    }

    #[test]
    fn consistent_cell_height_assumption() {
        const EXPECTED_CELL_HEIGHT: f64 = 15.0;
        let terminal_rows: u16 = 30;
        let grid_height = terminal_rows as f64 * EXPECTED_CELL_HEIGHT;

        for target_row in 0..terminal_rows {
            let center_y = target_row as f64 * EXPECTED_CELL_HEIGHT + EXPECTED_CELL_HEIGHT / 2.0;
            let result = pixel_y_to_row(center_y, grid_height, terminal_rows);
            assert_eq!(result, Some(target_row));
        }
    }

    // ── Column-specific click target tests ──────────────────────────────

    #[test]
    fn col_target_matches_within_range() {
        let mut cs = ClickState::new();
        cs.add_target_col(5, 0, 10, 'a');
        cs.add_target_col(5, 10, 20, 'b');
        assert_eq!(cs.find_target_key_at(5, Some(3)), Some('a'));
        assert_eq!(cs.find_target_key_at(5, Some(15)), Some('b'));
    }

    #[test]
    fn col_target_exclusive_end() {
        let mut cs = ClickState::new();
        cs.add_target_col(5, 0, 10, 'a');
        cs.add_target_col(5, 10, 20, 'b');
        // col 10 should match 'b' (start inclusive), not 'a' (end exclusive)
        assert_eq!(cs.find_target_key_at(5, Some(10)), Some('b'));
        // col 9 should match 'a'
        assert_eq!(cs.find_target_key_at(5, Some(9)), Some('a'));
    }

    #[test]
    fn col_target_falls_back_to_row_wide() {
        let mut cs = ClickState::new();
        cs.add_target_col(5, 0, 10, 'a');
        cs.add_target(5, 'z'); // row-wide fallback
        // Click in col range → col target
        assert_eq!(cs.find_target_key_at(5, Some(5)), Some('a'));
        // Click outside col range → row-wide fallback
        assert_eq!(cs.find_target_key_at(5, Some(30)), Some('z'));
    }

    #[test]
    fn col_target_no_col_uses_row_wide() {
        let mut cs = ClickState::new();
        cs.add_target_col(5, 0, 10, 'a');
        cs.add_target(5, 'z');
        // No column → row-wide only
        assert_eq!(cs.find_target_key_at(5, None), Some('z'));
    }

    #[test]
    fn pixel_x_to_col_basic() {
        assert_eq!(pixel_x_to_col(0.0, 800.0, 80), Some(0));
        assert_eq!(pixel_x_to_col(10.0, 800.0, 80), Some(1));
        assert_eq!(pixel_x_to_col(799.0, 800.0, 80), Some(79));
    }

    #[test]
    fn pixel_x_to_col_out_of_bounds() {
        assert_eq!(pixel_x_to_col(800.0, 800.0, 80), None);
        assert_eq!(pixel_x_to_col(-1.0, 800.0, 80), None);
    }

    #[test]
    fn resolve_tap_at_uses_col() {
        let mut cs = ClickState::new();
        cs.add_target_col(5, 0, 10, 'a');
        cs.add_target_col(5, 10, 20, 'b');
        assert_eq!(resolve_tap_at(5, 3, &cs), Some(InputEvent::Key('a')));
        assert_eq!(resolve_tap_at(5, 15, &cs), Some(InputEvent::Key('b')));
        assert_eq!(resolve_tap_at(5, 25, &cs), None); // no match
    }

    #[test]
    fn mobile_narrow_click_pipeline() {
        let mut cs = ClickState::new();
        cs.terminal_cols = 37;
        cs.terminal_rows = 50;

        cs.add_target(9, '1');
        cs.add_target(10, '2');
        cs.add_target(11, 'n');

        for row in 47..50 {
            cs.add_target(row, 'i');
        }

        let grid_height = 50.0 * 15.0;
        assert_tap_hits(&cs, grid_height, 9, '1');
        assert_tap_hits(&cs, grid_height, 10, '2');
        assert_tap_hits(&cs, grid_height, 11, 'n');
        assert_tap_hits(&cs, grid_height, 48, 'i');
    }
}
