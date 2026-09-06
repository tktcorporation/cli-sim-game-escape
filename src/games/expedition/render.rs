//! 遠征団の描画。

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
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(3),
        ])
        .split(area);

    let header = Paragraph::new(Line::from(vec![
        Span::styled(
            " 遠征団 ",
            Style::default().fg(accent()).add_modifier(Modifier::BOLD),
        ),
        Span::raw(format!(
            "行軍糧 {}/{}  メモ {}  次層 {}",
            state.rations,
            state.ration_cap(),
            state.scout_memos,
            state.best_depth
        )),
    ]))
    .block(Block::default().borders(Borders::ALL).title("拠点"));
    f.render_widget(header, chunks[0]);

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(Span::styled(
        "団員（絆は遠征クリアで伸びる）",
        Style::default().fg(Color::Gray),
    )));
    for h in &state.roster {
        lines.push(Line::from(format!(
            " [{}] {}  絆{}  ATK{}  HP{}/{}",
            h.role.label(),
            h.name,
            h.bond,
            h.atk(),
            h.hp,
            h.max_hp
        )));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "目標: 行軍糧を使って遠征し、絆を育てる",
        Style::default().fg(Color::Yellow),
    )));
    for line in state.log.iter().rev().take(4).rev() {
        lines.push(Line::from(Span::styled(
            line.clone(),
            Style::default().fg(Color::DarkGray),
        )));
    }
    let body = Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title("状況"));
    f.render_widget(body, chunks[1]);

    let btn = Paragraph::new(Line::from(Span::styled(
        " [出撃準備] ",
        Style::default()
            .fg(Color::Black)
            .bg(accent())
            .add_modifier(Modifier::BOLD),
    )));
    Clickable::new(btn, START_FORMING).render(f, chunks[2], &mut click_state.borrow_mut());
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
            Constraint::Length(3),
            Constraint::Min(6),
            Constraint::Length(3),
            Constraint::Length(3),
        ])
        .split(area);

    let header = Paragraph::new(format!(
        "編成 {}/{} — 選んで出撃",
        state.forming_count(),
        PARTY_SIZE
    ))
    .block(Block::default().borders(Borders::ALL).title("出撃準備"));
    f.render_widget(header, chunks[0]);

    let hero_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(vec![Constraint::Length(1); state.roster.len()])
        .split(chunks[1]);
    for (i, h) in state.roster.iter().enumerate() {
        let marked = state.forming.contains(&Some(h.id));
        let mark = if marked { "★" } else { "・" };
        let line = Paragraph::new(format!(
            "{mark} [{}] {}  絆{} ATK{}  (キー{})",
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
            Constraint::Percentage(34),
            Constraint::Percentage(33),
            Constraint::Percentage(33),
        ])
        .split(chunks[2]);
    Clickable::new(
        Paragraph::new(" [出撃] ").style(Style::default().fg(Color::Black).bg(accent())),
        LAUNCH,
    )
    .render(f, row[0], &mut click_state.borrow_mut());
    Clickable::new(
        Paragraph::new(" [メモ付き] ").style(Style::default().fg(Color::Cyan)),
        LAUNCH_WITH_SCOUT,
    )
    .render(f, row[1], &mut click_state.borrow_mut());
    Clickable::new(
        Paragraph::new(" [戻る] ").style(Style::default().fg(Color::Gray)),
        CANCEL_FORMING,
    )
    .render(f, row[2], &mut click_state.borrow_mut());

    let help = Paragraph::new("1-4で選択 / Space出撃 / Sメモ付き / Q戻る")
        .style(Style::default().fg(Color::DarkGray));
    f.render_widget(help, chunks[3]);
}

fn render_running(state: &ExpeditionState, f: &mut Frame, area: Rect) {
    let Some(sortie) = state.sortie.as_ref() else {
        return;
    };
    let mut lines = vec![format!(
        "遠征中  第{}層  ノード {}/{}",
        sortie.depth,
        sortie.node_index + 1,
        sortie.nodes_total
    )];
    if let Some(hint) = sortie.scout_hint {
        lines.push(format!("下調べ: {hint}"));
    }
    if let Some(enemy) = sortie.enemy.as_ref() {
        lines.push(format!(
            "敵 {}  HP {}/{}  ATK{}",
            enemy.name, enemy.hp, enemy.max_hp, enemy.atk
        ));
    }
    lines.push("パーティ:".into());
    for &id in &sortie.party {
        if let Some(h) = state.hero(id) {
            lines.push(format!(
                "  {} [{}] HP {}/{}",
                h.name,
                h.role.label(),
                h.hp,
                h.max_hp
            ));
        }
    }
    lines.push(String::new());
    for line in state.log.iter().rev().take(5).rev() {
        lines.push(line.clone());
    }
    let para = Paragraph::new(lines.join("\n"))
        .block(Block::default().borders(Borders::ALL).title("戦闘"));
    f.render_widget(para, area);
}

fn render_choice(f: &mut Frame, area: Rect, click_state: &Rc<RefCell<ClickState>>) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(5), Constraint::Length(3)])
        .split(area);
    let para = Paragraph::new(
        "分かれ道だ。\n[休む] で回復して進む。\n[突っ込む] で強い敵と引き換えに絆を増やす。",
    )
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
        .constraints([Constraint::Min(5), Constraint::Length(3)])
        .split(area);
    let para = Paragraph::new(state.result_summary.clone())
        .block(Block::default().borders(Borders::ALL).title("遠征結果"));
    f.render_widget(para, chunks[0]);
    Clickable::new(
        Paragraph::new(" [拠点へ戻る] ").style(
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

    #[test]
    fn camp_shows_goal_and_rations() {
        let state = ExpeditionState::new();
        let backend = TestBackend::new(70, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let click = Rc::new(RefCell::new(ClickState::new()));
        terminal
            .draw(|f| render(&state, f, f.area(), &click))
            .unwrap();
        let text = buffer_text(terminal.backend().buffer());
        assert!(text.contains("遠征団"), "{text}");
        assert!(text.contains("行軍糧"), "{text}");
        assert!(text.contains("目標"), "{text}");
        assert!(text.contains("出撃準備"), "{text}");
    }

    #[test]
    #[ignore = "手動で画面形を見るための dump"]
    fn dump_camp_screen() {
        let state = ExpeditionState::new();
        let backend = TestBackend::new(70, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let click = Rc::new(RefCell::new(ClickState::new()));
        terminal
            .draw(|f| render(&state, f, f.area(), &click))
            .unwrap();
        eprintln!("{}", buffer_text(terminal.backend().buffer()));
    }
}
