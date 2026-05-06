mod games;
mod input;
mod time;
mod widgets;

use std::{cell::RefCell, io, rc::Rc};

use games::{create_game, AppState, GameChoice};
use input::{
    is_narrow_layout, pixel_x_to_col, pixel_y_to_row, ClickScope, ClickState, InputEvent,
};
use widgets::{Clickable, ClickableList};
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
pub const MENU_SELECT_CAFE: u16 = 5;
pub const MENU_SELECT_ABYSS: u16 = 6;
pub const MENU_SELECT_GODFIELD: u16 = 7;
pub const MENU_SELECT_METROPOLIS: u16 = 8;
pub const MENU_SELECT_SETTINGS: u16 = 9;
pub const MENU_SCROLL_UP: u16 = 10;
pub const MENU_SCROLL_DOWN: u16 = 11;
pub const BACK_TO_MENU: u16 = 65535;

/// Last valid index of the main menu cards (8 games + settings → 0..=8).
const MENU_LAST_INDEX: u8 = 8;

/// Cursor → menu action, used for the A button on the main menu.
enum MenuPick {
    Game(GameChoice),
    Settings,
}

fn menu_pick_for(idx: u8) -> MenuPick {
    match idx {
        0 => MenuPick::Game(GameChoice::Cookie),
        1 => MenuPick::Game(GameChoice::Factory),
        2 => MenuPick::Game(GameChoice::Career),
        3 => MenuPick::Game(GameChoice::Rpg),
        4 => MenuPick::Game(GameChoice::Cafe),
        5 => MenuPick::Game(GameChoice::Abyss),
        6 => MenuPick::Game(GameChoice::Godfield),
        7 => MenuPick::Game(GameChoice::Metropolis),
        _ => MenuPick::Settings,
    }
}

// ── Settings action IDs ─────────────────────────────────────────
const SETTINGS_RESET_COOKIE: u16 = 10;
const SETTINGS_RESET_CAREER: u16 = 11;
const SETTINGS_CONFIRM_YES: u16 = 12;
const SETTINGS_CONFIRM_NO: u16 = 13;
const SETTINGS_RESET_ABYSS: u16 = 14;
const SETTINGS_RESET_METROPOLIS: u16 = 15;

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

/// Fallback: derive `(row, col)` from the click position relative to the grid
/// container's bounding rect.  Used when [`dom_element_to_cell`] returns
/// `None` — typically because an overlay element, browser zoom, or CSS
/// transform put something other than `<pre>` at the click point.
///
/// Less precise than the elementFromPoint path (assumes uniform cell size,
/// which can be off by a sub-pixel under zoom), but covers cases where the
/// primary path silently fails.
fn pixel_fallback_to_cell(
    client_x: f64,
    client_y: f64,
    terminal_cols: u16,
    terminal_rows: u16,
) -> Option<(u16, u16)> {
    let window = web_sys::window()?;
    let document = window.document()?;
    let grid = document
        .get_element_by_id("grid")
        .or_else(|| document.query_selector("body > div").ok().flatten())?;
    let rect = grid.get_bounding_client_rect();
    let local_x = client_x - rect.left();
    let local_y = client_y - rect.top();
    let row = pixel_y_to_row(local_y, rect.height(), terminal_rows)?;
    let col = pixel_x_to_col(local_x, rect.width(), terminal_cols)?;
    Some((row, col))
}

/// Read the current high-resolution timestamp (ms since page navigation).
/// Returns `None` when the Performance API is unavailable (rare, e.g.
/// headless or sandboxed contexts); callers must decide what `None` means
/// for them — for tap dedup, that means *skip* dedup rather than treating
/// every tap as happening "now".
fn now_ms() -> Option<f64> {
    web_sys::window()
        .and_then(|w| w.performance())
        .map(|p| p.now())
}

/// Process a tap/click at the given client coordinates.
///
/// `ClickState::try_consume_tap` drops compatibility mouse events that the
/// browser fires for the same touch (timestamp-based dedup), so a single
/// physical tap is dispatched once even if the render loop stutters between
/// the two synthesized events.
fn handle_tap(
    client_x: f64,
    client_y: f64,
    app_state: &Rc<RefCell<AppState>>,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let mut cs = click_state.borrow_mut();
    let (row, col) = match dom_element_to_cell(client_x, client_y, cs.terminal_cols) {
        Some(r) => r,
        None => {
            // elementFromPoint missed the <pre> row.  Try the pixel-based
            // fallback so an overlay or zoom edge case doesn't leave the
            // user with a silently dead tap.  Warn so the frequency is
            // observable in DevTools.
            web_sys::console::warn_1(
                &"click missed <pre>; trying pixel fallback".into(),
            );
            match pixel_fallback_to_cell(
                client_x,
                client_y,
                cs.terminal_cols,
                cs.terminal_rows,
            ) {
                Some(r) => r,
                None => return,
            }
        }
    };

    // Skip dedup entirely when the high-resolution clock is unavailable;
    // JS-side `e.preventDefault()` already suppresses the compatibility
    // event, so the only loss is the second-line-of-defence guarantee.
    // (Dropping every tap on the same cell because we'd otherwise compare
    // `0.0 - 0.0 < 30ms` would be a far worse failure mode.)
    if let Some(t) = now_ms() {
        if !cs.try_consume_tap(col, row, t) {
            return;
        }
    }

    if let Some(action_id) = cs.hit_test(col, row) {
        // Pair the action ID with the scope that registered the target so
        // the dispatcher can verify the click is bound for the screen the
        // user actually saw — protecting against late-arriving compatibility
        // events crossing a screen transition.
        let scope = cs
            .current_scope()
            .cloned()
            .unwrap_or(ClickScope::Menu);
        drop(cs);
        dispatch_event(&InputEvent::Click(scope, action_id), app_state);
    }
}

/// Returns `true` if the click's scope matches the currently active screen.
/// Stale clicks from a previous screen (rare but possible at screen
/// transitions) are caught here in debug builds and silently dropped in
/// release.
fn click_scope_matches_state(scope: &ClickScope, state: &AppState) -> bool {
    match (scope, state) {
        (ClickScope::Menu, AppState::Menu { .. }) => true,
        (ClickScope::Settings, AppState::Settings { .. }) => true,
        (ClickScope::Game(c), AppState::Playing { game }) => *c == game.choice(),
        _ => false,
    }
}

/// Dispatch an input event to the current app state.
fn dispatch_event(event: &InputEvent, app_state: &Rc<RefCell<AppState>>) {
    let mut state = app_state.borrow_mut();

    if let InputEvent::Click(scope, _) = event {
        if !click_scope_matches_state(scope, &state) {
            debug_assert!(
                false,
                "click scope {:?} doesn't match active state",
                scope,
            );
            // In release: drop the stale click rather than misroute it.
            return;
        }
    }

    match &mut *state {
        AppState::Menu { scroll, selected } => {
            let direct = match event {
                InputEvent::Key('1') | InputEvent::Click(_, MENU_SELECT_COOKIE) => {
                    Some(MenuPick::Game(GameChoice::Cookie))
                }
                InputEvent::Key('2') | InputEvent::Click(_, MENU_SELECT_FACTORY) => {
                    Some(MenuPick::Game(GameChoice::Factory))
                }
                InputEvent::Key('3') | InputEvent::Click(_, MENU_SELECT_CAREER) => {
                    Some(MenuPick::Game(GameChoice::Career))
                }
                InputEvent::Key('4') | InputEvent::Click(_, MENU_SELECT_RPG) => {
                    Some(MenuPick::Game(GameChoice::Rpg))
                }
                InputEvent::Key('5') | InputEvent::Click(_, MENU_SELECT_CAFE) => {
                    Some(MenuPick::Game(GameChoice::Cafe))
                }
                InputEvent::Key('6') | InputEvent::Click(_, MENU_SELECT_ABYSS) => {
                    Some(MenuPick::Game(GameChoice::Abyss))
                }
                InputEvent::Key('7') | InputEvent::Click(_, MENU_SELECT_GODFIELD) => {
                    Some(MenuPick::Game(GameChoice::Godfield))
                }
                InputEvent::Key('8') | InputEvent::Click(_, MENU_SELECT_METROPOLIS) => {
                    Some(MenuPick::Game(GameChoice::Metropolis))
                }
                InputEvent::Key('9') | InputEvent::Click(_, MENU_SELECT_SETTINGS) => {
                    Some(MenuPick::Settings)
                }
                // A button (' ' / Enter via main.rs key map) confirms the
                // currently highlighted card, so keyboard-only and tap users
                // share the same selection model.
                InputEvent::Key(' ') => Some(menu_pick_for(*selected)),
                _ => None,
            };
            if let Some(pick) = direct {
                match pick {
                    MenuPick::Game(choice) => {
                        let game = create_game(&choice);
                        *state = AppState::Playing { game };
                    }
                    MenuPick::Settings => {
                        *state = AppState::Settings { confirm_reset: None };
                    }
                }
            } else {
                match event {
                    // Arrow up/k: move highlight up. Auto-scroll so the
                    // selection always stays visible (keeps the UX usable
                    // when the menu list is taller than the viewport).
                    InputEvent::Key('k') | InputEvent::Click(_, MENU_SCROLL_UP) => {
                        *selected = selected.saturating_sub(1);
                        // 3 lines per game card → keep ~one card above
                        let target = (*selected as u16) * 3;
                        if *scroll > target {
                            *scroll = target;
                        }
                    }
                    InputEvent::Key('j') | InputEvent::Click(_, MENU_SCROLL_DOWN) => {
                        *selected = (*selected + 1).min(MENU_LAST_INDEX);
                        // No upper-bound auto-scroll here — render_menu
                        // re-clamps `scroll` against the actual viewport.
                        *scroll = scroll.saturating_add(0);
                    }
                    _ => {}
                }
            }
        }
        AppState::Settings { confirm_reset } => {
            if confirm_reset.is_some() {
                // Confirmation dialog is active
                match event {
                    InputEvent::Key('y') | InputEvent::Click(_, SETTINGS_CONFIRM_YES) => {
                        let game = confirm_reset.take().unwrap();
                        perform_reset(&game);
                        *state = AppState::Settings {
                            confirm_reset: None,
                        };
                    }
                    InputEvent::Key('n')
                    | InputEvent::Key('q')
                    | InputEvent::Click(_, SETTINGS_CONFIRM_NO) => {
                        *confirm_reset = None;
                    }
                    _ => {}
                }
            } else {
                match event {
                    InputEvent::Key('1') | InputEvent::Click(_, SETTINGS_RESET_COOKIE) => {
                        *confirm_reset = Some(GameChoice::Cookie);
                    }
                    InputEvent::Key('2') | InputEvent::Click(_, SETTINGS_RESET_CAREER) => {
                        *confirm_reset = Some(GameChoice::Career);
                    }
                    InputEvent::Key('3') | InputEvent::Click(_, SETTINGS_RESET_ABYSS) => {
                        *confirm_reset = Some(GameChoice::Abyss);
                    }
                    InputEvent::Key('4') | InputEvent::Click(_, SETTINGS_RESET_METROPOLIS) => {
                        *confirm_reset = Some(GameChoice::Metropolis);
                    }
                    InputEvent::Key('q') | InputEvent::Click(_, BACK_TO_MENU) => {
                        *state = AppState::Menu { scroll: 0, selected: 0 };
                    }
                    _ => {}
                }
            }
        }
        AppState::Playing { game } => {
            if matches!(event, InputEvent::Key('q') | InputEvent::Click(_, BACK_TO_MENU)) {
                // Let the game handle back first (e.g., sub-screen → main screen).
                // Only go to menu if the game didn't consume it.
                if !game.handle_input(event) {
                    *state = AppState::Menu { scroll: 0, selected: 0 };
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
        GameChoice::Abyss => games::abyss::save::delete_save(),
        GameChoice::Metropolis => games::metropolis::save::delete_save(),
        _ => {}
    }
    #[cfg(not(target_arch = "wasm32"))]
    let _ = game;
}

fn main() -> io::Result<()> {
    console_error_panic_hook::set_once();

    let app_state = Rc::new(RefCell::new(AppState::Menu { scroll: 0, selected: 0 }));
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
                // Enter is a synonym for the A button — confirms whatever
                // the cursor is currently highlighting (RPG menus, main
                // menu game selection, etc.). ' ' is the canonical char
                // for A; mapping Enter to it lets us reuse all existing
                // handlers without per-scene Enter wiring.
                KeyCode::Enter => InputEvent::Key(' '),
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

            // Get current timestamp for game time.  Without a high-res clock
            // the game effectively pauses (delta_ticks stays 0), which is
            // acceptable for the rare headless / no-Performance-API case.
            let delta_ticks = game_time.borrow_mut().update(now_ms().unwrap_or(0.0));

            let mut state = app_state.borrow_mut();
            // Stamp the frame with the scope of click targets it'll register,
            // so handle_tap can pair it with the action ID for dispatch-time
            // validation.
            click_state.borrow_mut().set_scope(match &*state {
                AppState::Menu { .. } => ClickScope::Menu,
                AppState::Settings { .. } => ClickScope::Settings,
                AppState::Playing { game } => ClickScope::Game(game.choice()),
            });
            match &mut *state {
                AppState::Menu { scroll, selected } => {
                    render_menu(f, size, &click_state, scroll, *selected);
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

                    // Overlay back button in top-left corner.  Registered
                    // last so it wins over any game-area target on overlap.
                    let back_area = Rect::new(size.x, size.y, 6, 1);
                    let back = Paragraph::new(Span::styled(
                        " ◀戻る",
                        Style::default().fg(Color::DarkGray),
                    ));
                    Clickable::new(back, BACK_TO_MENU).render(
                        f,
                        back_area,
                        &mut click_state.borrow_mut(),
                    );
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
    scroll: &mut u16,
    selected: u8,
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

    // Menu items — driven by a single source of truth (MENU_ENTRIES) so
    // adding a new game is one entry edit. Each card occupies 3 visual
    // rows: blank / title / description; sharing the action ID across
    // title + desc lets the player tap either row.
    type Entry = (&'static str, &'static str, u16, char);
    const MENU_ENTRIES: &[Entry] = &[
        ("Cookie Factory", "クッキーをクリックして増やす放置ゲーム", MENU_SELECT_COOKIE, '▶'),
        ("Tiny Factory", "工場を作って生産ラインを最適化する放置ゲーム", MENU_SELECT_FACTORY, '▶'),
        ("Career Simulator", "スキルを磨いて転職・投資でキャリアを築くシミュレーション", MENU_SELECT_CAREER, '▶'),
        ("Dungeon Dive", "ダンジョンを探索して帰還するローグライト風RPG", MENU_SELECT_RPG, '▶'),
        ("廃墟カフェ復興記", "廃墟カフェを復興するシナリオ経営SLG", MENU_SELECT_CAFE, '▶'),
        ("深淵潜行 (Abyss Idle)", "自動戦闘で深層を目指す放置型ローグダンジョン", MENU_SELECT_ABYSS, '▶'),
        ("神の戦場 (God Field)", "4人で戦うターン制カードバトルロイヤル", MENU_SELECT_GODFIELD, '▶'),
        ("Idle Metropolis", "AIが街を建てるのを眺める放置シティビルダー", MENU_SELECT_METROPOLIS, '▶'),
        ("設定", "セーブデータの管理", MENU_SELECT_SETTINGS, '⚙'),
    ];

    let mut cl = ClickableList::new();
    for (i, (name, desc, action_id, default_marker)) in MENU_ENTRIES.iter().enumerate() {
        let is_selected = i as u8 == selected;
        // Highlighted card: solid yellow ▶ marker + bold yellow title.
        // Unselected: same shape but muted, so the layout doesn't shift
        // when the cursor moves.
        let marker = if is_selected { '▶' } else { *default_marker };
        let marker_style = if is_selected {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else if *default_marker == '⚙' {
            Style::default().fg(Color::Gray).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let title_style = if is_selected {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else if *default_marker == '⚙' {
            Style::default().fg(Color::Gray)
        } else {
            Style::default().fg(Color::White)
        };
        cl.push(Line::from(""));
        cl.push_clickable(
            Line::from(vec![
                Span::styled(format!(" {} ", marker), marker_style),
                Span::styled(*name, title_style),
            ]),
            *action_id,
        );
        cl.push_clickable(
            Line::from(Span::styled(
                format!("    {}", desc),
                Style::default().fg(Color::DarkGray),
            )),
            *action_id,
        );
    }

    let menu_block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Green))
        .title(" Games ");

    // Clamp scroll to content height. With wrap=false each logical line is
    // exactly one visual row, so visible_rows is the inner height.
    let inner = menu_block.inner(chunks[1]);
    let total_lines = cl.len() as u16;
    let visible_rows = inner.height;
    let max_scroll = total_lines.saturating_sub(visible_rows);
    if *scroll > max_scroll {
        *scroll = max_scroll;
    }

    // Auto-scroll so the highlighted card stays visible. Each card spans
    // 3 rows (blank / title / desc), with the title at row 3*selected + 1.
    // We aim to keep the title row inside [scroll, scroll + visible_rows).
    let card_top = (selected as u16) * 3;
    let card_bottom = card_top + 3;
    if card_top < *scroll {
        *scroll = card_top;
    } else if visible_rows > 0 && card_bottom > *scroll + visible_rows {
        *scroll = card_bottom.saturating_sub(visible_rows);
    }
    if *scroll > max_scroll {
        *scroll = max_scroll;
    }
    let can_scroll_up = *scroll > 0;
    let can_scroll_down = *scroll < max_scroll;
    let scroll_value = *scroll;

    {
        let mut cs = click_state.borrow_mut();
        cl.render(f, chunks[1], menu_block, &mut cs, false, scroll_value);
    }

    // Scroll indicator overlays — registered last so they win over rows below.
    if can_scroll_up && inner.height > 0 && inner.width > 0 {
        let arrow_area = Rect::new(inner.x + inner.width - 3, inner.y, 3, 1);
        let arrow = Paragraph::new(Span::styled(
            " ▲ ",
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        ));
        Clickable::new(arrow, MENU_SCROLL_UP).render(
            f,
            arrow_area,
            &mut click_state.borrow_mut(),
        );
    }
    if can_scroll_down && inner.height > 0 && inner.width > 0 {
        let arrow_area = Rect::new(
            inner.x + inner.width - 3,
            inner.y + inner.height - 1,
            3,
            1,
        );
        let arrow = Paragraph::new(Span::styled(
            " ▼ ",
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        ));
        Clickable::new(arrow, MENU_SCROLL_DOWN).render(
            f,
            arrow_area,
            &mut click_state.borrow_mut(),
        );
    }

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

    // 深淵潜行 (Abyss Idle)
    cl.push_clickable(
        Line::from(vec![
            Span::styled(" ✕ ", Style::default().fg(Color::Red)),
            Span::styled("深淵潜行", Style::default().fg(Color::White)),
            Span::styled(" — データをリセット", Style::default().fg(Color::DarkGray)),
        ]),
        SETTINGS_RESET_ABYSS,
    );

    cl.push(Line::from(""));

    // Idle Metropolis
    cl.push_clickable(
        Line::from(vec![
            Span::styled(" ✕ ", Style::default().fg(Color::Red)),
            Span::styled("Idle Metropolis", Style::default().fg(Color::White)),
            Span::styled(" — データをリセット", Style::default().fg(Color::DarkGray)),
        ]),
        SETTINGS_RESET_METROPOLIS,
    );

    cl.push(Line::from(""));
    cl.push(Line::from(""));
    cl.push(Line::from(Span::styled(
        " ※ Tiny Factory / Dungeon Dive は",
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
        GameChoice::Cafe => "廃墟カフェ復興記",
        GameChoice::Abyss => "深淵潜行",
        GameChoice::Metropolis => "Idle Metropolis",
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
