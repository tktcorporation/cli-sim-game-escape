//! Shared input handling: coordinate conversion, click targets, and event types.
//!
//! This module is game-agnostic. Each game implements its own input dispatch.

use ratzilla::ratatui::layout::Rect;
use ratzilla::ratatui::text::Line;

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

/// A builder that pairs rendered [`Line`]s with click actions.
///
/// Instead of manually calculating row offsets for click targets, use this
/// builder to annotate lines as clickable when you add them.  Then call
/// [`register_targets`](ClickableList::register_targets) once after rendering
/// to register all targets at the correct rows automatically.
///
/// # Example
/// ```ignore
/// let mut cl = ClickableList::new();
/// cl.push(Line::from("Header (not clickable)"));
/// cl.push_clickable(Line::from("Buy item"), BUY_ITEM_ACTION);
/// // ... render Paragraph::new(cl.into_lines()) ...
/// // ... cl.register_targets(area, &mut cs, 1, 1, 0) ...
/// ```
pub struct ClickableList<'a> {
    lines: Vec<Line<'a>>,
    /// `(line_index, action_id)` pairs — line_index is the index into `lines`.
    actions: Vec<(u16, u16)>,
}

impl<'a> ClickableList<'a> {
    pub fn new() -> Self {
        Self {
            lines: Vec::new(),
            actions: Vec::new(),
        }
    }

    /// Add a non-clickable line.
    pub fn push(&mut self, line: Line<'a>) {
        self.lines.push(line);
    }

    /// Add a clickable line with a semantic action ID.
    ///
    /// The action is bound to whatever row this line ends up on — if you
    /// insert or remove lines before it, the target moves automatically.
    pub fn push_clickable(&mut self, line: Line<'a>, action_id: u16) {
        let idx = self.lines.len() as u16;
        self.actions.push((idx, action_id));
        self.lines.push(line);
    }

    /// Total number of lines.
    pub fn len(&self) -> usize {
        self.lines.len()
    }

    /// Borrow the lines (e.g. for computing wrapped-line estimates before consuming).
    pub fn lines(&self) -> &[Line<'a>] {
        &self.lines
    }

    /// Consume the builder, returning the lines for rendering.
    pub fn into_lines(self) -> Vec<Line<'a>> {
        self.lines
    }

    /// Register click targets for all clickable lines.
    ///
    /// * `area` — the widget area (including borders).
    /// * `cs` — mutable reference to the shared click state.
    /// * `top_offset` — rows before content (e.g. 1 for a top border).
    /// * `bottom_offset` — rows after content (e.g. 1 for a bottom border).
    /// * `scroll` — vertical scroll offset (0 if not scrollable).
    pub fn register_targets(
        &self,
        area: Rect,
        cs: &mut ClickState,
        top_offset: u16,
        bottom_offset: u16,
        scroll: u16,
    ) {
        let content_y = area.y + top_offset;
        let content_end = area.y + area.height.saturating_sub(bottom_offset);

        for &(line_idx, action_id) in &self.actions {
            if line_idx < scroll {
                continue;
            }
            let row = content_y + (line_idx - scroll);
            if row >= content_end {
                continue;
            }
            cs.add_row_target(area, row, action_id);
        }
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

    // ── ClickableList tests ────────────────────────────────────────

    #[test]
    fn clickable_list_basic() {
        let mut cl = ClickableList::new();
        cl.push(Line::from("header"));
        cl.push_clickable(Line::from("item 0"), 10);
        cl.push_clickable(Line::from("item 1"), 11);
        cl.push(Line::from("footer"));

        assert_eq!(cl.len(), 4);

        // area with Borders::ALL → top_offset=1, bottom_offset=1
        let area = Rect::new(0, 5, 80, 10);
        let mut cs = ClickState::new();
        cl.register_targets(area, &mut cs, 1, 1, 0);

        // "header" is line 0, not clickable
        // "item 0" is line 1 → row = 5 + 1 + 1 = 7
        // "item 1" is line 2 → row = 5 + 1 + 2 = 8
        assert_eq!(cs.targets.len(), 2);
        assert_eq!(cs.hit_test(10, 7), Some(10));
        assert_eq!(cs.hit_test(10, 8), Some(11));
        // header row and footer rows should not match
        assert_eq!(cs.hit_test(10, 6), None);
        assert_eq!(cs.hit_test(10, 9), None);
    }

    #[test]
    fn clickable_list_with_scroll() {
        let mut cl = ClickableList::new();
        cl.push_clickable(Line::from("item 0"), 100);
        cl.push_clickable(Line::from("item 1"), 101);
        cl.push_clickable(Line::from("item 2"), 102);
        cl.push_clickable(Line::from("item 3"), 103);

        // Area: no top border, 1 bottom border (like prestige sections)
        let area = Rect::new(0, 10, 80, 5);
        let mut cs = ClickState::new();
        // scroll=2: items 0 and 1 are scrolled out of view
        cl.register_targets(area, &mut cs, 0, 1, 2);

        // item 2 (line_idx=2) → row = 10 + 0 + (2-2) = 10
        // item 3 (line_idx=3) → row = 10 + 0 + (3-2) = 11
        assert_eq!(cs.targets.len(), 2);
        assert_eq!(cs.hit_test(10, 10), Some(102));
        assert_eq!(cs.hit_test(10, 11), Some(103));
        // scrolled items should not register
        assert_eq!(cs.hit_test(10, 8), None);
        assert_eq!(cs.hit_test(10, 9), None);
    }

    #[test]
    fn clickable_list_clipped_by_area() {
        let mut cl = ClickableList::new();
        for i in 0..20 {
            cl.push_clickable(Line::from(format!("item {}", i)), 50 + i as u16);
        }

        // Small area with borders: only 3 content rows (height=5, border top+bottom)
        let area = Rect::new(0, 0, 80, 5);
        let mut cs = ClickState::new();
        cl.register_targets(area, &mut cs, 1, 1, 0);

        // content rows: y=1, y=2, y=3 (3 rows)
        assert_eq!(cs.targets.len(), 3);
        assert_eq!(cs.hit_test(10, 1), Some(50)); // item 0
        assert_eq!(cs.hit_test(10, 2), Some(51)); // item 1
        assert_eq!(cs.hit_test(10, 3), Some(52)); // item 2
        assert_eq!(cs.hit_test(10, 4), None);     // clipped by bottom border
    }

    #[test]
    fn clickable_list_empty() {
        let cl: ClickableList = ClickableList::new();
        assert_eq!(cl.len(), 0);

        let area = Rect::new(0, 0, 80, 10);
        let mut cs = ClickState::new();
        cl.register_targets(area, &mut cs, 1, 1, 0);
        assert_eq!(cs.targets.len(), 0);
    }

    #[test]
    fn clickable_list_into_lines() {
        let mut cl = ClickableList::new();
        cl.push(Line::from("a"));
        cl.push_clickable(Line::from("b"), 1);
        cl.push(Line::from("c"));

        let lines = cl.into_lines();
        assert_eq!(lines.len(), 3);
    }

    #[test]
    fn clickable_list_insert_line_shifts_targets() {
        // Demonstrates the key advantage: inserting a non-clickable line
        // before clickable items automatically adjusts their rows.
        let mut cl = ClickableList::new();
        cl.push(Line::from("header 1"));
        cl.push(Line::from("header 2")); // extra header
        cl.push_clickable(Line::from("buy item"), 42);

        let area = Rect::new(0, 0, 80, 10);
        let mut cs = ClickState::new();
        cl.register_targets(area, &mut cs, 1, 1, 0);

        // "buy item" is line 2 → row = 0 + 1 + 2 = 3
        assert_eq!(cs.hit_test(10, 3), Some(42));
        assert_eq!(cs.hit_test(10, 2), None); // header 2, not clickable
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
