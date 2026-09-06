//! 遠征団の描画。
//!
//! 常設シェル（上部 HUD + 下部タブバー）の上に拠点本文や遠征シートを載せる。
//! ループは説明文ではなく、行き先・メンバー・主ボタンのビジュアルで伝える。

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
use crate::widgets::{Clickable, TabBar};

use super::actions::{
    upgrade_hero_id, toggle_hero_id, ACK_RESULT, CANCEL_FORMING, LAUNCH, LAUNCH_WITH_SCOUT,
    OPEN_FORMING, START_FORMING, TAB_CAMP, TAB_TRAIN, USE_AID,
};
use super::state::{ExpeditionState, HubTab, Screen, PARTY_SIZE};

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
        Screen::Camp if state.rations == 0 => "次: 行軍糧が貯まるのを待つ".into(),
        Screen::Camp if state.last_failed => format!("次: 育成で強化してから {} 再挑戦", state.current_stage_label()),
        Screen::Camp => format!("次: {} を攻略する", state.current_stage_label()),
        Screen::Forming => "次: 3人を選んで出発する".into(),
        Screen::Running => format!("探索中 {} …", state.sortie.as_ref().map(|s| ExpeditionState::stage_label(s.chapter, s.stage)).unwrap_or_default()),
        Screen::Result if state.last_failed => "次: 育成でレベルを上げる".into(),
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
    // ネイティブアプリの safe-area: 上 HUD / 下タブは常に確保し、中央だけがコンテンツ。
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
            HubTab::Train => render_train(state, f, chunks[1], click_state),
        },
        Screen::Forming => render_forming(state, f, chunks[1], click_state),
        Screen::Running => render_running(state, f, chunks[1], click_state),
        Screen::Result => render_result(state, f, chunks[1], click_state),
    }
    render_bottom_nav(state, f, chunks[2], click_state);
}

fn render_status_bar(state: &ExpeditionState, f: &mut Frame, area: Rect) {
    let fill = ration_fill_bar(state);
    let phase = match state.screen {
        Screen::Camp => Span::styled(" 待機", label_style()),
        Screen::Forming => Span::styled(" 編成中", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Screen::Running => Span::styled(" 遠征中", Style::default().fg(accent()).add_modifier(Modifier::BOLD)),
        Screen::Result => Span::styled(" 帰還", Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD)),
    };

    // リソースを「チップ」として並べ、タイトルは HUD 名だけにする（ネイティブの top bar）。
    let line1 = Line::from(vec![
        Span::styled(" 行軍糧 ", Style::default().fg(accent()).add_modifier(Modifier::BOLD)),
        Span::styled(
            format!("{}/{}", state.rations, state.ration_cap()),
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
        ),
        Span::raw(format!(" {fill}  ")),
        Span::styled("下調べメモ ", label_style()),
        Span::styled(
            format!("{}", state.scout_memos),
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled("Lv", label_style()),
        Span::styled(
            format!("{}", state.total_level()),
            Style::default().fg(Color::LightYellow).add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(format!("補給{} ", state.supplies), Style::default().fg(Color::LightYellow)),
        Span::styled(state.current_stage_label(), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        phase,
    ]);

    let para = Paragraph::new(line1).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title("ステータス"),
    );
    f.render_widget(para, area);
}

fn tab_style(active: bool) -> Style {
    if active {
        Style::default()
            .fg(Color::Black)
            .bg(accent())
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    }
}

fn render_bottom_nav(
    state: &ExpeditionState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let block = Block::default()
        .borders(Borders::TOP | Borders::BOTTOM)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(if on_sortie(state) {
            " ナビ（遠征中も切替可・反映は帰還後） "
        } else {
            " ナビ "
        });
    let inner = block.inner(area);
    f.render_widget(block, area);

    // 選択中は「●」でテキスト dump でも分かるようにする（色だけだとネイティブ感が出ない）。
    let camp_label = if state.hub_tab == HubTab::Camp {
        "● 拠点"
    } else {
        "拠点"
    };
    let roster_label = if state.hub_tab == HubTab::Train {
        "● 育成"
    } else {
        "育成"
    };

    let mut cs = click_state.borrow_mut();
    TabBar::new("  ")
        .tab(
            camp_label,
            tab_style(state.hub_tab == HubTab::Camp),
            TAB_CAMP,
        )
        .tab(
            roster_label,
            tab_style(state.hub_tab == HubTab::Train),
            TAB_TRAIN,
        )
        .render(f, inner, &mut cs);
}

fn section_title(text: &str) -> Line<'static> {
    Line::from(Span::styled(
        format!("── {text} ──"),
        Style::default().fg(Color::DarkGray),
    ))
}

fn hp_bar(hp: i32, max_hp: i32) -> String {
    let max_hp = max_hp.max(1);
    let cells = 8usize;
    let filled = ((hp.max(0) as usize * cells) / max_hp as usize).min(cells);
    let bar: String = (0..cells)
        .map(|i| if i < filled { '█' } else { '░' })
        .collect();
    bar
}

fn render_train(
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
        ])
        .split(area);

    let hint = if state.last_failed {
        "敗退した。補給で鍛えて再挑戦"
    } else {
        "探索で補給を稼ぎ、ここで強くする"
    };
    let head = Paragraph::new(vec![
        Line::from(Span::styled(
            format!("補給 {}  —  {hint}", state.supplies),
            Style::default().fg(Color::LightYellow).add_modifier(Modifier::BOLD),
        )),
    ])
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title("育成"),
    );
    f.render_widget(head, chunks[0]);

    let row_h = 3u16;
    let n = state.roster.len().max(1) as u16;
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints(vec![Constraint::Length(row_h); state.roster.len()])
        .split(chunks[1]);
    for (idx, h) in state.roster.iter().enumerate() {
        let cost = ExpeditionState::upgrade_cost(h.level);
        let can = state.supplies >= cost;
        let info = Paragraph::new(Line::from(vec![
            Span::styled(
                format!(" [{}]{} ", h.role.label(), h.name),
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!("Lv{}  ", h.level), Style::default().fg(Color::LightYellow)),
            Span::styled(format!("力{}  ", h.atk()), muted()),
            Span::styled(hp_bar(h.hp, h.max_hp), Style::default().fg(Color::LightGreen)),
        ]));
        let btn_style = if can {
            Style::default().fg(Color::Black).bg(accent()).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let btn = Paragraph::new(Line::from(Span::styled(
            format!(" Lv↑ 補給-{cost} (キー{}) ", idx + 1),
            btn_style,
        )))
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::ALL));

        let pair = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
            .split(rows[idx]);
        f.render_widget(info.block(Block::default().borders(Borders::ALL)), pair[0]);
        Clickable::new(btn, upgrade_hero_id(h.id)).render(
            f,
            pair[1],
            &mut click_state.borrow_mut(),
        );
    }
    let _ = n;
}


fn render_camp(
    state: &ExpeditionState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    // 説明でループを説かない。行き先・メンバー・主ボタンだけで状況が読めること。
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
            Style::default().fg(accent()).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            format!(" {map_line}"),
            Style::default().fg(Color::White),
        )),
        Line::from(Span::styled(
            if state.last_failed {
                " 敗退 — 育成で強くして再挑戦"
            } else {
                " 節をクリアすると次が開く"
            },
            muted(),
        )),
    ])
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title("行き先"),
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
                            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(
                            format!("Lv{} ", h.level),
                            Style::default().fg(Color::LightYellow),
                        ),
                        Span::styled(
                            hp_bar(h.hp, h.max_hp),
                            Style::default().fg(Color::LightGreen),
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
    let cta = Paragraph::new(Line::from(Span::styled(format!(" {label} "), style)))
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(border)),
        );
    Clickable::new(cta, action).render(f, chunks[2], &mut click_state.borrow_mut());
}


fn ration_fill_bar(state: &ExpeditionState) -> String {
    if state.rations >= state.ration_cap() {
        return "[満タン]".into();
    }
    let need = state.ration_regen_ticks().max(1);
    let done = state.ration_progress.min(need);
    let cells = 8u32;
    let filled = (done * cells / need) as usize;
    let bar: String = (0..cells as usize)
        .map(|i| if i < filled { '█' } else { '░' })
        .collect();
    format!("[{bar}]")
}

fn render_forming(
    state: &ExpeditionState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let hero_rows = state.roster.len() as u16;
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),
            Constraint::Length(hero_rows.max(1)),
            Constraint::Length(3),
            Constraint::Min(2),
        ])
        .split(area);

    let header = Paragraph::new(vec![
        Line::from(Span::styled(next_goal_line(state), goal_style())),
        Line::from(vec![
            Span::raw(format!(
                " 参加 {}/{}  ",
                state.forming_count(),
                PARTY_SIZE
            )),
            Span::styled("★", Style::default().fg(Color::LightYellow)),
            Span::raw(" が行く人   "),
            Span::styled(
                "出発で行軍糧-1",
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            ),
        ]),
    ])
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(accent()))
            .title(" 編成シート "),
    );
    f.render_widget(header, chunks[0]);

    let hero_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(vec![Constraint::Length(1); state.roster.len()])
        .split(chunks[1]);
    for (i, h) in state.roster.iter().enumerate() {
        let marked = state.forming.contains(&Some(h.id));
        let mark = if marked { "★" } else { "·" };
        let style = if marked {
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        };
        let line = Paragraph::new(Span::styled(
            format!(
                " {mark} [{}]{}  Lv{}  力{}  キー{}",
                h.role.label(),
                h.name,
                h.level,
                h.atk(),
                h.id + 1
            ),
            style,
        ));
        Clickable::new(line, toggle_hero_id(h.id)).render(
            f,
            hero_chunks[i],
            &mut click_state.borrow_mut(),
        );
    }

    let row = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(42),
            Constraint::Percentage(36),
            Constraint::Percentage(22),
        ])
        .split(chunks[2]);
    let ready = state.forming_count() == PARTY_SIZE && state.rations > 0;
    let launch_style = if ready {
        Style::default()
            .fg(Color::Black)
            .bg(accent())
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    Clickable::new(
        Paragraph::new(" 出発する ")
            .alignment(Alignment::Center)
            .style(launch_style)
            .block(Block::default().borders(Borders::ALL)),
        LAUNCH,
    )
    .render(f, row[0], &mut click_state.borrow_mut());
    let scout_style = if state.scout_memos > 0 && ready {
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    Clickable::new(
        Paragraph::new(" 下調べ出発 ")
            .alignment(Alignment::Center)
            .style(scout_style)
            .block(Block::default().borders(Borders::ALL)),
        LAUNCH_WITH_SCOUT,
    )
    .render(f, row[1], &mut click_state.borrow_mut());
    Clickable::new(
        Paragraph::new(" やめる ")
            .alignment(Alignment::Center)
            .style(Style::default().fg(Color::Gray))
            .block(Block::default().borders(Borders::ALL)),
        CANCEL_FORMING,
    )
    .render(f, row[2], &mut click_state.borrow_mut());

    let help = Paragraph::new("下調べ=メモを1消費し敵の特徴が見える / 1-4選択 Space出発 Q戻る")
        .style(muted());
    f.render_widget(help, chunks[3]);
}


fn road_progress(node_index: u32, nodes_total: u32) -> String {
    let total = nodes_total.max(1) as usize;
    let done = (node_index as usize + 1).min(total);
    (0..total)
        .map(|i| if i < done { '●' } else { '○' })
        .collect()
}

fn render_running(
    state: &ExpeditionState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let Some(sortie) = state.sortie.as_ref() else {
        return;
    };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(8), Constraint::Length(3)])
        .split(area);

    let _progress = road_progress(sortie.node_index, sortie.nodes_total);
    let mut lines = vec![
        Line::from(Span::styled(next_goal_line(state), goal_style())),
        Line::from(vec![
            Span::styled(
                format!(" {} ", ExpeditionState::stage_label(sortie.chapter, sortie.stage)),
                Style::default().fg(accent()).add_modifier(Modifier::BOLD),
            ),
            Span::raw(format!(
                "難度{}  {}",
                sortie.difficulty,
                if ExpeditionState::is_boss_stage(sortie.stage) { "BOSS戦" } else { "戦闘" },
            )),
        ]),
    ];
    if let Some(hint) = sortie.scout_hint {
        lines.push(Line::from(Span::styled(
            format!(" 下調べ: {hint}"),
            Style::default().fg(Color::Cyan),
        )));
    }
    if let Some(enemy) = sortie.enemy.as_ref() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!(" 敵 {}", enemy.name),
            Style::default().fg(Color::LightRed).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(format!(
            " 体力 {}/{} {}  攻撃{}",
            enemy.hp,
            enemy.max_hp,
            hp_bar(enemy.hp, enemy.max_hp),
            enemy.atk
        )));
    } else {
        lines.push(Line::from(Span::styled(
            " 次の地点へ…",
            label_style(),
        )));
    }
    lines.push(Line::from(""));
    if !sortie.last_hit_log.is_empty() {
        lines.push(Line::from(Span::styled(
            format!(" » {}", sortie.last_hit_log),
            Style::default().fg(Color::LightCyan),
        )));
    }
    lines.push(section_title("仲間"));
    for &id in &sortie.party {
        if let Some(h) = state.hero(id) {
            lines.push(Line::from(format!(
                "  {} [{}]  {} {}/{}",
                h.name,
                h.role.label(),
                hp_bar(h.hp, h.max_hp),
                h.hp,
                h.max_hp
            )));
        }
    }
    if !state.log.is_empty() {
        lines.push(Line::from(""));
        for line in state.log.iter().rev().take(3).rev() {
            lines.push(Line::from(Span::styled(
                format!(" · {line}"),
                muted(),
            )));
        }
    }
    let para = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(accent()))
            .title(" 探索中 "),
    );
    f.render_widget(para, chunks[0]);

    if sortie.aid_ready && sortie.enemy.is_some() {
        Clickable::new(
            Paragraph::new(" ▶ 援護する ")
                .alignment(Alignment::Center)
                .style(
                    Style::default()
                        .fg(Color::Black)
                        .bg(accent())
                        .add_modifier(Modifier::BOLD),
                )
                .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(accent()))),
            USE_AID,
        )
        .render(f, chunks[1], &mut click_state.borrow_mut());
    } else {
        let msg = if sortie.aid_ready {
            " 自動進行中 "
        } else {
            " 援護済み · 自動進行中 "
        };
        f.render_widget(
            Paragraph::new(msg)
                .alignment(Alignment::Center)
                .style(muted())
                .block(Block::default().borders(Borders::ALL)),
            chunks[1],
        );
    }
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
        Line::from(Span::styled(
            state.result_summary.clone(),
            Style::default().fg(Color::White),
        )),
        Line::from(""),
        section_title("育成のレベル"),
    ];
    for h in &state.roster {
        lines.push(Line::from(format!(
            " [{}]{}  Lv{}  力{}",
            h.role.label(),
            h.name,
            h.level,
            h.atk()
        )));
    }
    let para = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::LightGreen))
            .title(" 遠征の結果 "),
    );
    f.render_widget(para, chunks[0]);
    if state.last_failed {
        let row = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(chunks[1]);
        Clickable::new(
            Paragraph::new(" 育成へ ")
                .alignment(Alignment::Center)
                .style(Style::default().fg(Color::Black).bg(Color::LightYellow).add_modifier(Modifier::BOLD))
                .block(Block::default().borders(Borders::ALL)),
            TAB_TRAIN,
        )
        .render(f, row[0], &mut click_state.borrow_mut());
        Clickable::new(
            Paragraph::new(" 拠点へ ")
                .alignment(Alignment::Center)
                .style(Style::default().fg(Color::Black).bg(accent()).add_modifier(Modifier::BOLD))
                .block(Block::default().borders(Borders::ALL)),
            ACK_RESULT,
        )
        .render(f, row[1], &mut click_state.borrow_mut());
    } else {
        Clickable::new(
            Paragraph::new(" 拠点に戻る ")
                .alignment(Alignment::Center)
                .style(Style::default().fg(Color::Black).bg(accent()).add_modifier(Modifier::BOLD))
                .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(accent()))),
            ACK_RESULT,
        )
        .render(f, chunks[1], &mut click_state.borrow_mut());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui_inspect::buffer_text;
    use ratzilla::ratatui::backend::TestBackend;
    use ratzilla::ratatui::Terminal;

    fn draw_text(state: &ExpeditionState) -> String {
        let backend = TestBackend::new(72, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let click = Rc::new(RefCell::new(ClickState::new()));
        terminal
            .draw(|f| render(state, f, f.area(), &click))
            .unwrap();
        buffer_text(terminal.backend().buffer())
    }

    #[test]
    fn shell_shows_status_bar_and_bottom_nav() {
        let state = ExpeditionState::new();
        let text = draw_text(&state);
        assert!(text.contains("ステータス"), "{text}");
        assert!(text.contains("行軍糧"), "{text}");
        assert!(text.contains("下調べメモ"), "{text}");
        assert!(text.contains("拠点"), "{text}");
        assert!(text.contains("育成"), "{text}");
        assert!(text.contains("● 拠点"), "{text}");
    }

    #[test]
    fn train_tab_lists_members_with_upgrade_buttons() {
        let mut state = ExpeditionState::new();
        state.hub_tab = HubTab::Train;
        let text = draw_text(&state);
        assert!(text.contains("育成"), "{text}");
        assert!(text.contains("Lv↑"), "{text}");
        assert!(text.contains("灰"), "{text}");
        assert!(text.contains("ステータス"), "{text}");
        assert!(text.contains("拠点"), "{text}");
        assert!(text.contains("● 育成"), "{text}");
    }

    #[test]
    fn camp_shows_destination_party_and_primary_cta() {
        let state = ExpeditionState::new();
        let text = draw_text(&state);
        assert!(text.contains("行き先") || text.contains("第1章"), "{text}");
        assert!(text.contains("1-1"), "{text}");
        assert!(text.contains("出撃メンバー"), "{text}");
        assert!(text.contains("1-1 に出る") || text.contains("に出る"), "{text}");
        assert!(text.contains("行軍糧"), "{text}");
        assert!(!text.contains("やること"), "{text}");
        assert!(!text.contains("放置では戦力は上がらない"), "{text}");
        assert!(text.contains("灰"), "{text}");
        assert!(text.contains("焔"), "{text}");
        assert!(text.contains("雫"), "{text}");
    }

    #[test]
    fn camp_without_rations_shows_recovery_on_primary() {
        let mut state = ExpeditionState::new();
        state.rations = 0;
        let text = draw_text(&state);
        assert!(text.contains("行軍糧"), "{text}");
        assert!(text.contains("回復中"), "{text}");
        assert!(!text.contains("▶ 遠征に出る"), "{text}");
    }

    #[test]
    fn forming_explains_cost_and_scout() {
        let mut state = ExpeditionState::new();
        assert!(super::super::logic::begin_forming(&mut state));
        let text = draw_text(&state);
        assert!(text.contains("行軍糧-1"), "{text}");
        assert!(text.contains("出発する"), "{text}");
        assert!(text.contains("下調べ"), "{text}");
        assert!(text.contains("ステータス"), "{text}");
        assert!(text.contains("育成"), "{text}");
        assert!(text.contains("編成シート"), "{text}");
        assert!(text.contains("編成中"), "{text}");
    }

    #[test]
    #[ignore = "手動で画面形を見るための dump"]
    fn dump_shell_screens() {
        let mut state = ExpeditionState::new();
        eprintln!("=== CAMP ===\n{}", draw_text(&state));
        state.hub_tab = HubTab::Train;
        eprintln!("=== ROSTER ===\n{}", draw_text(&state));
        state.hub_tab = HubTab::Camp;
        assert!(super::super::logic::begin_forming(&mut state));
        eprintln!("=== FORMING ===\n{}", draw_text(&state));
        assert!(super::super::logic::launch_sortie(&mut state, false));
        eprintln!("=== RUNNING ===\n{}", draw_text(&state));
    }
}
