//! Reusable clickable UI components.
//!
//! Each component encapsulates both rendering and click target registration,
//! following a component-oriented pattern where visual output and interactive
//! behaviour are co-located.
//!
//! # Components
//!
//! - [`TabBar`] — Horizontal tab navigation (rendering + click targets).
//! - [`ClickableList`] — Vertical list with per-row click targets.
//! - [`ClickableGrid`] — 2D grid with per-cell click targets.
//!
//! These builders are the **only** sanctioned way to register click targets.
//! Direct calls to `ClickState::add_click_target` / `add_row_target` are
//! banned by clippy (see `clippy.toml`).
#![allow(clippy::disallowed_methods)]

use ratzilla::ratatui::layout::Rect;
use ratzilla::ratatui::style::{Color, Style};
use ratzilla::ratatui::text::{Line, Span};
use ratzilla::ratatui::widgets::{Block, Paragraph, Wrap};
use ratzilla::ratatui::Frame;

use crate::input::ClickState;

// ── TabBar ─────────────────────────────────────────────────────

/// A horizontal tab bar component.
///
/// Renders tabs as a single row of styled labels separated by a configurable
/// separator string, and registers click targets that match the actual
/// rendered positions (accounting for CJK character widths and dynamic labels).
///
/// # Example
/// ```ignore
/// TabBar::new(" │ ")
///     .tab("生産", tab_style(0), TAB_PRODUCERS)
///     .tab("強化", tab_style(1), TAB_UPGRADES)
///     .render(f, area, &mut cs);
/// ```
pub struct TabBar<'a> {
    tabs: Vec<(String, Style, u16)>,
    separator: &'a str,
    block: Option<Block<'a>>,
}

impl<'a> TabBar<'a> {
    pub fn new(separator: &'a str) -> Self {
        Self {
            tabs: Vec::new(),
            separator,
            block: None,
        }
    }

    /// Add a tab with its label, style, and action ID.
    pub fn tab(mut self, label: impl Into<String>, style: Style, action_id: u16) -> Self {
        self.tabs.push((label.into(), style, action_id));
        self
    }

    /// Wrap the tab bar in a [`Block`].
    ///
    /// When a block with borders is provided, click target positions are
    /// automatically adjusted using `Block::inner()`.
    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }

    /// Render the tab bar and register click targets.
    pub fn render(self, f: &mut Frame, area: Rect, cs: &mut ClickState) {
        let mut spans: Vec<Span> = Vec::new();
        let sep_width = Line::from(self.separator).width() as u16;
        let mut tab_widths: Vec<(u16, u16)> = Vec::new();

        for (i, (label, style, action_id)) in self.tabs.iter().enumerate() {
            if i > 0 {
                spans.push(Span::styled(
                    self.separator,
                    Style::default().fg(Color::DarkGray),
                ));
            }
            let padded = format!(" {} ", label);
            tab_widths.push((Line::from(padded.as_str()).width() as u16, *action_id));
            spans.push(Span::styled(padded, *style));
        }

        // Compute inner content area (accounting for borders) before consuming block
        let inner = match &self.block {
            Some(block) => block.inner(area),
            None => area,
        };

        let line = Line::from(spans);
        let paragraph = match self.block {
            Some(block) => Paragraph::new(line).block(block),
            None => Paragraph::new(line),
        };
        f.render_widget(paragraph, area);

        // Use inner x/width for horizontal accuracy,
        // outer y/height for better tap tolerance on the full tab bar
        cs.register_tab_targets(
            &tab_widths,
            sep_width,
            inner.x,
            area.y,
            inner.width,
            area.height.max(1),
        );
    }
}

// ── ClickableList ──────────────────────────────────────────────

/// A builder that pairs rendered [`Line`]s with click actions.
///
/// Instead of manually calculating row offsets for click targets, use this
/// builder to annotate lines as clickable when you add them.
///
/// **Preferred API**: Use [`render()`](ClickableList::render) to atomically
/// register click targets and render the `Paragraph`.  This guarantees that
/// wrap mode, block offsets, and scroll position are always consistent,
/// eliminating tap-offset bugs.
///
/// Use [`register_targets()`](ClickableList::register_targets) only when
/// the widget is not a `Paragraph` (e.g. `List`).
///
/// # Example (recommended)
/// ```ignore
/// let mut cl = ClickableList::new();
/// cl.push(Line::from("Header (not clickable)"));
/// cl.push_clickable(Line::from("Buy item"), BUY_ITEM_ACTION);
/// let block = Block::default().borders(Borders::ALL);
/// cl.render(f, area, block, &mut cs, true, 0);
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
    /// * `scroll` — vertical scroll offset in visual rows (0 if not scrollable).
    /// * `inner_width` — content width for wrap calculation. Pass `0` when the
    ///   widget does **not** use `Wrap`, in which case each logical line is
    ///   assumed to occupy exactly one visual row.
    pub fn register_targets(
        &self,
        area: Rect,
        cs: &mut ClickState,
        top_offset: u16,
        bottom_offset: u16,
        scroll: u16,
        inner_width: u16,
    ) {
        let content_y = area.y + top_offset;
        let content_end = area.y + area.height.saturating_sub(bottom_offset);

        if inner_width == 0 {
            // Legacy path: 1 logical line = 1 visual row (no wrapping).
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
            return;
        }

        // Wrap-aware path: compute the visual row offset for each logical line.
        let w = inner_width as usize;
        let mut visual_starts: Vec<u16> = Vec::with_capacity(self.lines.len());
        let mut visual_heights: Vec<u16> = Vec::with_capacity(self.lines.len());
        let mut cumulative: u16 = 0;
        for line in &self.lines {
            visual_starts.push(cumulative);
            let lw = line.width();
            let h = if lw <= w { 1 } else { lw.div_ceil(w) as u16 };
            visual_heights.push(h);
            cumulative += h;
        }

        for &(line_idx, action_id) in &self.actions {
            let li = line_idx as usize;
            if li >= self.lines.len() {
                continue;
            }
            let vstart = visual_starts[li];
            let vheight = visual_heights[li];

            // Register a click target for every visual row this line spans.
            for r in 0..vheight {
                let vr = vstart + r;
                if vr < scroll {
                    continue;
                }
                let screen_row = content_y + (vr - scroll);
                if screen_row >= content_end {
                    break;
                }
                cs.add_row_target(area, screen_row, action_id);
            }
        }
    }

    /// Register click targets using a [`Block`] to automatically derive offsets.
    ///
    /// This eliminates the need to manually calculate `top_offset` and
    /// `bottom_offset` — they are computed from the block's borders.
    ///
    /// * `area` — the widget area (including borders).
    /// * `block` — the block wrapping the content (used to compute inner area).
    /// * `cs` — mutable reference to the shared click state.
    /// * `scroll` — vertical scroll offset in visual rows (0 if not scrollable).
    /// * `inner_width` — content width for wrap calculation. Pass `0` when the
    ///   widget does **not** use `Wrap`.
    #[cfg(test)]
    pub fn register_targets_with_block(
        &self,
        area: Rect,
        block: &Block,
        cs: &mut ClickState,
        scroll: u16,
        inner_width: u16,
    ) {
        let inner = block.inner(area);
        let top_offset = inner.y.saturating_sub(area.y);
        let bottom_offset = (area.y + area.height).saturating_sub(inner.y + inner.height);
        self.register_targets(area, cs, top_offset, bottom_offset, scroll, inner_width);
    }

    /// Render the list as a [`Paragraph`] and register click targets atomically.
    ///
    /// This is the **preferred API** for rendering a `ClickableList`.  It
    /// guarantees that the wrap mode, block offsets, scroll position, and
    /// content area are **identical** between the visual output and the
    /// hit-test regions, eliminating an entire class of tap-offset bugs.
    ///
    /// * `f` — the frame to render into.
    /// * `area` — the widget area (including borders).
    /// * `block` — the block wrapping the content.
    /// * `cs` — mutable reference to the shared click state.
    /// * `wrap` — whether to enable text wrapping (`Wrap { trim: false }`).
    /// * `scroll` — vertical scroll offset in visual rows (0 if not scrollable).
    pub fn render(
        self,
        f: &mut Frame,
        area: Rect,
        block: Block<'a>,
        cs: &mut ClickState,
        wrap: bool,
        scroll: u16,
    ) {
        let inner = block.inner(area);
        let top_offset = inner.y.saturating_sub(area.y);
        let bottom_offset = (area.y + area.height).saturating_sub(inner.y + inner.height);
        let inner_width = if wrap { inner.width } else { 0 };
        self.register_targets(area, cs, top_offset, bottom_offset, scroll, inner_width);

        let paragraph = Paragraph::new(self.into_lines()).block(block);
        let paragraph = if wrap {
            paragraph.wrap(Wrap { trim: false })
        } else {
            paragraph
        };
        let paragraph = if scroll > 0 {
            paragraph.scroll((scroll, 0))
        } else {
            paragraph
        };
        f.render_widget(paragraph, area);
    }
}

// ── ClickableGrid ─────────────────────────────────────────────

/// A builder for 2D grids with per-cell click targets.
///
/// Encapsulates the coordinate→action_id mapping so that render and input
/// use the same formula.  The action ID for cell `(col, row)` in the
/// viewport is `base + row * view_w + col`.
///
/// # Example
/// ```ignore
/// let mut grid = ClickableGrid::new(VIEW_W, VIEW_H, GRID_CLICK_BASE, 2);
/// // ... render cells ...
/// grid.register_targets(area, &block, &mut cs);
/// ```
pub struct ClickableGrid {
    view_w: usize,
    view_h: usize,
    action_base: u16,
    /// Display width of each cell in terminal columns.
    cell_display_width: u16,
    /// Display height of each cell in terminal rows.
    cell_display_height: u16,
}

impl ClickableGrid {
    pub fn new(view_w: usize, view_h: usize, action_base: u16, cell_display_width: u16) -> Self {
        Self {
            view_w,
            view_h,
            action_base,
            cell_display_width,
            cell_display_height: 1,
        }
    }

    /// Create a grid with multi-row cells.
    pub fn with_cell_height(mut self, h: u16) -> Self {
        self.cell_display_height = h;
        self
    }

    /// Decode an action_id back into viewport-relative `(col, row)`.
    ///
    /// Returns `None` if the action_id is not in the grid range.
    pub fn decode(action_base: u16, view_w: usize, action_id: u16) -> Option<(usize, usize)> {
        if action_id < action_base {
            return None;
        }
        let offset = (action_id - action_base) as usize;
        let row = offset / view_w;
        let col = offset % view_w;
        Some((col, row))
    }

    /// Register click targets for all cells in the viewport.
    ///
    /// * `area` — the widget area (including borders).
    /// * `block` — the block wrapping the grid (used to compute inner area).
    /// * `cs` — mutable reference to the shared click state.
    /// * `padding_left` — left padding columns before grid content starts.
    pub fn register_targets(
        &self,
        area: Rect,
        block: &Block,
        cs: &mut ClickState,
        padding_left: u16,
    ) {
        let inner = block.inner(area);
        for gy in 0..self.view_h {
            for gx in 0..self.view_w {
                let term_col = inner.x + padding_left + gx as u16 * self.cell_display_width;
                let term_row = inner.y + gy as u16 * self.cell_display_height;
                let action_id = self.action_base + (gy * self.view_w + gx) as u16;
                cs.add_click_target(
                    Rect::new(
                        term_col,
                        term_row,
                        self.cell_display_width,
                        self.cell_display_height,
                    ),
                    action_id,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::ClickState;

    // ── TabBar tests ───────────────────────────────────────────

    #[test]
    fn tab_bar_registers_targets_based_on_text_width() {
        // Verify that TabBar internally produces correct tab_widths and
        // delegates to register_tab_targets (tested separately in input.rs).
        // Here we just check the high-level behaviour: that targets are
        // registered for each tab.
        let mut cs = ClickState::new();
        // We can't call render() without a Frame, so test via register_tab_targets
        // which TabBar delegates to.
        let tabs: Vec<(u16, u16)> = vec![(6, 10), (6, 11), (6, 12)];
        cs.register_tab_targets(&tabs, 3, 0, 0, 80, 1);
        assert_eq!(cs.targets.len(), 3);
    }

    // ── ClickableList tests ────────────────────────────────────

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
        cl.register_targets(area, &mut cs, 1, 1, 0, 0);

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
        cl.register_targets(area, &mut cs, 0, 1, 2, 0);

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
        cl.register_targets(area, &mut cs, 1, 1, 0, 0);

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
        cl.register_targets(area, &mut cs, 1, 1, 0, 0);
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
        cl.register_targets(area, &mut cs, 1, 1, 0, 0);

        // "buy item" is line 2 → row = 0 + 1 + 2 = 3
        assert_eq!(cs.hit_test(10, 3), Some(42));
        assert_eq!(cs.hit_test(10, 2), None); // header 2, not clickable
    }

    #[test]
    fn clickable_list_wrap_aware_targets() {
        // When inner_width is specified, lines wider than inner_width occupy
        // multiple visual rows and push subsequent targets down.
        let mut cl = ClickableList::new();
        // Line 0: 20 chars, fits in 10-wide → 2 visual rows when wrapped
        cl.push(Line::from("12345678901234567890"));
        // Line 1: clickable, 5 chars, fits in 1 row
        cl.push_clickable(Line::from("item0"), 10);

        // area: y=0, height=10, no borders
        let area = Rect::new(0, 0, 12, 10); // inner_width = 12 - 2 = 10
        let mut cs = ClickState::new();
        cl.register_targets(area, &mut cs, 0, 0, 0, 10);

        // Line 0 wraps to 2 visual rows (row 0, row 1)
        // Line 1 starts at visual row 2
        assert_eq!(cs.hit_test(5, 2), Some(10));
        assert_eq!(cs.hit_test(5, 0), None); // header row 1
        assert_eq!(cs.hit_test(5, 1), None); // header row 2 (wrapped)
    }

    #[test]
    fn clickable_list_wrap_covers_all_rows() {
        // A clickable line that wraps should be clickable on all its visual rows.
        let mut cl = ClickableList::new();
        // 30 chars wide, wraps to 3 rows in 10-wide area
        cl.push_clickable(Line::from("123456789012345678901234567890"), 42);

        let area = Rect::new(0, 0, 12, 10);
        let mut cs = ClickState::new();
        cl.register_targets(area, &mut cs, 0, 0, 0, 10);

        // All 3 visual rows should be clickable
        assert_eq!(cs.hit_test(5, 0), Some(42));
        assert_eq!(cs.hit_test(5, 1), Some(42));
        assert_eq!(cs.hit_test(5, 2), Some(42));
        assert_eq!(cs.hit_test(5, 3), None);
    }

    #[test]
    fn clickable_list_wrap_with_scroll() {
        let mut cl = ClickableList::new();
        // Line 0: 20 chars → 2 visual rows in 10-wide
        cl.push_clickable(Line::from("12345678901234567890"), 10);
        // Line 1: 5 chars → 1 visual row
        cl.push_clickable(Line::from("item1"), 11);

        let area = Rect::new(0, 0, 12, 10);
        let mut cs = ClickState::new();
        // scroll=1: skip first visual row
        cl.register_targets(area, &mut cs, 0, 0, 1, 10);

        // Line 0 row 0 scrolled out, row 1 at screen row 0
        assert_eq!(cs.hit_test(5, 0), Some(10));
        // Line 1 at visual row 2, screen row = 2-1 = 1
        assert_eq!(cs.hit_test(5, 1), Some(11));
    }

    // ── register_targets_with_block tests ─────────────────────

    #[test]
    fn register_targets_with_block_borders_all() {
        let mut cl = ClickableList::new();
        cl.push_clickable(Line::from("item 0"), 10);
        cl.push_clickable(Line::from("item 1"), 11);

        let block = Block::default().borders(ratzilla::ratatui::widgets::Borders::ALL);
        let area = Rect::new(0, 5, 80, 10);
        let mut cs = ClickState::new();
        cl.register_targets_with_block(area, &block, &mut cs, 0, 0);

        // Borders::ALL → top_offset=1, bottom_offset=1
        // item 0 → row 6, item 1 → row 7
        assert_eq!(cs.hit_test(10, 6), Some(10));
        assert_eq!(cs.hit_test(10, 7), Some(11));
        assert_eq!(cs.hit_test(10, 5), None); // top border
    }

    #[test]
    fn register_targets_with_block_no_borders() {
        let mut cl = ClickableList::new();
        cl.push_clickable(Line::from("item 0"), 10);

        let block = Block::default();
        let area = Rect::new(0, 0, 80, 10);
        let mut cs = ClickState::new();
        cl.register_targets_with_block(area, &block, &mut cs, 0, 0);

        // No borders → top_offset=0
        assert_eq!(cs.hit_test(10, 0), Some(10));
    }

    // ── ClickableGrid tests ───────────────────────────────────

    #[test]
    fn clickable_grid_register_targets() {
        let grid = ClickableGrid::new(3, 2, 100, 2);
        let block = Block::default().borders(ratzilla::ratatui::widgets::Borders::ALL);
        let area = Rect::new(0, 0, 20, 10);
        let mut cs = ClickState::new();
        grid.register_targets(area, &block, &mut cs, 1);

        // 3×2 grid = 6 targets
        assert_eq!(cs.targets.len(), 6);

        // inner = (1, 1, 18, 8) with Borders::ALL
        // cell (0,0): term_col = 1 + 1 + 0*2 = 2, term_row = 1 + 0 = 1, action_id = 100
        assert_eq!(cs.hit_test(2, 1), Some(100));
        // cell (1,0): term_col = 1 + 1 + 1*2 = 4, action_id = 101
        assert_eq!(cs.hit_test(4, 1), Some(101));
        // cell (0,1): term_row = 1 + 1 = 2, action_id = 103
        assert_eq!(cs.hit_test(2, 2), Some(103));
    }

    #[test]
    fn clickable_grid_decode() {
        let view_w = 5;
        let base = 100;

        // cell (2, 3) → action_id = 100 + 3*5 + 2 = 117
        assert_eq!(ClickableGrid::decode(base, view_w, 117), Some((2, 3)));
        // cell (0, 0) → action_id = 100
        assert_eq!(ClickableGrid::decode(base, view_w, 100), Some((0, 0)));
        // Below base → None
        assert_eq!(ClickableGrid::decode(base, view_w, 99), None);
    }
}
