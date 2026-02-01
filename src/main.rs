mod game;

use std::{cell::RefCell, io, rc::Rc};

use game::{GamePhase, GameState, InputMode};
use ratzilla::event::{KeyCode, MouseButton, MouseEventKind};
use ratzilla::ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratzilla::ratatui::style::{Color, Modifier, Style};
use ratzilla::ratatui::text::{Line, Span};
use ratzilla::ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};
use ratzilla::ratatui::Terminal;
use ratzilla::{DomBackend, WebRenderer};

/// A region on screen that can be tapped/clicked to trigger an action.
struct ClickTarget {
    row: u16,
    key: char,
}

/// Shared state between the render loop and click handler.
struct ClickState {
    targets: Vec<ClickTarget>,
    terminal_cols: u16,
    terminal_rows: u16,
}

/// Convert pixel coordinates to a terminal row using the <pre> element's dimensions.
fn pixel_to_row(mouse_x: u32, mouse_y: u32, cs: &ClickState) -> Option<u16> {
    let window = web_sys::window()?;
    let document = window.document()?;
    let pre = document.query_selector("pre").ok()??;
    let rect = pre.get_bounding_client_rect();
    let pre_width = rect.width();
    let pre_height = rect.height();

    if pre_width == 0.0 || pre_height == 0.0 {
        return None;
    }

    let cell_height = pre_height / cs.terminal_rows as f64;
    let click_y = mouse_y as f64 - rect.top();

    if click_y < 0.0 || mouse_x as f64 - rect.left() < 0.0 {
        return None;
    }

    Some((click_y / cell_height) as u16)
}

fn main() -> io::Result<()> {
    console_error_panic_hook::set_once();

    let state = Rc::new(RefCell::new(GameState::new()));
    let click_state = Rc::new(RefCell::new(ClickState {
        targets: Vec::new(),
        terminal_cols: 0,
        terminal_rows: 0,
    }));
    let backend = DomBackend::new()?;
    let terminal = Terminal::new(backend)?;

    // Mouse/touch click handler
    terminal.on_mouse_event({
        let state = state.clone();
        let click_state = click_state.clone();
        move |mouse_event| {
            if mouse_event.event != MouseEventKind::Pressed
                || mouse_event.button != MouseButton::Left
            {
                return;
            }

            let cs = click_state.borrow();
            if cs.terminal_rows == 0 || cs.terminal_cols == 0 {
                return;
            }

            let row = match pixel_to_row(mouse_event.x, mouse_event.y, &cs) {
                Some(r) => r,
                None => return,
            };

            // Find the click target for this row
            let matched_key = cs.targets.iter().find(|t| t.row == row).map(|t| t.key);
            drop(cs);

            if let Some(key) = matched_key {
                let mut gs = state.borrow_mut();
                match gs.input_mode {
                    InputMode::Explore => {
                        if key == 'i' {
                            gs.input_mode = InputMode::Inventory;
                        } else if key == 'r' && gs.phase == GamePhase::Escaped {
                            *gs = GameState::new();
                        } else {
                            gs.handle_action(key);
                        }
                    }
                    InputMode::Inventory => {
                        if key == 'i' {
                            gs.input_mode = InputMode::Explore;
                        }
                    }
                }
            }
        }
    });

    // Keyboard handler
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

    terminal.draw_web({
        let click_state = click_state.clone();
        move |f| {
            let gs = state.borrow();
            let size = f.area();

            // Update terminal dimensions and clear click targets
            {
                let mut cs = click_state.borrow_mut();
                cs.terminal_cols = size.width;
                cs.terminal_rows = size.height;
                cs.targets.clear();
            }

            let is_narrow = size.width < 60;

            // Main layout: title, content, help
            let main_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3),
                    Constraint::Min(10),
                    Constraint::Length(3),
                ])
                .split(size);

            // Title bar
            render_title(f, &gs, main_chunks[0]);

            // Content area — responsive layout
            if is_narrow {
                render_narrow_layout(f, &gs, main_chunks[1], &click_state);
            } else {
                render_wide_layout(f, &gs, main_chunks[1], &click_state);
            }

            // Help bar (also a click target)
            render_help(f, &gs, main_chunks[2], &click_state);
        }
    });

    Ok(())
}

fn render_title(f: &mut ratzilla::ratatui::Frame, gs: &GameState, area: Rect) {
    let title = if gs.phase == GamePhase::Escaped {
        "★ 脱出成功！ ★"
    } else {
        "脱出ゲーム - Escape Room"
    };
    let title_style = if gs.phase == GamePhase::Escaped {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    };
    let title_block = Paragraph::new(Line::from(Span::styled(title, title_style)))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray)),
        )
        .alignment(ratzilla::ratatui::layout::Alignment::Center);
    f.render_widget(title_block, area);
}

/// Wide layout: left panel (room + actions) | right panel (log)
fn render_wide_layout(
    f: &mut ratzilla::ratatui::Frame,
    gs: &GameState,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let content_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(area);

    let left_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(7), Constraint::Min(5)])
        .split(content_chunks[0]);

    render_room_description(f, gs, left_chunks[0]);
    render_actions_or_inventory(f, gs, left_chunks[1], click_state);
    render_log(f, gs, content_chunks[1]);
}

/// Narrow layout: room description, actions, log stacked vertically
fn render_narrow_layout(
    f: &mut ratzilla::ratatui::Frame,
    gs: &GameState,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),
            Constraint::Min(4),
            Constraint::Min(4),
        ])
        .split(area);

    render_room_description(f, gs, chunks[0]);
    render_actions_or_inventory(f, gs, chunks[1], click_state);
    render_log(f, gs, chunks[2]);
}

fn render_room_description(f: &mut ratzilla::ratatui::Frame, gs: &GameState, area: Rect) {
    let room_desc = Paragraph::new(gs.room_description())
        .style(Style::default().fg(Color::White))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Green))
                .title(" 現在地 "),
        )
        .wrap(Wrap { trim: false });
    f.render_widget(room_desc, area);
}

fn render_actions_or_inventory(
    f: &mut ratzilla::ratatui::Frame,
    gs: &GameState,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    match gs.input_mode {
        InputMode::Explore => {
            let action_items: Vec<ListItem> = gs
                .actions
                .iter()
                .map(|a| {
                    ListItem::new(Line::from(vec![
                        Span::styled(
                            format!(" [{}] ", a.key.to_uppercase()),
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
                    .title(" ▶ アクション（タップで選択） "),
            );
            f.render_widget(actions_block, area);

            // Record click targets: each action starts at area.y + 1 (border)
            let mut cs = click_state.borrow_mut();
            for (i, action) in gs.actions.iter().enumerate() {
                cs.targets.push(ClickTarget {
                    row: area.y + 1 + i as u16,
                    key: action.key,
                });
            }
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
            f.render_widget(inv_block, area);

            // The whole inventory panel area is clickable to close
            let mut cs = click_state.borrow_mut();
            for row in area.y..area.y + area.height {
                cs.targets.push(ClickTarget { row, key: 'i' });
            }
        }
    }
}

fn render_log(f: &mut ratzilla::ratatui::Frame, gs: &GameState, area: Rect) {
    let visible_height = area.height.saturating_sub(2) as usize;
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

fn render_help(
    f: &mut ratzilla::ratatui::Frame,
    gs: &GameState,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let help_text = if gs.phase == GamePhase::Escaped {
        "[R] もう一度プレイ"
    } else {
        match gs.input_mode {
            InputMode::Explore => "[I] 持ち物を見る",
            InputMode::Inventory => "[I] 閉じる",
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
    f.render_widget(help, area);

    // Register help bar as click target
    let mut cs = click_state.borrow_mut();
    let key = if gs.phase == GamePhase::Escaped {
        'r'
    } else {
        'i'
    };
    for row in area.y..area.y + area.height {
        cs.targets.push(ClickTarget { row, key });
    }
}
