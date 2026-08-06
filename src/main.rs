use std::{
    cell::{Cell, RefCell},
    f64::consts::TAU,
    io,
    rc::Rc,
};

use cli_sim_game_escape::canvas_fx;
use cli_sim_game_escape::games::{create_game, AppState, GameChoice};
use cli_sim_game_escape::input::{
    is_narrow_layout, pixel_x_to_col, pixel_y_to_row, ClickScope, ClickState, InputEvent,
};
use cli_sim_game_escape::sound;
use cli_sim_game_escape::theme;
use cli_sim_game_escape::widgets::{line_visual_height, Clickable, ClickableList, ScrollableTab};
use cli_sim_game_escape::time::{now_ms, GameTime};
use cli_sim_game_escape::BACK_TO_MENU;

use ratzilla::event::{KeyCode, MouseButton, MouseEventKind};
use ratzilla::ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratzilla::ratatui::style::{Color, Modifier, Style};
use ratzilla::ratatui::symbols::Marker;
use ratzilla::ratatui::text::{Line, Span};
use ratzilla::ratatui::widgets::canvas::{Canvas, Line as CanvasLine, Points};
use ratzilla::ratatui::widgets::{Block, BorderType, Borders, Paragraph};
use ratzilla::ratatui::Terminal;
use ratzilla::{DomBackend, WebRenderer};

// ── Menu action IDs ─────────────────────────────────────────────
pub const MENU_SELECT_COOKIE: u16 = 1;
pub const MENU_SELECT_FACTORY: u16 = 2;
pub const MENU_SELECT_RPG: u16 = 3;
pub const MENU_SELECT_ABYSS: u16 = 4;
pub const MENU_SELECT_GODFIELD: u16 = 5;
pub const MENU_SELECT_METROPOLIS: u16 = 6;
pub const MENU_SELECT_SETTINGS: u16 = 7;
pub const MENU_SCROLL_UP: u16 = 8;
pub const MENU_SCROLL_DOWN: u16 = 9;
// 1-9 は Menu scope で使い切っているため、Settings scope (10-15) の
// 次番から採る。scope が異なる衝突は無害 (ClickScope で分離される) だが、
// 追加者が採番に迷わないよう連続させている。
pub const MENU_SELECT_LOOPMARCH: u16 = 16;
pub const MENU_SELECT_EVERLIGHT: u16 = 19;
pub const MENU_SELECT_SHATTERLAB: u16 = 21;

/// Last valid index of the main menu cards (9 games + settings → 0..=9).
const MENU_LAST_INDEX: u8 = 9;

/// Cursor → menu action, used for the A button on the main menu.
enum MenuPick {
    Game(GameChoice),
    Settings,
}

fn menu_pick_for(idx: u8) -> MenuPick {
    match idx {
        0 => MenuPick::Game(GameChoice::Cookie),
        1 => MenuPick::Game(GameChoice::Factory),
        2 => MenuPick::Game(GameChoice::Rpg),
        3 => MenuPick::Game(GameChoice::Abyss),
        4 => MenuPick::Game(GameChoice::Godfield),
        5 => MenuPick::Game(GameChoice::Metropolis),
        6 => MenuPick::Game(GameChoice::LoopMarch),
        7 => MenuPick::Game(GameChoice::Everlight),
        8 => MenuPick::Game(GameChoice::ShatterLab),
        _ => MenuPick::Settings,
    }
}

// ── Settings action IDs ─────────────────────────────────────────
const SETTINGS_RESET_COOKIE: u16 = 10;
const SETTINGS_RESET_ABYSS: u16 = 11;
const SETTINGS_RESET_METROPOLIS: u16 = 12;
const SETTINGS_CONFIRM_YES: u16 = 13;
const SETTINGS_CONFIRM_NO: u16 = 14;
const SETTINGS_RESET_LOOPMARCH: u16 = 15;
const SETTINGS_SCROLL_UP: u16 = 17;
const SETTINGS_SCROLL_DOWN: u16 = 18;
const SETTINGS_RESET_EVERLIGHT: u16 = 20;
/// 1クリック/1行キー入力あたりのスクロール量。
const SETTINGS_SCROLL_STEP: i32 = 3;

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
        AppState::Menu { scroll: _, selected } => {
            let direct = match event {
                InputEvent::Key('1') | InputEvent::Click(_, MENU_SELECT_COOKIE) => {
                    Some(MenuPick::Game(GameChoice::Cookie))
                }
                InputEvent::Key('2') | InputEvent::Click(_, MENU_SELECT_FACTORY) => {
                    Some(MenuPick::Game(GameChoice::Factory))
                }
                InputEvent::Key('3') | InputEvent::Click(_, MENU_SELECT_RPG) => {
                    Some(MenuPick::Game(GameChoice::Rpg))
                }
                InputEvent::Key('4') | InputEvent::Click(_, MENU_SELECT_ABYSS) => {
                    Some(MenuPick::Game(GameChoice::Abyss))
                }
                InputEvent::Key('5') | InputEvent::Click(_, MENU_SELECT_GODFIELD) => {
                    Some(MenuPick::Game(GameChoice::Godfield))
                }
                InputEvent::Key('6') | InputEvent::Click(_, MENU_SELECT_METROPOLIS) => {
                    Some(MenuPick::Game(GameChoice::Metropolis))
                }
                InputEvent::Key('7') | InputEvent::Click(_, MENU_SELECT_LOOPMARCH) => {
                    Some(MenuPick::Game(GameChoice::LoopMarch))
                }
                InputEvent::Key('8') | InputEvent::Click(_, MENU_SELECT_EVERLIGHT) => {
                    Some(MenuPick::Game(GameChoice::Everlight))
                }
                InputEvent::Key('9') | InputEvent::Click(_, MENU_SELECT_SHATTERLAB) => {
                    Some(MenuPick::Game(GameChoice::ShatterLab))
                }
                InputEvent::Key('0') | InputEvent::Click(_, MENU_SELECT_SETTINGS) => {
                    Some(MenuPick::Settings)
                }
                // A button (' ' / Enter via main.rs key map) confirms the
                // currently highlighted card, so keyboard-only and tap users
                // share the same selection model.
                InputEvent::Key(' ') => Some(menu_pick_for(*selected)),
                _ => None,
            };
            if let Some(pick) = direct {
                sound::play(sound::SELECT);
                match pick {
                    MenuPick::Game(choice) => {
                        let game = create_game(&choice);
                        *state = AppState::Playing { game };
                    }
                    MenuPick::Settings => {
                        *state = AppState::Settings {
                            confirm_reset: None,
                            scroll: Cell::new(0),
                        };
                    }
                }
            } else {
                match event {
                    // Arrow up/k, down/j: move the highlight. `render_menu`
                    // re-clamps `scroll` every frame using each card's actual
                    // (wrap-aware, so variable-height) row count, so the
                    // selection is guaranteed to stay visible without this
                    // handler needing to duplicate that layout math.
                    InputEvent::Key('k') | InputEvent::Click(_, MENU_SCROLL_UP) => {
                        let before = *selected;
                        *selected = selected.saturating_sub(1);
                        if *selected != before {
                            sound::play(sound::CLICK);
                        }
                    }
                    InputEvent::Key('j') | InputEvent::Click(_, MENU_SCROLL_DOWN) => {
                        let before = *selected;
                        *selected = (*selected + 1).min(MENU_LAST_INDEX);
                        if *selected != before {
                            sound::play(sound::CLICK);
                        }
                    }
                    _ => {}
                }
            }
        }
        AppState::Settings { confirm_reset, scroll } => {
            if confirm_reset.is_some() {
                // Confirmation dialog is active
                match event {
                    InputEvent::Key('y') | InputEvent::Click(_, SETTINGS_CONFIRM_YES) => {
                        let game = confirm_reset.take().unwrap();
                        perform_reset(&game);
                        *state = AppState::Settings {
                            confirm_reset: None,
                            scroll: Cell::new(0),
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
                    InputEvent::Key('2') | InputEvent::Click(_, SETTINGS_RESET_ABYSS) => {
                        *confirm_reset = Some(GameChoice::Abyss);
                    }
                    InputEvent::Key('3') | InputEvent::Click(_, SETTINGS_RESET_METROPOLIS) => {
                        *confirm_reset = Some(GameChoice::Metropolis);
                    }
                    InputEvent::Key('4') | InputEvent::Click(_, SETTINGS_RESET_LOOPMARCH) => {
                        *confirm_reset = Some(GameChoice::LoopMarch);
                    }
                    InputEvent::Key('5') | InputEvent::Click(_, SETTINGS_RESET_EVERLIGHT) => {
                        *confirm_reset = Some(GameChoice::Everlight);
                    }
                    InputEvent::Key('k') | InputEvent::Click(_, SETTINGS_SCROLL_UP) => {
                        adjust_scroll(scroll, -SETTINGS_SCROLL_STEP);
                    }
                    InputEvent::Key('j') | InputEvent::Click(_, SETTINGS_SCROLL_DOWN) => {
                        adjust_scroll(scroll, SETTINGS_SCROLL_STEP);
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
                    game.on_leave();
                    *state = AppState::Menu { scroll: 0, selected: 0 };
                }
            } else {
                game.handle_input(event);
            }
        }
    }
}

/// `Cell<u16>` スクロール値を負にならないよう飽和加算/減算で更新する。
/// 上限側のクランプは描画側 (`ScrollableTab`) がコンテンツ高さに合わせて行う。
fn adjust_scroll(cell: &Cell<u16>, delta: i32) {
    let cur = cell.get() as i32;
    let next = (cur + delta).clamp(0, u16::MAX as i32) as u16;
    cell.set(next);
}

/// Delete localStorage save data for the specified game.
fn perform_reset(game: &GameChoice) {
    #[cfg(target_arch = "wasm32")]
    match game {
        GameChoice::Cookie => cli_sim_game_escape::games::cookie::save::delete_save(),
        GameChoice::Abyss => cli_sim_game_escape::games::abyss::save::delete_save(),
        GameChoice::Metropolis => cli_sim_game_escape::games::metropolis::save::delete_save(),
        GameChoice::LoopMarch => cli_sim_game_escape::games::loopmarch::save::delete_save(),
        GameChoice::Everlight => cli_sim_game_escape::games::everlight::save::delete_save(),
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
                AppState::Settings { confirm_reset, scroll } => {
                    render_settings(f, size, &click_state, confirm_reset.as_ref(), scroll);
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

/// 指定 `width` で wrap した時の visual 行数。`widgets::line_visual_height` に
/// 委譲し、実際の render 時の wrap 計算と一致させる (drift しない)。
fn wrapped_line_height(text: &str, width: u16) -> u16 {
    line_visual_height(&Line::from(text), width)
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
            Constraint::Min(8),   // Menu items (+ 装飾パネル)
            Constraint::Length(3), // Footer
        ])
        .split(area);

    let borders = if is_narrow {
        Borders::TOP | Borders::BOTTOM
    } else {
        Borders::ALL
    };

    // Title — Double border でホーム画面としての存在感を強める (通常画面は
    // Borders::ALL/Plain のままにして、メニューだけ一段強い枠にする)。
    let title = if is_narrow {
        "Game Select"
    } else {
        "Game Select - ゲームを選んでください"
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
            .border_type(BorderType::Double)
            .border_style(Style::default().fg(Color::Cyan)),
    )
    .alignment(Alignment::Center);
    f.render_widget(title_widget, chunks[0]);

    // ワイドレイアウトのみ、リストの右側にドット(Braille)表現の軌道パネルを
    // 添える。everlight拠点画面の`render_camp_ambience`と同じ「Canvas+
    // Brailleの質感をゲーム外の画面にも持ち込む」作法。ナロー(モバイル)幅
    // では画面が足りずリストの可読性を優先し、パネルは出さない (同じ理由の
    // 判断を`render_camp`側でも既に取っている)。
    let (list_area, orbit_area) = if is_narrow {
        (chunks[1], None)
    } else {
        let hchunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(30), Constraint::Length(22)])
            .split(chunks[1]);
        (hchunks[0], Some(hchunks[1]))
    };

    // Menu items — driven by a single source of truth (MENU_ENTRIES) so
    // adding a new game is one entry edit. Each card occupies 3 visual
    // rows: blank / title / description; sharing the action ID across
    // title + desc lets the player tap either row.
    // accent はゲームの「顔」となる固有色。タイトル文字に常時乗せることで、
    // 一覧をスクロールした時にどのゲームか色で識別できる。ゲーム内 UI とも
    // 共有できるよう `theme::accent` を単一ソースにしている
    // (「設定」は GameChoice を持たないメニュー項目なので固定色のまま)。
    // 先頭の digit は数字キーによるショートカット (dispatch_event 参照) を
    // 常時可視化する — 存在を知らないと使われない機能だったため。
    type Entry = (char, &'static str, &'static str, u16, char, Color);
    const MENU_ENTRIES: &[Entry] = &[
        ('1', "Cookie Factory", "クッキーをクリックして増やす放置ゲーム", MENU_SELECT_COOKIE, '▶', theme::accent(&GameChoice::Cookie)),
        ('2', "Tiny Factory", "工場を作って生産ラインを最適化する放置ゲーム", MENU_SELECT_FACTORY, '▶', theme::accent(&GameChoice::Factory)),
        ('3', "Dungeon Dive", "ダンジョンを探索して帰還するローグライト風RPG", MENU_SELECT_RPG, '▶', theme::accent(&GameChoice::Rpg)),
        ('4', "深淵潜行 (Abyss Idle)", "自動戦闘で深層を目指す放置型ローグダンジョン", MENU_SELECT_ABYSS, '▶', theme::accent(&GameChoice::Abyss)),
        ('5', "神の戦場 (God Field)", "4人で戦うターン制カードバトルロイヤル", MENU_SELECT_GODFIELD, '▶', theme::accent(&GameChoice::Godfield)),
        ('6', "Idle Metropolis", "AIが街を建てるのを眺める放置シティビルダー", MENU_SELECT_METROPOLIS, '▶', theme::accent(&GameChoice::Metropolis)),
        ('7', "周回討伐", "地形を配置し勇者が自動周回するローグライト", MENU_SELECT_LOOPMARCH, '▶', theme::accent(&GameChoice::LoopMarch)),
        ('8', "常夜灯", "降り注ぐ魔物から灯を守る縦画面バレットヘヴン", MENU_SELECT_EVERLIGHT, '▶', theme::accent(&GameChoice::Everlight)),
        ('9', "破壊VFXラボ", "破壊表現の試作を並べて見比べる（本編ではない）", MENU_SELECT_SHATTERLAB, '▶', theme::accent(&GameChoice::ShatterLab)),
        ('0', "設定", "セーブデータの管理", MENU_SELECT_SETTINGS, '⚙', Color::Gray),
    ];

    let menu_block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Green))
        .title(" Games ");
    // wrap=true で render するので、事前計算も同じ inner width で行う (行数は
    // `ClickableList::visual_height` / `render` 内の wrap と一致させる必要がある)。
    let inner = menu_block.inner(list_area);

    let mut cl = ClickableList::new();
    // 各カードは blank(1) + title(1) + desc(wrap後の可変行数) 行で構成される。
    // ナロー幅では説明文がタイトルより長いことが多く、wrap 無しだと単語途中で
    // 見切れていたため、カードの高さを可変にしてスクロール計算もそれに追従させる。
    let mut cumulative_rows: u16 = 0;
    let mut selected_card_top: u16 = 0;
    let mut selected_card_bottom: u16 = 0;
    for (i, (digit, name, desc, action_id, default_marker, accent)) in MENU_ENTRIES.iter().enumerate() {
        let is_selected = i as u8 == selected;
        let card_top = cumulative_rows;
        // Highlighted card: solid yellow ▶ marker + bold yellow title.
        // Unselected: same shape but muted accent color, so the layout
        // doesn't shift when the cursor moves and each game keeps its hue.
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
            Style::default().fg(*accent)
        };
        // digit バッジ: 選択中は反転(黄地に黒文字)でホットキーが今どれかを
        // 目立たせる。rail (│) も選択中のカードだけ黄色にして揃える。
        let digit_style = if is_selected {
            Style::default().fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let rail_style = Style::default().fg(if is_selected { Color::Yellow } else { Color::DarkGray });

        cl.push(Line::from(""));
        let title_text = format!(" {} │ {} {}", digit, marker, name);
        let title_rows = wrapped_line_height(&title_text, inner.width);
        cl.push_clickable(
            Line::from(vec![
                Span::styled(format!(" {} ", digit), digit_style),
                Span::styled("│", rail_style),
                Span::styled(format!(" {} ", marker), marker_style),
                Span::styled(*name, title_style),
            ]),
            *action_id,
        );
        // rail (│) はタイトル行だけに付ける。説明文は折り返すことが多く、
        // 折り返し継続行には ratatui の word-wrap が前の行のインデントを
        // 引き継がない (2行目以降が桁0から始まる) ため、│ を続けると折れて
        // 見える。単純な字下げなら折り返しても違和感が出ない。
        let desc_text = format!("       {}", desc);
        let desc_rows = wrapped_line_height(&desc_text, inner.width);
        cl.push_clickable(
            Line::from(Span::styled(desc_text, Style::default().fg(Color::DarkGray))),
            *action_id,
        );
        let card_height = 1 + title_rows + desc_rows;
        cumulative_rows += card_height;
        if is_selected {
            selected_card_top = card_top;
            selected_card_bottom = card_top + card_height;
        }
    }

    let visible_rows = inner.height;
    let max_scroll = cumulative_rows.saturating_sub(visible_rows);
    if *scroll > max_scroll {
        *scroll = max_scroll;
    }

    // Auto-scroll so the highlighted card stays fully visible.
    if selected_card_top < *scroll {
        *scroll = selected_card_top;
    } else if visible_rows > 0 && selected_card_bottom > *scroll + visible_rows {
        *scroll = selected_card_bottom.saturating_sub(visible_rows);
    }
    if *scroll > max_scroll {
        *scroll = max_scroll;
    }
    let can_scroll_up = *scroll > 0;
    let can_scroll_down = *scroll < max_scroll;
    let scroll_value = *scroll;

    {
        let mut cs = click_state.borrow_mut();
        cl.render(f, list_area, menu_block, &mut cs, true, scroll_value);
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

    if let Some(orbit_area) = orbit_area {
        let orbit_entries: Vec<(Color, &str)> = MENU_ENTRIES.iter().map(|e| (e.5, e.1)).collect();
        render_menu_orbit(f, orbit_area, borders, selected, &orbit_entries);
    }

    // Footer
    let footer_text = if is_narrow {
        "タップ / ↑↓+Enter で選択"
    } else {
        "↑↓ で移動 ・ Enter/Space で決定 ・ タップでも選択可"
    };
    let footer_widget = Paragraph::new(Line::from(Span::styled(
        footer_text,
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

/// 選択中のゲームをドット(Braille)表現で示す、ワイドレイアウト専用の装飾
/// パネル。中心の「灯台」から各ゲームへ軌道上のノードとして線を延ばし、
/// 選択中のノードだけ大きく・白いハローを添えて浮かび上がらせる —
/// リスト側の ▶ マーカーと同じ選択状態を、別の見た目でもう一度伝える
/// ことで「今どれを見ているか」を視覚的に強化する。クリック判定は持たない
/// (everlightの`render_camp_ambience`と同じ、純粋な装飾)。
fn render_menu_orbit(
    f: &mut ratzilla::ratatui::Frame,
    area: Rect,
    borders: Borders,
    selected: u8,
    entries: &[(Color, &str)],
) {
    const W: f64 = 40.0;
    const H: f64 = 60.0;
    const ORBIT_R: f64 = 15.0;
    let cx = W / 2.0;
    let cy = H * 0.42;
    let n = entries.len().max(1);

    let hub_glow = canvas_fx::filled_ellipse_points(cx, cy, 3.2, 3.2, 0.6);
    let hub_core = canvas_fx::filled_ellipse_points(cx, cy, 1.3, 1.3, 0.4);
    let hub_ring = canvas_fx::ring_points(cx, cy, 4.4, 0.18);

    let mut spokes: Vec<(f64, f64, f64, f64, Color)> = Vec::with_capacity(n);
    let mut nodes: Vec<(f64, f64, Color, bool)> = Vec::with_capacity(n);
    for (i, &(color, _)) in entries.iter().enumerate() {
        let angle = -std::f64::consts::FRAC_PI_2 + TAU * (i as f64) / (n as f64);
        let (sin, cos) = angle.sin_cos();
        let nx = cx + cos * ORBIT_R;
        let ny = cy + sin * ORBIT_R;
        let is_selected = i as u8 == selected;
        spokes.push((cx, cy, nx, ny, if is_selected { color } else { Color::DarkGray }));
        nodes.push((nx, ny, color, is_selected));
    }

    let selected_entry = entries.get(selected as usize);
    let selected_name = selected_entry.map(|e| e.1).unwrap_or("");
    let selected_color = selected_entry.map(|e| e.0).unwrap_or(Color::DarkGray);

    let canvas = Canvas::default()
        .x_bounds([0.0, W])
        .y_bounds([0.0, H])
        .marker(Marker::Braille)
        .paint(move |ctx| {
            for &(x1, y1, x2, y2, color) in &spokes {
                ctx.draw(&CanvasLine { x1, y1, x2, y2, color });
            }
            ctx.draw(&Points { coords: &hub_ring, color: Color::DarkGray });
            ctx.draw(&Points { coords: &hub_glow, color: Color::Cyan });
            ctx.draw(&Points { coords: &hub_core, color: Color::White });
            for &(nx, ny, color, is_selected) in &nodes {
                let r = if is_selected { 2.6 } else { 1.3 };
                let pts = canvas_fx::filled_ellipse_points(nx, ny, r, r, 0.55);
                ctx.draw(&Points { coords: &pts, color });
                if is_selected {
                    let halo = canvas_fx::ring_points(nx, ny, r + 1.2, 0.3);
                    ctx.draw(&Points { coords: &halo, color: Color::White });
                }
            }
        })
        .block(
            Block::default()
                .borders(borders)
                .border_style(Style::default().fg(selected_color))
                .title(Span::styled(
                    format!(" {} ", selected_name),
                    Style::default().fg(selected_color).add_modifier(Modifier::BOLD),
                )),
        );
    f.render_widget(canvas, area);
}

fn render_settings(
    f: &mut ratzilla::ratatui::Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
    confirm_reset: Option<&GameChoice>,
    scroll: &Cell<u16>,
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
        render_settings_main(f, chunks[1], click_state, borders, scroll);
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
    scroll: &Cell<u16>,
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

    // 周回討伐
    cl.push_clickable(
        Line::from(vec![
            Span::styled(" ✕ ", Style::default().fg(Color::Red)),
            Span::styled("周回討伐", Style::default().fg(Color::White)),
            Span::styled(" — データをリセット", Style::default().fg(Color::DarkGray)),
        ]),
        SETTINGS_RESET_LOOPMARCH,
    );

    cl.push(Line::from(""));

    // 常夜灯
    cl.push_clickable(
        Line::from(vec![
            Span::styled(" ✕ ", Style::default().fg(Color::Red)),
            Span::styled("常夜灯", Style::default().fg(Color::White)),
            Span::styled(" — データをリセット", Style::default().fg(Color::DarkGray)),
        ]),
        SETTINGS_RESET_EVERLIGHT,
    );

    cl.push(Line::from(""));
    cl.push(Line::from(""));
    cl.push(Line::from(Span::styled(
        " ※ Tiny Factory / Dungeon Dive / God Field は",
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
    let mut cs = click_state.borrow_mut();
    ScrollableTab::new(cl, scroll, SETTINGS_SCROLL_UP, SETTINGS_SCROLL_DOWN)
        .block(block)
        .arrow_color(Color::Green)
        .render(f, area, &mut cs);
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
        GameChoice::Abyss => "深淵潜行",
        GameChoice::Metropolis => "Idle Metropolis",
        GameChoice::LoopMarch => "周回討伐",
        GameChoice::Everlight => "常夜灯",
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

#[cfg(test)]
mod tests {
    use super::*;
    use ratzilla::ratatui::backend::TestBackend;

    fn render_menu_to_test_backend(width: u16, height: u16, selected: u8) -> Rc<RefCell<ClickState>> {
        let cs = Rc::new(RefCell::new(ClickState::new()));
        cs.borrow_mut().terminal_cols = width;
        cs.borrow_mut().terminal_rows = height;
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        let mut scroll = 0u16;
        terminal
            .draw(|f| {
                render_menu(f, f.area(), &cs, &mut scroll, selected);
            })
            .unwrap();
        cs
    }

    #[test]
    fn render_menu_does_not_panic_narrow_and_wide() {
        // 軌道パネル (ワイドのみ) の有無、桁数バッジ・rail の折り返しなど
        // 幅依存のレイアウトを一通り踏む。selected は先頭・中間・末尾
        // (= 設定, 数字キー無しの ⚙ マーカー) を網羅する。
        for &(w, h) in &[(40u16, 30u16), (60, 30), (100, 40)] {
            for selected in [0u8, 4, 9] {
                render_menu_to_test_backend(w, h, selected);
            }
        }
    }

    /// カード本体を digit バッジ + rail (│) + マーカー + 名前の複数 Span に
    /// 組み替えたので、`ClickableList` の wrap-aware クリック登録が依然
    /// 全カードで機能していることを確認する回帰テスト。
    ///
    /// 「全カードが常に同時に画面内にある」とは限らない (説明文の折り返しで
    /// 縦に溢れればスクロールされ、後方のカードは一時的に隠れる) ので、
    /// 「selected=i で描画した時、そのカード自身のタップ対象は必ず見える
    /// 範囲に入る」という auto-scroll の契約を検証する。
    fn assert_selected_card_is_always_reachable(width: u16, height: u16) {
        const ACTION_IDS: [u16; 10] = [
            MENU_SELECT_COOKIE,
            MENU_SELECT_FACTORY,
            MENU_SELECT_RPG,
            MENU_SELECT_ABYSS,
            MENU_SELECT_GODFIELD,
            MENU_SELECT_METROPOLIS,
            MENU_SELECT_LOOPMARCH,
            MENU_SELECT_EVERLIGHT,
            MENU_SELECT_SHATTERLAB,
            MENU_SELECT_SETTINGS,
        ];
        for (i, &action_id) in ACTION_IDS.iter().enumerate() {
            let cs = render_menu_to_test_backend(width, height, i as u8);
            let cs = cs.borrow();
            let hit = (0..height).any(|y| (0..width).any(|x| cs.hit_test(x, y) == Some(action_id)));
            assert!(hit, "selected={i} なのに action_id {action_id} のタップ対象が見当たらない");
        }
    }

    #[test]
    fn render_menu_registers_click_target_for_selected_card_in_wide_layout() {
        assert_selected_card_is_always_reachable(100, 40);
    }

    #[test]
    fn render_menu_registers_click_target_for_selected_card_in_narrow_layout() {
        assert_selected_card_is_always_reachable(40, 40);
    }

    /// 装飾用の軌道パネルはワイドレイアウトでのみ描画される
    /// (`render_camp_ambience` と同じ規約)。ナロー幅では
    /// `Constraint::Length(22)` 分の列を消費しないぶん、リストの
    /// 折り返し幅がワイド時と変わるだけで panic しないことを確認する。
    #[test]
    fn render_menu_orbit_panel_does_not_panic_for_various_selections() {
        let entries: Vec<(Color, &str)> = (0..10).map(|_| (Color::LightMagenta, "x")).collect();
        for selected in 0u8..10 {
            let mut terminal = Terminal::new(TestBackend::new(22, 20)).unwrap();
            terminal
                .draw(|f| {
                    render_menu_orbit(f, f.area(), Borders::ALL, selected, &entries);
                })
                .unwrap();
        }
    }
}
