//! Story mode (Novel ADV) rendering.

use std::cell::RefCell;
use std::rc::Rc;

use ratzilla::ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratzilla::ratatui::style::{Color, Modifier, Style};
use ratzilla::ratatui::text::{Line, Span};
use ratzilla::ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratzilla::ratatui::Frame;

use crate::input::ClickState;
use crate::widgets::ClickableList;

use super::super::actions::*;
use super::super::scenario::{self, get_chapter_scenes, PROLOGUE_SCENES};
use super::super::state::CafeState;

pub(super) fn render_story(
    state: &CafeState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let scenes = if state.current_chapter == 0 {
        PROLOGUE_SCENES
    } else {
        get_chapter_scenes(state.current_chapter)
    };
    let scene_count = scenes.len();
    if scene_count == 0 || state.current_scene_index >= scene_count {
        return;
    }

    let scene = scenes[state.current_scene_index];
    let line_data = &scene.lines[state.current_line_index];

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(5),
            Constraint::Length(2),
        ])
        .split(area);

    let ch_title = scenario::chapter_title(state.current_chapter);
    let scene_label = format!(
        " Ch.{} — {}  [{}/{}]",
        state.current_chapter,
        ch_title,
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
