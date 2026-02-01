/// Click/tap handling logic for the escape room game.
///
/// This module separates the pure logic (coordinate conversion, target matching,
/// action dispatch) from web_sys DOM access so it can be unit tested.

/// A region on screen that can be tapped/clicked to trigger an action.
#[derive(Debug, Clone)]
pub struct ClickTarget {
    pub row: u16,
    pub key: char,
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
        self.targets.push(ClickTarget { row, key });
    }

    /// Find the action key for a given terminal row.
    pub fn find_target_key(&self, row: u16) -> Option<char> {
        self.targets.iter().find(|t| t.row == row).map(|t| t.key)
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

#[cfg(test)]
mod tests {
    use super::*;

    // ── pixel_y_to_row tests ───────────────────────────────────────────

    #[test]
    fn pixel_to_row_basic() {
        // 30 rows, grid 450px tall → each row is 15px
        assert_eq!(pixel_y_to_row(0.0, 450.0, 30), Some(0));
        assert_eq!(pixel_y_to_row(14.0, 450.0, 30), Some(0));
        assert_eq!(pixel_y_to_row(15.0, 450.0, 30), Some(1));
        assert_eq!(pixel_y_to_row(29.0, 450.0, 30), Some(1));
        assert_eq!(pixel_y_to_row(449.0, 450.0, 30), Some(29));
    }

    #[test]
    fn pixel_to_row_out_of_bounds() {
        // 450px / 30 rows = 15px per row; clicking at y=450 → row 30, which is out of bounds
        assert_eq!(pixel_y_to_row(450.0, 450.0, 30), None);
        assert_eq!(pixel_y_to_row(500.0, 450.0, 30), None);
    }

    #[test]
    fn pixel_to_row_negative_y() {
        assert_eq!(pixel_y_to_row(-1.0, 450.0, 30), None);
        assert_eq!(pixel_y_to_row(-100.0, 450.0, 30), None);
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
        // 24 rows, 400px → cell_height ≈ 16.67px
        assert_eq!(pixel_y_to_row(0.0, 400.0, 24), Some(0));
        assert_eq!(pixel_y_to_row(16.0, 400.0, 24), Some(0)); // 16/16.67 = 0.96
        assert_eq!(pixel_y_to_row(17.0, 400.0, 24), Some(1)); // 17/16.67 = 1.02
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
        cs.add_target(6, '2');

        assert_eq!(cs.find_target_key(0), None);
        assert_eq!(cs.find_target_key(4), None);
        assert_eq!(cs.find_target_key(7), None);
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

        // Should return the first match
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

    #[test]
    fn click_state_help_bar_range() {
        // Help bar occupies rows 27-29 (3 rows), all mapped to 'i'
        let mut cs = ClickState::new();
        for row in 27..30 {
            cs.add_target(row, 'i');
        }
        assert_eq!(cs.find_target_key(27), Some('i'));
        assert_eq!(cs.find_target_key(28), Some('i'));
        assert_eq!(cs.find_target_key(29), Some('i'));
        assert_eq!(cs.find_target_key(26), None);
        assert_eq!(cs.find_target_key(30), None);
    }

    // ── Layout responsive tests ────────────────────────────────────────

    #[test]
    fn narrow_layout_threshold() {
        assert!(is_narrow_layout(30));
        assert!(is_narrow_layout(59));
        assert!(!is_narrow_layout(60));
        assert!(!is_narrow_layout(80));
        assert!(!is_narrow_layout(120));
    }

    // ── Integration-style: pixel → target key pipeline ─────────────────

    #[test]
    fn full_click_pipeline() {
        // Simulate a 80x30 terminal, grid height 450px
        let mut cs = ClickState::new();
        cs.terminal_cols = 80;
        cs.terminal_rows = 30;

        // Actions panel starts at row 11 (title=3, room_desc=7, +1 for border)
        cs.add_target(11, '1'); // "デスクを調べる"
        cs.add_target(12, '2'); // "引き出し"
        cs.add_target(13, 'n'); // "北のドア"

        // Help bar at rows 27-29
        for row in 27..30 {
            cs.add_target(row, 'i');
        }

        let grid_height = 450.0;
        let cell_height = grid_height / 30.0; // 15px

        // Click on action '1' (row 11, pixel y = 11 * 15 + 7 = 172)
        let click_y = 11.0 * cell_height + 7.0;
        let row = pixel_y_to_row(click_y, grid_height, cs.terminal_rows).unwrap();
        assert_eq!(row, 11);
        assert_eq!(cs.find_target_key(row), Some('1'));

        // Click on action 'n' (row 13, pixel y = 13 * 15 + 3 = 198)
        let click_y = 13.0 * cell_height + 3.0;
        let row = pixel_y_to_row(click_y, grid_height, cs.terminal_rows).unwrap();
        assert_eq!(row, 13);
        assert_eq!(cs.find_target_key(row), Some('n'));

        // Click on help bar (row 28, pixel y = 28 * 15 + 10 = 430)
        let click_y = 28.0 * cell_height + 10.0;
        let row = pixel_y_to_row(click_y, grid_height, cs.terminal_rows).unwrap();
        assert_eq!(row, 28);
        assert_eq!(cs.find_target_key(row), Some('i'));

        // Click on log area (row 20) — no target
        let click_y = 20.0 * cell_height + 5.0;
        let row = pixel_y_to_row(click_y, grid_height, cs.terminal_rows).unwrap();
        assert_eq!(row, 20);
        assert_eq!(cs.find_target_key(row), None);
    }

    // ── Grid height drift regression tests ───────────────────────────────
    //
    // DomBackend sets each <pre> to height: 15px. If CSS overrides (e.g.
    // box-sizing: border-box) change the effective row height, grid_height
    // passed to pixel_y_to_row will differ from the expected value, causing
    // row calculations to drift and targets to be missed.

    /// Helper: simulate tapping the center of a given target row and check
    /// that the correct key is returned.
    fn assert_tap_hits(cs: &ClickState, grid_height: f64, target_row: u16, expected_key: char) {
        let cell_height = grid_height / cs.terminal_rows as f64;
        // Tap center of the target row
        let click_y = target_row as f64 * cell_height + cell_height / 2.0;
        let row = pixel_y_to_row(click_y, grid_height, cs.terminal_rows);
        assert_eq!(
            row,
            Some(target_row),
            "grid_height={grid_height}, target_row={target_row}: expected row {target_row}, got {row:?}"
        );
        assert_eq!(
            cs.find_target_key(target_row),
            Some(expected_key),
            "grid_height={grid_height}: no target at row {target_row}"
        );
    }

    #[test]
    fn grid_height_drift_breaks_targeting() {
        // Scenario: 30 rows, expected cell_height=15px, grid_height=450px.
        // If CSS shrinks grid_height (e.g. box-sizing: border-box reducing
        // each row by 4px padding → cell_height=11px → grid_height=330px),
        // a tap at pixel y=172 (intended for row 11) would land on a
        // different row.
        let mut cs = ClickState::new();
        cs.terminal_cols = 40;
        cs.terminal_rows = 30;

        cs.add_target(11, '1');
        cs.add_target(12, '2');
        cs.add_target(13, 'n');

        let correct_grid_height = 450.0; // 30 rows × 15px
        let cell_height = correct_grid_height / 30.0;

        // With correct grid_height, tapping center of row 11 hits target '1'
        assert_tap_hits(&cs, correct_grid_height, 11, '1');

        // Simulate CSS shrinking grid_height (e.g. box-sizing eats padding)
        let broken_grid_height = 330.0; // 30 rows × 11px

        // Same pixel coordinate that was intended for row 11 center
        let pixel_y = 11.0 * cell_height + cell_height / 2.0; // 172.5px

        // With the broken (smaller) grid, this pixel maps to a higher row
        let drifted_row = pixel_y_to_row(pixel_y, broken_grid_height, 30);
        assert!(
            drifted_row.is_none() || drifted_row != Some(11),
            "Expected drift: pixel {pixel_y} with grid_height {broken_grid_height} \
             should NOT map to row 11 (got {drifted_row:?}). \
             If grid_height shrinks but pixel coords don't change, rows drift."
        );

        // This demonstrates the bug: the pixel is now out of bounds or on wrong row
        let drifted = drifted_row.unwrap_or(u16::MAX);
        assert_ne!(
            cs.find_target_key(drifted),
            Some('1'),
            "Drifted row {drifted} should NOT hit target '1' at row 11"
        );
    }

    #[test]
    fn consistent_cell_height_assumption() {
        // Verify that pixel_y_to_row produces correct rows when
        // grid_height = terminal_rows × EXPECTED_CELL_HEIGHT.
        // This is the contract between DomBackend (height: 15px per pre)
        // and our click handler.
        const EXPECTED_CELL_HEIGHT: f64 = 15.0;
        let terminal_rows: u16 = 30;
        let grid_height = terminal_rows as f64 * EXPECTED_CELL_HEIGHT;

        // Every row center should map to the correct row
        for target_row in 0..terminal_rows {
            let center_y = target_row as f64 * EXPECTED_CELL_HEIGHT + EXPECTED_CELL_HEIGHT / 2.0;
            let result = pixel_y_to_row(center_y, grid_height, terminal_rows);
            assert_eq!(
                result,
                Some(target_row),
                "Row {target_row} center (y={center_y}) should map correctly \
                 with cell_height={EXPECTED_CELL_HEIGHT}"
            );
        }

        // Row boundaries: top pixel of each row
        for target_row in 0..terminal_rows {
            let top_y = target_row as f64 * EXPECTED_CELL_HEIGHT;
            let result = pixel_y_to_row(top_y, grid_height, terminal_rows);
            assert_eq!(result, Some(target_row));
        }

        // Bottom pixel of each row (just before next row)
        for target_row in 0..terminal_rows {
            let bottom_y = (target_row + 1) as f64 * EXPECTED_CELL_HEIGHT - 0.1;
            let result = pixel_y_to_row(bottom_y, grid_height, terminal_rows);
            assert_eq!(result, Some(target_row));
        }
    }

    #[test]
    fn mobile_narrow_click_pipeline() {
        // Simulate a narrow mobile terminal (e.g. 37x50 on a phone)
        // with DomBackend's cell_height = 15px → grid_height = 750px
        let mut cs = ClickState::new();
        cs.terminal_cols = 37;
        cs.terminal_rows = 50;

        // Narrow layout: title(3) + room(5) → actions start at row 9
        // (row 8 = top border of actions panel, row 9 = first action)
        cs.add_target(9, '1');
        cs.add_target(10, '2');
        cs.add_target(11, 'n');

        // Help bar at rows 47-49
        for row in 47..50 {
            cs.add_target(row, 'i');
        }

        let grid_height = 50.0 * 15.0; // 750px
        assert_tap_hits(&cs, grid_height, 9, '1');
        assert_tap_hits(&cs, grid_height, 10, '2');
        assert_tap_hits(&cs, grid_height, 11, 'n');
        assert_tap_hits(&cs, grid_height, 48, 'i');

        // Verify that non-target rows don't match
        let cell_h = grid_height / 50.0;
        let click_y = 20.0 * cell_h + cell_h / 2.0;
        let row = pixel_y_to_row(click_y, grid_height, 50).unwrap();
        assert_eq!(cs.find_target_key(row), None);
    }
}
