mod game;

use std::{cell::RefCell, io, rc::Rc};

use game::{GamePhase, GameState, InputMode};
use ratzilla::event::KeyCode;
use ratzilla::ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratzilla::ratatui::style::{Color, Modifier, Style};
use ratzilla::ratatui::text::{Line, Span};
use ratzilla::ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};
use ratzilla::ratatui::Terminal;
use ratzilla::{DomBackend, WebRenderer};

fn main() -> io::Result<()> {
    console_error_panic_hook::set_once();

    let state = Rc::new(RefCell::new(GameState::new()));
    let backend = DomBackend::new()?;
    let terminal = Terminal::new(backend)?;

    terminal.on_key_event({
        let state = state.clone();
        move |key_event| {
            let mut gs = state.borrow_mut();
            match gs.input_mode {
                InputMode::Explore => match key_event.code {
                    KeyCode::Char('i') => {
                        gs.input_mode = InputMode::Inventory;
                    }
                    KeyCode::Char('r') if gs.phase == GamePhase::Escaped => {
                        *gs = GameState::new();
                    }
                    KeyCode::Char(c) => {
                        gs.handle_action(c);
                    }
                    _ => {}
                },
                InputMode::Inventory => match key_event.code {
                    KeyCode::Char('i') | KeyCode::Esc => {
                        gs.input_mode = InputMode::Explore;
                    }
                    _ => {}
                },
            }
        }
    });

    terminal.draw_web(move |f| {
        let gs = state.borrow();
        let size = f.area();

        // Main layout: top bar, content, bottom bar
        let main_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),  // Title bar
                Constraint::Min(10),   // Main content
                Constraint::Length(3), // Help bar
            ])
            .split(size);

        // Title bar
        let title = if gs.phase == GamePhase::Escaped {
            "★ 脱出成功！ ★"
        } else {
            "脱出ゲーム - Escape Room"
        };
        let title_style = if gs.phase == GamePhase::Escaped {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
        };
        let title_block = Paragraph::new(Line::from(Span::styled(title, title_style)))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::DarkGray)),
            )
            .alignment(ratzilla::ratatui::layout::Alignment::Center);
        f.render_widget(title_block, main_chunks[0]);

        // Content area: split into left (room + actions) and right (log + inventory)
        let content_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(55), // Room + Actions
                Constraint::Percentage(45), // Log
            ])
            .split(main_chunks[1]);

        // Left panel: room description + actions
        let left_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(7),  // Room description
                Constraint::Min(5),    // Actions or inventory
            ])
            .split(content_chunks[0]);

        // Room description
        let room_desc = Paragraph::new(gs.room_description())
            .style(Style::default().fg(Color::White))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Green))
                    .title(" 現在地 "),
            )
            .wrap(Wrap { trim: false });
        f.render_widget(room_desc, left_chunks[0]);

        // Actions or Inventory panel
        match gs.input_mode {
            InputMode::Explore => {
                let action_items: Vec<ListItem> = gs
                    .actions
                    .iter()
                    .map(|a| {
                        ListItem::new(Line::from(vec![
                            Span::styled(
                                format!("[{}] ", a.key),
                                Style::default()
                                    .fg(Color::Yellow)
                                    .add_modifier(Modifier::BOLD),
                            ),
                            Span::styled(&a.label, Style::default().fg(Color::White)),
                        ]))
                    })
                    .collect();

                let actions_block = List::new(action_items).block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Yellow))
                        .title(" アクション "),
                );
                f.render_widget(actions_block, left_chunks[1]);
            }
            InputMode::Inventory => {
                let inv_items: Vec<ListItem> = gs
                    .inventory_display()
                    .iter()
                    .map(|item| {
                        ListItem::new(Span::styled(
                            format!("  {}", item),
                            Style::default().fg(Color::Magenta),
                        ))
                    })
                    .collect();

                let inv_block = List::new(inv_items).block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Magenta))
                        .title(" 持ち物 "),
                );
                f.render_widget(inv_block, left_chunks[1]);
            }
        }

        // Right panel: message log
        let log_area = content_chunks[1];
        render_log(f, &gs, log_area);

        // Bottom help bar
        let help_text = if gs.phase == GamePhase::Escaped {
            "[R] もう一度プレイ"
        } else {
            match gs.input_mode {
                InputMode::Explore => "[1-9/N/S/W] アクション選択  [I] 持ち物を見る",
                InputMode::Inventory => "[I/Esc] 閉じる",
            }
        };
        let help = Paragraph::new(Line::from(Span::styled(
            help_text,
            Style::default().fg(Color::DarkGray),
        )))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray)),
        )
        .alignment(ratzilla::ratatui::layout::Alignment::Center);
        f.render_widget(help, main_chunks[2]);
    });

    Ok(())
}

fn render_log(f: &mut ratzilla::ratatui::Frame, gs: &GameState, area: Rect) {
    let visible_height = area.height.saturating_sub(2) as usize; // account for borders
    let start = if gs.log.len() > visible_height {
        gs.log.len() - visible_height
    } else {
        0
    };

    let log_lines: Vec<Line> = gs.log[start..]
        .iter()
        .map(|entry| {
            if entry.is_important {
                Line::from(Span::styled(
                    &entry.text,
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ))
            } else {
                Line::from(Span::styled(
                    &entry.text,
                    Style::default().fg(Color::Gray),
                ))
            }
        })
        .collect();

    let log_widget = Paragraph::new(log_lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Blue))
                .title(" ログ "),
        )
        .wrap(Wrap { trim: false });

    f.render_widget(log_widget, area);
}
