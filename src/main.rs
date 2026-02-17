mod games;
mod input;
mod time;
mod widgets;

use std::{cell::Cell, cell::RefCell, io, rc::Rc};

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
pub const MENU_SELECT_RPG: u16 = 4;
pub const MENU_SELECT_SETTINGS: u16 = 5;
pub const BACK_TO_MENU: u16 = 65535;

// ── Settings action IDs ─────────────────────────────────────────
const SETTINGS_RESET_COOKIE: u16 = 10;
const SETTINGS_RESET_CAREER: u16 = 11;
const SETTINGS_CONFIRM_YES: u16 = 12;
const SETTINGS_CONFIRM_NO: u16 = 13;

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
///
/// The `tap_handled` guard in [`ClickState`] ensures that only the first
/// mouse event per render frame is dispatched.  This prevents the same
/// physical tap from being processed twice when the browser fires both a
/// synthetic (from our touchstart handler) and a compatibility mouse event.
fn handle_tap(
    client_x: f64,
    client_y: f64,
    app_state: &Rc<RefCell<AppState>>,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let mut cs = click_state.borrow_mut();
    if cs.tap_handled {
        return;
    }
    let (row, col) = match dom_element_to_cell(client_x, client_y, cs.terminal_cols) {
        Some(r) => r,
        None => return,
    };

    if let Some(action_id) = cs.hit_test(col, row) {
        cs.tap_handled = true;
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
                InputEvent::Key('4') | InputEvent::Click(MENU_SELECT_RPG) => {
                    Some(GameChoice::Rpg)
                }
                _ => None,
            };
            if let Some(choice) = choice {
                let game = create_game(&choice);
                *state = AppState::Playing { game };
            } else if matches!(
                event,
                InputEvent::Key('5') | InputEvent::Click(MENU_SELECT_SETTINGS)
            ) {
                *state = AppState::Settings {
                    confirm_reset: None,
                };
            }
        }
        AppState::Settings { confirm_reset } => {
            if confirm_reset.is_some() {
                // Confirmation dialog is active
                match event {
                    InputEvent::Key('y') | InputEvent::Click(SETTINGS_CONFIRM_YES) => {
                        let game = confirm_reset.take().unwrap();
                        perform_reset(&game);
                        *state = AppState::Settings {
                            confirm_reset: None,
                        };
                    }
                    InputEvent::Key('n')
                    | InputEvent::Key('q')
                    | InputEvent::Click(SETTINGS_CONFIRM_NO) => {
                        *confirm_reset = None;
                    }
                    _ => {}
                }
            } else {
                match event {
                    InputEvent::Key('1') | InputEvent::Click(SETTINGS_RESET_COOKIE) => {
                        *confirm_reset = Some(GameChoice::Cookie);
                    }
                    InputEvent::Key('2') | InputEvent::Click(SETTINGS_RESET_CAREER) => {
                        *confirm_reset = Some(GameChoice::Career);
                    }
                    InputEvent::Key('q') | InputEvent::Click(BACK_TO_MENU) => {
                        *state = AppState::Menu;
                    }
                    _ => {}
                }
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

/// Delete localStorage save data for the specified game.
fn perform_reset(game: &GameChoice) {
    #[cfg(target_arch = "wasm32")]
    match game {
        GameChoice::Cookie => games::cookie::save::delete_save(),
        GameChoice::Career => games::career::save::delete_save(),
        _ => {}
    }
    #[cfg(not(target_arch = "wasm32"))]
    let _ = game;
}

fn main() -> io::Result<()> {
    console_error_panic_hook::set_once();

    let app_state = Rc::new(RefCell::new(AppState::Menu));
    let click_state = Rc::new(RefCell::new(ClickState::new()));
    let game_time = Rc::new(RefCell::new(GameTime::new(10)));
    let menu_anim = Rc::new(Cell::new(0u32));
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
        let menu_anim = menu_anim.clone();
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

            // Increment menu animation counter
            if delta_ticks > 0 {
                menu_anim.set(menu_anim.get().wrapping_add(delta_ticks));
            }

            let mut state = app_state.borrow_mut();
            match &mut *state {
                AppState::Menu => {
                    render_menu(f, size, &click_state, menu_anim.get());
                }
                AppState::Settings { confirm_reset } => {
                    render_settings(f, size, &click_state, confirm_reset.as_ref());
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
                    #[allow(clippy::disallowed_methods)] // single back button
                    click_state
                        .borrow_mut()
                        .add_click_target(back_area, BACK_TO_MENU);
                }
            }
        }
    });

    Ok(())
}

// ── Menu animation constants ──────────────────────────────────
/// Braille spinner for menu decorations (smooth 8-frame rotation).
const MENU_SPINNER: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

/// Animated game icons for each menu entry (cycle per game).
const ICON_COOKIE: &[&str] = &["🍪", "🍩", "🍪", "✨"];
const ICON_FACTORY: &[&str] = &["⚙", "⛏", "🔧", "⚡"];
const ICON_CAREER: &[&str] = &["💼", "📈", "🎓", "💰"];
const ICON_RPG: &[&str] = &["⚔", "🛡", "🗡", "🐉"];

/// Wave characters for footer animation.
const WAVE: &[&str] = &[
    "░▒▓█▓▒░  ",
    " ░▒▓█▓▒░ ",
    "  ░▒▓█▓▒░",
    " ░▒▓█▓▒░ ",
];

fn render_menu(
    f: &mut ratzilla::ratatui::Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
    anim_frame: u32,
) {
    let is_narrow = is_narrow_layout(area.width);

    // Title height: 5 lines for wide (banner), 3 for narrow
    let title_height = if is_narrow { 3 } else { 5 };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(title_height), // Title banner
            Constraint::Min(8),              // Menu items
            Constraint::Length(3),            // Footer
        ])
        .split(area);

    let borders = if is_narrow {
        Borders::TOP | Borders::BOTTOM
    } else {
        Borders::ALL
    };

    // ── Animated title banner ──
    let spinner_idx = (anim_frame / 2) as usize % MENU_SPINNER.len();
    let spinner = MENU_SPINNER[spinner_idx];

    // Color cycling for title
    let title_color = match (anim_frame / 8) % 4 {
        0 => Color::Cyan,
        1 => Color::LightCyan,
        2 => Color::White,
        _ => Color::LightCyan,
    };

    let border_color = match (anim_frame / 12) % 3 {
        0 => Color::Cyan,
        1 => Color::Blue,
        _ => Color::LightCyan,
    };

    if is_narrow {
        let title_widget = Paragraph::new(Line::from(vec![
            Span::styled(
                format!("{} ", spinner),
                Style::default().fg(Color::Cyan),
            ),
            Span::styled(
                "Game Select",
                Style::default()
                    .fg(title_color)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" {}", spinner),
                Style::default().fg(Color::Cyan),
            ),
        ]))
        .block(
            Block::default()
                .borders(borders)
                .border_style(Style::default().fg(border_color)),
        )
        .alignment(Alignment::Center);
        f.render_widget(title_widget, chunks[0]);
    } else {
        // Wide: Animated ASCII art banner
        let deco_phase = (anim_frame / 4) as usize % 4;
        let deco_chars = ["╍╍╍", "━━━", "═══", "━━━"];
        let deco = deco_chars[deco_phase];

        let mut title_lines = Vec::new();
        title_lines.push(Line::from(vec![
            Span::styled(
                format!(" {} ╔{}╗ ", spinner, "═".repeat(32)),
                Style::default().fg(border_color),
            ),
        ]));
        title_lines.push(Line::from(vec![
            Span::styled("   ║  ", Style::default().fg(border_color)),
            Span::styled(
                format!("{} GAME SELECT {}", deco, deco),
                Style::default()
                    .fg(title_color)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("   ║ {}", spinner),
                Style::default().fg(border_color),
            ),
        ]));
        title_lines.push(Line::from(vec![
            Span::styled(
                format!("   ╚{}╝  ", "═".repeat(32)),
                Style::default().fg(border_color),
            ),
        ]));

        let title_widget = Paragraph::new(title_lines).block(
            Block::default()
                .borders(borders)
                .border_style(Style::default().fg(border_color)),
        );
        f.render_widget(title_widget, chunks[0]);
    }

    // ── Menu items with animated game icons ──
    let mut cl = ClickableList::new();
    let icon_phase = (anim_frame / 5) as usize;

    // Cookie Factory
    let cookie_icon = ICON_COOKIE[icon_phase % ICON_COOKIE.len()];
    cl.push(Line::from(""));
    cl.push_clickable(Line::from(vec![
        Span::styled(
            format!(" {} ", cookie_icon),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("Cookie Factory", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
    ]), MENU_SELECT_COOKIE);
    cl.push_clickable(Line::from(Span::styled(
        "    クッキーをクリックして増やす放置ゲーム",
        Style::default().fg(Color::DarkGray),
    )), MENU_SELECT_COOKIE);

    // Tiny Factory
    let factory_icon = ICON_FACTORY[icon_phase % ICON_FACTORY.len()];
    cl.push(Line::from(""));
    cl.push_clickable(Line::from(vec![
        Span::styled(
            format!(" {} ", factory_icon),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("Tiny Factory", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
    ]), MENU_SELECT_FACTORY);
    cl.push_clickable(Line::from(Span::styled(
        "    工場を作って生産ラインを最適化する放置ゲーム",
        Style::default().fg(Color::DarkGray),
    )), MENU_SELECT_FACTORY);

    // Career Simulator
    let career_icon = ICON_CAREER[icon_phase % ICON_CAREER.len()];
    cl.push(Line::from(""));
    cl.push_clickable(Line::from(vec![
        Span::styled(
            format!(" {} ", career_icon),
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("Career Simulator", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
    ]), MENU_SELECT_CAREER);
    cl.push_clickable(Line::from(Span::styled(
        "    スキルを磨いて転職・投資でキャリアを築くシミュレーション",
        Style::default().fg(Color::DarkGray),
    )), MENU_SELECT_CAREER);

    // Dungeon Dive
    let rpg_icon = ICON_RPG[icon_phase % ICON_RPG.len()];
    cl.push(Line::from(""));
    cl.push_clickable(Line::from(vec![
        Span::styled(
            format!(" {} ", rpg_icon),
            Style::default()
                .fg(Color::Red)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("Dungeon Dive", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
    ]), MENU_SELECT_RPG);
    cl.push_clickable(Line::from(Span::styled(
        "    ダンジョンを探索して帰還するローグライト風RPG",
        Style::default().fg(Color::DarkGray),
    )), MENU_SELECT_RPG);

    // Settings
    let settings_spin = if (anim_frame / 6).is_multiple_of(2) { "⚙" } else { "⛭" };
    cl.push(Line::from(""));
    cl.push_clickable(Line::from(vec![
        Span::styled(
            format!(" {} ", settings_spin),
            Style::default()
                .fg(Color::Gray)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("設定", Style::default().fg(Color::Gray)),
    ]), MENU_SELECT_SETTINGS);
    cl.push_clickable(Line::from(Span::styled(
        "    セーブデータの管理",
        Style::default().fg(Color::DarkGray),
    )), MENU_SELECT_SETTINGS);

    let menu_block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Green))
        .title(" Games ");
    {
        let mut cs = click_state.borrow_mut();
        cl.render(f, chunks[1], menu_block, &mut cs, false, 0);
    }

    // ── Animated footer ──
    let wave_idx = (anim_frame / 3) as usize % WAVE.len();
    let wave = WAVE[wave_idx];
    let footer_color = match (anim_frame / 10) % 3 {
        0 => Color::DarkGray,
        1 => Color::Gray,
        _ => Color::DarkGray,
    };

    let footer_widget = Paragraph::new(Line::from(vec![
        Span::styled(wave, Style::default().fg(footer_color)),
        Span::styled(
            " タップでゲームを選択 ",
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(wave, Style::default().fg(footer_color)),
    ]))
    .block(
        Block::default()
            .borders(borders)
            .border_style(Style::default().fg(Color::DarkGray)),
    )
    .alignment(Alignment::Center);
    f.render_widget(footer_widget, chunks[2]);
}

fn render_settings(
    f: &mut ratzilla::ratatui::Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
    confirm_reset: Option<&GameChoice>,
) {
    let is_narrow = is_narrow_layout(area.width);
    let borders = if is_narrow {
        Borders::TOP | Borders::BOTTOM
    } else {
        Borders::ALL
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Title
            Constraint::Min(8),   // Content
            Constraint::Length(3), // Footer
        ])
        .split(area);

    // Title
    let title_widget = Paragraph::new(Line::from(Span::styled(
        "設定",
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

    if let Some(game) = confirm_reset {
        render_confirm_dialog(f, chunks[1], click_state, borders, game);
    } else {
        render_settings_main(f, chunks[1], click_state, borders);
    }

    // Footer — back to menu
    let mut cl = ClickableList::new();
    cl.push_clickable(
        Line::from(Span::styled(
            "◀ メニューに戻る",
            Style::default().fg(Color::DarkGray),
        )),
        BACK_TO_MENU,
    );
    let footer_block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::DarkGray));
    {
        let mut cs = click_state.borrow_mut();
        cl.render(f, chunks[2], footer_block, &mut cs, false, 0);
    }
}

fn render_settings_main(
    f: &mut ratzilla::ratatui::Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
    borders: Borders,
) {
    let mut cl = ClickableList::new();

    cl.push(Line::from(""));
    cl.push(Line::from(Span::styled(
        " セーブデータ管理",
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    )));
    cl.push(Line::from(""));

    // Cookie Factory
    cl.push_clickable(
        Line::from(vec![
            Span::styled(" ✕ ", Style::default().fg(Color::Red)),
            Span::styled("Cookie Factory", Style::default().fg(Color::White)),
            Span::styled(" — データをリセット", Style::default().fg(Color::DarkGray)),
        ]),
        SETTINGS_RESET_COOKIE,
    );

    cl.push(Line::from(""));

    // Career Simulator
    cl.push_clickable(
        Line::from(vec![
            Span::styled(" ✕ ", Style::default().fg(Color::Red)),
            Span::styled("Career Simulator", Style::default().fg(Color::White)),
            Span::styled(" — データをリセット", Style::default().fg(Color::DarkGray)),
        ]),
        SETTINGS_RESET_CAREER,
    );

    cl.push(Line::from(""));
    cl.push(Line::from(""));
    cl.push(Line::from(Span::styled(
        " ※ Tiny Factory と Dungeon Dive は",
        Style::default().fg(Color::DarkGray),
    )));
    cl.push(Line::from(Span::styled(
        "   セーブデータがありません",
        Style::default().fg(Color::DarkGray),
    )));

    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Green))
        .title(" Data Reset ");
    {
        let mut cs = click_state.borrow_mut();
        cl.render(f, area, block, &mut cs, false, 0);
    }
}

fn render_confirm_dialog(
    f: &mut ratzilla::ratatui::Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
    borders: Borders,
    game: &GameChoice,
) {
    let game_name = match game {
        GameChoice::Cookie => "Cookie Factory",
        GameChoice::Career => "Career Simulator",
        _ => "Unknown",
    };

    let mut cl = ClickableList::new();

    cl.push(Line::from(""));
    cl.push(Line::from(Span::styled(
        format!(" {game_name} のセーブデータを"),
        Style::default().fg(Color::White),
    )));
    cl.push(Line::from(Span::styled(
        " 本当にリセットしますか？",
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    )));
    cl.push(Line::from(""));
    cl.push(Line::from(Span::styled(
        " ※ この操作は取り消せません",
        Style::default().fg(Color::Red),
    )));
    cl.push(Line::from(""));

    cl.push_clickable(
        Line::from(Span::styled(
            " ▶ はい、リセットする",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )),
        SETTINGS_CONFIRM_YES,
    );
    cl.push(Line::from(""));
    cl.push_clickable(
        Line::from(Span::styled(
            " ▶ キャンセル",
            Style::default().fg(Color::Green),
        )),
        SETTINGS_CONFIRM_NO,
    );

    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Red))
        .title(" 確認 ");
    {
        let mut cs = click_state.borrow_mut();
        cl.render(f, area, block, &mut cs, false, 0);
    }
}
