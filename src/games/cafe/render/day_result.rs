//! Day result rendering.

use std::cell::RefCell;
use std::rc::Rc;

use ratzilla::ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratzilla::ratatui::style::{Color, Modifier, Style};
use ratzilla::ratatui::text::{Line, Span};
use ratzilla::ratatui::widgets::{Block, Borders, Paragraph};
use ratzilla::ratatui::Frame;

use crate::input::ClickState;
use crate::widgets::ClickableList;

use super::super::actions::*;
use super::super::social_sys::STAMINA_MAX;
use super::super::state::CafeState;

pub(super) fn render_day_result(
    state: &CafeState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // title
            Constraint::Min(5),   // results
            Constraint::Length(3), // next button
        ])
        .split(area);

    // ── Title ──
    let title = Paragraph::new(Line::from(Span::styled(
        format!(" Day {} — 営業結果", state.day),
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    )))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow)),
    );
    f.render_widget(title, chunks[0]);

    // ── Visit log ──
    let mut cl = ClickableList::new();
    cl.push(Line::from(""));

    for visit in &state.today_visits {
        let status = if visit.satisfied { "😊" } else { "😐" };
        cl.push(Line::from(vec![
            Span::styled(
                format!(" {status} {}", visit.name),
                Style::default().fg(Color::White),
            ),
            Span::styled(
                format!(" → {}", visit.order),
                Style::default().fg(Color::Cyan),
            ),
            Span::styled(
                format!("  +¥{}", visit.revenue),
                Style::default().fg(Color::Green),
            ),
        ]));
    }

    cl.push(Line::from(""));

    // Revenue summary
    cl.push(Line::from(Span::styled(
        format!(
            " 売上: ¥{} │ 経費: ¥{} │ 利益: ¥{}",
            state.today_revenue(),
            state.today_cost(),
            state.today_revenue() as i64 - state.today_cost() as i64
        ),
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    )));

    // Stamina remaining
    cl.push(Line::from(Span::styled(
        format!(" 残り予算: {}/{}", state.stamina.current, STAMINA_MAX),
        Style::default().fg(Color::Cyan),
    )));

    let result_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Green))
        .title(" 来客ログ ");
    {
        let mut cs = click_state.borrow_mut();
        cl.render(f, chunks[1], result_block, &mut cs, false, 0);
    }

    // ── Next day button ──
    let mut next_cl = ClickableList::new();
    next_cl.push_clickable(
        Line::from(Span::styled(
            " ▶ 次の日へ",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        )),
        SERVE_CONFIRM, // reuse ID for "next" action
    );
    let next_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Green));
    {
        let mut cs = click_state.borrow_mut();
        next_cl.render(f, chunks[2], next_block, &mut cs, false, 0);
    }
}
