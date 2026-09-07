//! 遠征団の描画。
//!
//! 常設シェル（上部 HUD + 下部タブバー）の上に拠点・配置・防衛・遊技場を載せる。
//! ループは説明文ではなく、マップ・配置盤・残りマスと主ボタンで伝える。

use std::cell::RefCell;
use std::rc::Rc;

use ratzilla::ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratzilla::ratatui::style::{Color, Modifier, Style};
use ratzilla::ratatui::text::{Line, Span};
use ratzilla::ratatui::widgets::{Block, Borders, Paragraph};
use ratzilla::ratatui::Frame;

use crate::games::GameChoice;
use crate::input::ClickState;
use crate::theme;
use crate::widgets::{Clickable, ClickableList, TabBar};

use super::actions::{
    level_hero_id, place_slot_id, toggle_hero_id, ACK_RESULT, CANCEL_FORMING, CONFIRM_PLACEMENT,
    LAUNCH, MEDAL_ROLL, OPEN_FORMING, START_FORMING, TAB_ARCADE, TAB_CAMP,
};
use super::state::{ExpeditionState, HubTab, Screen, MEDAL_BET, PARTY_SIZE, PATH_LEN};

fn accent() -> Color {
    theme::accent(&GameChoice::Expedition)
}

fn muted() -> Style {
    Style::default().fg(Color::DarkGray)
}

fn label_style() -> Style {
    Style::default().fg(Color::Gray)
}

fn goal_style() -> Style {
    Style::default()
        .fg(Color::LightGreen)
        .add_modifier(Modifier::BOLD)
}

/// 画面に出す「いまの目標」一文。critique の goal 照合にも使う。
pub fn next_goal_line(state: &ExpeditionState) -> String {
    match state.screen {
        Screen::Camp if state.hub_tab == HubTab::Arcade && state.pending_level_pick => {
            "次: 育てる団員を選ぶ".into()
        }
        Screen::Camp if state.hub_tab == HubTab::Arcade => {
            format!("次: 目的地まであと{}", state.board_remaining())
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

fn on_sortie(state: &ExpeditionState) -> bool {
    !matches!(state.screen, Screen::Camp)
}

pub fn render(
    state: &ExpeditionState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(6),
            Constraint::Length(3),
        ])
        .split(area);

    render_status_bar(state, f, chunks[0]);
    match state.screen {
        Screen::Camp => match state.hub_tab {
            HubTab::Camp => render_camp(state, f, chunks[1], click_state),
            HubTab::Arcade => render_arcade(state, f, chunks[1], click_state),
        },
        Screen::Forming => render_forming(state, f, chunks[1], click_state),
        Screen::Placing => render_placing(state, f, chunks[1], click_state),
        Screen::Running => render_running(state, f, chunks[1], click_state),
        Screen::Result => render_result(state, f, chunks[1], click_state),
    }
    render_bottom_nav(state, f, chunks[2], click_state);
}

fn render_status_bar(state: &ExpeditionState, f: &mut Frame, area: Rect) {
    let fill = ration_fill_bar(state);
    let phase = match state.screen {
        Screen::Camp => Span::styled(" 待機", label_style()),
        Screen::Forming => Span::styled(
            " 編成中",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Screen::Placing => Span::styled(
            " 配置中",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Screen::Running => Span::styled(
            " 防衛中",
            Style::default().fg(accent()).add_modifier(Modifier::BOLD),
        ),
        Screen::Result => Span::styled(
            " 帰還",
            Style::default()
                .fg(Color::LightGreen)
                .add_modifier(Modifier::BOLD),
        ),
    };

    let line1 = Line::from(vec![
        Span::styled(
            " 遠征団 ",
            Style::default()
                .fg(accent())
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("{} ", state.current_stage_label()),
            Style::default().fg(Color::White),
        ),
        phase,
    ]);
    let line2 = Line::from(vec![
        Span::styled(
            format!("糧{}/{}{} ", state.rations, state.ration_cap(), fill),
            Style::default().fg(Color::LightGreen),
        ),
        Span::styled(
            format!("メダル{} ", state.medals),
            Style::default().fg(Color::LightYellow),
        ),
        Span::styled(
            format!("あと{}マス ", state.board_remaining()),
            Style::default().fg(Color::Cyan),
        ),
    ]);
    f.render_widget(Paragraph::new(vec![line1, line2]), area);
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
    if on_sortie(state) && state.screen != Screen::Result {
        f.render_widget(
            Paragraph::new(" 出撃中 — タブ切替不可 ")
                .alignment(Alignment::Center)
                .style(muted()),
            area,
        );
        return;
    }
    let camp_label = if state.hub_tab == HubTab::Camp {
        "● 拠点"
    } else {
        "拠点"
    };
    let arcade_label = if state.hub_tab == HubTab::Arcade {
        "● 遊技場"
    } else {
        "遊技場"
    };
    TabBar::new("│")
        .tab(
            camp_label,
            tab_style(state.hub_tab == HubTab::Camp),
            TAB_CAMP,
        )
        .tab(
            arcade_label,
            tab_style(state.hub_tab == HubTab::Arcade),
            TAB_ARCADE,
        )
        .render(f, area, &mut click_state.borrow_mut());
}

fn section_title(text: &str) -> Line<'static> {
    Line::from(Span::styled(
        format!(" {text}"),
        Style::default().fg(Color::Gray),
    ))
}

fn hp_bar(hp: i32, max_hp: i32) -> String {
    let max_hp = max_hp.max(1);
    let filled = ((hp.max(0) * 8) / max_hp).clamp(0, 8) as usize;
    let empty = 8 - filled;
    format!("[{}{}]", "■".repeat(filled), "·".repeat(empty))
}

fn ration_fill_bar(state: &ExpeditionState) -> String {
    if state.rations >= state.ration_cap() {
        return String::new();
    }
    let need = state.ration_regen_ticks().max(1);
    let p = ((state.ration_progress * 4) / need).min(3);
    format!("[{}{}]", "#".repeat(p as usize), "-".repeat(3 - p as usize))
}

fn render_arcade(
    state: &ExpeditionState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(6),
            Constraint::Min(5),
            Constraint::Length(3),
        ])
        .split(area);

    let track = {
        let goal = state.board_goal.max(1);
        let pos = state.board_pos.min(goal);
        let mut cells = Vec::new();
        for i in 0..=goal {
            if i == pos {
                cells.push("●".to_string());
            } else if i == goal {
                cells.push("旗".to_string());
            } else {
                cells.push("·".to_string());
            }
        }
        cells.join("─")
    };
    let board = Paragraph::new(vec![
        Line::from(Span::styled(
            " 街道すごろく",
            Style::default()
                .fg(accent())
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            format!(" {track}"),
            Style::default().fg(Color::White),
        )),
        Line::from(Span::styled(
            format!(
                " 位置 {}/{}  残り{}",
                state.board_pos,
                state.board_goal,
                state.board_remaining()
            ),
            Style::default().fg(Color::Cyan),
        )),
        Line::from(Span::styled(
            format!(" メダル {}  (1回 -{MEDAL_BET})", state.medals),
            Style::default().fg(Color::LightYellow),
        )),
    ])
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title("遊技場"),
    );
    f.render_widget(board, chunks[0]);

    if state.pending_level_pick {
        let mut cl = ClickableList::new();
        cl.push(Line::from(Span::styled(
            " 目的地！ 育てる団員を選ぶ",
            goal_style(),
        )));
        for h in &state.roster {
            cl.push_clickable(
                Line::from(format!(
                    " ▶ {} [{}] Lv{}",
                    h.name,
                    h.role.label(),
                    h.level
                )),
                level_hero_id(h.id),
            );
        }
        cl.render(
            f,
            chunks[1],
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray)),
            &mut click_state.borrow_mut(),
            false,
            0,
        );
    } else {
        let mut lines = vec![section_title("団員")];
        for h in &state.roster {
            lines.push(Line::from(format!(
                "  {} [{}] Lv{}",
                h.name,
                h.role.label(),
                h.level
            )));
        }
        f.render_widget(
            Paragraph::new(lines).block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::DarkGray)),
            ),
            chunks[1],
        );
    }

    if state.pending_level_pick {
        f.render_widget(
            Paragraph::new(" 上の団員をタップ ")
                .alignment(Alignment::Center)
                .style(muted())
                .block(Block::default().borders(Borders::ALL)),
            chunks[2],
        );
    } else {
        let can = state.medals >= MEDAL_BET;
        let (label, style, border) = if can {
            (
                format!(" ▶ サイコロ -{MEDAL_BET} "),
                Style::default()
                    .fg(Color::Black)
                    .bg(accent())
                    .add_modifier(Modifier::BOLD),
                accent(),
            )
        } else {
            (
                " メダル不足 — 戦役で集める ".into(),
                Style::default().fg(Color::DarkGray),
                Color::DarkGray,
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
            MEDAL_ROLL,
        )
        .render(f, chunks[2], &mut click_state.borrow_mut());
    }
}

fn render_camp(
    state: &ExpeditionState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),
            Constraint::Min(5),
            Constraint::Length(3),
        ])
        .split(area);

    let map_line = {
        let mut parts = Vec::new();
        for s in 1..=4u32 {
            let label = format!("{}-{}", state.chapter, s);
            if s < state.stage {
                parts.push(format!("[{label}✓]"));
            } else if s == state.stage {
                parts.push(format!("▶{label}"));
            } else {
                parts.push(format!(" {label} "));
            }
        }
        parts.join("─")
    };
    let boss_note = if state.stage >= 4 { " Boss" } else { "" };
    let dest = Paragraph::new(vec![
        Line::from(Span::styled(
            format!(" 第{}章{}", state.chapter, boss_note),
            Style::default()
                .fg(accent())
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            format!(" {map_line}"),
            Style::default().fg(Color::White),
        )),
        Line::from(Span::styled(
            if state.last_failed {
                " 敗退 — 遊技場で育てて再配置"
            } else {
                " 道に置いて防衛し、次の節を開く"
            },
            muted(),
        )),
    ])
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title("戦役"),
    );
    f.render_widget(dest, chunks[0]);

    let mut party_lines: Vec<Line> = Vec::new();
    party_lines.push(Line::from(Span::styled(
        " 出撃メンバー",
        Style::default().fg(Color::Gray),
    )));
    for slot in &state.forming {
        match slot {
            Some(id) => {
                if let Some(h) = state.hero(*id) {
                    party_lines.push(Line::from(vec![
                        Span::styled(
                            format!(" [{}]", h.role.label()),
                            Style::default().fg(accent()).add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(
                            format!("{} ", h.name),
                            Style::default()
                                .fg(Color::White)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(
                            format!("Lv{} ", h.level),
                            Style::default().fg(Color::LightYellow),
                        ),
                    ]));
                }
            }
            None => {
                party_lines.push(Line::from(Span::styled(
                    " [?] 未選択",
                    Style::default().fg(Color::DarkGray),
                )));
            }
        }
    }
    let party = Paragraph::new(party_lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(" 編成 "),
    );
    Clickable::new(party, OPEN_FORMING).render(f, chunks[1], &mut click_state.borrow_mut());

    let party_ready = state.forming_count() == PARTY_SIZE;
    let (label, style, border, action) = if state.rations == 0 {
        (
            format!("行軍糧 回復中 {}", ration_fill_bar(state)),
            Style::default().fg(Color::DarkGray),
            Color::DarkGray,
            START_FORMING,
        )
    } else if !party_ready {
        (
            "メンバーを選ぶ".into(),
            Style::default()
                .fg(Color::Black)
                .bg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
            Color::Yellow,
            OPEN_FORMING,
        )
    } else {
        (
            format!("▶ {} に出る", state.current_stage_label()),
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
    cl.push(Line::from(Span::styled(next_goal_line(state), goal_style())));
    cl.push(section_title("名簿"));
    for h in &state.roster {
        let selected = state.forming.contains(&Some(h.id));
        let mark = if selected { "◆" } else { "◇" };
        cl.push_clickable(
            Line::from(format!(
                " {mark} {} [{}] Lv{}",
                h.name,
                h.role.label(),
                h.level
            )),
            toggle_hero_id(h.id),
        );
    }
    cl.render(
        f,
        chunks[0],
        Block::default().borders(Borders::ALL).title(" 編成 "),
        &mut click_state.borrow_mut(),
        false,
        0,
    );

    let ready = state.forming_count() == PARTY_SIZE;
    if ready {
        Clickable::new(
            Paragraph::new(" ▶ 配置へ ")
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
            Paragraph::new(" 3人選ぶ ")
                .alignment(Alignment::Center)
                .style(muted())
                .block(Block::default().borders(Borders::ALL)),
            chunks[1],
        );
    }
    Clickable::new(
        Paragraph::new(" 戻る ")
            .alignment(Alignment::Center)
            .style(muted())
            .block(Block::default().borders(Borders::ALL)),
        CANCEL_FORMING,
    )
    .render(f, chunks[2], &mut click_state.borrow_mut());
}

fn path_cell_label(state: &ExpeditionState, slot: usize) -> String {
    let Some(sortie) = state.sortie.as_ref() else {
        return "·".into();
    };
    if let Some(id) = sortie.path[slot] {
        if let Some(h) = state.hero(id) {
            return format!("{}{}", h.role.label(), h.name.chars().next().unwrap_or('?'));
        }
    }
    // 敵がこのマスにいるか
    let enemies_here: Vec<_> = sortie
        .creeps
        .iter()
        .filter(|c| c.pos == slot && c.hp > 0)
        .collect();
    if !enemies_here.is_empty() {
        let n = enemies_here.len();
        return format!("敵{n}");
    }
    format!("{slot}")
}

fn render_placing(
    state: &ExpeditionState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),
            Constraint::Min(4),
            Constraint::Length(3),
            Constraint::Length(3),
        ])
        .split(area);

    f.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(next_goal_line(state), goal_style())),
            Line::from(Span::styled(
                " 出現 → 道 → 門   マスをタップして配置",
                muted(),
            )),
        ])
        .block(Block::default().borders(Borders::ALL).title(" 配置 ")),
        chunks[0],
    );

    let mut cl = ClickableList::new();
    cl.push(Line::from(Span::styled(
        " 出現側 ──────────────── 門",
        muted(),
    )));
    for i in 0..PATH_LEN {
        let cell = path_cell_label(state, i);
        let label = if i + 1 == PATH_LEN {
            format!(" マス{i} [{cell}] ←門")
        } else if i == 0 {
            format!(" マス{i} [{cell}] 出現")
        } else {
            format!(" マス{i} [{cell}]")
        };
        cl.push_clickable(Line::from(label), place_slot_id(i as u8));
    }
    cl.render(
        f,
        chunks[1],
        Block::default().borders(Borders::ALL).title(" 道 "),
        &mut click_state.borrow_mut(),
        false,
        0,
    );

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
    .render(f, chunks[2], &mut click_state.borrow_mut());

    Clickable::new(
        Paragraph::new(" 撤退 ")
            .alignment(Alignment::Center)
            .style(muted())
            .block(Block::default().borders(Borders::ALL)),
        CANCEL_FORMING,
    )
    .render(f, chunks[3], &mut click_state.borrow_mut());
}

fn render_running(
    state: &ExpeditionState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let _ = click_state;
    let Some(sortie) = state.sortie.as_ref() else {
        return;
    };
    let mut lines = vec![
        Line::from(Span::styled(next_goal_line(state), goal_style())),
        Line::from(vec![
            Span::styled(
                format!(
                    " {} ",
                    ExpeditionState::stage_label(sortie.chapter, sortie.stage)
                ),
                Style::default().fg(accent()).add_modifier(Modifier::BOLD),
            ),
            Span::raw(format!(
                "波{}/{}  門{} {}",
                sortie.wave_index + 1,
                sortie.waves_total,
                hp_bar(sortie.gate_hp, sortie.gate_max_hp),
                if ExpeditionState::is_boss_stage(sortie.stage) {
                    "BOSS"
                } else {
                    ""
                }
            )),
        ]),
        Line::from(""),
    ];

    // 道の可視化
    let mut road = String::from(" ");
    for i in 0..PATH_LEN {
        road.push_str(&path_cell_label(state, i));
        if i + 1 < PATH_LEN {
            road.push('─');
        }
    }
    road.push_str("■門");
    lines.push(Line::from(Span::styled(
        road,
        Style::default().fg(Color::White),
    )));

    if !sortie.last_hit_log.is_empty() {
        lines.push(Line::from(Span::styled(
            format!(" » {}", sortie.last_hit_log),
            Style::default().fg(Color::LightCyan),
        )));
    }

    lines.push(section_title("配置"));
    for (i, slot) in sortie.path.iter().enumerate() {
        if let Some(id) = slot {
            if let Some(h) = state.hero(*id) {
                lines.push(Line::from(format!(
                    "  マス{i} {} [{}] 射程{} 攻{}",
                    h.name,
                    h.role.label(),
                    h.role.range(),
                    h.atk()
                )));
            }
        }
    }

    if !sortie.creeps.is_empty() {
        lines.push(section_title("敵"));
        for c in sortie.creeps.iter().take(4) {
            lines.push(Line::from(format!(
                "  {} @{} {}/{}",
                c.name, c.pos, c.hp, c.max_hp
            )));
        }
    }

    f.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(accent()))
                .title(" 防衛中 "),
        ),
        area,
    );
}

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

    let mut lines = vec![
        Line::from(Span::styled(next_goal_line(state), goal_style())),
        Line::from(""),
    ];
    for line in state.result_summary.lines() {
        lines.push(Line::from(Span::styled(
            format!(" {line}"),
            Style::default().fg(Color::White),
        )));
    }
    lines.push(Line::from(""));
    lines.push(section_title("団員 Lv"));
    for h in &state.roster {
        lines.push(Line::from(format!(
            "  {} [{}] Lv{}",
            h.name,
            h.role.label(),
            h.level
        )));
    }
    f.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" 結果 "),
        ),
        chunks[0],
    );

    let cta = if state.last_failed {
        " 遊技場へ "
    } else {
        " 拠点へ "
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
        let backend = TestBackend::new(60, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                render(state, f, f.area(), &Rc::new(RefCell::new(ClickState::new())));
            })
            .unwrap();
        buffer_text(terminal.backend().buffer())
    }

    #[test]
    fn camp_shows_stage_cta() {
        let state = ExpeditionState::new();
        let text = draw_text(&state);
        assert!(text.contains("1-1"));
        assert!(text.contains("に出る") || text.contains("出る"));
    }

    #[test]
    fn arcade_shows_sugoroku() {
        let mut state = ExpeditionState::new();
        state.hub_tab = HubTab::Arcade;
        let text = draw_text(&state);
        assert!(text.contains("すごろく") || text.contains("遊技場"));
        assert!(text.contains("サイコロ") || text.contains("メダル"));
    }

    #[test]
    fn bottom_nav_labels() {
        let mut state = ExpeditionState::new();
        state.hub_tab = HubTab::Arcade;
        let text = draw_text(&state);
        assert!(text.contains("拠点"));
        assert!(text.contains("遊技場"));
        state.hub_tab = HubTab::Camp;
        let text = draw_text(&state);
        assert!(text.contains("拠点"));
    }

    #[test]
    #[ignore]
    fn dump_camp_arcade_placing() {
        let mut state = ExpeditionState::new();
        eprintln!("=== CAMP ===\n{}", draw_text(&state));
        state.hub_tab = HubTab::Arcade;
        eprintln!("=== ARCADE ===\n{}", draw_text(&state));
        assert!(super::super::logic::launch_sortie(&mut state));
        eprintln!("=== PLACING ===\n{}", draw_text(&state));
    }
}
