//! Idle Metropolis rendering.
//!
//! Layout intent:
//!   ┌── Header ─────────────────────────────────┐
//!   │ city grid (24×12)        │ status panel    │
//!   │                          │ buttons         │
//!   │                          │ AI thought log  │
//!   └──────────────────────────┴─────────────────┘
//!
//! The grid itself is read-only — the player never clicks on cells; that's
//! the AI's job.  All click targets live in the side panel and are
//! registered via the `Clickable` widget so the project's
//! widgets-only-clicks rule is honored.

use std::cell::RefCell;
use std::rc::Rc;

use ratzilla::ratatui::layout::{Constraint, Direction as LayoutDir, Layout, Rect};
use ratzilla::ratatui::style::{Color, Modifier, Style};
use ratzilla::ratatui::text::{Line, Span};
use ratzilla::ratatui::widgets::{Block, Borders, Paragraph};
use ratzilla::ratatui::Frame;

use crate::input::{is_narrow_layout, ClickState};
use crate::widgets::Clickable;

use super::logic;
use super::state::{Building, City, Strategy, Tile, GRID_H, GRID_W};
use super::{
    ACT_HIRE_WORKER, ACT_STRATEGY_BALANCED, ACT_STRATEGY_GROWTH, ACT_STRATEGY_INCOME,
    ACT_UPGRADE_AI,
};

pub fn render(state: &City, f: &mut Frame, area: Rect, click_state: &Rc<RefCell<ClickState>>) {
    if is_narrow_layout(area.width) {
        render_narrow(state, f, area, click_state);
    } else {
        render_wide(state, f, area, click_state);
    }
}

fn render_wide(state: &City, f: &mut Frame, area: Rect, click_state: &Rc<RefCell<ClickState>>) {
    let h = Layout::default()
        .direction(LayoutDir::Horizontal)
        .constraints([Constraint::Length(GRID_W as u16 + 2), Constraint::Min(28)])
        .split(area);

    let left = Layout::default()
        .direction(LayoutDir::Vertical)
        .constraints([Constraint::Length(3), Constraint::Length(GRID_H as u16 + 2)])
        .split(h[0]);

    let right = Layout::default()
        .direction(LayoutDir::Vertical)
        .constraints([
            Constraint::Length(6),  // status
            Constraint::Length(10), // buttons
            Constraint::Min(5),     // log
        ])
        .split(h[1]);

    render_header(state, f, left[0]);
    render_grid(state, f, left[1]);
    render_status(state, f, right[0]);
    render_buttons(state, f, right[1], click_state);
    render_log(state, f, right[2]);
}

fn render_narrow(state: &City, f: &mut Frame, area: Rect, click_state: &Rc<RefCell<ClickState>>) {
    let chunks = Layout::default()
        .direction(LayoutDir::Vertical)
        .constraints([
            Constraint::Length(3),                   // header
            Constraint::Length(GRID_H as u16 + 2),   // grid
            Constraint::Length(6),                   // status
            Constraint::Length(10),                  // buttons
            Constraint::Min(4),                      // log
        ])
        .split(area);
    render_header(state, f, chunks[0]);
    render_grid(state, f, chunks[1]);
    render_status(state, f, chunks[2]);
    render_buttons(state, f, chunks[3], click_state);
    render_log(state, f, chunks[4]);
}

fn render_header(state: &City, f: &mut Frame, area: Rect) {
    let title = format!(
        " 🏙 Metropolis  CPU: {}  作業員: {}人  戦略: {} ",
        state.ai_tier.name(),
        state.workers,
        strategy_label(state.strategy),
    );
    let p = Paragraph::new(title).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan)),
    );
    f.render_widget(p, area);
}

fn render_grid(state: &City, f: &mut Frame, area: Rect) {
    let mut lines: Vec<Line> = Vec::with_capacity(GRID_H);
    for y in 0..GRID_H {
        let mut spans: Vec<Span> = Vec::with_capacity(GRID_W);
        for x in 0..GRID_W {
            spans.push(tile_span(state.tile(x, y)));
        }
        lines.push(Line::from(spans));
    }
    let p = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(" City "),
    );
    f.render_widget(p, area);
}

fn tile_span(tile: &Tile) -> Span<'static> {
    match tile {
        Tile::Empty => Span::styled(".", Style::default().fg(Color::DarkGray)),
        Tile::Construction { target, .. } => match target {
            Building::Road => Span::styled(",", Style::default().fg(Color::Yellow)),
            Building::House => Span::styled(
                "h",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::DIM),
            ),
            Building::Shop => Span::styled(
                "s",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::DIM),
            ),
        },
        Tile::Built(b) => match b {
            Building::Road => Span::styled("+", Style::default().fg(Color::Gray)),
            Building::House => Span::styled(
                "H",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Building::Shop => Span::styled(
                "S",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
        },
    }
}

fn render_status(state: &City, f: &mut Frame, area: Rect) {
    let income = logic::compute_income_per_sec(state);
    let pop = state.population();
    let active = state.active_constructions();

    let lines = vec![
        Line::from(vec![
            Span::styled("💰 ", Style::default().fg(Color::Yellow)),
            Span::styled(
                format!("${}", state.cash),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled(format!("(+${}/s)", income), Style::default().fg(Color::Green)),
        ]),
        Line::from(vec![
            Span::styled("🏠 人口: ", Style::default().fg(Color::Green)),
            Span::raw(format!("{}", pop)),
            Span::raw("  "),
            Span::styled("🏗 建設中: ", Style::default().fg(Color::Yellow)),
            Span::raw(format!("{}", active)),
        ]),
        Line::from(vec![
            Span::styled("🏗 累計: ", Style::default().fg(Color::DarkGray)),
            Span::raw(format!("{}棟  ", state.buildings_finished)),
            Span::styled("⏱ ", Style::default().fg(Color::DarkGray)),
            Span::raw(format!("{}秒", state.tick / 10)),
        ]),
    ];
    let p = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Status ")
            .border_style(Style::default().fg(Color::Cyan)),
    );
    f.render_widget(p, area);
}

fn render_buttons(state: &City, f: &mut Frame, area: Rect, click_state: &Rc<RefCell<ClickState>>) {
    let inner = Block::default()
        .borders(Borders::ALL)
        .title(" Manager ")
        .border_style(Style::default().fg(Color::Magenta));
    f.render_widget(&inner, area);
    let inner_area = inner.inner(area);

    // 5 rows: strategy×3, hire, upgrade
    let rows = Layout::default()
        .direction(LayoutDir::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(inner_area);

    let mut cs = click_state.borrow_mut();

    // Strategy buttons
    button_row(
        f,
        rows[0],
        &mut cs,
        ACT_STRATEGY_GROWTH,
        "[G] 成長重視",
        state.strategy == Strategy::Growth,
        Color::Green,
    );
    button_row(
        f,
        rows[1],
        &mut cs,
        ACT_STRATEGY_INCOME,
        "[I] 収入重視",
        state.strategy == Strategy::Income,
        Color::Yellow,
    );
    button_row(
        f,
        rows[2],
        &mut cs,
        ACT_STRATEGY_BALANCED,
        "[B] バランス",
        state.strategy == Strategy::Balanced,
        Color::Cyan,
    );

    // Hire worker
    let hire_cost: i64 = 100 * (1i64 << (state.workers - 1));
    let hire_label = format!("[W] 作業員雇用 (${})", hire_cost);
    let hire_color = if state.cash >= hire_cost {
        Color::White
    } else {
        Color::DarkGray
    };
    let p = Paragraph::new(Span::styled(hire_label, Style::default().fg(hire_color)));
    Clickable::new(p, ACT_HIRE_WORKER).render(f, rows[3], &mut cs);

    // Upgrade AI
    let upgrade_label = if let Some(next) = state.ai_tier.next() {
        let color = if state.cash >= next.upgrade_cost() {
            Color::Magenta
        } else {
            Color::DarkGray
        };
        let label = format!("[U] CPU進化 → {} (${})", next.name(), next.upgrade_cost());
        let p = Paragraph::new(Span::styled(label, Style::default().fg(color)));
        Clickable::new(p, ACT_UPGRADE_AI).render(f, rows[4], &mut cs);
        return;
    } else {
        Paragraph::new(Span::styled(
            "[U] CPU最大Tier到達",
            Style::default().fg(Color::DarkGray),
        ))
    };
    f.render_widget(upgrade_label, rows[4]);
}

fn button_row(
    f: &mut Frame,
    area: Rect,
    cs: &mut ClickState,
    action_id: u16,
    label: &str,
    selected: bool,
    accent: Color,
) {
    let style = if selected {
        Style::default()
            .fg(accent)
            .add_modifier(Modifier::BOLD | Modifier::REVERSED)
    } else {
        Style::default().fg(accent)
    };
    let p = Paragraph::new(Span::styled(label.to_string(), style));
    Clickable::new(p, action_id).render(f, area, cs);
}

fn render_log(state: &City, f: &mut Frame, area: Rect) {
    let lines: Vec<Line> = state
        .events
        .iter()
        .map(|e| Line::from(Span::raw(e.clone())))
        .collect();
    let p = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" AI Activity ")
            .border_style(Style::default().fg(Color::DarkGray)),
    );
    f.render_widget(p, area);
}

fn strategy_label(s: Strategy) -> &'static str {
    match s {
        Strategy::Growth => "成長",
        Strategy::Income => "収入",
        Strategy::Balanced => "バランス",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratzilla::ratatui::backend::TestBackend;
    use ratzilla::ratatui::Terminal;

    #[test]
    fn render_does_not_panic_on_empty_city() {
        let city = City::new();
        let mut terminal = Terminal::new(TestBackend::new(80, 30)).unwrap();
        let cs = Rc::new(RefCell::new(ClickState::new()));
        terminal
            .draw(|f| {
                render(&city, f, Rect::new(0, 0, 80, 30), &cs);
            })
            .unwrap();
    }

    #[test]
    fn manager_buttons_register_click_targets() {
        let city = City::new();
        let mut terminal = Terminal::new(TestBackend::new(80, 30)).unwrap();
        let cs = Rc::new(RefCell::new(ClickState::new()));
        terminal
            .draw(|f| {
                render(&city, f, Rect::new(0, 0, 80, 30), &cs);
            })
            .unwrap();
        let registered: Vec<u16> = cs.borrow().targets.iter().map(|t| t.action_id).collect();
        for id in [
            ACT_STRATEGY_GROWTH,
            ACT_STRATEGY_INCOME,
            ACT_STRATEGY_BALANCED,
            ACT_HIRE_WORKER,
            ACT_UPGRADE_AI,
        ] {
            assert!(
                registered.contains(&id),
                "action {} missing from targets {:?}",
                id,
                registered
            );
        }
    }
}
