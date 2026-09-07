//! 遠征団の描画。
//!
//! 説明文は出さず、マップ・道・プッシャー場・バー／記号で状況を伝える。

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
    drop_lane_id, level_hero_id, place_slot_id, toggle_hero_id, ACK_RESULT, CANCEL_FORMING,
    CONFIRM_PLACEMENT, LAUNCH, OPEN_FORMING, START_FORMING, TAB_ARCADE, TAB_CAMP,
};
use super::state::{
    ExpeditionState, Hero, HubTab, Role, Screen, ORB_NEED, PARTY_SIZE, PATH_LEN, PUSH_D, PUSH_W,
};

fn accent() -> Color {
    theme::accent(&GameChoice::Expedition)
}

fn muted() -> Style {
    Style::default().fg(Color::DarkGray)
}

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
            Constraint::Length(2),
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

fn pip_bar(filled: u32, cap: u32, on: char, off: char) -> String {
    let mut s = String::new();
    for i in 0..cap {
        s.push(if i < filled { on } else { off });
    }
    s
}

fn render_status_bar(state: &ExpeditionState, f: &mut Frame, area: Rect) {
    let ration_pips = pip_bar(state.rations, state.ration_cap(), '◆', '◇');
    let orb_pips = pip_bar(state.orb_gauge.min(ORB_NEED), ORB_NEED, '●', '○');
    let phase = match state.screen {
        Screen::Camp => "·",
        Screen::Forming => "◇",
        Screen::Placing => "▣",
        Screen::Running => "⚔",
        Screen::Result => "✓",
    };
    let line = Line::from(vec![
        Span::styled(
            format!(" {} ", state.current_stage_label()),
            Style::default()
                .fg(accent())
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!("{phase} "), Style::default().fg(Color::White)),
        Span::styled(
            format!("{ration_pips} "),
            Style::default().fg(Color::LightGreen),
        ),
        Span::styled(
            format!("◎{} ", state.medals),
            Style::default().fg(Color::LightYellow),
        ),
        Span::styled(orb_pips, Style::default().fg(Color::Cyan)),
    ]);
    f.render_widget(Paragraph::new(line), area);
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
    let empty = 8 - filled;
    format!("{}{}", "█".repeat(filled), "░".repeat(empty))
}

fn ration_fill_bar(state: &ExpeditionState) -> String {
    if state.rations >= state.ration_cap() {
        return String::new();
    }
    let need = state.ration_regen_ticks().max(1);
    let p = ((state.ration_progress * 4) / need).min(3);
    format!("{}{}", "▓".repeat(p as usize), "░".repeat(3 - p as usize))
}

fn cell_glyph(cell: super::state::PushCell) -> char {
    if cell.has_orb {
        '◉'
    } else {
        match cell.medals {
            0 => '·',
            1 => '○',
            2 => '◎',
            3 => '◍',
            4 => '◉',
            _ => '█',
        }
    }
}

/// 山の高さを縦棒で（端レーンの危険度）。
fn stack_bar(n: u8) -> String {
    match n {
        0 => "·".into(),
        1 => "▂".into(),
        2 => "▃".into(),
        3 => "▅".into(),
        4 => "▆".into(),
        _ => "█".into(),
    }
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
            Constraint::Length(8),
            Constraint::Min(4),
            Constraint::Length(3),
        ])
        .split(area);

    let mut plate = vec!['─'; PUSH_W];
    if state.pusher.plate_col < PUSH_W {
        plate[state.pusher.plate_col] = '▓';
    }
    let plate_line: String = plate.into_iter().collect();

    let mut field_lines = vec![Line::from(Span::styled(
        format!("  {plate_line}"),
        Style::default().fg(Color::DarkGray),
    ))];
    for row in 0..PUSH_D {
        let mut spans = vec![Span::raw("  ")];
        for col in 0..PUSH_W {
            let g = cell_glyph(state.pusher.cells[row][col]);
            let style = if row + 1 == PUSH_D {
                Style::default()
                    .fg(Color::LightYellow)
                    .add_modifier(Modifier::BOLD)
            } else if state.pusher.cells[row][col].has_orb {
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            spans.push(Span::styled(format!("{g} "), style));
        }
        if row + 1 == PUSH_D {
            spans.push(Span::styled("▽", Style::default().fg(Color::LightRed)));
        }
        field_lines.push(Line::from(spans));
    }
    // 端の山高さ
    let mut edge = String::from("  ");
    for col in 0..PUSH_W {
        edge.push_str(&stack_bar(state.pusher.cells[PUSH_D - 1][col].medals));
        edge.push(' ');
    }
    field_lines.push(Line::from(Span::styled(edge, Style::default().fg(Color::Yellow))));

    if state.pusher.last_drop_medals > 0 || state.pusher.last_drop_orb {
        let flash = if state.pusher.last_drop_orb {
            format!("  ✦ +{}", state.pusher.last_drop_medals)
        } else {
            format!("  ↓ +{}", state.pusher.last_drop_medals)
        };
        field_lines.push(Line::from(Span::styled(
            flash,
            Style::default().fg(Color::LightCyan),
        )));
    }

    let orb = pip_bar(state.orb_gauge.min(ORB_NEED), ORB_NEED, '●', '○');
    f.render_widget(
        Paragraph::new(field_lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(format!(" {orb} ")),
        ),
        chunks[0],
    );

    if state.pending_level_pick {
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
            chunks[1],
            Block::default().borders(Borders::ALL),
            &mut click_state.borrow_mut(),
            false,
            0,
        );
        f.render_widget(Paragraph::new("").block(Block::default().borders(Borders::ALL)), chunks[2]);
    } else {
        // レーンは ▼ だけの横一列風リスト
        let mut cl = ClickableList::new();
        let mut row = String::from(" ");
        for lane in 0..PUSH_W {
            let hot = state.pusher.cells[PUSH_D - 1][lane].medals >= 3;
            row.push_str(if hot { "▼!" } else { "▼ " });
            let _ = lane;
        }
        cl.push(Line::from(Span::styled(
            row,
            Style::default().fg(accent()),
        )));
        for lane in 0..PUSH_W {
            let edge_n = state.pusher.cells[PUSH_D - 1][lane].medals;
            let label = format!(
                "  [{}] {}{}",
                lane,
                stack_bar(edge_n),
                if state.medals == 0 { " ·" } else { "" }
            );
            cl.push_clickable(Line::from(label), drop_lane_id(lane as u8));
        }
        cl.render(
            f,
            chunks[1],
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" ◎{} ", state.medals)),
            &mut click_state.borrow_mut(),
            false,
            0,
        );
        f.render_widget(
            Paragraph::new("")
                .block(Block::default().borders(Borders::ALL)),
            chunks[2],
        );
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
            Constraint::Min(4),
            Constraint::Length(3),
        ])
        .split(area);

    // 章マップ: ●─●─▶◉─○ 形式
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
            (
                "●",
                Style::default().fg(Color::LightGreen),
            )
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
    let dest = Paragraph::new(vec![
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
    );
    f.render_widget(dest, chunks[0]);

    // パーティを横一列のチップで
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
    let party = Paragraph::new(Line::from(Span::styled(
        chips,
        Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
    )))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray)),
    );
    Clickable::new(party, OPEN_FORMING).render(f, chunks[1], &mut click_state.borrow_mut());

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
    // 選択状況を ◆◆◆ で
    let sel = state.forming_count();
    cl.push(Line::from(Span::styled(
        format!("  {}{}", "◆".repeat(sel), "◇".repeat(PARTY_SIZE.saturating_sub(sel))),
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

    let ready = state.forming_count() == PARTY_SIZE;
    if ready {
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

fn path_glyph(state: &ExpeditionState, slot: usize) -> (String, Style) {
    let Some(sortie) = state.sortie.as_ref() else {
        return ("·".into(), muted());
    };
    let enemies = sortie
        .creeps
        .iter()
        .filter(|c| c.pos == slot && c.hp > 0)
        .count();
    if let Some(id) = sortie.path[slot] {
        if let Some(h) = state.hero(id) {
            let g = format!("{}{}", role_glyph(h.role), h.name.chars().next().unwrap_or('?'));
            let style = if enemies > 0 {
                Style::default()
                    .fg(Color::LightRed)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(accent()).add_modifier(Modifier::BOLD)
            };
            return (g, style);
        }
    }
    if enemies > 0 {
        let g = match enemies {
            1 => "※",
            2 => "※※",
            _ => "※3",
        };
        return (
            g.into(),
            Style::default()
                .fg(Color::LightRed)
                .add_modifier(Modifier::BOLD),
        );
    }
    ("·".into(), muted())
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
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(3),
            Constraint::Length(3),
        ])
        .split(area);

    // 概略ロード
    let mut overview = String::from(" ※ ");
    for i in 0..PATH_LEN {
        let (g, _) = path_glyph(state, i);
        overview.push_str(&g);
        if i + 1 < PATH_LEN {
            overview.push('─');
        }
    }
    overview.push_str(" ■");
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            overview,
            Style::default().fg(Color::White),
        )))
        .block(Block::default().borders(Borders::ALL)),
        chunks[0],
    );

    let mut cl = ClickableList::new();
    for i in 0..PATH_LEN {
        let (g, style) = path_glyph(state, i);
        let edge = if i == 0 {
            "◁"
        } else if i + 1 == PATH_LEN {
            "■"
        } else {
            " "
        };
        cl.push_clickable(
            Line::from(vec![
                Span::raw(format!("  {edge}[")),
                Span::styled(g, style),
                Span::raw("]"),
            ]),
            place_slot_id(i as u8),
        );
    }
    cl.render(
        f,
        chunks[1],
        Block::default().borders(Borders::ALL),
        &mut click_state.borrow_mut(),
        false,
        0,
    );

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
    .render(f, chunks[2], &mut click_state.borrow_mut());

    Clickable::new(
        Paragraph::new(" ← ")
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
    _click_state: &Rc<RefCell<ClickState>>,
) {
    let Some(sortie) = state.sortie.as_ref() else {
        return;
    };

    // ウェーブを点で
    let wave = pip_bar(
        sortie.wave_index + 1,
        sortie.waves_total,
        '◆',
        '◇',
    );

    let mut lines = vec![
        Line::from(vec![
            Span::styled(
                format!(" {} ", ExpeditionState::stage_label(sortie.chapter, sortie.stage)),
                Style::default().fg(accent()).add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!("{wave} "), Style::default().fg(Color::Gray)),
            Span::styled(
                format!("■{}", hp_bar(sortie.gate_hp, sortie.gate_max_hp)),
                Style::default().fg(Color::LightGreen),
            ),
        ]),
        Line::from(""),
    ];

    // 2行の道: 上=敵、下=配置
    let mut enemy_row = String::from(" ");
    let mut hero_row = String::from(" ");
    for i in 0..PATH_LEN {
        let enemies = sortie
            .creeps
            .iter()
            .filter(|c| c.pos == i && c.hp > 0)
            .count();
        enemy_row.push_str(match enemies {
            0 => " · ",
            1 => " ※ ",
            2 => "※※ ",
            _ => "※※※",
        });
        if let Some(id) = sortie.path[i] {
            if let Some(h) = state.hero(id) {
                hero_row.push(role_glyph(h.role));
                hero_row.push(h.name.chars().next().unwrap_or('?'));
                hero_row.push(' ');
            }
        } else {
            hero_row.push_str(" · ");
        }
    }
    enemy_row.push(' ');
    hero_row.push_str("■");
    lines.push(Line::from(Span::styled(
        enemy_row,
        Style::default().fg(Color::LightRed),
    )));
    lines.push(Line::from(Span::styled(
        hero_row,
        Style::default().fg(accent()).add_modifier(Modifier::BOLD),
    )));

    if !sortie.last_hit_log.is_empty() {
        // ログも短く記号化: 撃破っぽければ ✦
        let flash = if sortie.last_hit_log.contains("撃破") {
            " ✦"
        } else if sortie.last_hit_log.contains("門") {
            " ■!"
        } else {
            " ›"
        };
        lines.push(Line::from(Span::styled(
            flash,
            Style::default().fg(Color::LightCyan),
        )));
    }

    // 敵HPをミニバーで（名前なし）
    for c in sortie.creeps.iter().take(3) {
        let frac = if c.max_hp > 0 {
            (c.hp.max(0) * 4) / c.max_hp
        } else {
            0
        };
        let bar = format!(
            "  ※{} {}",
            c.pos,
            "▮".repeat(frac as usize) + &"▯".repeat(4usize.saturating_sub(frac as usize))
        );
        lines.push(Line::from(Span::styled(
            bar,
            Style::default().fg(Color::LightRed),
        )));
    }

    f.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(accent())),
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

    let big = if state.last_failed { " ✗ " } else { " ✓ " };
    let big_color = if state.last_failed {
        Color::LightRed
    } else {
        Color::LightGreen
    };

    let mut lines = vec![
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
    ];
    // 結果サマリから数字だけ拾うのは難しいので、メダル表記は HUD に任せる
    // 団員チップ
    let mut chips = String::from("  ");
    for h in &state.roster {
        chips.push_str(&hero_chip(h));
        chips.push(' ');
    }
    lines.push(Line::from(Span::styled(
        chips,
        Style::default().fg(Color::LightYellow),
    )));

    f.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL)),
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
        let backend = TestBackend::new(60, 24);
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
    fn arcade_shows_pusher_field() {
        let mut state = ExpeditionState::new();
        state.hub_tab = HubTab::Arcade;
        let text = draw_text(&state);
        assert!(text.contains('▼') || text.contains('[') || text.contains('◎'));
        assert!(text.contains('·') || text.contains('○') || text.contains('◉'));
    }

    #[test]
    fn bottom_nav_uses_icons() {
        let mut state = ExpeditionState::new();
        state.hub_tab = HubTab::Arcade;
        let text = draw_text(&state);
        assert!(text.contains('⌂') || text.contains('◎'));
        state.hub_tab = HubTab::Camp;
        let text = draw_text(&state);
        assert!(text.contains('⌂'));
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
        assert!(super::super::logic::confirm_placement(&mut state));
        for _ in 0..30 {
            super::super::logic::tick(&mut state, 1);
        }
        eprintln!("=== RUNNING ===\n{}", draw_text(&state));
    }
}
