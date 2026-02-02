mod games;
mod input;
mod time;

use std::{cell::RefCell, io, rc::Rc};

use games::{create_game, AppState, GameChoice};
use input::{is_narrow_layout, pixel_x_to_col, pixel_y_to_row, resolve_tap_at, ClickState, InputEvent};
use time::GameTime;

use ratzilla::event::{KeyCode, MouseButton, MouseEventKind};
use ratzilla::ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratzilla::ratatui::style::{Color, Modifier, Style};
use ratzilla::ratatui::text::{Line, Span};
use ratzilla::ratatui::widgets::{Block, Borders, Paragraph};
use ratzilla::ratatui::Terminal;
use ratzilla::{DomBackend, WebRenderer};

/// Query the grid container's bounding rect and convert pixel coordinates to a (row, col).
fn dom_pixel_to_row_col(client_x: f64, client_y: f64, cs: &ClickState) -> Option<(u16, u16)> {
    let window = web_sys::window()?;
    let document = window.document()?;
    let grid = document.query_selector("body > div").ok()??;
    let rect = grid.get_bounding_client_rect();

    let click_y = client_y - rect.top();
    let click_x = client_x - rect.left();

    if click_x < 0.0 {
        return None;
    }

    let row = pixel_y_to_row(click_y, rect.height(), cs.terminal_rows)?;
    let col = pixel_x_to_col(click_x, rect.width(), cs.terminal_cols).unwrap_or(0);
    Some((row, col))
}

/// Process a tap/click at the given client coordinates.
fn handle_tap(
    client_x: f64,
    client_y: f64,
    app_state: &Rc<RefCell<AppState>>,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let cs = click_state.borrow();
    if cs.terminal_rows == 0 || cs.terminal_cols == 0 {
        return;
    }

    let (row, col) = match dom_pixel_to_row_col(client_x, client_y, &cs) {
        Some(rc) => rc,
        None => return,
    };

    if let Some(event) = resolve_tap_at(row, col, &cs) {
        drop(cs);
        dispatch_event(&event, app_state);
    }
}

/// Dispatch an input event to the current app state.
fn dispatch_event(event: &InputEvent, app_state: &Rc<RefCell<AppState>>) {
    let key = match event {
        InputEvent::Key(c) => *c,
    };

    let mut state = app_state.borrow_mut();
    match &mut *state {
        AppState::Menu => match key {
            '1' => {
                let game = create_game(&GameChoice::Cookie);
                *state = AppState::Playing { game };
            }
            '2' => {
                let game = create_game(&GameChoice::Factory);
                *state = AppState::Playing { game };
            }
            _ => {}
        },
        AppState::Playing { game } => {
            if key == 'q' {
                *state = AppState::Menu;
            } else {
                game.handle_input(event);
            }
        }
    }
}

fn main() -> io::Result<()> {
    console_error_panic_hook::set_once();

    let app_state = Rc::new(RefCell::new(AppState::Menu));
    let click_state = Rc::new(RefCell::new(ClickState::new()));
    let game_time = Rc::new(RefCell::new(GameTime::new(10)));
    let backend = DomBackend::new()?;
    let terminal = Terminal::new(backend)?;

    // Mouse click handler
    terminal.on_mouse_event({
        let app_state = app_state.clone();
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
                &app_state,
                &click_state,
            );
        }
    });

    // Keyboard handler
    terminal.on_key_event({
        let app_state = app_state.clone();
        move |key_event| {
            let event = match key_event.code {
                KeyCode::Char(c) => InputEvent::Key(c),
                KeyCode::Esc => InputEvent::Key('q'),
                KeyCode::Left => InputEvent::Key('h'),
                KeyCode::Right => InputEvent::Key('l'),
                KeyCode::Up => InputEvent::Key('k'),
                KeyCode::Down => InputEvent::Key('j'),
                _ => return,
            };
            dispatch_event(&event, &app_state);
        }
    });

    // Draw loop
    terminal.draw_web({
        let click_state = click_state.clone();
        let game_time = game_time.clone();
        move |f| {
            let size = f.area();

            // Update terminal dimensions and clear click targets
            {
                let mut cs = click_state.borrow_mut();
                cs.terminal_cols = size.width;
                cs.terminal_rows = size.height;
                cs.clear_targets();
            }

            // Get current timestamp for game time
            let now_ms = web_sys::window()
                .and_then(|w| w.performance())
                .map(|p| p.now())
                .unwrap_or(0.0);
            let delta_ticks = game_time.borrow_mut().update(now_ms);

            let mut state = app_state.borrow_mut();
            match &mut *state {
                AppState::Menu => {
                    render_menu(f, size, &click_state);
                }
                AppState::Playing { game } => {
                    // Tick game logic
                    if delta_ticks > 0 {
                        game.tick(delta_ticks);
                    }

                    // Main layout: title area + game area
                    let chunks = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints([Constraint::Min(1)])
                        .split(size);

                    game.render(f, chunks[0], &click_state);
                }
            }
        }
    });

    Ok(())
}

fn render_menu(
    f: &mut ratzilla::ratatui::Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let is_narrow = is_narrow_layout(area.width);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Title
            Constraint::Min(8),   // Menu items
            Constraint::Length(3), // Footer
        ])
        .split(area);

    // Title
    let title = if is_narrow {
        "Game Select"
    } else {
        "Game Select - ゲームを選んでください"
    };
    let borders = if is_narrow {
        Borders::TOP | Borders::BOTTOM
    } else {
        Borders::ALL
    };
    let title_widget = Paragraph::new(Line::from(Span::styled(
        title,
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )))
    .block(
        Block::default()
            .borders(borders)
            .border_style(Style::default().fg(Color::Cyan)),
    )
    .alignment(Alignment::Center);
    f.render_widget(title_widget, chunks[0]);

    // Menu items
    let menu_lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled(
                " [1] ",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("Cookie Factory", Style::default().fg(Color::White)),
        ]),
        Line::from(Span::styled(
            "     クッキーをクリックして増やす放置ゲーム",
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                " [2] ",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("Tiny Factory", Style::default().fg(Color::White)),
        ]),
        Line::from(Span::styled(
            "     工場を作って生産ラインを最適化する放置ゲーム",
            Style::default().fg(Color::DarkGray),
        )),
    ];

    let menu_widget = Paragraph::new(menu_lines).block(
        Block::default()
            .borders(borders)
            .border_style(Style::default().fg(Color::Green))
            .title(" Games "),
    );
    f.render_widget(menu_widget, chunks[1]);

    // Register click targets
    {
        let mut cs = click_state.borrow_mut();
        // Menu item 1: title row
        cs.add_target(chunks[1].y + 2, '1');
        cs.add_target(chunks[1].y + 3, '1');
        // Menu item 2:
        cs.add_target(chunks[1].y + 5, '2');
        cs.add_target(chunks[1].y + 6, '2');
    }

    // Footer
    let footer_widget = Paragraph::new(Line::from(Span::styled(
        "数字キーまたはタップでゲームを選択",
        Style::default().fg(Color::DarkGray),
    )))
    .block(
        Block::default()
            .borders(borders)
            .border_style(Style::default().fg(Color::DarkGray)),
    )
    .alignment(Alignment::Center);
    f.render_widget(footer_widget, chunks[2]);
}
