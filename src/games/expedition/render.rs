//! 遠征団の描画。
//!
//! 開口で「ループ」と「次に押すボタン」が見えることを最優先する。
//! 用語は画面上で一度は平易語に言い換える。

use std::cell::RefCell;
use std::rc::Rc;

use ratzilla::ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratzilla::ratatui::style::{Color, Modifier, Style};
use ratzilla::ratatui::text::{Line, Span};
use ratzilla::ratatui::widgets::{Block, Borders, Paragraph};
use ratzilla::ratatui::Frame;

use crate::games::GameChoice;
use crate::input::ClickState;
use crate::theme;
use crate::widgets::Clickable;

use super::actions::{
    toggle_hero_id, ACK_RESULT, CANCEL_FORMING, CHOICE_PUSH, CHOICE_REST, LAUNCH,
    LAUNCH_WITH_SCOUT, START_FORMING,
};
use super::state::{ExpeditionState, Screen, PARTY_SIZE};

fn accent() -> Color {
    theme::accent(&GameChoice::Expedition)
}

/// 画面に出す「いまの目標」一文。critique の goal 照合にも使う。
pub fn next_goal_line(state: &ExpeditionState) -> String {
    match state.screen {
        Screen::Camp if state.rations == 0 => {
            "次: 行軍糧（出撃燃料）が貯まるのを待つ".into()
        }
        Screen::Camp => "次: 遠征に出る".into(),
        Screen::Forming => "次: 3人を選んで出発する".into(),
        Screen::Running => "自動戦闘中… 道中の分かれ道まで待つ".into(),
        Screen::Choice => "次: 休むか、突っ込むか選ぶ".into(),
        Screen::Result => "次: 拠点に戻る".into(),
    }
}

pub fn render(
    state: &ExpeditionState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    match state.screen {
        Screen::Camp => render_camp(state, f, area, click_state),
        Screen::Forming => render_forming(state, f, area, click_state),
        Screen::Running => render_running(state, f, area),
        Screen::Choice => render_choice(f, area, click_state),
        Screen::Result => render_result(state, f, area, click_state),
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
            Constraint::Length(4),
            Constraint::Min(8),
            Constraint::Length(3),
        ])
        .split(area);

    let fill = ration_fill_bar(state);
    let header = Paragraph::new(vec![
        Line::from(vec![
            Span::styled(
                " 遠征団 ",
                Style::default().fg(accent()).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "糧がたまる → 遠征する → 絆が育つ",
                Style::default().fg(Color::Gray),
            ),
        ]),
        Line::from(format!(
            " 行軍糧(燃料) {}/{} {}  下調べメモ {}",
            state.rations,
            state.ration_cap(),
            fill,
            state.scout_memos
        )),
    ])
    .block(Block::default().borders(Borders::ALL).title("拠点"));
    f.render_widget(header, chunks[0]);

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(Span::styled(
        next_goal_line(state),
        Style::default()
            .fg(Color::LightGreen)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(format!(
        " 突破目標: 第{}層 （クリアすると団が強くなる）",
        state.best_depth
    )));
    lines.push(Line::from(Span::styled(
        " ※放置では戦力は上がらない。貯まるのは出撃用の糧だけ。",
        Style::default().fg(Color::DarkGray),
    )));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "やること",
        Style::default().fg(Color::Yellow),
    )));
    lines.push(Line::from(" 1. 糧を貯める（放置でもOK）"));
    lines.push(Line::from(" 2. 下のボタンで3人を選んで出発"));
    lines.push(Line::from(" 3. 自動戦闘 → 分かれ道で選ぶ → 絆ゲット"));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "団員  (絆↑で強くなり、糧の回復も少し速くなる)",
        Style::default().fg(Color::Gray),
    )));
    for h in &state.roster {
        lines.push(Line::from(format!(
            " [{}]{} 絆{}  力{}  体力{}/{}",
            h.role.label(),
            h.name,
            h.bond,
            h.atk(),
            h.hp,
            h.max_hp
        )));
    }
    if !state.log.is_empty() {
        lines.push(Line::from(Span::styled(
            "最近",
            Style::default().fg(Color::Gray),
        )));
        for line in state.log.iter().rev().take(2).rev() {
            lines.push(Line::from(Span::styled(
                format!(" · {line}"),
                Style::default().fg(Color::DarkGray),
            )));
        }
    }
    let body = Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title("いま"));
    f.render_widget(body, chunks[1]);

    let (label, style) = if state.rations == 0 {
        (
            " [糧が貯まるまで待つ] ",
            Style::default().fg(Color::DarkGray),
        )
    } else {
        (
            " [▶ 遠征に出る] ",
            Style::default()
                .fg(Color::Black)
                .bg(accent())
                .add_modifier(Modifier::BOLD),
        )
    };
    // 糧0でもタップ可能にして、logic 側のログで理由を返す（押した反応を残す）。
    Clickable::new(Paragraph::new(Line::from(Span::styled(label, style))), START_FORMING)
        .render(f, chunks[2], &mut click_state.borrow_mut());
}

fn ration_fill_bar(state: &ExpeditionState) -> String {
    if state.rations >= state.ration_cap() {
        return "[満タン]".into();
    }
    let need = state.ration_regen_ticks().max(1);
    let done = state.ration_progress.min(need);
    let cells = 6u32;
    let filled = (done * cells / need) as usize;
    let bar: String = (0..cells as usize)
        .map(|i| if i < filled { '■' } else { '□' })
        .collect();
    format!("次+1 {bar}")
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
        Line::from(Span::styled(
            next_goal_line(state),
            Style::default()
                .fg(Color::LightGreen)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(format!(
            " 参加 {}/{}  ★が行く人  出発で行軍糧-1",
            state.forming_count(),
            PARTY_SIZE
        )),
    ])
    .block(Block::default().borders(Borders::ALL).title("編成"));
    f.render_widget(header, chunks[0]);

    let hero_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(vec![Constraint::Length(1); state.roster.len()])
        .split(chunks[1]);
    for (i, h) in state.roster.iter().enumerate() {
        let marked = state.forming.contains(&Some(h.id));
        let mark = if marked { "★" } else { "・" };
        let line = Paragraph::new(format!(
            "{mark} [{}]{} 絆{} 力{}  (キー{})",
            h.role.label(),
            h.name,
            h.bond,
            h.atk(),
            h.id + 1
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
            Constraint::Percentage(40),
            Constraint::Percentage(40),
            Constraint::Percentage(20),
        ])
        .split(chunks[2]);
    let ready = state.forming_count() == PARTY_SIZE && state.rations > 0;
    let launch_style = if ready {
        Style::default().fg(Color::Black).bg(accent()).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    Clickable::new(Paragraph::new(" [出発する] ").style(launch_style), LAUNCH)
        .render(f, row[0], &mut click_state.borrow_mut());
    let scout_style = if state.scout_memos > 0 && ready {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    Clickable::new(
        Paragraph::new(" [下調べ出発] ").style(scout_style),
        LAUNCH_WITH_SCOUT,
    )
    .render(f, row[1], &mut click_state.borrow_mut());
    Clickable::new(
        Paragraph::new(" [やめる] ").style(Style::default().fg(Color::Gray)),
        CANCEL_FORMING,
    )
    .render(f, row[2], &mut click_state.borrow_mut());

    let help = Paragraph::new("下調べ=メモを1消費し敵の特徴が見える / 1-4選択 Space出発 Q戻る")
        .style(Style::default().fg(Color::DarkGray));
    f.render_widget(help, chunks[3]);
}

fn render_running(state: &ExpeditionState, f: &mut Frame, area: Rect) {
    let Some(sortie) = state.sortie.as_ref() else {
        return;
    };
    let progress = road_progress(sortie.node_index, sortie.nodes_total);
    let mut lines = vec![
        next_goal_line(state),
        format!(
            "第{}層  道のり {}  ({}/{})",
            sortie.depth,
            progress,
            sortie.node_index + 1,
            sortie.nodes_total
        ),
    ];
    if let Some(hint) = sortie.scout_hint {
        lines.push(format!("下調べ: {hint}"));
    }
    if let Some(enemy) = sortie.enemy.as_ref() {
        lines.push(format!(
            "敵 {}  体力 {}/{}  攻撃{}",
            enemy.name, enemy.hp, enemy.max_hp, enemy.atk
        ));
    } else {
        lines.push("次の地点へ進んでいます…".into());
    }
    lines.push("仲間:".into());
    for &id in &sortie.party {
        if let Some(h) = state.hero(id) {
            lines.push(format!(
                "  {} [{}] 体力 {}/{}",
                h.name,
                h.role.label(),
                h.hp,
                h.max_hp
            ));
        }
    }
    lines.push(String::new());
    for line in state.log.iter().rev().take(4).rev() {
        lines.push(format!(" · {line}"));
    }
    let para = Paragraph::new(lines.join("\n"))
        .block(Block::default().borders(Borders::ALL).title("遠征中（自動戦闘）"));
    f.render_widget(para, area);
}

fn road_progress(node_index: u32, nodes_total: u32) -> String {
    let total = nodes_total.max(1) as usize;
    let done = (node_index as usize + 1).min(total);
    (0..total)
        .map(|i| if i < done { '●' } else { '○' })
        .collect()
}

fn render_choice(f: &mut Frame, area: Rect, click_state: &Rc<RefCell<ClickState>>) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(6), Constraint::Length(3)])
        .split(area);
    let para = Paragraph::new(vec![
        Line::from(Span::styled(
            "次: 休むか、突っ込むか選ぶ",
            Style::default()
                .fg(Color::LightGreen)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from("分かれ道に来た。"),
        Line::from(Span::styled(
            " 休む … 体力を回復して次へ進む（安全）",
            Style::default().fg(Color::Green),
        )),
        Line::from(Span::styled(
            " 突っ込む … 強い敵と戦い、絆ボーナス（危険）",
            Style::default().fg(Color::Red),
        )),
    ])
    .block(Block::default().borders(Borders::ALL).title("選択"));
    f.render_widget(para, chunks[0]);

    let row = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[1]);
    Clickable::new(
        Paragraph::new(" [休む] (R) ")
            .style(Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
        CHOICE_REST,
    )
    .render(f, row[0], &mut click_state.borrow_mut());
    Clickable::new(
        Paragraph::new(" [突っ込む] (P) ")
            .style(Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
        CHOICE_PUSH,
    )
    .render(f, row[1], &mut click_state.borrow_mut());
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
        Line::from(Span::styled(
            next_goal_line(state),
            Style::default()
                .fg(Color::LightGreen)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(state.result_summary.clone()),
        Line::from(""),
        Line::from(Span::styled(
            "団員のいまの絆",
            Style::default().fg(Color::Gray),
        )),
    ];
    for h in &state.roster {
        lines.push(Line::from(format!(
            " [{}]{} 絆{}  力{}",
            h.role.label(),
            h.name,
            h.bond,
            h.atk()
        )));
    }
    let para = Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title("遠征の結果"));
    f.render_widget(para, chunks[0]);
    Clickable::new(
        Paragraph::new(" [拠点に戻る] ").style(
            Style::default()
                .fg(Color::Black)
                .bg(accent())
                .add_modifier(Modifier::BOLD),
        ),
        ACK_RESULT,
    )
    .render(f, chunks[1], &mut click_state.borrow_mut());
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
    fn camp_shows_loop_goal_and_primary_cta() {
        let state = ExpeditionState::new();
        let text = draw_text(&state);
        assert!(text.contains("遠征団"), "{text}");
        assert!(text.contains("糧がたまる"), "{text}");
        assert!(text.contains("行軍糧"), "{text}");
        assert!(text.contains("次: 遠征に出る"), "{text}");
        assert!(text.contains("遠征に出る"), "{text}");
        assert!(text.contains("やること"), "{text}");
        assert!(text.contains("突破目標"), "{text}");
        assert!(
            text.contains("放置では戦力は上がらない"),
            "{text}"
        );
    }

    #[test]
    fn camp_without_rations_tells_player_to_wait() {
        let mut state = ExpeditionState::new();
        state.rations = 0;
        let text = draw_text(&state);
        assert!(text.contains("行軍糧（出撃燃料）が貯まるのを待つ"), "{text}");
        assert!(text.contains("糧が貯まるまで待つ"), "{text}");
    }

    #[test]
    fn forming_explains_cost_and_scout() {
        let mut state = ExpeditionState::new();
        assert!(super::super::logic::begin_forming(&mut state));
        let text = draw_text(&state);
        assert!(text.contains("行軍糧-1"), "{text}");
        assert!(text.contains("出発する"), "{text}");
        assert!(text.contains("下調べ"), "{text}");
    }

    #[test]
    #[ignore = "手動で画面形を見るための dump"]
    fn dump_camp_and_forming_screens() {
        let mut state = ExpeditionState::new();
        eprintln!("=== CAMP ===\n{}", draw_text(&state));
        assert!(super::super::logic::begin_forming(&mut state));
        eprintln!("=== FORMING ===\n{}", draw_text(&state));
    }
}
