//! たまごっち風育成ゲームの描画。

use std::cell::RefCell;
use std::rc::Rc;

use ratzilla::ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratzilla::ratatui::style::{Color, Modifier, Style};
use ratzilla::ratatui::text::{Line, Span};
use ratzilla::ratatui::widgets::{Block, Borders, Paragraph};
use ratzilla::ratatui::Frame;

use crate::input::{is_narrow_layout, ClickState};
use crate::widgets::{Clickable, ClickableList};

use super::actions::*;
use super::state::{LastAction, Stage, TamaState};

pub fn render(
    state: &TamaState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let is_narrow = is_narrow_layout(area.width);
    let borders = if is_narrow {
        Borders::TOP | Borders::BOTTOM
    } else {
        Borders::ALL
    };

    // ヘッダー (タイトル + 世代/ベスト) / ステータスバー / ペット表示 /
    // メッセージログ / アクションボタン の 5 段。狭幅でもログを 1 行は
    // 残して何が起きてるか分かるようにしている。
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(4),
            Constraint::Min(7),
            Constraint::Length(if is_narrow { 4 } else { 5 }),
            Constraint::Length(5),
        ])
        .split(area);

    render_header(state, f, chunks[0], borders);
    render_stats(state, f, chunks[1], borders);
    render_pet_pane(state, f, chunks[2], click_state, borders);
    render_log(state, f, chunks[3], borders);
    render_actions(state, f, chunks[4], click_state, borders);
}

fn render_header(state: &TamaState, f: &mut Frame, area: Rect, borders: Borders) {
    let best_label = if state.best_age_ticks == 0 {
        "—".to_string()
    } else {
        format_age(state.best_age_ticks)
    };
    let title = Paragraph::new(Line::from(vec![
        Span::styled(
            "🥚 たまごっち  ",
            Style::default()
                .fg(Color::LightYellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("第 {} 世代 ", state.generation),
            Style::default().fg(Color::White),
        ),
        Span::styled(
            format!("ベスト寿命: {}", best_label),
            Style::default().fg(Color::DarkGray),
        ),
    ]))
    .block(
        Block::default()
            .borders(borders)
            .border_style(Style::default().fg(Color::Yellow)),
    )
    .alignment(Alignment::Center);
    f.render_widget(title, area);
}

fn render_stats(state: &TamaState, f: &mut Frame, area: Rect, borders: Borders) {
    // 4 ステータスを 2x2 で表示。bar は 10 文字 (10% ごと 1 セル)。
    let mut lines: Vec<Line> = Vec::new();
    lines.push(stat_line(
        "🍔 おなか",
        state.stats.hunger,
        Color::Yellow,
        "💛 きげん",
        state.stats.happiness,
        Color::Magenta,
    ));
    lines.push(stat_line(
        "✨ せいけつ",
        state.stats.cleanliness,
        Color::Cyan,
        "❤  HP   ",
        state.stats.health,
        hp_color(state.stats.health),
    ));

    let mut stage_spans = vec![
        Span::styled(" Stage: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            state.stage.label(),
            Style::default()
                .fg(stage_color(state.stage))
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("   年齢: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format_age(state.age_ticks),
            Style::default().fg(Color::White),
        ),
    ];
    if state.sleeping {
        stage_spans.push(Span::styled(
            "   💤 睡眠中",
            Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD),
        ));
    }
    lines.push(Line::from(stage_spans));

    let para = Paragraph::new(lines).block(
        Block::default()
            .borders(borders)
            .border_style(Style::default().fg(Color::DarkGray)),
    );
    f.render_widget(para, area);
}

fn stat_line<'a>(
    label_a: &'a str,
    val_a: u8,
    color_a: Color,
    label_b: &'a str,
    val_b: u8,
    color_b: Color,
) -> Line<'a> {
    Line::from(vec![
        Span::raw(" "),
        Span::styled(label_a, Style::default().fg(color_a)),
        Span::raw(" "),
        Span::styled(stat_bar(val_a), Style::default().fg(color_a)),
        Span::styled(format!(" {:>3}", val_a), Style::default().fg(color_a)),
        Span::raw("    "),
        Span::styled(label_b, Style::default().fg(color_b)),
        Span::raw(" "),
        Span::styled(stat_bar(val_b), Style::default().fg(color_b)),
        Span::styled(format!(" {:>3}", val_b), Style::default().fg(color_b)),
    ])
}

fn stat_bar(value: u8) -> String {
    // 10 文字の bar: 0..=100 を 10 段階に。
    let filled = (value as usize / 10).min(10);
    let mut s = String::with_capacity(12);
    s.push('[');
    for _ in 0..filled {
        s.push('█');
    }
    for _ in filled..10 {
        s.push('░');
    }
    s.push(']');
    s
}

fn hp_color(hp: u8) -> Color {
    match hp {
        0..=25 => Color::Red,
        26..=60 => Color::Yellow,
        _ => Color::Green,
    }
}

fn stage_color(stage: Stage) -> Color {
    match stage {
        Stage::Egg => Color::White,
        Stage::Baby => Color::LightYellow,
        Stage::Child => Color::LightGreen,
        Stage::Teen => Color::LightCyan,
        Stage::Adult => Color::LightBlue,
        Stage::Elder => Color::Gray,
        Stage::Dead => Color::DarkGray,
    }
}

/// 年齢 (tick) を分秒表記に整形。10 tick = 1 sec。
fn format_age(ticks: u64) -> String {
    let total_sec = ticks / 10;
    let m = total_sec / 60;
    let s = total_sec % 60;
    format!("{:>2}:{:02}", m, s)
}

fn render_pet_pane(
    state: &TamaState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
    borders: Borders,
) {
    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::LightMagenta))
        .title(" ペット ");
    let inner = block.inner(area);

    let lines = pet_art(state);
    // ペット ASCII art を中央揃えで縦中央に配置。
    let mut rendered: Vec<Line> = Vec::new();
    let pad_top = inner.height.saturating_sub(lines.len() as u16) / 2;
    for _ in 0..pad_top {
        rendered.push(Line::from(""));
    }
    rendered.extend(lines);

    // 吹き出し / メッセージ
    if let Some(bubble) = speech_bubble(state) {
        rendered.push(Line::from(""));
        rendered.push(Line::from(Span::styled(
            bubble,
            Style::default().fg(Color::White),
        )));
    }

    let para = Paragraph::new(rendered).alignment(Alignment::Center);

    // pane 全体を Clickable でラップして「ペットをタップでなでる/孵化」
    // を実現する。block と para を別々に描画する代わりに、Clickable に
    // block 付き Paragraph を渡して「描画 + クリック領域登録」を 1 回に
    // まとめる (widget primitive 規約)。
    // 生きてる時はペット領域タップで「なでる」、卵なら孵化。死亡中は領域
    // タップを reactive にしない — 直前まで連打していたタップが新世代開始を
    // 誤発火させると「えっ今のペット消えた!?」になるため、リスタートは
    // 必ず明示的なボタン (ACT_NEW_PET) を経由させる。
    let para_with_block = para.block(block);
    if state.is_dead() {
        f.render_widget(para_with_block, area);
    } else {
        let action = if state.is_egg() { ACT_HATCH } else { ACT_PET };
        Clickable::new(para_with_block, action).render(f, area, &mut click_state.borrow_mut());
    }
}

fn pet_art(state: &TamaState) -> Vec<Line<'static>> {
    let frame = (state.anim_frame / 5) % 2;
    let face = match (state.stage, state.last_action, state.sleeping) {
        (Stage::Dead, _, _) => "  ✟",
        (Stage::Egg, _, _) => {
            if frame == 0 {
                ".oOo."
            } else {
                "oOoO."
            }
        }
        (_, _, true) => "(- . -) zzz",
        (_, Some(LastAction::Fed), _) => "( ﾟ▽ﾟ)~~ paku",
        (_, Some(LastAction::Played), _) => "(^ω^)/",
        (_, Some(LastAction::Bathed), _) => "(*ﾟ▽ﾟ*)",
        (_, Some(LastAction::Medicated), _) => "(>_<;)",
        (_, Some(LastAction::Petted), _) => "(´ω`*)",
        (_, Some(LastAction::Slept), _) => "(- . -)",
        (_, Some(LastAction::Refused), _) => "(￣^￣)",
        (Stage::Baby, _, _) => {
            if frame == 0 {
                "(･ω･)"
            } else {
                "(･ω･)ﾉ"
            }
        }
        (Stage::Child, _, _) => {
            if frame == 0 {
                "(=ﾟωﾟ)"
            } else {
                "(=ﾟωﾟ)ﾉ"
            }
        }
        (Stage::Teen, _, _) => {
            if frame == 0 {
                "(￣ε￣)"
            } else {
                "(￣ε￣ )"
            }
        }
        (Stage::Adult, _, _) => {
            if frame == 0 {
                "(´∀`)"
            } else {
                " (´∀`)"
            }
        }
        (Stage::Elder, _, _) => "(´-ω-`)",
    };

    let body = match state.stage {
        Stage::Egg => "  ___",
        Stage::Dead => " R.I.P. ",
        _ => "/| |\\",
    };
    let feet = match state.stage {
        Stage::Egg | Stage::Dead => "",
        Stage::Baby | Stage::Child => "u u",
        _ => "U U",
    };

    let mut lines = vec![Line::from(face.to_string())];
    if !body.is_empty() {
        lines.push(Line::from(body.to_string()));
    }
    if !feet.is_empty() {
        lines.push(Line::from(feet.to_string()));
    }

    if state.poop_count > 0 && !state.is_egg() && !state.is_dead() {
        let poops: String = std::iter::repeat_n('💩', state.poop_count as usize).collect();
        lines.push(Line::from(Span::styled(
            poops,
            Style::default().fg(Color::Red),
        )));
    }
    lines
}

fn speech_bubble(state: &TamaState) -> Option<String> {
    if state.is_egg() {
        return Some("(タップして孵化させる)".into());
    }
    if state.is_dead() {
        return Some("[N] / 下のボタンで新しい卵を迎える".into());
    }
    if state.sleeping {
        return None;
    }
    if state.stats.health < 30 {
        return Some("⚠ 体調が悪い… 薬がほしい".into());
    }
    if state.stats.hunger < 25 {
        return Some("おなかすいた…".into());
    }
    if state.stats.cleanliness < 25 {
        return Some("くさい… お風呂入りたい".into());
    }
    if state.stats.happiness < 25 {
        return Some("つまんない…".into());
    }
    None
}

fn render_log(state: &TamaState, f: &mut Frame, area: Rect, borders: Borders) {
    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(" ログ ");
    let inner = block.inner(area);

    let visible = inner.height as usize;
    let start = state.log.len().saturating_sub(visible);
    let lines: Vec<Line> = state.log[start..]
        .iter()
        .map(|m| Line::from(Span::styled(m.clone(), Style::default().fg(Color::Gray))))
        .collect();

    let para = Paragraph::new(lines).block(block);
    f.render_widget(para, area);
}

fn render_actions(
    state: &TamaState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
    borders: Borders,
) {
    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Green))
        .title(" アクション ");

    let mut cl = ClickableList::new();
    if state.is_egg() {
        cl.push(Line::from(""));
        cl.push_clickable(
            Line::from(Span::styled(
                "  ▶ [Space] 孵化させる  ",
                Style::default()
                    .fg(Color::LightYellow)
                    .add_modifier(Modifier::BOLD),
            )),
            ACT_HATCH,
        );
    } else if state.is_dead() {
        cl.push(Line::from(""));
        cl.push_clickable(
            Line::from(Span::styled(
                "  ▶ [N] 新しい卵を迎える  ",
                Style::default()
                    .fg(Color::LightCyan)
                    .add_modifier(Modifier::BOLD),
            )),
            ACT_NEW_PET,
        );
    } else {
        // 生きてる時のメニュー。寝てる時は sleep 解除以外グレーアウト。
        let dim = state.sleeping;
        cl.push_clickable(action_line(" [F] 食事 ", "🍔", dim), ACT_FEED);
        cl.push_clickable(action_line(" [P] 遊ぶ ", "🎈", dim), ACT_PLAY);
        cl.push_clickable(action_line(" [B] お風呂 ", "🛁", dim), ACT_BATH);
        cl.push_clickable(action_line(" [M] 薬 ", "💊", dim), ACT_MEDICINE);
        cl.push_clickable(
            Line::from(vec![
                Span::raw(" "),
                Span::styled(
                    if state.sleeping {
                        "[S] 起こす 🌅"
                    } else {
                        "[S] 寝かす 💤"
                    },
                    Style::default()
                        .fg(Color::LightBlue)
                        .add_modifier(Modifier::BOLD),
                ),
            ]),
            ACT_SLEEP_TOGGLE,
        );
    }

    {
        let mut cs = click_state.borrow_mut();
        cl.render(f, area, block, &mut cs, false, 0);
    }
}

fn action_line(label: &str, icon: &str, dim: bool) -> Line<'static> {
    let style = if dim {
        Style::default().fg(Color::DarkGray)
    } else {
        Style::default().fg(Color::White)
    };
    Line::from(vec![
        Span::raw(" "),
        Span::styled(icon.to_string(), style),
        Span::styled(label.to_string(), style),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stat_bar_extremes() {
        assert_eq!(stat_bar(0), "[░░░░░░░░░░]");
        assert_eq!(stat_bar(100), "[██████████]");
        // 50 → 5 filled
        let mid = stat_bar(50);
        assert_eq!(mid.matches('█').count(), 5);
        assert_eq!(mid.matches('░').count(), 5);
    }

    #[test]
    fn format_age_basic() {
        assert_eq!(format_age(0), " 0:00");
        assert_eq!(format_age(600), " 1:00"); // 60 sec
        assert_eq!(format_age(6005), "10:00"); // 600 sec
    }

    #[test]
    fn speech_bubble_priority() {
        let mut s = TamaState::new();
        // 卵
        assert!(speech_bubble(&s).unwrap().contains("孵化"));
        // baby + 健康
        super::super::logic::hatch(&mut s);
        assert!(speech_bubble(&s).is_none());
        // 健康優先 (HP ↓ なら他を上書き)
        s.stats.health = 10;
        s.stats.hunger = 10;
        assert!(speech_bubble(&s).unwrap().contains("体調"));
    }
}
