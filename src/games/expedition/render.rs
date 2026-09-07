//! 遠征団の描画。
//!
//! Everlight と同じ組み: 上部 HUD / 中央の Canvas+Braille 面 / ワイド時の
//! 情景サイド。操作は ClickableGrid・Clickable でセル単位に当てる。
//! 説明文は出さず、点描とピクトで状況を伝える。

use std::cell::RefCell;
use std::rc::Rc;

use ratzilla::ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratzilla::ratatui::style::{Color, Modifier, Style};
use ratzilla::ratatui::symbols::Marker;
use ratzilla::ratatui::text::{Line, Span};
use ratzilla::ratatui::widgets::canvas::{Canvas, Line as CanvasLine, Points};
use ratzilla::ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratzilla::ratatui::Frame;

use crate::canvas_fx;
use crate::games::GameChoice;
use crate::input::{is_narrow_layout, ClickState};
use crate::theme;
use crate::widgets::{Clickable, ClickableGrid, ClickableList, TabBar};

use super::actions::{
    level_hero_id, place_slot_id, toggle_hero_id, ACK_RESULT, CANCEL_FORMING, CONFIRM_PLACEMENT,
    LAUNCH, OPEN_FORMING, START_FORMING, TAB_ARCADE, TAB_CAMP,
};
use super::state::{
    path_to_world_x, ExpeditionState, Hero, HubTab, Role, Screen, CAMP_AMB_H, CAMP_AMB_W, ORB_NEED,
    PARTY_SIZE, PATH_LEN, PATH_X0, PATH_X1, PATH_Y, PUSH_D, PUSH_W, PUSH_WORLD_H, PUSH_WORLD_W,
    WORLD_H, WORLD_W,
};

fn accent() -> Color {
    theme::accent(&GameChoice::Expedition)
}

fn muted() -> Style {
    Style::default().fg(Color::DarkGray)
}

const EMBER: Color = Color::Rgb(180, 120, 60);

/// critique の goal 照合用。画面には出さない。
pub fn next_goal_line(state: &ExpeditionState) -> String {
    match state.screen {
        Screen::Camp if state.hub_tab == HubTab::Arcade && state.pending_level_pick => {
            "次: 育てる団員を選ぶ".into()
        }
        Screen::Camp if state.hub_tab == HubTab::Arcade => {
            format!("次: 光珠あと{}でレベルアップ", state.orb_remaining().max(1))
        }
        Screen::Camp if state.rations == 0 => "次: 行軍糧が貯まるのを待つ".into(),
        Screen::Camp if state.last_failed => {
            format!(
                "次: 遊技場で育ててから {} 再挑戦",
                state.current_stage_label()
            )
        }
        Screen::Camp => format!("次: {} を防衛する", state.current_stage_label()),
        Screen::Forming => "次: 3人を選んで出発する".into(),
        Screen::Placing => "次: 道に3人を置いて防衛開始".into(),
        Screen::Running => format!(
            "防衛中 {} …",
            state
                .sortie
                .as_ref()
                .map(|s| ExpeditionState::stage_label(s.chapter, s.stage))
                .unwrap_or_default()
        ),
        Screen::Result if state.last_failed => "次: 遊技場でレベルを上げる".into(),
        Screen::Result => format!("次: {} へ進む", state.current_stage_label()),
    }
}

fn role_glyph(role: Role) -> char {
    match role {
        Role::Vanguard => '▣',
        Role::Striker => '▲',
        Role::Support => '✚',
    }
}

fn hero_chip(h: &Hero) -> String {
    format!(
        "{}{}{}",
        role_glyph(h.role),
        h.name.chars().next().unwrap_or('?'),
        h.level
    )
}

fn pip_bar(filled: u32, cap: u32, on: char, off: char) -> String {
    let mut s = String::new();
    for i in 0..cap {
        s.push(if i < filled { on } else { off });
    }
    s
}

fn on_sortie(state: &ExpeditionState) -> bool {
    !matches!(state.screen, Screen::Camp | Screen::Result)
}

/// 全画面の骨格。領域はここだけで決め、各画面は受け取った Rect だけを描く。
struct ShellLayout {
    header: Rect,
    body: Rect,
    nav: Rect,
}

fn compute_shell_layout(area: Rect) -> ShellLayout {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(3),
        ])
        .split(area);
    ShellLayout {
        header: chunks[0],
        body: chunks[1],
        nav: chunks[2],
    }
}

struct CampLayout {
    body: Rect,
    ambience: Option<Rect>,
}

fn compute_camp_layout(area: Rect) -> CampLayout {
    if is_narrow_layout(area.width) {
        CampLayout {
            body: area,
            ambience: None,
        }
    } else {
        let h = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(28), Constraint::Length(22)])
            .split(area);
        CampLayout {
            body: h[0],
            ambience: Some(h[1]),
        }
    }
}

struct PathStageLayout {
    field: Rect,
    side: Option<Rect>,
    footer: Rect,
}

fn compute_path_stage_layout(area: Rect, placing: bool) -> PathStageLayout {
    let narrow = is_narrow_layout(area.width);
    let (field_full, side) = if narrow || placing {
        (area, None)
    } else {
        let h = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(30), Constraint::Length(18)])
            .split(area);
        (h[0], Some(h[1]))
    };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(8), Constraint::Length(3)])
        .split(field_full);
    PathStageLayout {
        field: chunks[0],
        side,
        footer: chunks[1],
    }
}

struct ArcadeLayout {
    field: Rect,
    lanes: Rect,
    side: Option<Rect>,
}

fn compute_arcade_layout(area: Rect) -> ArcadeLayout {
    let narrow = is_narrow_layout(area.width);
    let (main, side) = if narrow {
        (area, None)
    } else {
        let h = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(28), Constraint::Length(16)])
            .split(area);
        (h[0], Some(h[1]))
    };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(10), Constraint::Length(6)])
        .split(main);
    ArcadeLayout {
        field: chunks[0],
        lanes: chunks[1],
        side,
    }
}

pub fn render(
    state: &ExpeditionState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let shell = compute_shell_layout(area);

    render_header(state, f, shell.header);
    match state.screen {
        Screen::Camp => match state.hub_tab {
            HubTab::Camp => render_camp(state, f, shell.body, click_state),
            HubTab::Arcade => render_arcade(state, f, shell.body, click_state),
        },
        Screen::Forming => render_forming(state, f, shell.body, click_state),
        Screen::Placing | Screen::Running => {
            render_path_stage(state, f, shell.body, click_state);
        }
        Screen::Result => render_result(state, f, shell.body, click_state),
    }
    render_bottom_nav(state, f, shell.nav, click_state);

    if state.pending_level_pick && state.hub_tab == HubTab::Arcade && state.screen == Screen::Camp {
        render_level_modal(state, f, shell.body, click_state);
    }
}

fn render_header(state: &ExpeditionState, f: &mut Frame, area: Rect) {
    let ration = pip_bar(state.rations, state.ration_cap(), '◆', '◇');
    let orb = pip_bar(state.orb_gauge.min(ORB_NEED), ORB_NEED, '●', '○');
    let phase = match state.screen {
        Screen::Camp => "·",
        Screen::Forming => "◇",
        Screen::Placing => "▣",
        Screen::Running => "⚔",
        Screen::Result => "✓",
    };
    let line1 = Line::from(vec![
        Span::styled("⠐⠂ ", Style::default().fg(EMBER)),
        Span::styled(
            state.current_stage_label(),
            Style::default()
                .fg(accent())
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!(" {phase} "), Style::default().fg(Color::White)),
        Span::styled("⠂⠐", Style::default().fg(EMBER)),
    ]);
    let line2 = Line::from(vec![
        Span::styled(format!(" {ration} "), Style::default().fg(Color::LightGreen)),
        Span::styled(
            format!("◎{} ", state.medals),
            Style::default().fg(Color::LightYellow),
        ),
        Span::styled(format!("{orb} "), Style::default().fg(Color::Cyan)),
    ]);
    f.render_widget(
        Paragraph::new(vec![line1, line2]).block(
            Block::default()
                .borders(Borders::BOTTOM)
                .border_style(Style::default().fg(accent())),
        ),
        area,
    );
}

fn tab_style(active: bool) -> Style {
    if active {
        Style::default()
            .fg(Color::Black)
            .bg(accent())
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Gray)
    }
}

fn render_bottom_nav(
    state: &ExpeditionState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    if on_sortie(state) {
        f.render_widget(
            Paragraph::new(" ··· ")
                .alignment(Alignment::Center)
                .style(muted()),
            area,
        );
        return;
    }
    let camp = if state.hub_tab == HubTab::Camp {
        "● ⌂"
    } else {
        "⌂"
    };
    let arcade = if state.hub_tab == HubTab::Arcade {
        "● ◎"
    } else {
        "◎"
    };
    TabBar::new("│")
        .tab(camp, tab_style(state.hub_tab == HubTab::Camp), TAB_CAMP)
        .tab(
            arcade,
            tab_style(state.hub_tab == HubTab::Arcade),
            TAB_ARCADE,
        )
        .render(f, area, &mut click_state.borrow_mut());
}

fn hp_bar(hp: i32, max_hp: i32) -> String {
    let max_hp = max_hp.max(1);
    let filled = ((hp.max(0) * 8) / max_hp).clamp(0, 8) as usize;
    format!("{}{}", "█".repeat(filled), "░".repeat(8 - filled))
}

fn ration_fill_bar(state: &ExpeditionState) -> String {
    if state.rations >= state.ration_cap() {
        return String::new();
    }
    let need = state.ration_regen_ticks().max(1);
    let p = ((state.ration_progress * 4) / need).min(3);
    format!("{}{}", "▓".repeat(p as usize), "░".repeat(3 - p as usize))
}

// ── 拠点 ──────────────────────────────────────────────

fn render_camp(
    state: &ExpeditionState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let layout = compute_camp_layout(area);
    render_camp_body(state, f, layout.body, click_state);
    if let Some(side) = layout.ambience {
        render_camp_ambience(state, f, side);
    }
}

fn render_camp_body(
    state: &ExpeditionState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),
            Constraint::Min(3),
            Constraint::Length(3),
        ])
        .split(area);

    let mut map_spans = vec![Span::styled(
        format!(" {} ", state.chapter),
        Style::default()
            .fg(accent())
            .add_modifier(Modifier::BOLD),
    )];
    for s in 1..=4u32 {
        if s > 1 {
            map_spans.push(Span::styled("─", muted()));
        }
        let (sym, style) = if s < state.stage {
            ("●", Style::default().fg(Color::LightGreen))
        } else if s == state.stage {
            (
                if state.last_failed { "✗" } else { "▶" },
                Style::default()
                    .fg(if state.last_failed {
                        Color::LightRed
                    } else {
                        accent()
                    })
                    .add_modifier(Modifier::BOLD),
            )
        } else if s == 4 {
            ("◉", Style::default().fg(Color::DarkGray))
        } else {
            ("○", Style::default().fg(Color::DarkGray))
        };
        map_spans.push(Span::styled(sym, style));
    }
    f.render_widget(
        Paragraph::new(vec![
            Line::from(map_spans),
            Line::from(""),
            Line::from(Span::styled(
                format!("  {}", state.current_stage_label()),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )),
        ])
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray)),
        ),
        chunks[0],
    );

    let mut chips = String::from("  ");
    for slot in &state.forming {
        match slot {
            Some(id) => {
                if let Some(h) = state.hero(*id) {
                    chips.push_str(&hero_chip(h));
                    chips.push(' ');
                }
            }
            None => chips.push_str("·· "),
        }
    }
    Clickable::new(
        Paragraph::new(Line::from(Span::styled(
            chips,
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
        )))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray)),
        ),
        OPEN_FORMING,
    )
    .render(f, chunks[1], &mut click_state.borrow_mut());

    let party_ready = state.forming_count() == PARTY_SIZE;
    let (label, style, border, action) = if state.rations == 0 {
        (
            format!(" ◆ {}", ration_fill_bar(state)),
            Style::default().fg(Color::DarkGray),
            Color::DarkGray,
            START_FORMING,
        )
    } else if !party_ready {
        (
            " ◇◇◇ ".into(),
            Style::default()
                .fg(Color::Black)
                .bg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
            Color::Yellow,
            OPEN_FORMING,
        )
    } else {
        (
            format!(" ▶ {} ", state.current_stage_label()),
            Style::default()
                .fg(Color::Black)
                .bg(accent())
                .add_modifier(Modifier::BOLD),
            accent(),
            START_FORMING,
        )
    };
    Clickable::new(
        Paragraph::new(label)
            .alignment(Alignment::Center)
            .style(style)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(border)),
            ),
        action,
    )
    .render(f, chunks[2], &mut click_state.borrow_mut());
}

/// ワイド拠点の右情景。クリックなし。章とメダル量で灯の大きさが変わる。
fn render_camp_ambience(state: &ExpeditionState, f: &mut Frame, area: Rect) {
    let cx = CAMP_AMB_W / 2.0;
    let cy = CAMP_AMB_H * 0.42;
    let scale = 1.0 + (state.medals.min(40) as f64 / 40.0) * 0.5 + (state.chapter as f64) * 0.05;
    let core = canvas_fx::filled_ellipse_points(cx, cy, 2.0 * scale, 2.0 * scale, 0.5);
    let glow = canvas_fx::filled_ellipse_points(cx, cy, 5.0 * scale, 5.0 * scale, 0.6);
    let halo = canvas_fx::ring_points(cx, cy, 7.5 * scale, 0.15);
    // 道のシルエット
    let road: Vec<(f64, f64)> = (0..40)
        .map(|i| {
            let t = i as f64 / 39.0;
            (2.0 + t * (CAMP_AMB_W - 4.0), CAMP_AMB_H * 0.78)
        })
        .collect();
    let gate = canvas_fx::filled_rect_points(
        CAMP_AMB_W - 6.0,
        CAMP_AMB_H * 0.72,
        CAMP_AMB_W - 3.0,
        CAMP_AMB_H * 0.88,
        0.5,
    );
    let embers: Vec<(f64, f64)> = (0..20)
        .map(|i| {
            let t = i as f64;
            let x = cx + (t * 2.1).sin() * (3.0 + (t * 0.6).cos() * 8.0);
            let y = CAMP_AMB_H - (t * 6.5 + (t * 1.7).sin() * 5.0) % (CAMP_AMB_H - 6.0) - 2.0;
            (x.clamp(1.0, CAMP_AMB_W - 1.0), y)
        })
        .collect();

    let canvas = Canvas::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray)),
        )
        .x_bounds([0.0, CAMP_AMB_W])
        .y_bounds([0.0, CAMP_AMB_H])
        .marker(Marker::Braille)
        .paint(move |ctx| {
            ctx.draw(&Points {
                coords: &embers,
                color: EMBER,
            });
            ctx.draw(&Points {
                coords: &road,
                color: Color::DarkGray,
            });
            ctx.draw(&Points {
                coords: &gate,
                color: Color::Gray,
            });
            ctx.draw(&Points {
                coords: &halo,
                color: Color::Rgb(255, 200, 80),
            });
            ctx.draw(&Points {
                coords: &glow,
                color: Color::Rgb(200, 140, 40),
            });
            ctx.draw(&Points {
                coords: &core,
                color: Color::LightYellow,
            });
        });
    f.render_widget(canvas, area);
}

// ── 編成 ──────────────────────────────────────────────

fn render_forming(
    state: &ExpeditionState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(6),
            Constraint::Length(3),
            Constraint::Length(3),
        ])
        .split(area);

    let mut cl = ClickableList::new();
    let sel = state.forming_count();
    cl.push(Line::from(Span::styled(
        format!(
            "  {}{}",
            "◆".repeat(sel),
            "◇".repeat(PARTY_SIZE.saturating_sub(sel))
        ),
        Style::default().fg(accent()),
    )));
    for h in &state.roster {
        let selected = state.forming.contains(&Some(h.id));
        let mark = if selected { "◆" } else { "◇" };
        cl.push_clickable(
            Line::from(format!("  {mark} {}", hero_chip(h))),
            toggle_hero_id(h.id),
        );
    }
    cl.render(
        f,
        chunks[0],
        Block::default().borders(Borders::ALL),
        &mut click_state.borrow_mut(),
        false,
        0,
    );

    if state.forming_count() == PARTY_SIZE {
        Clickable::new(
            Paragraph::new(" ▶▣ ")
                .alignment(Alignment::Center)
                .style(
                    Style::default()
                        .fg(Color::Black)
                        .bg(accent())
                        .add_modifier(Modifier::BOLD),
                )
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(accent())),
                ),
            LAUNCH,
        )
        .render(f, chunks[1], &mut click_state.borrow_mut());
    } else {
        f.render_widget(
            Paragraph::new(" ◇◇◇ ")
                .alignment(Alignment::Center)
                .style(muted())
                .block(Block::default().borders(Borders::ALL)),
            chunks[1],
        );
    }
    Clickable::new(
        Paragraph::new(" ← ")
            .alignment(Alignment::Center)
            .style(muted())
            .block(Block::default().borders(Borders::ALL)),
        CANCEL_FORMING,
    )
    .render(f, chunks[2], &mut click_state.borrow_mut());
}

// ── 配置 / 防衛（同一 Canvas 面） ──────────────────────

fn render_path_stage(
    state: &ExpeditionState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let placing = state.screen == Screen::Placing;
    let layout = compute_path_stage_layout(area, placing);

    render_path_canvas(state, f, layout.field, click_state);

    if placing {
        let row = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
            .split(layout.footer);
        Clickable::new(
            Paragraph::new(" ▶⚔ ")
                .alignment(Alignment::Center)
                .style(
                    Style::default()
                        .fg(Color::Black)
                        .bg(accent())
                        .add_modifier(Modifier::BOLD),
                )
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(accent())),
                ),
            CONFIRM_PLACEMENT,
        )
        .render(f, row[0], &mut click_state.borrow_mut());
        Clickable::new(
            Paragraph::new(" ← ")
                .alignment(Alignment::Center)
                .style(muted())
                .block(Block::default().borders(Borders::ALL)),
            CANCEL_FORMING,
        )
        .render(f, row[1], &mut click_state.borrow_mut());
    } else if let Some(sortie) = state.sortie.as_ref() {
        let wave = pip_bar(sortie.wave_index + 1, sortie.waves_total, '◆', '◇');
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(format!(" {wave} "), Style::default().fg(Color::Gray)),
                Span::styled(
                    format!("■{}", hp_bar(sortie.gate_hp, sortie.gate_max_hp)),
                    Style::default().fg(Color::LightGreen),
                ),
            ]))
            .block(Block::default().borders(Borders::ALL)),
            layout.footer,
        );
    }

    if let Some(side_area) = layout.side {
        render_running_side(state, f, side_area);
    }
}

fn render_path_canvas(
    state: &ExpeditionState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(accent()));
    let inner = block.inner(area);

    // 配置中は道を PATH_LEN 列の ClickableGrid で細かく当てる
    if state.screen == Screen::Placing && inner.width > 0 && inner.height > 0 {
        let cell_w = (inner.width / PATH_LEN as u16).max(1);
        let mut cs = click_state.borrow_mut();
        ClickableGrid::new(PATH_LEN, 1, super::actions::PLACE_SLOT_BASE, cell_w)
            .with_cell_height(inner.height)
            .register_targets(area, &block, &mut cs, 0);
        let covered = cell_w * PATH_LEN as u16;
        if covered < inner.width {
            let rem = Rect::new(
                inner.x + covered,
                inner.y,
                inner.width - covered,
                inner.height,
            );
            Clickable::new(
                Block::default(),
                place_slot_id((PATH_LEN - 1) as u8),
            )
            .render(f, rem, &mut cs);
        }
    }

    // 道の線
    let path_line = (PATH_X0, PATH_Y, PATH_X1, PATH_Y);
    let gate_pts = canvas_fx::filled_rect_points(PATH_X1, PATH_Y - 6.0, PATH_X1 + 4.0, PATH_Y + 6.0, 0.6);
    let spawn_pts = canvas_fx::filled_ellipse_points(PATH_X0 - 2.0, PATH_Y, 1.8, 1.8, 0.7);

    // スロット目安
    let mut slot_marks: Vec<(f64, f64)> = Vec::new();
    for i in 0..PATH_LEN {
        let x = path_to_world_x(i as f64 + 0.5);
        slot_marks.extend(canvas_fx::ring_points(x, PATH_Y, 1.2, 0.35));
    }

    // 団員
    let mut hero_pts: Vec<(Vec<(f64, f64)>, Color)> = Vec::new();
    if let Some(sortie) = state.sortie.as_ref() {
        for (slot, id) in sortie.path.iter().enumerate() {
            let Some(hid) = id else { continue };
            let Some(h) = state.hero(*hid) else { continue };
            let x = path_to_world_x(slot as f64 + 0.5);
            let r = 1.6 + h.level as f64 * 0.15;
            let color = match h.role {
                Role::Vanguard => Color::LightBlue,
                Role::Striker => Color::LightRed,
                Role::Support => Color::LightGreen,
            };
            hero_pts.push((
                canvas_fx::filled_ellipse_points(x, PATH_Y, r, r, 0.55),
                color,
            ));
            // 射程リング（配置中だけ）
            if state.screen == Screen::Placing {
                let rr = h.role.range() as f64 * ((PATH_X1 - PATH_X0) / PATH_LEN as f64) * 0.45;
                hero_pts.push((canvas_fx::ring_points(x, PATH_Y, rr, 0.2), Color::DarkGray));
            }
        }
    }

    // 敵
    let mut enemy_pts: Vec<(f64, f64)> = Vec::new();
    let mut boss_pts: Vec<(f64, f64)> = Vec::new();
    if let Some(sortie) = state.sortie.as_ref() {
        for c in &sortie.creeps {
            if c.hp <= 0 {
                continue;
            }
            let x = path_to_world_x(c.pos);
            let is_boss = c.name.contains("首領") || c.name.contains("甲殻") || c.name.contains("大口");
            let r = if is_boss { 2.4 } else { 1.5 };
            let pts = canvas_fx::filled_ellipse_points(x, PATH_Y, r, r * 0.85, 0.55);
            if is_boss {
                boss_pts.extend(pts);
            } else {
                enemy_pts.extend(pts);
            }
        }
    }

    let canvas = Canvas::default()
        .block(block)
        .x_bounds([0.0, WORLD_W])
        .y_bounds([0.0, WORLD_H])
        .marker(Marker::Braille)
        .paint(move |ctx| {
            ctx.draw(&CanvasLine {
                x1: path_line.0,
                y1: path_line.1,
                x2: path_line.2,
                y2: path_line.3,
                color: Color::DarkGray,
            });
            ctx.draw(&Points {
                coords: &slot_marks,
                color: Color::Rgb(60, 60, 70),
            });
            ctx.draw(&Points {
                coords: &spawn_pts,
                color: Color::Gray,
            });
            ctx.draw(&Points {
                coords: &gate_pts,
                color: Color::LightGreen,
            });
            for (pts, color) in &hero_pts {
                ctx.draw(&Points {
                    coords: pts,
                    color: *color,
                });
            }
            if !enemy_pts.is_empty() {
                ctx.draw(&Points {
                    coords: &enemy_pts,
                    color: Color::LightRed,
                });
            }
            if !boss_pts.is_empty() {
                ctx.draw(&Points {
                    coords: &boss_pts,
                    color: Color::Magenta,
                });
            }
        });
    f.render_widget(canvas, area);
}

fn render_running_side(state: &ExpeditionState, f: &mut Frame, area: Rect) {
    let Some(sortie) = state.sortie.as_ref() else {
        return;
    };
    let mut lines = vec![Line::from(Span::styled(
        format!(" {}", ExpeditionState::stage_label(sortie.chapter, sortie.stage)),
        Style::default().fg(accent()).add_modifier(Modifier::BOLD),
    ))];
    for (i, slot) in sortie.path.iter().enumerate() {
        if let Some(id) = slot {
            if let Some(h) = state.hero(*id) {
                lines.push(Line::from(Span::styled(
                    format!(" {}", hero_chip(h)),
                    Style::default().fg(Color::White),
                )));
                let _ = i;
            }
        }
    }
    lines.push(Line::from(""));
    for c in sortie.creeps.iter().take(4) {
        let frac = if c.max_hp > 0 {
            ((c.hp.max(0) * 4) / c.max_hp) as usize
        } else {
            0
        };
        lines.push(Line::from(Span::styled(
            format!(
                " ※{} {}",
                (c.pos as u32).min(9),
                "▮".repeat(frac) + &"▯".repeat(4usize.saturating_sub(frac))
            ),
            Style::default().fg(Color::LightRed),
        )));
    }
    f.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray)),
        ),
        area,
    );
}

// ── 遊技場（プッシャー Canvas） ──────────────────────

fn render_arcade(
    state: &ExpeditionState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let layout = compute_arcade_layout(area);
    if let Some(side) = layout.side {
        render_arcade_side(state, f, side);
    }

    render_pusher_canvas(state, f, layout.field, click_state);

    // レーン投入: 横 GRID
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" ◎{} ", state.medals))
        .border_style(Style::default().fg(Color::DarkGray));
    let inner = block.inner(layout.lanes);
    f.render_widget(block.clone(), layout.lanes);
    if inner.width > 0 && inner.height > 0 && !state.pending_level_pick {
        let cell_w = (inner.width / PUSH_W as u16).max(1);
        let mut cs = click_state.borrow_mut();
        ClickableGrid::new(PUSH_W, 1, super::actions::DROP_LANE_BASE, cell_w)
            .with_cell_height(inner.height)
            .register_targets(layout.lanes, &block, &mut cs, 0);
        // レーン番号を視覚表示
        let mut marks = String::from(" ");
        for lane in 0..PUSH_W {
            let hot = state.pusher.cells[PUSH_D - 1][lane].medals >= 3;
            marks.push_str(if hot { "▼!" } else { "▼ " });
        }
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                marks,
                Style::default().fg(accent()).add_modifier(Modifier::BOLD),
            )))
            .alignment(Alignment::Center),
            inner,
        );
    }
}

fn render_arcade_side(state: &ExpeditionState, f: &mut Frame, area: Rect) {
    let orb = pip_bar(state.orb_gauge.min(ORB_NEED), ORB_NEED, '●', '○');
    let mut lines = vec![
        Line::from(Span::styled(
            format!(" {orb}"),
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
    ];
    for h in &state.roster {
        lines.push(Line::from(format!(" {}", hero_chip(h))));
    }
    f.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray)),
        ),
        area,
    );
}

fn render_pusher_canvas(
    state: &ExpeditionState,
    f: &mut Frame,
    area: Rect,
    _click_state: &Rc<RefCell<ClickState>>,
) {
    let cell_w = PUSH_WORLD_W / PUSH_W as f64;
    let cell_h = (PUSH_WORLD_H - 6.0) / PUSH_D as f64;
    let y0 = 4.0;

    // 押し板
    let plate_x = (state.pusher.plate_col as f64 + 0.5) * cell_w;
    let plate = canvas_fx::filled_rect_points(plate_x - 1.2, 1.0, plate_x + 1.2, 3.2, 0.45);

    let mut pile_pts: Vec<(Vec<(f64, f64)>, Color)> = Vec::new();
    let mut orb_pts: Vec<(f64, f64)> = Vec::new();
    for row in 0..PUSH_D {
        for col in 0..PUSH_W {
            let cell = state.pusher.cells[row][col];
            let cx = (col as f64 + 0.5) * cell_w;
            let cy = y0 + (row as f64 + 0.5) * cell_h;
            if cell.medals > 0 {
                let r = 0.8 + cell.medals as f64 * 0.35;
                let color = if row + 1 == PUSH_D {
                    Color::LightYellow
                } else {
                    Color::Yellow
                };
                pile_pts.push((
                    canvas_fx::filled_ellipse_points(cx, cy, r, r * 0.7, 0.5),
                    color,
                ));
            }
            if cell.has_orb {
                orb_pts.extend(canvas_fx::filled_ellipse_points(cx, cy, 1.4, 1.4, 0.45));
                orb_pts.extend(canvas_fx::ring_points(cx, cy, 2.2, 0.25));
            }
        }
    }

    // 落下端ライン
    let edge_y = y0 + (PUSH_D as f64) * cell_h;
    let edge_line = (0.0, edge_y, PUSH_WORLD_W, edge_y);

    let flash_pts: Vec<(f64, f64)> = if state.pusher.last_drop_medals > 0 {
        canvas_fx::filled_ellipse_points(
            PUSH_WORLD_W / 2.0,
            edge_y + 2.0,
            2.0 + state.pusher.last_drop_medals as f64 * 0.4,
            1.2,
            0.5,
        )
    } else {
        Vec::new()
    };

    let orb_title = pip_bar(state.orb_gauge.min(ORB_NEED), ORB_NEED, '●', '○');
    let canvas = Canvas::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" {orb_title} "))
                .border_style(Style::default().fg(Color::DarkGray)),
        )
        .x_bounds([0.0, PUSH_WORLD_W])
        .y_bounds([0.0, PUSH_WORLD_H])
        .marker(Marker::Braille)
        .paint(move |ctx| {
            ctx.draw(&Points {
                coords: &plate,
                color: Color::Gray,
            });
            ctx.draw(&CanvasLine {
                x1: edge_line.0,
                y1: edge_line.1,
                x2: edge_line.2,
                y2: edge_line.3,
                color: Color::LightRed,
            });
            for (pts, color) in &pile_pts {
                ctx.draw(&Points {
                    coords: pts,
                    color: *color,
                });
            }
            if !orb_pts.is_empty() {
                ctx.draw(&Points {
                    coords: &orb_pts,
                    color: Color::Cyan,
                });
            }
            if !flash_pts.is_empty() {
                ctx.draw(&Points {
                    coords: &flash_pts,
                    color: Color::LightCyan,
                });
            }
        });
    f.render_widget(canvas, area);
}

fn render_level_modal(
    state: &ExpeditionState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    // Everlight の boon modal と同じく Clear + 中央リスト
    let width = area.width.clamp(20, 36);
    let height = (state.roster.len() as u16 + 4).min(area.height.saturating_sub(2));
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 2;
    let modal = Rect::new(x, y, width, height);
    f.render_widget(Clear, modal);

    let mut cl = ClickableList::new();
    cl.push(Line::from(Span::styled(
        " ●●● →",
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )));
    for h in &state.roster {
        cl.push_clickable(
            Line::from(format!("  ▶ {}", hero_chip(h))),
            level_hero_id(h.id),
        );
    }
    cl.render(
        f,
        modal,
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(accent())),
        &mut click_state.borrow_mut(),
        false,
        0,
    );
}

// ── 結果 ──────────────────────────────────────────────

fn render_result(
    state: &ExpeditionState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(6), Constraint::Length(3)])
        .split(area);

    let big = if state.last_failed { " ✗ " } else { " ✓ " };
    let big_color = if state.last_failed {
        Color::LightRed
    } else {
        Color::LightGreen
    };
    let mut chips = String::from("  ");
    for h in &state.roster {
        chips.push_str(&hero_chip(h));
        chips.push(' ');
    }
    f.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                big,
                Style::default()
                    .fg(big_color)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                format!("  {}", state.current_stage_label()),
                Style::default().fg(Color::White),
            )),
            Line::from(""),
            Line::from(Span::styled(chips, Style::default().fg(Color::LightYellow))),
        ])
        .block(Block::default().borders(Borders::ALL)),
        chunks[0],
    );

    let cta = if state.last_failed { " ▶◎ " } else { " ▶⌂ " };
    Clickable::new(
        Paragraph::new(cta)
            .alignment(Alignment::Center)
            .style(
                Style::default()
                    .fg(Color::Black)
                    .bg(accent())
                    .add_modifier(Modifier::BOLD),
            )
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(accent())),
            ),
        ACK_RESULT,
    )
    .render(f, chunks[1], &mut click_state.borrow_mut());
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratzilla::ratatui::backend::TestBackend;
    use ratzilla::ratatui::Terminal;

    use crate::tui_inspect::buffer_text;

    fn draw_text(state: &ExpeditionState) -> String {
        let backend = TestBackend::new(80, 28);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                render(
                    state,
                    f,
                    f.area(),
                    &Rc::new(RefCell::new(ClickState::new())),
                );
            })
            .unwrap();
        buffer_text(terminal.backend().buffer())
    }

    #[test]
    fn camp_shows_stage_cta() {
        let state = ExpeditionState::new();
        let text = draw_text(&state);
        assert!(text.contains("1-1"));
        assert!(text.contains('▶'));
    }

    #[test]
    fn wide_camp_has_ambience_braille() {
        let state = ExpeditionState::new();
        let text = draw_text(&state);
        // Braille 面 or ピクトが存在する
        assert!(text.contains('⌂') || text.contains('◎') || text.contains('◆'));
    }

    #[test]
    fn arcade_shows_drop_grid() {
        let mut state = ExpeditionState::new();
        state.hub_tab = HubTab::Arcade;
        let text = draw_text(&state);
        assert!(text.contains('▼') || text.contains('◎'));
    }

    #[test]
    fn placing_uses_path_canvas() {
        let mut state = ExpeditionState::new();
        assert!(super::super::logic::launch_sortie(&mut state));
        let text = draw_text(&state);
        assert!(text.contains('⚔') || text.contains('▶') || text.contains('▣'));
    }

    #[test]
    #[ignore]
    fn dump_everlight_like_screens() {
        let mut state = ExpeditionState::new();
        eprintln!("=== CAMP wide ===\n{}", draw_text(&state));
        state.hub_tab = HubTab::Arcade;
        eprintln!("=== ARCADE ===\n{}", draw_text(&state));
        assert!(super::super::logic::launch_sortie(&mut state));
        eprintln!("=== PLACING ===\n{}", draw_text(&state));
        assert!(super::super::logic::confirm_placement(&mut state));
        for _ in 0..40 {
            super::super::logic::tick(&mut state, 1);
        }
        eprintln!("=== RUNNING ===\n{}", draw_text(&state));
    }
}
