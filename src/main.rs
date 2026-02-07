mod games;
mod input;
mod time;
mod widgets;

use std::{cell::RefCell, io, rc::Rc};

use games::{create_game, AppState, GameChoice};
use input::{is_narrow_layout, ClickState, InputEvent};
use widgets::ClickableList;
use time::GameTime;

use ratzilla::event::{KeyCode, MouseButton, MouseEventKind};
use ratzilla::ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratzilla::ratatui::style::{Color, Modifier, Style};
use ratzilla::ratatui::text::{Line, Span};
use ratzilla::ratatui::widgets::{Block, Borders, Paragraph};
use ratzilla::ratatui::Terminal;
use ratzilla::{DomBackend, WebRenderer};

// ── Menu action IDs ─────────────────────────────────────────────
pub const MENU_SELECT_COOKIE: u16 = 1;
pub const MENU_SELECT_FACTORY: u16 = 2;
pub const MENU_SELECT_CAREER: u16 = 3;
pub const BACK_TO_MENU: u16 = 65535;

/// Use `elementFromPoint` to find which grid cell was clicked.
///
/// Ratzilla renders each terminal row as a `<pre>` child of `div#grid`.
/// Instead of pixel-math (fragile under zoom / scroll / CSS transforms),
/// we ask the browser which element sits at the click coordinates,
/// then walk up to the `<pre>` and find its index among siblings.
///
/// Returns `(row, col)` in terminal cell coordinates.
fn dom_element_to_cell(
    client_x: f64,
    client_y: f64,
    terminal_cols: u16,
) -> Option<(u16, u16)> {
    let window = web_sys::window()?;
    let document = window.document()?;
    let element = document.element_from_point(client_x as f32, client_y as f32)?;

    // Walk up to the <pre> row element (ratzilla may nest <span>s inside <pre>)
    let pre = find_ancestor_pre(&element)?;

    // The parent of the <pre> is the grid container
    let grid = pre.parent_element()?;
    let children = grid.children();
    let len = children.length();
    let mut row = None;
    for i in 0..len {
        if let Some(child) = children.item(i) {
            if child == pre {
                row = Some(i as u16);
                break;
            }
        }
    }
    let row = row?;

    // Compute column from x position within the <pre> element.
    // All <pre> elements use a monospace font, so character width is uniform.
    let rect = pre.get_bounding_client_rect();
    let pre_left = rect.left();
    let pre_width = rect.width();
    if pre_width <= 0.0 || terminal_cols == 0 {
        return Some((row, 0));
    }
    let relative_x = (client_x - pre_left).max(0.0);
    let col = ((relative_x / pre_width) * terminal_cols as f64) as u16;
    let col = col.min(terminal_cols.saturating_sub(1));

    Some((row, col))
}

/// Walk up the DOM from `el` to find the nearest `<pre>` ancestor (or self).
fn find_ancestor_pre(el: &web_sys::Element) -> Option<web_sys::Element> {
    let mut current = Some(el.clone());
    while let Some(e) = current {
        if e.tag_name().eq_ignore_ascii_case("PRE") {
            return Some(e);
        }
        current = e.parent_element();
    }
    None
}

/// Process a tap/click at the given client coordinates.
fn handle_tap(
    client_x: f64,
    client_y: f64,
    app_state: &Rc<RefCell<AppState>>,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let cs = click_state.borrow();
    let (row, col) = match dom_element_to_cell(client_x, client_y, cs.terminal_cols) {
        Some(r) => r,
        None => return,
    };

    if let Some(action_id) = cs.hit_test(col, row) {
        drop(cs);
        dispatch_event(&InputEvent::Click(action_id), app_state);
    }
}

/// Dispatch an input event to the current app state.
fn dispatch_event(event: &InputEvent, app_state: &Rc<RefCell<AppState>>) {
    let mut state = app_state.borrow_mut();
    match &mut *state {
        AppState::Menu => {
            let choice = match event {
                InputEvent::Key('1') | InputEvent::Click(MENU_SELECT_COOKIE) => {
                    Some(GameChoice::Cookie)
                }
                InputEvent::Key('2') | InputEvent::Click(MENU_SELECT_FACTORY) => {
                    Some(GameChoice::Factory)
                }
                InputEvent::Key('3') | InputEvent::Click(MENU_SELECT_CAREER) => {
                    Some(GameChoice::Career)
                }
                _ => None,
            };
            if let Some(choice) = choice {
                let game = create_game(&choice);
                *state = AppState::Playing { game };
            }
        }
        AppState::Playing { game } => {
            if matches!(event, InputEvent::Key('q') | InputEvent::Click(BACK_TO_MENU)) {
                // Let the game handle back first (e.g., sub-screen → main screen).
                // Only go to menu if the game didn't consume it.
                if !game.handle_input(event) {
                    *state = AppState::Menu;
                }
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

                    game.render(f, size, &click_state);

                    // Overlay back button in top-left corner
                    let back_area = Rect::new(size.x, size.y, 6, 1);
                    let back = Paragraph::new(Span::styled(
                        " ◀戻る",
                        Style::default().fg(Color::DarkGray),
                    ));
                    f.render_widget(back, back_area);
                    click_state
                        .borrow_mut()
                        .add_click_target(back_area, BACK_TO_MENU);
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

    // Menu items — title + description rows share the same action ID
    let mut cl = ClickableList::new();

    cl.push(Line::from(""));
    cl.push_clickable(Line::from(vec![
        Span::styled(
            " ▶ ",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("Cookie Factory", Style::default().fg(Color::White)),
    ]), MENU_SELECT_COOKIE);
    cl.push_clickable(Line::from(Span::styled(
        "    クッキーをクリックして増やす放置ゲーム",
        Style::default().fg(Color::DarkGray),
    )), MENU_SELECT_COOKIE);

    cl.push(Line::from(""));
    cl.push_clickable(Line::from(vec![
        Span::styled(
            " ▶ ",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("Tiny Factory", Style::default().fg(Color::White)),
    ]), MENU_SELECT_FACTORY);
    cl.push_clickable(Line::from(Span::styled(
        "    工場を作って生産ラインを最適化する放置ゲーム",
        Style::default().fg(Color::DarkGray),
    )), MENU_SELECT_FACTORY);

    cl.push(Line::from(""));
    cl.push_clickable(Line::from(vec![
        Span::styled(
            " ▶ ",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("Career Simulator", Style::default().fg(Color::White)),
    ]), MENU_SELECT_CAREER);
    cl.push_clickable(Line::from(Span::styled(
        "    スキルを磨いて転職・投資でキャリアを築くシミュレーション",
        Style::default().fg(Color::DarkGray),
    )), MENU_SELECT_CAREER);

    // Register click targets (borders → top=1, bottom=1)
    let top_offset = if borders.contains(Borders::TOP) { 1 } else { 0 };
    let bottom_offset = if borders.contains(Borders::BOTTOM) { 1 } else { 0 };
    {
        let mut cs = click_state.borrow_mut();
        cl.register_targets(chunks[1], &mut cs, top_offset, bottom_offset, 0);
    }

    let menu_widget = Paragraph::new(cl.into_lines()).block(
        Block::default()
            .borders(borders)
            .border_style(Style::default().fg(Color::Green))
            .title(" Games "),
    );
    f.render_widget(menu_widget, chunks[1]);

    // Footer
    let footer_widget = Paragraph::new(Line::from(Span::styled(
        "タップでゲームを選択",
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
