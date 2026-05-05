//! Rendering for 神の戦場.  Reads `GfState`, draws a vertical 4-section
//! layout (status / hand / action / log) that fits both narrow phones and
//! wide desktop terminals.

use std::cell::RefCell;
use std::rc::Rc;

use ratzilla::ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratzilla::ratatui::style::{Color, Modifier, Style};
use ratzilla::ratatui::text::{Line, Span};
use ratzilla::ratatui::widgets::{Block, Borders, Paragraph};
use ratzilla::ratatui::Frame;

use crate::input::{is_narrow_layout, ClickState};
use crate::widgets::ClickableList;

use super::actions::*;
use super::state::*;

pub fn render(
    state: &GfState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    if state.phase == Phase::Intro {
        render_intro(state, f, area, click_state);
        return;
    }
    if state.phase == Phase::Victory || state.phase == Phase::Defeat {
        render_result(state, f, area, click_state);
        return;
    }

    let is_narrow = is_narrow_layout(area.width);
    let borders = if is_narrow {
        Borders::TOP | Borders::BOTTOM
    } else {
        Borders::ALL
    };

    // Vertical sections:
    //   1 status (one row per player + border)
    //   2 hand (one row per card + header + border)
    //   3 action / context-specific picker — height varies with phase so
    //     pickers that list 3 targets + cancel + header still fit
    //   4 log (rest, min 3)
    let action_h: u16 = match state.phase {
        Phase::PlayerAction => 7,         // header + 4 actions
        Phase::PlayerSelectTarget => 7,    // header + up to 3 targets + cancel
        Phase::PlayerSelectWeapons => 6,
        Phase::PlayerSelectHeal | Phase::PlayerSelectSpecial => 5,
        _ => 5,
    };
    let constraints = [
        Constraint::Length(NUM_PLAYERS as u16 + 2),
        Constraint::Length(HAND_SIZE as u16 + 3),
        Constraint::Length(action_h),
        Constraint::Min(3),
    ];
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    render_status(state, f, chunks[0], borders);
    render_hand(state, f, chunks[1], borders, click_state);
    render_action_panel(state, f, chunks[2], borders, click_state);
    render_log(state, f, chunks[3], borders);
}

// ── Intro screen ───────────────────────────────────────────────

fn render_intro(
    state: &GfState,
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

    let mut cl = ClickableList::new();
    cl.push(Line::from(""));
    cl.push(Line::from(Span::styled(
        "       ✧✦ 神の戦場 ✦✧",
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
    )));
    cl.push(Line::from(""));
    cl.push(Line::from(Span::styled(
        "  4人の戦士、最後の一人になるまで戦え。",
        Style::default().fg(Color::White),
    )));
    cl.push(Line::from(Span::styled(
        "  神々はあなたに5枚の手札を授ける。",
        Style::default().fg(Color::Gray),
    )));
    cl.push(Line::from(""));
    cl.push(Line::from(Span::styled(
        "  ◇ ルール",
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
    )));
    cl.push(Line::from(Span::styled(
        "    ・武器で攻撃、防具で防御、回復で立て直す",
        Style::default().fg(Color::Gray),
    )));
    cl.push(Line::from(Span::styled(
        "    ・同じ武器を組み合わせるとコンボ +2",
        Style::default().fg(Color::Gray),
    )));
    cl.push(Line::from(Span::styled(
        "    ・槍は防御を 2 削り、魔法は法衣・結界でしか防げない",
        Style::default().fg(Color::Gray),
    )));
    cl.push(Line::from(Span::styled(
        "    ・反射は受けたダメージを攻撃者に返す",
        Style::default().fg(Color::Gray),
    )));
    cl.push(Line::from(""));
    cl.push(Line::from(Span::styled(
        "  ◇ 対戦相手",
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
    )));
    for p in &state.players[1..] {
        cl.push(Line::from(Span::styled(
            format!("    ・{}", p.name),
            Style::default().fg(Color::White),
        )));
    }
    cl.push(Line::from(""));
    cl.push_clickable(
        Line::from(Span::styled(
            "       ▶ タップで戦闘開始",
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        )),
        ACTION_START,
    );

    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Yellow))
        .title(" 神の戦場 ");
    let mut cs = click_state.borrow_mut();
    cl.render(f, area, block, &mut cs, false, 0);
}

// ── Result screen ──────────────────────────────────────────────

fn render_result(
    state: &GfState,
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
    let (title, color, headline) = if state.phase == Phase::Victory {
        (" 勝利 ", Color::Yellow, "✦ 神々の祝福、あなたへ ✦")
    } else {
        (" 敗北 ", Color::Red, "☠ あなたは戦場に倒れた…")
    };

    let mut cl = ClickableList::new();
    cl.push(Line::from(""));
    cl.push(Line::from(Span::styled(
        format!("       {}", headline),
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    )));
    cl.push(Line::from(""));
    cl.push(Line::from(Span::styled(
        format!("       ラウンド: {}", state.round),
        Style::default().fg(Color::White),
    )));
    cl.push(Line::from(""));
    cl.push(Line::from(Span::styled(
        "  ◇ 戦況",
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
    )));
    for p in &state.players {
        let status = if p.alive { "生存" } else { "倒れた" };
        let st_color = if p.alive { Color::Green } else { Color::DarkGray };
        cl.push(Line::from(vec![
            Span::raw("    "),
            Span::styled(format!("{:8}", p.name), Style::default().fg(Color::White)),
            Span::styled(format!(" HP {:>2}/{:<2}  ", p.hp, p.max_hp), Style::default().fg(Color::Gray)),
            Span::styled(status, Style::default().fg(st_color)),
        ]));
    }
    cl.push(Line::from(""));
    cl.push_clickable(
        Line::from(Span::styled(
            "       ▶ もう一度戦う",
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        )),
        ACTION_RESTART,
    );

    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(color))
        .title(title);
    let mut cs = click_state.borrow_mut();
    cl.render(f, area, block, &mut cs, false, 0);
}

// ── Status panel: HP bars for all 4 players ────────────────────

fn render_status(state: &GfState, f: &mut Frame, area: Rect, borders: Borders) {
    let mut lines: Vec<Line> = Vec::new();
    for (i, p) in state.players.iter().enumerate() {
        let hp_color = match p.hp {
            h if h <= 0 => Color::DarkGray,
            h if h * 3 <= p.max_hp => Color::Red,
            h if h * 3 <= p.max_hp * 2 => Color::Yellow,
            _ => Color::Green,
        };
        let bar = hp_bar(p.hp, p.max_hp, 12);
        let marker = if i == state.turn && p.alive {
            Span::styled(" ◀ 行動中", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
        } else if !p.alive {
            Span::styled(" ☠", Style::default().fg(Color::DarkGray))
        } else {
            Span::raw("")
        };
        let name_style = if p.alive {
            Style::default().fg(Color::White)
        } else {
            Style::default().fg(Color::DarkGray).add_modifier(Modifier::CROSSED_OUT)
        };
        lines.push(Line::from(vec![
            Span::styled(format!(" {:8}", p.name), name_style),
            Span::raw(" "),
            Span::styled(bar, Style::default().fg(hp_color)),
            Span::styled(format!(" {:>2}/{:<2}", p.hp.max(0), p.max_hp), Style::default().fg(hp_color)),
            marker,
        ]));
    }

    let title = format!(" 戦況 ─ ラウンド {} ", state.round);
    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Cyan))
        .title(title);
    f.render_widget(Paragraph::new(lines).block(block), area);
}

fn hp_bar(hp: i32, max: i32, width: usize) -> String {
    if max <= 0 { return String::new(); }
    let filled = ((hp.max(0) as f32 / max as f32) * width as f32).round() as usize;
    let filled = filled.min(width);
    let mut s = String::with_capacity(width + 2);
    s.push('[');
    for i in 0..width {
        s.push(if i < filled { '█' } else { '░' });
    }
    s.push(']');
    s
}

// ── Hand panel ─────────────────────────────────────────────────

fn render_hand(
    state: &GfState,
    f: &mut Frame,
    area: Rect,
    borders: Borders,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let h = state.human_idx();
    let hand = &state.players[h].hand;

    let mut cl = ClickableList::new();
    cl.push(Line::from(Span::styled(
        " 手札 (タップで選択)",
        Style::default().fg(Color::Gray),
    )));

    for (i, c) in hand.iter().enumerate() {
        let d = c.def();
        let (color, kind_label) = match d.kind {
            CardKind::Weapon => (Color::Red, "武器"),
            CardKind::Armor => (Color::Blue, "防具"),
            CardKind::Heal => (Color::Green, "回復"),
            CardKind::Special => (Color::Yellow, "特殊"),
        };
        let stat = card_stat_text(*c);
        let key = card_key_label(i);
        let selected = state.selected_weapons.contains(&i);

        let clickable = is_card_clickable(state, *c);

        let prefix = if selected {
            Span::styled(" ✓ ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
        } else if clickable {
            Span::styled(" · ", Style::default().fg(Color::Gray))
        } else {
            Span::raw("   ")
        };

        let line = Line::from(vec![
            prefix,
            Span::styled(format!(" {} ", key), Style::default().fg(if clickable { Color::Yellow } else { Color::DarkGray })),
            Span::styled(format!("{:4}", kind_label), Style::default().fg(color)),
            Span::styled(format!(" {:8}", d.name), Style::default().fg(if clickable { Color::White } else { Color::DarkGray })),
            Span::styled(stat, Style::default().fg(Color::Gray)),
        ]);

        if clickable {
            cl.push_clickable(line, HAND_BASE + i as u16);
        } else {
            cl.push(line);
        }
    }

    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Magenta))
        .title(format!(" 手札 ({}枚) ", hand.len()));
    let mut cs = click_state.borrow_mut();
    cl.render(f, area, block, &mut cs, false, 0);
}

fn card_stat_text(c: Card) -> String {
    let d = c.def();
    match d.kind {
        CardKind::Weapon => {
            let mut s = if d.hits > 1 {
                format!("{}ダメ ×{}", d.power, d.hits)
            } else {
                format!("{}ダメ", d.power)
            };
            if d.pierce { s.push_str(" 貫通"); }
            if d.magic { s.push_str(" 魔法"); }
            s
        }
        CardKind::Armor => {
            let mut s = format!("{}防御", d.power);
            if d.blocks_magic { s.push_str(" 魔法可"); }
            s
        }
        CardKind::Heal => format!("HP+{}", d.power),
        CardKind::Special => match c {
            Card::Pray => "HP+3 ・引く".into(),
            Card::Reflect => "防御時に半減反射".into(),
            Card::Steal => "1枚奪う".into(),
            Card::Trial => "全員に5ダメ".into(),
            _ => String::new(),
        },
    }
}

fn card_key_label(i: usize) -> char {
    match i {
        0 => '1', 1 => '2', 2 => '3', 3 => '4', 4 => '5',
        5 => '6', 6 => '7',
        _ => '?',
    }
}

/// Whether the given card is interactable in the current phase.
fn is_card_clickable(state: &GfState, c: Card) -> bool {
    match state.phase {
        Phase::PlayerSelectWeapons => c.kind() == CardKind::Weapon,
        Phase::PlayerSelectHeal => c.kind() == CardKind::Heal,
        Phase::PlayerSelectSpecial => c.kind() == CardKind::Special && c != Card::Reflect,
        _ => false,
    }
}

// ── Action panel: depends on phase ─────────────────────────────

fn render_action_panel(
    state: &GfState,
    f: &mut Frame,
    area: Rect,
    borders: Borders,
    click_state: &Rc<RefCell<ClickState>>,
) {
    match state.phase {
        Phase::PlayerAction => render_main_actions(state, f, area, borders, click_state),
        Phase::PlayerSelectWeapons => render_weapon_picker(state, f, area, borders, click_state),
        Phase::PlayerSelectTarget => render_target_picker(state, f, area, borders, click_state),
        Phase::PlayerSelectHeal => render_heal_picker(f, area, borders, click_state),
        Phase::PlayerSelectSpecial => render_special_picker(f, area, borders, click_state),
        Phase::CpuTurn { idx, .. } => render_cpu_thinking(state, f, area, borders, idx),
        Phase::BetweenTurns { .. } => render_pause(f, area, borders),
        _ => {}
    }
}

fn render_main_actions(
    state: &GfState,
    f: &mut Frame,
    area: Rect,
    borders: Borders,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let h = state.human_idx();
    let hand = &state.players[h].hand;
    let has_weapon = hand.iter().any(|c| c.kind() == CardKind::Weapon);
    let has_heal = hand.iter().any(|c| c.kind() == CardKind::Heal);
    let has_special = hand.iter().any(|c| c.kind() == CardKind::Special && *c != Card::Reflect);

    let mut cl = ClickableList::new();
    cl.push(Line::from(Span::styled(
        " あなたの番 ─ 行動を選んでください",
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
    )));

    push_action(&mut cl, "攻撃", "武器を相手にぶつける", Color::Red, has_weapon, ACTION_ATTACK);
    push_action(&mut cl, "回復", "HPを取り戻す", Color::Green, has_heal, ACTION_HEAL);
    push_action(&mut cl, "特殊", "祈り・略奪・神の試練など", Color::Yellow, has_special, ACTION_SPECIAL);
    push_action(&mut cl, "パス", "何もせず次の番へ", Color::Gray, true, ACTION_PASS);

    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Yellow))
        .title(" 行動 ");
    let mut cs = click_state.borrow_mut();
    cl.render(f, area, block, &mut cs, false, 0);
}

/// One row in the main-action panel: prefix bullet, label, hint.
/// Disabled (no matching card) actions render dimmed and unregistered.
fn push_action(
    cl: &mut ClickableList<'static>,
    label: &'static str,
    hint: &'static str,
    color: Color,
    enabled: bool,
    action_id: u16,
) {
    if enabled {
        cl.push_clickable(
            Line::from(vec![
                Span::styled(" ▶ ", Style::default().fg(color)),
                Span::styled(label, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                Span::styled(format!(" — {}", hint), Style::default().fg(Color::DarkGray)),
            ]),
            action_id,
        );
    } else {
        cl.push(Line::from(vec![
            Span::styled(" · ", Style::default().fg(Color::DarkGray)),
            Span::styled(label, Style::default().fg(Color::DarkGray)),
            Span::styled(format!(" — {} (使えない)", hint), Style::default().fg(Color::DarkGray)),
        ]));
    }
}

fn render_weapon_picker(
    state: &GfState,
    f: &mut Frame,
    area: Rect,
    borders: Borders,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let weapons: Vec<Card> = state.selected_weapons.iter()
        .filter_map(|&i| state.players[state.human_idx()].hand.get(i).copied())
        .collect();
    let (dmg, pierce, magic) = super::logic::weapon_attack_stats(&weapons);

    let mut cl = ClickableList::new();
    cl.push(Line::from(Span::styled(
        " 武器を選んでください (複数選べばコンボ)",
        Style::default().fg(Color::Yellow),
    )));
    let summary = if weapons.is_empty() {
        Line::from(Span::styled(" 未選択", Style::default().fg(Color::DarkGray)))
    } else {
        Line::from(vec![
            Span::raw(" 合計: "),
            Span::styled(format!("{}ダメ", dmg), Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
            Span::styled(if pierce { " 貫通" } else { "" }, Style::default().fg(Color::Yellow)),
            Span::styled(if magic { " 魔法" } else { "" }, Style::default().fg(Color::Magenta)),
        ])
    };
    cl.push(summary);

    if !weapons.is_empty() {
        cl.push_clickable(
            Line::from(vec![
                Span::styled(" ▶ ", Style::default().fg(Color::Green)),
                Span::styled("攻撃する相手を選ぶ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            ]),
            ACTION_CONFIRM_WEAPONS,
        );
    }
    cl.push_clickable(
        Line::from(vec![
            Span::styled(" ◀ ", Style::default().fg(Color::Gray)),
            Span::styled("やめる", Style::default().fg(Color::Gray)),
        ]),
        ACTION_CANCEL,
    );

    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Red))
        .title(" 武器選択 ");
    let mut cs = click_state.borrow_mut();
    cl.render(f, area, block, &mut cs, false, 0);
}

fn render_target_picker(
    state: &GfState,
    f: &mut Frame,
    area: Rect,
    borders: Borders,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let mut cl = ClickableList::new();
    cl.push(Line::from(Span::styled(
        " 攻撃する相手を選んでください",
        Style::default().fg(Color::Yellow),
    )));
    for (i, p) in state.players.iter().enumerate() {
        if i == state.human_idx() || !p.alive { continue; }
        cl.push_clickable(
            Line::from(vec![
                Span::styled(" ▶ ", Style::default().fg(Color::Red)),
                Span::styled(format!("{:8}", p.name), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                Span::styled(format!(" HP {}/{}", p.hp, p.max_hp), Style::default().fg(Color::Gray)),
            ]),
            TARGET_BASE + i as u16,
        );
    }
    cl.push_clickable(
        Line::from(vec![
            Span::styled(" ◀ ", Style::default().fg(Color::Gray)),
            Span::styled("やめる", Style::default().fg(Color::Gray)),
        ]),
        ACTION_CANCEL,
    );

    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Red))
        .title(" 攻撃対象 ");
    let mut cs = click_state.borrow_mut();
    cl.render(f, area, block, &mut cs, false, 0);
}

fn render_heal_picker(
    f: &mut Frame,
    area: Rect,
    borders: Borders,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let mut cl = ClickableList::new();
    cl.push(Line::from(Span::styled(
        " 上の手札から使う回復カードをタップ",
        Style::default().fg(Color::Yellow),
    )));
    cl.push_clickable(
        Line::from(vec![
            Span::styled(" ◀ ", Style::default().fg(Color::Gray)),
            Span::styled("やめる", Style::default().fg(Color::Gray)),
        ]),
        ACTION_CANCEL,
    );

    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Green))
        .title(" 回復選択 ");
    let mut cs = click_state.borrow_mut();
    cl.render(f, area, block, &mut cs, false, 0);
}

fn render_special_picker(
    f: &mut Frame,
    area: Rect,
    borders: Borders,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let mut cl = ClickableList::new();
    cl.push(Line::from(Span::styled(
        " 上の手札から使う特殊カードをタップ",
        Style::default().fg(Color::Yellow),
    )));
    cl.push(Line::from(Span::styled(
        " (反射カードは防御時に自動発動)",
        Style::default().fg(Color::DarkGray),
    )));
    cl.push_clickable(
        Line::from(vec![
            Span::styled(" ◀ ", Style::default().fg(Color::Gray)),
            Span::styled("やめる", Style::default().fg(Color::Gray)),
        ]),
        ACTION_CANCEL,
    );

    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Yellow))
        .title(" 特殊選択 ");
    let mut cs = click_state.borrow_mut();
    cl.render(f, area, block, &mut cs, false, 0);
}

fn render_cpu_thinking(
    state: &GfState,
    f: &mut Frame,
    area: Rect,
    borders: Borders,
    idx: usize,
) {
    let lines = vec![
        Line::from(Span::styled(
            format!(" ✦ {} の番...", state.players[idx].name),
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            "   神々が手を読んでいる",
            Style::default().fg(Color::Gray),
        )),
    ];
    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Cyan))
        .title(" CPUの番 ")
        .title_alignment(Alignment::Left);
    f.render_widget(Paragraph::new(lines).block(block), area);
}

fn render_pause(f: &mut Frame, area: Rect, borders: Borders) {
    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(" ─ ");
    let line = Line::from(Span::styled(
        " ……",
        Style::default().fg(Color::DarkGray),
    ));
    f.render_widget(Paragraph::new(line).block(block), area);
}

// ── Log panel ──────────────────────────────────────────────────

fn render_log(state: &GfState, f: &mut Frame, area: Rect, borders: Borders) {
    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(" 戦闘ログ ");

    let inner_h = block.inner(area).height as usize;
    let lines: Vec<Line> = state.log.iter()
        .rev()
        .take(inner_h)
        .map(|e| {
            let color = match e.kind {
                LogKind::Info => Color::Gray,
                LogKind::Attack => Color::Red,
                LogKind::Defend => Color::Blue,
                LogKind::Heal => Color::Green,
                LogKind::Damage => Color::LightRed,
                LogKind::Death => Color::DarkGray,
                LogKind::Special => Color::Yellow,
            };
            Line::from(Span::styled(format!(" {}", e.line), Style::default().fg(color)))
        })
        .collect();
    // Reverse so newest is at the bottom.
    let lines: Vec<Line> = lines.into_iter().rev().collect();
    f.render_widget(Paragraph::new(lines).block(block), area);
}

// ── Tests ──────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use ratzilla::ratatui::backend::TestBackend;
    use ratzilla::ratatui::Terminal;
    use crate::input::ClickScope;

    fn make_terminal(w: u16, h: u16) -> Terminal<TestBackend> {
        Terminal::new(TestBackend::new(w, h)).unwrap()
    }

    #[test]
    fn render_intro_does_not_panic() {
        let state = GfState::new(1);
        let mut term = make_terminal(80, 30);
        let click_state = Rc::new(RefCell::new(ClickState::new()));
        click_state.borrow_mut().set_scope(ClickScope::Game(crate::games::GameChoice::Godfield));
        term.draw(|f| render(&state, f, f.area(), &click_state)).unwrap();
    }

    #[test]
    fn render_player_action_does_not_panic() {
        let mut state = GfState::new(1);
        state.phase = Phase::PlayerAction;
        let mut term = make_terminal(80, 40);
        let click_state = Rc::new(RefCell::new(ClickState::new()));
        click_state.borrow_mut().set_scope(ClickScope::Game(crate::games::GameChoice::Godfield));
        term.draw(|f| render(&state, f, f.area(), &click_state)).unwrap();
    }

    #[test]
    fn render_narrow_layout() {
        let mut state = GfState::new(1);
        state.phase = Phase::PlayerAction;
        let mut term = make_terminal(40, 40);
        let click_state = Rc::new(RefCell::new(ClickState::new()));
        click_state.borrow_mut().set_scope(ClickScope::Game(crate::games::GameChoice::Godfield));
        term.draw(|f| render(&state, f, f.area(), &click_state)).unwrap();
    }

    #[test]
    fn render_target_picker_lists_living_only() {
        let mut state = GfState::new(1);
        state.phase = Phase::PlayerSelectTarget;
        state.selected_weapons = vec![0];
        state.players[2].alive = false;
        let mut term = make_terminal(80, 40);
        let click_state = Rc::new(RefCell::new(ClickState::new()));
        click_state.borrow_mut().set_scope(ClickScope::Game(crate::games::GameChoice::Godfield));
        term.draw(|f| render(&state, f, f.area(), &click_state)).unwrap();
        // Living opponents are 1 and 3; targets registered for those IDs.
        let cs = click_state.borrow();
        assert!(cs.targets.iter().any(|t| t.action_id == TARGET_BASE + 1));
        assert!(cs.targets.iter().any(|t| t.action_id == TARGET_BASE + 3));
        // Not for dead player 2.
        assert!(!cs.targets.iter().any(|t| t.action_id == TARGET_BASE + 2));
    }

    #[test]
    fn render_victory_screen() {
        let mut state = GfState::new(1);
        state.phase = Phase::Victory;
        let mut term = make_terminal(80, 30);
        let click_state = Rc::new(RefCell::new(ClickState::new()));
        click_state.borrow_mut().set_scope(ClickScope::Game(crate::games::GameChoice::Godfield));
        term.draw(|f| render(&state, f, f.area(), &click_state)).unwrap();
        // RESTART button is registered.
        let cs = click_state.borrow();
        assert!(cs.targets.iter().any(|t| t.action_id == ACTION_RESTART));
    }

    #[test]
    fn render_defeat_screen() {
        let mut state = GfState::new(1);
        state.phase = Phase::Defeat;
        let mut term = make_terminal(80, 30);
        let click_state = Rc::new(RefCell::new(ClickState::new()));
        click_state.borrow_mut().set_scope(ClickScope::Game(crate::games::GameChoice::Godfield));
        term.draw(|f| render(&state, f, f.area(), &click_state)).unwrap();
    }

    #[test]
    fn hand_clickable_only_on_matching_phase() {
        let mut state = GfState::new(1);
        state.phase = Phase::PlayerSelectWeapons;
        // Force the human's hand to a known mix.
        state.players[0].hand = vec![Card::Sword, Card::Shield, Card::Herb, Card::Pray];
        let mut term = make_terminal(80, 40);
        let click_state = Rc::new(RefCell::new(ClickState::new()));
        click_state.borrow_mut().set_scope(ClickScope::Game(crate::games::GameChoice::Godfield));
        term.draw(|f| render(&state, f, f.area(), &click_state)).unwrap();
        let cs = click_state.borrow();
        // Sword is clickable (HAND_BASE + 0)
        assert!(cs.targets.iter().any(|t| t.action_id == HAND_BASE));
        // Shield not clickable in weapon-select phase
        assert!(!cs.targets.iter().any(|t| t.action_id == HAND_BASE + 1));
    }

    #[test]
    fn hp_bar_lengths_correct() {
        assert_eq!(hp_bar(0, 30, 12), "[░░░░░░░░░░░░]");
        assert_eq!(hp_bar(30, 30, 12), "[████████████]");
        assert_eq!(hp_bar(15, 30, 12), "[██████░░░░░░]");
        // Negative HP clamps to empty.
        assert_eq!(hp_bar(-5, 30, 12), "[░░░░░░░░░░░░]");
    }
}
