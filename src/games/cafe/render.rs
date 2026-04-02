//! Rendering for the Café game.
//!
//! Story mode: novel-ADV style text display with speaker names and monologue.
//! Business mode: café management UI with tabs and menu.

use std::cell::RefCell;
use std::rc::Rc;

use ratzilla::ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratzilla::ratatui::style::{Color, Modifier, Style};
use ratzilla::ratatui::text::{Line, Span};
use ratzilla::ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratzilla::ratatui::Frame;

use crate::input::ClickState;
use crate::widgets::ClickableList;

use super::actions::*;
use super::scenario::PROLOGUE_SCENES;
use super::state::{CafeState, GamePhase};

/// Main render dispatcher.
pub fn render(state: &CafeState, f: &mut Frame, area: Rect, click_state: &Rc<RefCell<ClickState>>) {
    match state.phase {
        GamePhase::Story => render_story(state, f, area, click_state),
        GamePhase::Business => render_business(state, f, area, click_state),
        GamePhase::DayResult => render_day_result(state, f, area, click_state),
    }
}

// ═══════════════════════════════════════════════════════════
// Story Mode (Novel ADV)
// ═══════════════════════════════════════════════════════════

fn render_story(
    state: &CafeState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let scene_count = PROLOGUE_SCENES.len();
    if state.current_scene_index >= scene_count {
        return;
    }

    let scene = PROLOGUE_SCENES[state.current_scene_index];
    let line_data = &scene.lines[state.current_line_index];

    // Layout: [title bar] [text area] [prompt]
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2), // scene indicator
            Constraint::Min(5),   // text display
            Constraint::Length(2), // tap prompt
        ])
        .split(area);

    // ── Scene indicator ──
    let scene_label = format!(
        " Ch.0 — 廃墟と最初の一杯  [{}/{}]",
        state.current_line_index + 1,
        scene.lines.len()
    );
    let indicator = Paragraph::new(Line::from(Span::styled(
        scene_label,
        Style::default().fg(Color::DarkGray),
    )));
    f.render_widget(indicator, chunks[0]);

    // ── Text display area ──
    let text_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(Span::styled(
            " 月灯り ",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ));

    let inner = text_block.inner(chunks[1]);
    f.render_widget(text_block, chunks[1]);

    // Build text lines with proper styling
    let mut lines: Vec<Line> = Vec::new();

    // Speaker name (if dialogue)
    if let Some(speaker) = line_data.speaker {
        lines.push(Line::from(Span::styled(
            format!("【{speaker}】"),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));
    }

    // Main text
    let text_style = if line_data.is_monologue {
        Style::default().fg(Color::Gray)
    } else {
        Style::default().fg(Color::White)
    };

    let display_text = if line_data.is_monologue {
        format!("（{}）", line_data.text)
    } else if line_data.speaker.is_some() {
        format!("「{}」", line_data.text)
    } else {
        format!("　{}", line_data.text)
    };

    let text_paragraph = Paragraph::new(Line::from(Span::styled(display_text, text_style)))
        .wrap(Wrap { trim: false });
    f.render_widget(text_paragraph, Rect::new(inner.x, inner.y + lines.len() as u16, inner.width, inner.height.saturating_sub(lines.len() as u16)));

    // Render speaker name lines
    for (i, line) in lines.iter().enumerate() {
        let line_para = Paragraph::new(line.clone());
        if i < inner.height as usize {
            f.render_widget(line_para, Rect::new(inner.x, inner.y + i as u16, inner.width, 1));
        }
    }

    // ── Tap prompt ──
    let mut cl = ClickableList::new();
    cl.push_clickable(
        Line::from(Span::styled(
            "▼ タップで続ける",
            Style::default().fg(Color::DarkGray),
        )),
        STORY_ADVANCE,
    );
    {
        let mut cs = click_state.borrow_mut();
        cl.render(f, chunks[2], Block::default(), &mut cs, false, 0);
    }
}

// ═══════════════════════════════════════════════════════════
// Business Mode
// ═══════════════════════════════════════════════════════════

fn render_business(
    state: &CafeState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // header (day + money)
            Constraint::Min(8),   // main content
            Constraint::Length(3), // action bar
        ])
        .split(area);

    // ── Header ──
    let header_text = format!(
        " Day {} │ 所持金: ¥{} │ 累計客数: {}",
        state.day, state.money, state.total_customers_served
    );
    let header = Paragraph::new(Line::from(Span::styled(
        header_text,
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    )))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow))
            .title(" 月灯り "),
    );
    f.render_widget(header, chunks[0]);

    // ── Menu list ──
    let mut cl = ClickableList::new();
    cl.push(Line::from(Span::styled(
        " メニュー",
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )));
    cl.push(Line::from(""));

    for (i, item) in state.menu.iter().enumerate() {
        let marker = if i == state.selected_menu_item {
            "▶"
        } else {
            " "
        };
        cl.push_clickable(
            Line::from(vec![
                Span::styled(
                    format!(" {marker} [{}] ", i + 1),
                    Style::default().fg(Color::Yellow),
                ),
                Span::styled(item.name, Style::default().fg(Color::White)),
                Span::styled(
                    format!("  ¥{}", item.price),
                    Style::default().fg(Color::Green),
                ),
            ]),
            MENU_ITEM_BASE + i as u16,
        );
        cl.push_clickable(
            Line::from(Span::styled(
                format!("      {}", item.description),
                Style::default().fg(Color::DarkGray),
            )),
            MENU_ITEM_BASE + i as u16,
        );
    }

    let menu_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Green))
        .title(" 今日のメニュー ");
    {
        let mut cs = click_state.borrow_mut();
        cl.render(f, chunks[1], menu_block, &mut cs, false, 0);
    }

    // ── Action bar ──
    let mut action_cl = ClickableList::new();
    action_cl.push_clickable(
        Line::from(Span::styled(
            " ▶ 営業開始！",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        )),
        SERVE_CONFIRM,
    );
    let action_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Green));
    {
        let mut cs = click_state.borrow_mut();
        action_cl.render(f, chunks[2], action_block, &mut cs, false, 0);
    }
}

// ═══════════════════════════════════════════════════════════
// Day Result
// ═══════════════════════════════════════════════════════════

fn render_day_result(
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
