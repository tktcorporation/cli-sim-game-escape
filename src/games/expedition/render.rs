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

/// critique の goal 照合用。拠点ヘッダにも同じ文言を出す。
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
        Screen::Camp => format!("次: {} を防衛する", state.current_stage_title()),
        Screen::Forming => "次: 3人を選んで出発する".into(),
        Screen::Placing => "次: 道に3人を置いて防衛開始".into(),
        Screen::Running => format!(
            "防衛中 {} …",
            state
                .sortie
                .as_ref()
                .map(|s| ExpeditionState::stage_title(s.chapter, s.stage))
                .unwrap_or_default()
        ),
        Screen::Result if state.last_failed => "次: 遊技場でレベルを上げる".into(),
        Screen::Result => format!("次: {} へ進む", state.current_stage_title()),
    }
}

fn role_glyph(role: Role) -> char {
    match role {
        Role::Vanguard => '▣',
        Role::Striker => '▲',
        Role::Support => '✚',
    }
}

fn hero_detail(h: &Hero) -> String {
    format!(
        "{}{} Lv.{} [{}]",
        role_glyph(h.role),
        h.name,
        h.level,
        h.role.label()
    )
}

fn pip_bar(filled: u32, cap: u32, on: char, off: char) -> String {
    let mut s = String::new();
    for i in 0..cap {
        s.push(if i < filled { on } else { off });
    }
    s
}

fn dust_divider() -> Line<'static> {
    const DOTS: [char; 8] = ['⠁', '⠐', '⠂', '⠠', '⠄', '⢀', '⡀', '⠈'];
    let dots: String = DOTS.iter().cycle().take(20).collect();
    Line::from(Span::styled(format!(" {dots}"), Style::default().fg(EMBER)))
}

fn phase_label(screen: Screen) -> &'static str {
    match screen {
        Screen::Camp => "待機",
        Screen::Forming => "編成",
        Screen::Placing => "配置",
        Screen::Running => "防衛",
        Screen::Result => "結果",
    }
}

/// ターミナル向けの小さな銘標。figlet ではなく Braille 飾り＋漢字でブランドを示す。
fn brand_mark() -> Line<'static> {
    Line::from(vec![
        Span::styled(" ⠐⠂⠐ ", Style::default().fg(EMBER)),
        Span::styled(
            "遠征団",
            Style::default()
                .fg(Color::LightYellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" ⠂⠐⠂", Style::default().fg(EMBER)),
    ])
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
            Constraint::Length(5),
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
            .constraints([Constraint::Min(28), Constraint::Length(20)])
            .split(area);
        (h[0], Some(h[1]))
    };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(10), Constraint::Length(7)])
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
    let phase = phase_label(state.screen);
    let title = state.current_stage_title();

    let line1 = brand_mark();
    let line2 = Line::from(vec![
        Span::styled(
            format!(" {title} "),
            Style::default()
                .fg(accent())
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("· {phase} "),
            Style::default().fg(Color::Gray),
        ),
    ]);
    let line3 = Line::from(vec![
        Span::styled(" 行軍糧 ", muted()),
        Span::styled(format!("{ration} "), Style::default().fg(Color::LightGreen)),
        Span::styled("メダル ", muted()),
        Span::styled(
            format!("◎{} ", state.medals),
            Style::default().fg(Color::LightYellow),
        ),
        Span::styled("光珠 ", muted()),
        Span::styled(orb.clone(), Style::default().fg(Color::Cyan)),
    ]);
    let line4 = Line::from(Span::styled(
        format!(" {}", next_goal_line(state)),
        Style::default().fg(Color::White),
    ));

    f.render_widget(
        Paragraph::new(vec![line1, line2, line3, line4]).block(
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
            Paragraph::new(Line::from(Span::styled(
                " · 出撃中 · ",
                muted(),
            )))
            .alignment(Alignment::Center),
            area,
        );
        return;
    }
    let camp = if state.hub_tab == HubTab::Camp {
        "● 拠点 ⌂"
    } else {
        "  拠点 ⌂"
    };
    let arcade = if state.hub_tab == HubTab::Arcade {
        "● 遊技場 ◎"
    } else {
        "  遊技場 ◎"
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
            Constraint::Length(7),
            Constraint::Min(4),
            Constraint::Length(3),
        ])
        .split(area);

    // 戦役マップ
    let mut map_spans = vec![Span::styled(
        format!(" 第{}章 ", state.chapter),
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

    let flavor_row: Vec<Span> = (1..=4u32)
        .flat_map(|s| {
            let name = ExpeditionState::stage_flavor(s);
            let style = if s == state.stage {
                Style::default().fg(Color::White)
            } else {
                muted()
            };
            let pad = if s == 1 { "  " } else { " · " };
            vec![
                Span::styled(pad, muted()),
                Span::styled(name, style),
            ]
        })
        .collect();

    f.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                " 戦役",
                Style::default()
                    .fg(accent())
                    .add_modifier(Modifier::BOLD),
            )),
            dust_divider(),
            Line::from(map_spans),
            Line::from(flavor_row),
            Line::from(Span::styled(
                format!("  目標  {}", state.current_stage_title()),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )),
        ])
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(" マップ "),
        ),
        chunks[0],
    );

    // 編成パネル
    let mut party_lines = vec![
        Line::from(Span::styled(
            " 編成",
            Style::default()
                .fg(accent())
                .add_modifier(Modifier::BOLD),
        )),
        dust_divider(),
    ];
    for slot in &state.forming {
        match slot {
            Some(id) => {
                if let Some(h) = state.hero(*id) {
                    party_lines.push(Line::from(Span::styled(
                        format!("  ◆ {}", hero_detail(h)),
                        Style::default().fg(Color::White),
                    )));
                }
            }
            None => party_lines.push(Line::from(Span::styled(
                "  ◇ （空き）",
                muted(),
            ))),
        }
    }
    if party_lines.len() < 6 {
        party_lines.push(Line::from(Span::styled(
            "  タップで編成を開く",
            muted(),
        )));
    }
    Clickable::new(
        Paragraph::new(party_lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(" 団員 "),
        ),
        OPEN_FORMING,
    )
    .render(f, chunks[1], &mut click_state.borrow_mut());

    let party_ready = state.forming_count() == PARTY_SIZE;
    let (label, style, border, action) = if state.rations == 0 {
        (
            format!(" 行軍糧を回復中 ◆ {}", ration_fill_bar(state)),
            Style::default().fg(Color::DarkGray),
            Color::DarkGray,
            START_FORMING,
        )
    } else if !party_ready {
        (
            " 編成を揃える（3人） ".into(),
            Style::default()
                .fg(Color::Black)
                .bg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
            Color::Yellow,
            OPEN_FORMING,
        )
    } else {
        (
            format!(" ▶ 出撃  {} ", state.current_stage_title()),
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

/// ワイド拠点の右情景。行軍路・門・篝火を重ね、章の進行で空の稜線が伸びる。
fn render_camp_ambience(state: &ExpeditionState, f: &mut Frame, area: Rect) {
    let cx = CAMP_AMB_W / 2.0;
    let cy = CAMP_AMB_H * 0.38;
    let scale = 1.0 + (state.medals.min(40) as f64 / 40.0) * 0.45 + (state.chapter as f64) * 0.06;

    // 篝火（拠点の灯）
    let core = canvas_fx::filled_ellipse_points(cx, cy, 2.0 * scale, 2.2 * scale, 0.45);
    let glow = canvas_fx::filled_ellipse_points(cx, cy, 5.2 * scale, 5.5 * scale, 0.55);
    let halo = canvas_fx::ring_points(cx, cy, 8.0 * scale, 0.14);
    let smoke: Vec<(f64, f64)> = (0..14)
        .map(|i| {
            let t = i as f64;
            let x = cx + (t * 1.7).sin() * (1.2 + t * 0.15);
            let y = cy - 4.0 - t * 1.8 - (t * 0.9).cos() * 0.6;
            (x.clamp(1.0, CAMP_AMB_W - 1.0), y.max(1.0))
        })
        .collect();

    // 曲がりくねった行軍路
    let mut road_pts: Vec<(f64, f64)> = Vec::new();
    let mut road_edge: Vec<(f64, f64)> = Vec::new();
    for i in 0..64 {
        let t = i as f64 / 63.0;
        let x = 2.0 + t * (CAMP_AMB_W - 5.0);
        let y = CAMP_AMB_H * 0.72 + (t * std::f64::consts::PI * 2.0).sin() * 2.2;
        road_pts.push((x, y));
        road_edge.push((x, y - 1.4));
        road_edge.push((x, y + 1.4));
    }

    // 門（右側）
    let gate_x = CAMP_AMB_W - 5.5;
    let gate_posts = canvas_fx::filled_rect_points(
        gate_x - 0.6,
        CAMP_AMB_H * 0.58,
        gate_x + 0.6,
        CAMP_AMB_H * 0.86,
        0.4,
    );
    let gate_posts2 = canvas_fx::filled_rect_points(
        gate_x + 2.2,
        CAMP_AMB_H * 0.58,
        gate_x + 3.4,
        CAMP_AMB_H * 0.86,
        0.4,
    );
    let gate_arch = canvas_fx::ellipse_ring_points(
        gate_x + 1.4,
        CAMP_AMB_H * 0.60,
        2.4,
        1.6,
        0.22,
    );

    // 章が進むほど右へ伸びる稜線（街/砦の気配）
    let maturity = (state.chapter as usize).saturating_add(state.stage as usize).min(8);
    let ridge: Vec<(f64, f64)> = (0..36)
        .map(|i: usize| {
            let t = i as f64;
            let h = ((i.wrapping_mul(11) ^ (maturity * 3)) % (maturity + 1)) as f64;
            let x = 1.5 + t * (CAMP_AMB_W - 3.0) / 35.0;
            let y = CAMP_AMB_H * 0.22 - h * 1.1;
            (x, y)
        })
        .collect();

    let dust: Vec<(f64, f64)> = (0..28)
        .map(|i| {
            let t = i as f64;
            let x = cx + (t * 2.1).sin() * (3.0 + (t * 0.6).cos() * 9.0);
            let y = CAMP_AMB_H - (t * 6.5 + (t * 1.7).sin() * 5.0) % (CAMP_AMB_H - 8.0) - 2.0;
            (x.clamp(1.0, CAMP_AMB_W - 1.0), y)
        })
        .collect();

    // 章マーカー（道上の節）
    let mut stage_marks: Vec<(f64, f64)> = Vec::new();
    for s in 1..=4u32 {
        let t = (s as f64 - 0.5) / 4.0;
        let x = 2.0 + t * (CAMP_AMB_W - 5.0);
        let y = CAMP_AMB_H * 0.72 + (t * std::f64::consts::PI * 2.0).sin() * 2.2;
        let r = if s == state.stage { 1.6 } else { 1.0 };
        stage_marks.extend(canvas_fx::filled_ellipse_points(x, y, r, r * 0.7, 0.45));
    }

    let title = format!(" 行軍 · 第{}章 ", state.chapter);
    let canvas = Canvas::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(title),
        )
        .x_bounds([0.0, CAMP_AMB_W])
        .y_bounds([0.0, CAMP_AMB_H])
        .marker(Marker::Braille)
        .paint(move |ctx| {
            ctx.draw(&Points {
                coords: &dust,
                color: EMBER,
            });
            ctx.draw(&Points {
                coords: &ridge,
                color: Color::Rgb(70, 70, 85),
            });
            ctx.draw(&Points {
                coords: &road_edge,
                color: Color::Rgb(45, 45, 55),
            });
            ctx.draw(&Points {
                coords: &road_pts,
                color: Color::DarkGray,
            });
            ctx.draw(&Points {
                coords: &stage_marks,
                color: Color::Rgb(160, 130, 70),
            });
            ctx.draw(&Points {
                coords: &gate_posts,
                color: Color::Gray,
            });
            ctx.draw(&Points {
                coords: &gate_posts2,
                color: Color::Gray,
            });
            ctx.draw(&Points {
                coords: &gate_arch,
                color: Color::LightGreen,
            });
            ctx.draw(&Points {
                coords: &smoke,
                color: Color::Rgb(90, 90, 100),
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
        " 出撃編成",
        Style::default()
            .fg(accent())
            .add_modifier(Modifier::BOLD),
    )));
    cl.push(dust_divider());
    cl.push(Line::from(Span::styled(
        format!(
            "  選抜 {}/{}  {}",
            sel,
            PARTY_SIZE,
            "◆".repeat(sel) + &"◇".repeat(PARTY_SIZE.saturating_sub(sel))
        ),
        Style::default().fg(accent()),
    )));
    cl.push(Line::from(Span::styled(
        "  盾=近距離強打 / 刃=遠距離 / 癒=減速",
        muted(),
    )));
    cl.push(Line::from(""));
    for h in &state.roster {
        let selected = state.forming.contains(&Some(h.id));
        let mark = if selected { "◆" } else { "◇" };
        let style = if selected {
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        };
        cl.push_clickable(
            Line::from(Span::styled(
                format!("  {mark} {}", hero_detail(h)),
                style,
            )),
            toggle_hero_id(h.id),
        );
    }
    cl.render(
        f,
        chunks[0],
        Block::default()
            .borders(Borders::ALL)
            .title(" 団員を選ぶ ")
            .border_style(Style::default().fg(Color::DarkGray)),
        &mut click_state.borrow_mut(),
        false,
        0,
    );

    if state.forming_count() == PARTY_SIZE {
        Clickable::new(
            Paragraph::new(" ▶ 配置へ進む ")
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
            Paragraph::new(" あと人数を選んでください ")
                .alignment(Alignment::Center)
                .style(muted())
                .block(Block::default().borders(Borders::ALL)),
            chunks[1],
        );
    }
    Clickable::new(
        Paragraph::new(" ← 拠点へ戻る ")
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
            Paragraph::new(" ▶ 防衛開始 ")
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
            Paragraph::new(" ← 撤退 ")
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
                Span::styled(
                    format!(" 波 {wave} "),
                    Style::default().fg(Color::LightMagenta),
                ),
                Span::styled(
                    format!(
                        "門 ■{} ",
                        hp_bar(sortie.gate_hp, sortie.gate_max_hp)
                    ),
                    Style::default().fg(Color::LightGreen),
                ),
                Span::styled(
                    format!(
                        "{} ",
                        ExpeditionState::stage_title(sortie.chapter, sortie.stage)
                    ),
                    Style::default().fg(Color::Gray),
                ),
            ]))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" 防衛状況 "),
            ),
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
    let title = if state.screen == Screen::Placing {
        " 防衛路 · 配置 "
    } else {
        " 防衛路 "
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
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

    // 道の縁取り（上下二重）
    let path_mid = (PATH_X0, PATH_Y, PATH_X1, PATH_Y);
    let path_top = (PATH_X0, PATH_Y - 2.2, PATH_X1, PATH_Y - 2.2);
    let path_bot = (PATH_X0, PATH_Y + 2.2, PATH_X1, PATH_Y + 2.2);

    // 地面の砂粒
    let grit: Vec<(f64, f64)> = (0..48)
        .map(|i| {
            let t = i as f64;
            let x = PATH_X0 + (t * 7.3) % (PATH_X1 - PATH_X0);
            let y = PATH_Y + (t * 1.9).sin() * 3.8;
            (x, y)
        })
        .collect();

    // 門：柱＋アーチ
    let gate_l = canvas_fx::filled_rect_points(PATH_X1, PATH_Y - 7.0, PATH_X1 + 1.4, PATH_Y + 7.0, 0.45);
    let gate_r = canvas_fx::filled_rect_points(PATH_X1 + 3.2, PATH_Y - 7.0, PATH_X1 + 4.6, PATH_Y + 7.0, 0.45);
    let gate_arch = canvas_fx::ellipse_ring_points(PATH_X1 + 2.3, PATH_Y - 5.5, 3.0, 2.0, 0.18);
    let gate_glow = canvas_fx::filled_ellipse_points(PATH_X1 + 2.3, PATH_Y, 1.4, 2.8, 0.5);

    // 湧出側の渦
    let spawn_core = canvas_fx::filled_ellipse_points(PATH_X0 - 2.0, PATH_Y, 1.6, 1.6, 0.5);
    let spawn_ring = canvas_fx::ring_points(PATH_X0 - 2.0, PATH_Y, 3.2, 0.18);

    // スロット目安＋番号位置
    let mut slot_marks: Vec<(f64, f64)> = Vec::new();
    let mut empty_slots: Vec<(f64, f64)> = Vec::new();
    for i in 0..PATH_LEN {
        let x = path_to_world_x(i as f64 + 0.5);
        let occupied = state
            .sortie
            .as_ref()
            .and_then(|s| s.path.get(i).copied().flatten())
            .is_some();
        if occupied {
            slot_marks.extend(canvas_fx::ring_points(x, PATH_Y, 1.4, 0.3));
        } else {
            empty_slots.extend(canvas_fx::ring_points(x, PATH_Y, 1.1, 0.35));
            empty_slots.extend(canvas_fx::filled_ellipse_points(x, PATH_Y, 0.5, 0.5, 0.4));
        }
    }

    // 団員（役割で縦横比を変える）
    let mut hero_pts: Vec<(Vec<(f64, f64)>, Color)> = Vec::new();
    let mut range_rings: Vec<(Vec<(f64, f64)>, Color)> = Vec::new();
    if let Some(sortie) = state.sortie.as_ref() {
        for (slot, id) in sortie.path.iter().enumerate() {
            let Some(hid) = id else { continue };
            let Some(h) = state.hero(*hid) else { continue };
            let x = path_to_world_x(slot as f64 + 0.5);
            let (rx, ry, color) = match h.role {
                Role::Vanguard => (2.0, 1.7, Color::LightBlue),
                Role::Striker => (1.5, 2.2, Color::LightRed),
                Role::Support => (1.8, 1.8, Color::LightGreen),
            };
            let rboost = h.level as f64 * 0.12;
            hero_pts.push((
                canvas_fx::filled_ellipse_points(x, PATH_Y, rx + rboost, ry + rboost, 0.45),
                color,
            ));
            hero_pts.push((
                canvas_fx::ring_points(x, PATH_Y, (rx + rboost) * 1.15, 0.25),
                color,
            ));
            if state.screen == Screen::Placing {
                let rr = h.role.range() as f64 * ((PATH_X1 - PATH_X0) / PATH_LEN as f64) * 0.5;
                range_rings.push((canvas_fx::ring_points(x, PATH_Y, rr, 0.16), Color::DarkGray));
            }
        }
    }

    // 敵
    let mut enemy_pts: Vec<(f64, f64)> = Vec::new();
    let mut boss_pts: Vec<(f64, f64)> = Vec::new();
    let mut boss_rings: Vec<(f64, f64)> = Vec::new();
    if let Some(sortie) = state.sortie.as_ref() {
        for c in &sortie.creeps {
            if c.hp <= 0 {
                continue;
            }
            let x = path_to_world_x(c.pos);
            let is_boss =
                c.name.contains("首領") || c.name.contains("甲殻") || c.name.contains("大口");
            if is_boss {
                boss_pts.extend(canvas_fx::filled_ellipse_points(x, PATH_Y, 2.6, 2.2, 0.45));
                boss_rings.extend(canvas_fx::ring_points(x, PATH_Y, 3.4, 0.2));
            } else {
                enemy_pts.extend(canvas_fx::filled_ellipse_points(x, PATH_Y, 1.5, 1.3, 0.5));
            }
        }
    }

    let canvas = Canvas::default()
        .block(block)
        .x_bounds([0.0, WORLD_W])
        .y_bounds([0.0, WORLD_H])
        .marker(Marker::Braille)
        .paint(move |ctx| {
            ctx.draw(&Points {
                coords: &grit,
                color: Color::Rgb(40, 40, 48),
            });
            for line in [path_top, path_mid, path_bot] {
                ctx.draw(&CanvasLine {
                    x1: line.0,
                    y1: line.1,
                    x2: line.2,
                    y2: line.3,
                    color: Color::DarkGray,
                });
            }
            ctx.draw(&Points {
                coords: &empty_slots,
                color: Color::Rgb(70, 70, 90),
            });
            ctx.draw(&Points {
                coords: &slot_marks,
                color: Color::Rgb(100, 100, 120),
            });
            ctx.draw(&Points {
                coords: &spawn_ring,
                color: Color::Rgb(120, 60, 60),
            });
            ctx.draw(&Points {
                coords: &spawn_core,
                color: Color::LightRed,
            });
            ctx.draw(&Points {
                coords: &gate_l,
                color: Color::Gray,
            });
            ctx.draw(&Points {
                coords: &gate_r,
                color: Color::Gray,
            });
            ctx.draw(&Points {
                coords: &gate_arch,
                color: Color::LightGreen,
            });
            ctx.draw(&Points {
                coords: &gate_glow,
                color: Color::Rgb(80, 140, 80),
            });
            for (pts, color) in &range_rings {
                ctx.draw(&Points {
                    coords: pts,
                    color: *color,
                });
            }
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
                    coords: &boss_rings,
                    color: Color::Magenta,
                });
                ctx.draw(&Points {
                    coords: &boss_pts,
                    color: Color::LightMagenta,
                });
            }
        });
    f.render_widget(canvas, area);

    // 操作ヒント（Canvas 上端を帯で上書き。点描と混ざらないよう全幅）
    if inner.height > 0 && inner.width > 0 {
        let toast = if state.screen == Screen::Placing {
            let placed = state
                .sortie
                .as_ref()
                .map(|s| s.path.iter().flatten().count())
                .unwrap_or(0);
            format!("道をタップして配置  {placed}/{PARTY_SIZE}")
        } else {
            "敵は左から門へ進む · 射程内の団員が自動攻撃".into()
        };
        let toast_area = Rect::new(inner.x, inner.y, inner.width, 1);
        f.render_widget(Clear, toast_area);
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                format!(" {toast} "),
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::LightYellow),
            )))
            .alignment(Alignment::Center),
            toast_area,
        );
    }
}

fn render_running_side(state: &ExpeditionState, f: &mut Frame, area: Rect) {
    let Some(sortie) = state.sortie.as_ref() else {
        return;
    };
    let mut lines = vec![
        Line::from(Span::styled(
            format!(
                " {}",
                ExpeditionState::stage_title(sortie.chapter, sortie.stage)
            ),
            Style::default().fg(accent()).add_modifier(Modifier::BOLD),
        )),
        dust_divider(),
        Line::from(Span::styled(" 配置", muted())),
    ];
    for (i, slot) in sortie.path.iter().enumerate() {
        if let Some(id) = slot {
            if let Some(h) = state.hero(*id) {
                lines.push(Line::from(Span::styled(
                    format!("  {} {}", i + 1, hero_detail(h)),
                    Style::default().fg(Color::White),
                )));
            }
        } else {
            lines.push(Line::from(Span::styled(
                format!("  {} ···", i + 1),
                muted(),
            )));
        }
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(" 接近中", muted())));
    if sortie.creeps.is_empty() {
        lines.push(Line::from(Span::styled("  （敵なし）", muted())));
    }
    for c in sortie.creeps.iter().take(5) {
        let frac = if c.max_hp > 0 {
            ((c.hp.max(0) * 4) / c.max_hp) as usize
        } else {
            0
        };
        lines.push(Line::from(Span::styled(
            format!(
                "  {} {}{}",
                c.name.chars().take(2).collect::<String>(),
                "▮".repeat(frac),
                "▯".repeat(4usize.saturating_sub(frac))
            ),
            Style::default().fg(Color::LightRed),
        )));
    }
    f.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" 戦況 ")
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
        .title(format!(" 投入レーン · 所持メダル ◎{} ", state.medals))
        .border_style(Style::default().fg(Color::DarkGray));
    let inner = block.inner(layout.lanes);
    f.render_widget(block.clone(), layout.lanes);
    if inner.width > 0 && inner.height > 0 && !state.pending_level_pick {
        let cell_w = (inner.width / PUSH_W as u16).max(1);
        let mut cs = click_state.borrow_mut();
        ClickableGrid::new(PUSH_W, 1, super::actions::DROP_LANE_BASE, cell_w)
            .with_cell_height(inner.height)
            .register_targets(layout.lanes, &block, &mut cs, 0);
        let mut lines = vec![Line::from(Span::styled(
            " レーンを選んでメダルを落とす",
            muted(),
        ))];
        let mut marks = String::from(" ");
        for lane in 0..PUSH_W {
            let hot = state.pusher.cells[PUSH_D - 1][lane].medals >= 3;
            marks.push_str(&format!("{}{} ", lane + 1, if hot { "▼!" } else { "▼" }));
        }
        lines.push(Line::from(Span::styled(
            marks,
            Style::default().fg(accent()).add_modifier(Modifier::BOLD),
        )));
        if state.pusher.last_drop_medals > 0 || state.pusher.last_drop_orb {
            let mut flash = format!(" 落下 +{}", state.pusher.last_drop_medals);
            if state.pusher.last_drop_orb {
                flash.push_str("  光珠!");
            }
            lines.push(Line::from(Span::styled(
                flash,
                Style::default().fg(Color::LightCyan),
            )));
        }
        f.render_widget(Paragraph::new(lines).alignment(Alignment::Center), inner);
    }
}

fn render_arcade_side(state: &ExpeditionState, f: &mut Frame, area: Rect) {
    let orb = pip_bar(state.orb_gauge.min(ORB_NEED), ORB_NEED, '●', '○');
    let mut lines = vec![
        Line::from(Span::styled(
            " 育成",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        dust_divider(),
        Line::from(Span::styled(
            format!(" 光珠 {orb}"),
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            format!(" あと {} でレベルアップ", state.orb_remaining().max(1)),
            muted(),
        )),
        Line::from(""),
        Line::from(Span::styled(" 団員", muted())),
    ];
    for h in &state.roster {
        lines.push(Line::from(format!(" {}", hero_detail(h))));
    }
    f.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" 遊技場 ")
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

    // 筐体の左右壁
    let wall_l = canvas_fx::filled_rect_points(0.2, 1.0, 1.0, PUSH_WORLD_H - 1.0, 0.5);
    let wall_r = canvas_fx::filled_rect_points(
        PUSH_WORLD_W - 1.0,
        1.0,
        PUSH_WORLD_W - 0.2,
        PUSH_WORLD_H - 1.0,
        0.5,
    );

    // 押し板（歯付き）
    let plate_x = (state.pusher.plate_col as f64 + 0.5) * cell_w;
    let plate = canvas_fx::filled_rect_points(plate_x - 1.6, 1.0, plate_x + 1.6, 3.4, 0.4);
    let mut plate_teeth: Vec<(f64, f64)> = Vec::new();
    for t in 0..5 {
        let tx = plate_x - 1.2 + t as f64 * 0.6;
        plate_teeth.extend(canvas_fx::filled_ellipse_points(tx, 3.6, 0.35, 0.55, 0.35));
    }

    // マス目の淡い格子
    let mut grid_pts: Vec<(f64, f64)> = Vec::new();
    for row in 0..=PUSH_D {
        let y = y0 + row as f64 * cell_h;
        for i in 0..40 {
            let x = (i as f64 / 39.0) * PUSH_WORLD_W;
            grid_pts.push((x, y));
        }
    }
    for col in 0..=PUSH_W {
        let x = col as f64 * cell_w;
        for i in 0..30 {
            let y = y0 + (i as f64 / 29.0) * (PUSH_D as f64 * cell_h);
            grid_pts.push((x, y));
        }
    }

    let mut pile_pts: Vec<(Vec<(f64, f64)>, Color)> = Vec::new();
    let mut orb_pts: Vec<(f64, f64)> = Vec::new();
    let mut hot_edge: Vec<(f64, f64)> = Vec::new();
    for row in 0..PUSH_D {
        for col in 0..PUSH_W {
            let cell = state.pusher.cells[row][col];
            let cx = (col as f64 + 0.5) * cell_w;
            let cy = y0 + (row as f64 + 0.5) * cell_h;
            if cell.medals > 0 {
                // 積み上げ感: 枚数ぶん少しずらした楕円
                for n in 0..cell.medals.min(5) {
                    let ox = ((n as f64) - 2.0) * 0.25;
                    let oy = -(n as f64) * 0.35;
                    let r = 0.7 + cell.medals as f64 * 0.12;
                    let color = if row + 1 == PUSH_D {
                        Color::LightYellow
                    } else if cell.medals >= 5 {
                        Color::Yellow
                    } else {
                        Color::Rgb(200, 170, 40)
                    };
                    pile_pts.push((
                        canvas_fx::filled_ellipse_points(cx + ox, cy + oy, r, r * 0.65, 0.45),
                        color,
                    ));
                }
                if row + 1 == PUSH_D && cell.medals >= 3 {
                    hot_edge.extend(canvas_fx::ring_points(cx, cy + cell_h * 0.35, 2.0, 0.25));
                }
            }
            if cell.has_orb {
                orb_pts.extend(canvas_fx::filled_ellipse_points(cx, cy - 0.4, 1.5, 1.5, 0.4));
                orb_pts.extend(canvas_fx::ring_points(cx, cy - 0.4, 2.4, 0.2));
                orb_pts.extend(canvas_fx::ring_points(cx, cy - 0.4, 3.0, 0.3));
            }
        }
    }

    let edge_y = y0 + (PUSH_D as f64) * cell_h;
    let edge_line = (1.0, edge_y, PUSH_WORLD_W - 1.0, edge_y);
    let edge_glow = canvas_fx::filled_rect_points(1.0, edge_y - 0.4, PUSH_WORLD_W - 1.0, edge_y + 0.8, 0.45);

    let flash_pts: Vec<(f64, f64)> = if state.pusher.last_drop_medals > 0 || state.pusher.last_drop_orb
    {
        let mut pts = canvas_fx::filled_ellipse_points(
            PUSH_WORLD_W / 2.0,
            edge_y + 2.2,
            2.4 + state.pusher.last_drop_medals as f64 * 0.45,
            1.4,
            0.45,
        );
        if state.pusher.last_drop_orb {
            pts.extend(canvas_fx::ring_points(
                PUSH_WORLD_W / 2.0,
                edge_y + 2.2,
                4.0,
                0.2,
            ));
        }
        pts
    } else {
        Vec::new()
    };

    let orb_title = pip_bar(state.orb_gauge.min(ORB_NEED), ORB_NEED, '●', '○');
    let canvas = Canvas::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" メダルプッシャー  光珠{orb_title} "))
                .border_style(Style::default().fg(Color::DarkGray)),
        )
        .x_bounds([0.0, PUSH_WORLD_W])
        .y_bounds([0.0, PUSH_WORLD_H])
        .marker(Marker::Braille)
        .paint(move |ctx| {
            ctx.draw(&Points {
                coords: &grid_pts,
                color: Color::Rgb(35, 35, 42),
            });
            ctx.draw(&Points {
                coords: &wall_l,
                color: Color::Gray,
            });
            ctx.draw(&Points {
                coords: &wall_r,
                color: Color::Gray,
            });
            ctx.draw(&Points {
                coords: &edge_glow,
                color: Color::Rgb(80, 40, 40),
            });
            ctx.draw(&CanvasLine {
                x1: edge_line.0,
                y1: edge_line.1,
                x2: edge_line.2,
                y2: edge_line.3,
                color: Color::LightRed,
            });
            ctx.draw(&Points {
                coords: &plate,
                color: Color::DarkGray,
            });
            ctx.draw(&Points {
                coords: &plate_teeth,
                color: Color::White,
            });
            for (pts, color) in &pile_pts {
                ctx.draw(&Points {
                    coords: pts,
                    color: *color,
                });
            }
            if !hot_edge.is_empty() {
                ctx.draw(&Points {
                    coords: &hot_edge,
                    color: Color::LightYellow,
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
        " 光珠が満ちた — レベルアップ",
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )));
    cl.push(dust_divider());
    cl.push(Line::from(Span::styled(
        " 育てる団員を選ぶ",
        muted(),
    )));
    for h in &state.roster {
        cl.push_clickable(
            Line::from(format!("  ▶ {}", hero_detail(h))),
            level_hero_id(h.id),
        );
    }
    cl.render(
        f,
        modal,
        Block::default()
            .borders(Borders::ALL)
            .title(" 育成 ")
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

    let (headline, color) = if state.last_failed {
        (" 敗退 ", Color::LightRed)
    } else {
        (" 防衛成功 ", Color::LightGreen)
    };
    let mut lines = vec![
        brand_mark(),
        Line::from(Span::styled(
            headline,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        )),
        dust_divider(),
        Line::from(Span::styled(
            format!(" 節  {}", state.current_stage_title()),
            Style::default().fg(Color::White),
        )),
    ];
    if !state.result_summary.is_empty() {
        lines.push(Line::from(Span::styled(
            format!("  {}", state.result_summary),
            Style::default().fg(Color::LightYellow),
        )));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(" 団員", muted())));
    for h in &state.roster {
        lines.push(Line::from(Span::styled(
            format!("  {}", hero_detail(h)),
            Style::default().fg(Color::White),
        )));
    }
    if state.last_failed {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            " 遊技場で育てて再挑戦しよう",
            Style::default().fg(Color::Cyan),
        )));
    }

    f.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" 結果 ")
                .border_style(Style::default().fg(color)),
        ),
        chunks[0],
    );

    let cta = if state.last_failed {
        " ▶ 遊技場へ "
    } else {
        " ▶ 拠点に戻る "
    };
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
        assert!(text.contains("斥候道"));
        assert!(text.contains("出撃"));
        assert!(text.contains("遠征団"));
        assert!(text.contains("行軍糧"));
        assert!(text.contains("拠点"));
    }

    #[test]
    fn wide_camp_has_ambience_braille() {
        let state = ExpeditionState::new();
        let text = draw_text(&state);
        assert!(text.contains("行軍"));
        assert!(text.contains('⌂') || text.contains('◎') || text.contains('◆'));
    }

    #[test]
    fn camp_shows_labeled_resources_and_goal() {
        let state = ExpeditionState::new();
        let text = draw_text(&state);
        assert!(text.contains("メダル"));
        assert!(text.contains("光珠"));
        assert!(text.contains("次:"));
        assert!(text.contains("編成"));
    }

    #[test]
    fn arcade_shows_drop_grid() {
        let mut state = ExpeditionState::new();
        state.hub_tab = HubTab::Arcade;
        let text = draw_text(&state);
        assert!(text.contains("メダルプッシャー") || text.contains("投入レーン"));
        assert!(text.contains('▼') || text.contains('◎'));
        assert!(text.contains("遊技場"));
    }

    #[test]
    fn placing_uses_path_canvas() {
        let mut state = ExpeditionState::new();
        assert!(super::super::logic::launch_sortie(&mut state));
        let text = draw_text(&state);
        assert!(text.contains("防衛路") || text.contains("配置"));
        assert!(text.contains("防衛開始") || text.contains('▶'));
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
