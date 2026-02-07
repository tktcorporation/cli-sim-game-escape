//! Shared input handling: coordinate conversion, click targets, and event types.
//!
//! This module is game-agnostic. Each game implements its own input dispatch.

use ratzilla::ratatui::layout::Rect;

/// All possible input events, normalized from keyboard, mouse, and touch sources.
#[derive(Debug, Clone, PartialEq)]
pub enum InputEvent {
    /// A key press from keyboard.
    Key(char),
    /// A click/tap on a registered target, identified by a semantic action ID.
    /// Each game defines its own action ID constants.
    Click(u16),
}

/// A region on screen that can be tapped/clicked to trigger an action.
#[derive(Debug, Clone)]
pub struct ClickTarget {
    /// The rectangular region (in terminal cell coordinates) for hit testing.
    pub rect: Rect,
    /// Semantic action ID. Each game defines its own constants.
    pub action_id: u16,
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

    /// Register a click target with a rectangular hit region and a semantic action ID.
    pub fn add_click_target(&mut self, rect: Rect, action_id: u16) {
        self.targets.push(ClickTarget { rect, action_id });
    }

    /// Convenience: register a full-row click target at the given row within an area.
    pub fn add_row_target(&mut self, area: Rect, row: u16, action_id: u16) {
        if row >= area.y && row < area.y + area.height {
            self.targets.push(ClickTarget {
                rect: Rect::new(area.x, row, area.width, 1),
                action_id,
            });
        }
    }

    /// Register click targets for a horizontal tab bar based on actual text widths.
    ///
    /// Each entry in `tab_widths` is `(display_width, action_id)` for the **padded**
    /// label text of that tab (e.g. `" 生産 "` → display_width = 6).
    /// `separator_width` is the display width of the separator string between tabs.
    ///
    /// Click targets are computed from the actual text positions so each target
    /// covers its label plus half of the adjacent separator(s).  The first tab
    /// extends to the left edge and the last tab extends to the right edge of
    /// the area, ensuring full coverage with no gaps.
    pub fn register_tab_targets(
        &mut self,
        tab_widths: &[(u16, u16)],
        separator_width: u16,
        x: u16,
        y: u16,
        total_width: u16,
        height: u16,
    ) {
        let n = tab_widths.len();
        if n == 0 || total_width == 0 {
            return;
        }

        // Compute the starting column of each tab label
        let mut starts: Vec<u16> = Vec::with_capacity(n);
        let mut cursor: u16 = 0;
        for (i, &(w, _)) in tab_widths.iter().enumerate() {
            if i > 0 {
                cursor += separator_width;
            }
            starts.push(cursor);
            cursor += w;
        }

        for i in 0..n {
            let (_, action_id) = tab_widths[i];

            // Left boundary: first tab from 0, others from midpoint of left separator
            let left = if i == 0 {
                0
            } else {
                let prev_end = starts[i - 1] + tab_widths[i - 1].0;
                prev_end + (starts[i] - prev_end) / 2
            };

            // Right boundary: last tab to total_width, others to midpoint of right sep
            let right = if i == n - 1 {
                total_width
            } else {
                let cur_end = starts[i] + tab_widths[i].0;
                let next_start = starts[i + 1];
                cur_end + (next_start - cur_end) / 2
            };

            let w = right.saturating_sub(left);
            if w > 0 {
                self.add_click_target(Rect::new(x + left, y, w, height), action_id);
            }
        }
    }

    /// Hit-test a terminal cell coordinate against all registered targets.
    /// Returns the action ID of the first matching target (last registered takes priority
    /// when targets overlap, matching typical UI layering where later elements are on top).
    pub fn hit_test(&self, col: u16, row: u16) -> Option<u16> {
        // Iterate in reverse so later-registered (topmost) targets win.
        self.targets.iter().rev().find_map(|t| {
            let r = &t.rect;
            if col >= r.x && col < r.x + r.width && row >= r.y && row < r.y + r.height {
                Some(t.action_id)
            } else {
                None
            }
        })
    }
}

/// Determine whether a screen width (in columns) should use narrow layout.
pub fn is_narrow_layout(width: u16) -> bool {
    width < 60
}

/// Convert a pixel Y coordinate to a terminal row index.
///
/// `click_y` is relative to the grid container's top edge.
/// `grid_height` is the total pixel height of the grid container.
/// `terminal_rows` is the number of rows in the terminal.
///
/// Returns `None` if the click is outside the grid or inputs are invalid.
#[cfg(test)]
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

/// Convert a pixel X coordinate to a terminal column index.
#[cfg(test)]
pub fn pixel_x_to_col(click_x: f64, grid_width: f64, terminal_cols: u16) -> Option<u16> {
    if grid_width <= 0.0 || terminal_cols == 0 || click_x < 0.0 {
        return None;
    }
    let cell_width = grid_width / terminal_cols as f64;
    let col = (click_x / cell_width) as u16;
    if col >= terminal_cols { None } else { Some(col) }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── hit_test tests ──────────────────────────────────────────────

    #[test]
    fn hit_test_basic() {
        let mut cs = ClickState::new();
        cs.add_click_target(Rect::new(0, 10, 80, 1), 1);
        cs.add_click_target(Rect::new(0, 11, 80, 1), 2);

        assert_eq!(cs.hit_test(5, 10), Some(1));
        assert_eq!(cs.hit_test(5, 11), Some(2));
    }

    #[test]
    fn hit_test_miss_returns_none() {
        let mut cs = ClickState::new();
        cs.add_click_target(Rect::new(0, 10, 80, 1), 1);

        assert_eq!(cs.hit_test(5, 9), None);
        assert_eq!(cs.hit_test(5, 11), None);
    }

    #[test]
    fn hit_test_multi_row_rect() {
        let mut cs = ClickState::new();
        cs.add_click_target(Rect::new(0, 5, 40, 3), 42);

        assert_eq!(cs.hit_test(10, 4), None);
        assert_eq!(cs.hit_test(10, 5), Some(42));
        assert_eq!(cs.hit_test(10, 6), Some(42));
        assert_eq!(cs.hit_test(10, 7), Some(42));
        assert_eq!(cs.hit_test(10, 8), None);
    }

    #[test]
    fn hit_test_column_precision() {
        let mut cs = ClickState::new();
        // Two targets side by side on the same row
        cs.add_click_target(Rect::new(0, 5, 10, 1), 1);
        cs.add_click_target(Rect::new(10, 5, 10, 1), 2);

        assert_eq!(cs.hit_test(3, 5), Some(1));
        assert_eq!(cs.hit_test(9, 5), Some(1));
        assert_eq!(cs.hit_test(10, 5), Some(2));
        assert_eq!(cs.hit_test(15, 5), Some(2));
        assert_eq!(cs.hit_test(20, 5), None);
    }

    #[test]
    fn hit_test_overlap_last_wins() {
        let mut cs = ClickState::new();
        // Row-wide target registered first
        cs.add_click_target(Rect::new(0, 5, 80, 1), 1);
        // Narrower target registered later (on top)
        cs.add_click_target(Rect::new(5, 5, 10, 1), 2);

        // Inside the narrow target → later target wins
        assert_eq!(cs.hit_test(7, 5), Some(2));
        // Outside the narrow target → falls back to row-wide
        assert_eq!(cs.hit_test(0, 5), Some(1));
        assert_eq!(cs.hit_test(20, 5), Some(1));
    }

    #[test]
    fn hit_test_empty() {
        let cs = ClickState::new();
        assert_eq!(cs.hit_test(0, 0), None);
    }

    // ── add_row_target tests ──────────────────────────────────────

    #[test]
    fn add_row_target_within_area() {
        let mut cs = ClickState::new();
        let area = Rect::new(5, 10, 30, 5);
        cs.add_row_target(area, 12, 99);

        assert_eq!(cs.targets.len(), 1);
        assert_eq!(cs.hit_test(15, 12), Some(99));
    }

    #[test]
    fn add_row_target_outside_area_ignored() {
        let mut cs = ClickState::new();
        let area = Rect::new(5, 10, 30, 5);
        cs.add_row_target(area, 9, 99);  // before area
        cs.add_row_target(area, 15, 98); // after area

        assert_eq!(cs.targets.len(), 0);
    }

    // ── ClickState management tests ────────────────────────────────

    #[test]
    fn click_state_clear() {
        let mut cs = ClickState::new();
        cs.add_click_target(Rect::new(0, 1, 80, 1), 1);
        cs.add_click_target(Rect::new(0, 2, 80, 1), 2);
        assert_eq!(cs.targets.len(), 2);

        cs.clear_targets();
        assert_eq!(cs.targets.len(), 0);
        assert_eq!(cs.hit_test(0, 1), None);
    }

    // ── Layout responsive tests ────────────────────────────────────

    #[test]
    fn narrow_layout_threshold() {
        assert!(is_narrow_layout(30));
        assert!(is_narrow_layout(59));
        assert!(!is_narrow_layout(60));
        assert!(!is_narrow_layout(80));
    }

    // ── pixel coordinate conversion tests ──────────────────────────

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

    // ── Integration-style pipeline tests ────────────────────────────

    #[test]
    fn full_click_pipeline() {
        let mut cs = ClickState::new();
        cs.terminal_cols = 80;
        cs.terminal_rows = 30;

        cs.add_click_target(Rect::new(0, 11, 80, 1), 1);
        cs.add_click_target(Rect::new(0, 12, 80, 1), 2);
        cs.add_click_target(Rect::new(0, 13, 80, 1), 3);

        for row in 27..30 {
            cs.add_click_target(Rect::new(0, row, 80, 1), 99);
        }

        let grid_height = 450.0;
        let cell_height = grid_height / 30.0;

        let click_y = 11.0 * cell_height + 7.0;
        let row = pixel_y_to_row(click_y, grid_height, cs.terminal_rows).unwrap();
        assert_eq!(row, 11);
        assert_eq!(cs.hit_test(0, row), Some(1));

        let click_y = 28.0 * cell_height + 10.0;
        let row = pixel_y_to_row(click_y, grid_height, cs.terminal_rows).unwrap();
        assert_eq!(cs.hit_test(0, row), Some(99));
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

    // ── register_tab_targets tests ────────────────────────────────

    #[test]
    fn tab_targets_equal_width_labels() {
        // 3 tabs, each label 6 cols wide, separator 3 cols (" │ ")
        // Layout: [6][3][6][3][6] = 24 cols of content in 80-wide area
        let mut cs = ClickState::new();
        let tabs: Vec<(u16, u16)> = vec![(6, 10), (6, 11), (6, 12)];
        cs.register_tab_targets(&tabs, 3, 0, 5, 80, 1);

        assert_eq!(cs.targets.len(), 3);

        // Tab 0: left=0, right = 6 + (9-6)/2 = 7 → cols 0..7
        assert_eq!(cs.hit_test(0, 5), Some(10));
        assert_eq!(cs.hit_test(5, 5), Some(10)); // inside label
        assert_eq!(cs.hit_test(6, 5), Some(10)); // first separator col → tab 0

        // Tab 1: left = 6 + (9-6)/2 = 7, right = 15 + (18-15)/2 = 16 → cols 7..16
        assert_eq!(cs.hit_test(7, 5), Some(11));
        assert_eq!(cs.hit_test(14, 5), Some(11)); // inside label
        assert_eq!(cs.hit_test(15, 5), Some(11)); // separator col → tab 1

        // Tab 2: left = 15 + (18-15)/2 = 16, right = 80 (last tab) → cols 16..80
        assert_eq!(cs.hit_test(16, 5), Some(12));
        assert_eq!(cs.hit_test(23, 5), Some(12)); // inside label
        assert_eq!(cs.hit_test(79, 5), Some(12)); // last tab extends to edge
    }

    #[test]
    fn tab_targets_unequal_width_labels() {
        // Simulates dynamic labels: "生産"(6), "目標(3)"(11), "転生(+5)"(12)
        // Separator "|" = 1 col
        // Layout: [6][1][11][1][12] = 31 cols
        let mut cs = ClickState::new();
        let tabs: Vec<(u16, u16)> = vec![(6, 10), (11, 11), (12, 12)];
        cs.register_tab_targets(&tabs, 1, 0, 0, 60, 1);

        assert_eq!(cs.targets.len(), 3);

        // Tab 0: left=0, right = 6 + (7-6)/2 = 6 → cols 0..6
        assert_eq!(cs.hit_test(0, 0), Some(10));
        assert_eq!(cs.hit_test(5, 0), Some(10));

        // Tab 1: left = 6, right = 18 + (19-18)/2 = 18 → cols 6..18
        assert_eq!(cs.hit_test(6, 0), Some(11));
        assert_eq!(cs.hit_test(17, 0), Some(11));

        // Tab 2: left = 18, right = 60 → cols 18..60
        assert_eq!(cs.hit_test(18, 0), Some(12));
        assert_eq!(cs.hit_test(59, 0), Some(12));
    }

    #[test]
    fn tab_targets_single_tab() {
        let mut cs = ClickState::new();
        let tabs: Vec<(u16, u16)> = vec![(8, 42)];
        cs.register_tab_targets(&tabs, 3, 5, 10, 40, 1);

        assert_eq!(cs.targets.len(), 1);
        // Single tab covers full width
        assert_eq!(cs.hit_test(5, 10), Some(42));
        assert_eq!(cs.hit_test(44, 10), Some(42));
    }

    #[test]
    fn tab_targets_empty() {
        let mut cs = ClickState::new();
        let tabs: Vec<(u16, u16)> = vec![];
        cs.register_tab_targets(&tabs, 3, 0, 0, 80, 1);
        assert_eq!(cs.targets.len(), 0);
    }

    #[test]
    fn tab_targets_with_offset() {
        // Tab bar starting at x=5 (e.g. inside a bordered block)
        let mut cs = ClickState::new();
        let tabs: Vec<(u16, u16)> = vec![(6, 10), (6, 11)];
        cs.register_tab_targets(&tabs, 1, 5, 3, 30, 2);

        assert_eq!(cs.targets.len(), 2);
        // Tab 0: x=5, y=3, height=2
        assert_eq!(cs.hit_test(5, 3), Some(10));
        assert_eq!(cs.hit_test(5, 4), Some(10)); // height=2
        assert_eq!(cs.hit_test(4, 3), None);     // before x offset
    }

    #[test]
    fn mobile_narrow_click_pipeline() {
        let mut cs = ClickState::new();
        cs.terminal_cols = 37;
        cs.terminal_rows = 50;

        cs.add_click_target(Rect::new(0, 9, 37, 1), 1);
        cs.add_click_target(Rect::new(0, 10, 37, 1), 2);
        cs.add_click_target(Rect::new(0, 11, 37, 1), 3);

        for row in 47..50 {
            cs.add_click_target(Rect::new(0, row, 37, 1), 99);
        }

        let grid_height = 50.0 * 15.0;

        fn assert_tap_hits(cs: &ClickState, grid_height: f64, target_row: u16, expected_id: u16) {
            let cell_height = grid_height / cs.terminal_rows as f64;
            let click_y = target_row as f64 * cell_height + cell_height / 2.0;
            let row = pixel_y_to_row(click_y, grid_height, cs.terminal_rows);
            assert_eq!(row, Some(target_row));
            assert_eq!(cs.hit_test(0, target_row), Some(expected_id));
        }

        assert_tap_hits(&cs, grid_height, 9, 1);
        assert_tap_hits(&cs, grid_height, 10, 2);
        assert_tap_hits(&cs, grid_height, 11, 3);
        assert_tap_hits(&cs, grid_height, 48, 99);
    }
}
