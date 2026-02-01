mod click;
mod game;

use std::{cell::RefCell, io, rc::Rc};

use click::{dispatch_input, is_narrow_layout, pixel_y_to_row, resolve_tap, ClickState, InputEvent};
use game::{GamePhase, GameState, InputMode};
use ratzilla::event::{KeyCode, MouseButton, MouseEventKind};
use ratzilla::ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratzilla::ratatui::style::{Color, Modifier, Style};
use ratzilla::ratatui::text::{Line, Span};
use ratzilla::ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};
use ratzilla::ratatui::Terminal;
use ratzilla::{DomBackend, WebRenderer};
use wasm_bindgen::prelude::*;

/// Query the grid container's bounding rect and convert pixel coordinates to a row.
fn dom_pixel_to_row(client_x: f64, client_y: f64, cs: &ClickState) -> Option<u16> {
    let window = web_sys::window()?;
    let document = window.document()?;

    // DomBackend creates a <div> as the grid container inside <body>.
    let grid = document.query_selector("body > div").ok()??;
    let rect = grid.get_bounding_client_rect();

    let click_y = client_y - rect.top();
    let click_x = client_x - rect.left();

    if click_x < 0.0 {
        return None;
    }

    pixel_y_to_row(click_y, rect.height(), cs.terminal_rows)
}

/// Process a tap/click at the given client coordinates.
/// DOM layer: converts pixel coords to row, then delegates to pure logic.
fn handle_tap(
    client_x: f64,
    client_y: f64,
    state: &Rc<RefCell<GameState>>,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let cs = click_state.borrow();
    if cs.terminal_rows == 0 || cs.terminal_cols == 0 {
        return;
    }

    let row = match dom_pixel_to_row(client_x, client_y, &cs) {
        Some(r) => r,
        None => return,
    };

    // resolve_tap: row → Option<InputEvent::Key>
    if let Some(event) = resolve_tap(row, &cs) {
        drop(cs);
        let mut gs = state.borrow_mut();
        dispatch_input(&event, &mut gs);
    }
}

/// Register a native touchstart listener on document for reliable mobile tap handling.
fn register_touch_handler(
    state: &Rc<RefCell<GameState>>,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let state = state.clone();
    let click_state = click_state.clone();

    let closure = Closure::<dyn FnMut(_)>::new(move |event: web_sys::TouchEvent| {
        let touches = event.touches();
        if touches.length() == 0 {
            return;
        }
        let touch = match touches.get(0) {
            Some(t) => t,
            None => return,
        };
        handle_tap(
            touch.client_x() as f64,
            touch.client_y() as f64,
            &state,
            &click_state,
        );
    });

    let window = web_sys::window().unwrap();
    let document = window.document().unwrap();
    let _ = document.add_event_listener_with_callback(
        "touchstart",
        closure.as_ref().unchecked_ref(),
    );
    closure.forget();
}

fn main() -> io::Result<()> {
    console_error_panic_hook::set_once();

    let state = Rc::new(RefCell::new(GameState::new()));
    let click_state = Rc::new(RefCell::new(ClickState::new()));
    let backend = DomBackend::new()?;
    let terminal = Terminal::new(backend)?;

    // Mouse click handler (desktop)
    terminal.on_mouse_event({
        let state = state.clone();
        let click_state = click_state.clone();
        move |mouse_event| {
            if mouse_event.event != MouseEventKind::Pressed
                || mouse_event.button != MouseButton::Left
            {
                return;
            }
            handle_tap(
                mouse_event.x as f64,
                mouse_event.y as f64,
                &state,
                &click_state,
            );
        }
    });

    // Native touch handler (mobile) — touchstart fires reliably on mobile
    // whereas synthesized mousedown from touch events can be unreliable.
    register_touch_handler(&state, &click_state);

    // Keyboard handler — convert KeyCode to InputEvent, then dispatch
    terminal.on_key_event({
        let state = state.clone();
        move |key_event| {
            let event = match key_event.code {
                KeyCode::Char(c) => InputEvent::Key(c),
                KeyCode::Esc => InputEvent::Key('\x1b'),
                _ => return,
            };
            let mut gs = state.borrow_mut();
            dispatch_input(&event, &mut gs);
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
                cs.clear_targets();
            }

            let is_narrow = is_narrow_layout(size.width);

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
            render_title(f, &gs, main_chunks[0], is_narrow);

            // Content area — responsive layout
            if is_narrow {
                render_narrow_layout(f, &gs, main_chunks[1], &click_state);
            } else {
                render_wide_layout(f, &gs, main_chunks[1], &click_state);
            }

            // Help bar (also a click target)
            render_help(f, &gs, main_chunks[2], &click_state, is_narrow);
        }
    });

    Ok(())
}

fn render_title(f: &mut ratzilla::ratatui::Frame, gs: &GameState, area: Rect, is_narrow: bool) {
    let title = if gs.phase == GamePhase::Escaped {
        "★ 脱出成功！ ★"
    } else if is_narrow {
        "脱出ゲーム"
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
    let borders = if is_narrow {
        Borders::TOP | Borders::BOTTOM
    } else {
        Borders::ALL
    };
    let title_block = Paragraph::new(Line::from(Span::styled(title, title_style)))
        .block(
            Block::default()
                .borders(borders)
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

    render_room_description(f, gs, left_chunks[0], false);
    render_actions_or_inventory(f, gs, left_chunks[1], click_state, false);
    render_log(f, gs, content_chunks[1], false);
}

/// Narrow layout: room description, actions, log stacked vertically
fn render_narrow_layout(
    f: &mut ratzilla::ratatui::Frame,
    gs: &GameState,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    // Actions panel height: items + 2 (top/bottom border), minimum 3
    let action_count = match gs.input_mode {
        InputMode::Explore => gs.actions.len(),
        InputMode::Inventory => gs.inventory_display().len(),
    };
    let action_height = (action_count as u16 + 2).max(3);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),            // Room description (compact)
            Constraint::Length(action_height), // Actions: exact fit
            Constraint::Min(3),              // Log: gets all remaining space
        ])
        .split(area);

    render_room_description(f, gs, chunks[0], true);
    render_actions_or_inventory(f, gs, chunks[1], click_state, true);
    render_log(f, gs, chunks[2], true);
}

fn render_room_description(
    f: &mut ratzilla::ratatui::Frame,
    gs: &GameState,
    area: Rect,
    is_narrow: bool,
) {
    let borders = if is_narrow {
        Borders::TOP | Borders::BOTTOM
    } else {
        Borders::ALL
    };
    let room_desc = Paragraph::new(gs.room_description())
        .style(Style::default().fg(Color::White))
        .block(
            Block::default()
                .borders(borders)
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
    is_narrow: bool,
) {
    let borders = if is_narrow {
        Borders::TOP | Borders::BOTTOM
    } else {
        Borders::ALL
    };

    match gs.input_mode {
        InputMode::Explore => {
            let action_items: Vec<ListItem> = gs
                .actions
                .iter()
                .map(|a| {
                    let prefix = if is_narrow {
                        format!("[{}] ", a.key.to_uppercase())
                    } else {
                        format!(" [{}] ", a.key.to_uppercase())
                    };
                    ListItem::new(Line::from(vec![
                        Span::styled(
                            prefix,
                            Style::default()
                                .fg(Color::Yellow)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(&a.label, Style::default().fg(Color::White)),
                    ]))
                })
                .collect();

            let title = if is_narrow {
                " アクション "
            } else {
                " ▶ アクション（タップで選択） "
            };
            let actions_block = List::new(action_items).block(
                Block::default()
                    .borders(borders)
                    .border_style(Style::default().fg(Color::Yellow))
                    .title(title),
            );
            f.render_widget(actions_block, area);

            // Record click targets: each action starts at area.y + 1 (border)
            let mut cs = click_state.borrow_mut();
            for (i, action) in gs.actions.iter().enumerate() {
                cs.add_target(area.y + 1 + i as u16, action.key);
            }
        }
        InputMode::Inventory => {
            let inv_items: Vec<ListItem> = gs
                .inventory_display()
                .iter()
                .map(|item| {
                    let prefix = if is_narrow { "" } else { "  " };
                    ListItem::new(Span::styled(
                        format!("{}{}", prefix, item),
                        Style::default().fg(Color::Magenta),
                    ))
                })
                .collect();

            let inv_block = List::new(inv_items).block(
                Block::default()
                    .borders(borders)
                    .border_style(Style::default().fg(Color::Magenta))
                    .title(" 持ち物 "),
            );
            f.render_widget(inv_block, area);

            // The whole inventory panel area is clickable to close
            let mut cs = click_state.borrow_mut();
            for row in area.y..area.y + area.height {
                cs.add_target(row, 'i');
            }
        }
    }
}

fn render_log(f: &mut ratzilla::ratatui::Frame, gs: &GameState, area: Rect, is_narrow: bool) {
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

    let borders = if is_narrow {
        Borders::TOP | Borders::BOTTOM
    } else {
        Borders::ALL
    };
    let log_widget = Paragraph::new(log_lines)
        .block(
            Block::default()
                .borders(borders)
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
    is_narrow: bool,
) {
    let help_text = if gs.phase == GamePhase::Escaped {
        "[R] もう一度プレイ"
    } else {
        match gs.input_mode {
            InputMode::Explore => "[I] 持ち物を見る",
            InputMode::Inventory => "[I] 閉じる",
        }
    };
    let borders = if is_narrow {
        Borders::TOP | Borders::BOTTOM
    } else {
        Borders::ALL
    };
    let help = Paragraph::new(Line::from(Span::styled(
        help_text,
        Style::default().fg(Color::DarkGray),
    )))
    .block(
        Block::default()
            .borders(borders)
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
        cs.add_target(row, key);
    }
}
