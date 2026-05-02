//! Produce mode rendering — character select, training, results.

use std::cell::RefCell;
use std::rc::Rc;

use ratzilla::ratatui::layout::Rect;
use ratzilla::ratatui::style::{Color, Modifier, Style};
use ratzilla::ratatui::text::{Line, Span};
use ratzilla::ratatui::widgets::{Block, Borders};
use ratzilla::ratatui::Frame;

use crate::input::ClickState;
use crate::widgets::ClickableList;

use super::super::actions::*;
use super::super::produce::{ProduceRank, TrainingType, PRODUCE_TURNS};
use super::super::state::CafeState;

pub(super) fn render_produce_char_select(
    state: &CafeState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let mut cl = ClickableList::new();
    cl.push(Line::from(Span::styled(
        " プロデュース — 常連客を選ぶ",
        Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD),
    )));
    cl.push(Line::from(Span::styled(
        " 5ターンの特訓で接客力/調理力/雰囲気を鍛えよう",
        Style::default().fg(Color::White),
    )));
    cl.push(Line::from(""));

    let unlocked = state.unlocked_characters();
    for (i, ch) in unlocked.iter().enumerate() {
        let data = state.character_data.get(ch);
        let level = data.map(|d| d.level).unwrap_or(1);
        let stars = data.map(|d| d.stars).unwrap_or(1);
        let star_str = "★".repeat(stars as usize);
        cl.push_clickable(
            Line::from(vec![
                Span::styled(format!(" {}. ", i + 1), Style::default().fg(Color::Yellow)),
                Span::styled(ch.name(), Style::default().fg(Color::White)),
                Span::styled(format!("  {star_str} Lv.{level}"), Style::default().fg(Color::Cyan)),
            ]),
            PRODUCE_CHAR_BASE + i as u16,
        );
    }

    cl.push(Line::from(""));
    cl.push_clickable(
        Line::from(Span::styled(" ◀ 戻る", Style::default().fg(Color::DarkGray))),
        PRODUCE_BACK,
    );

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Magenta))
        .title(" プロデュース ");
    {
        let mut cs = click_state.borrow_mut();
        cl.render(f, area, block, &mut cs, false, 0);
    }
}

pub(super) fn render_produce_training(
    state: &CafeState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let produce = match &state.produce {
        Some(p) => p,
        None => return,
    };

    let mut cl = ClickableList::new();
    cl.push(Line::from(Span::styled(
        format!(
            " プロデュース — {} ターン {}/{}",
            produce.character.short_name(),
            produce.current_turn,
            PRODUCE_TURNS
        ),
        Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD),
    )));
    cl.push(Line::from(""));

    // Current stats
    cl.push(Line::from(vec![
        Span::styled(" 接客: ", Style::default().fg(Color::Red)),
        Span::styled(format!("{}", produce.stats.service), Style::default().fg(Color::White)),
        Span::styled("  調理: ", Style::default().fg(Color::Blue)),
        Span::styled(format!("{}", produce.stats.cooking), Style::default().fg(Color::White)),
        Span::styled("  雰囲気: ", Style::default().fg(Color::Green)),
        Span::styled(format!("{}", produce.stats.atmosphere), Style::default().fg(Color::White)),
    ]));
    // HP bar
    let hp = produce.hp;
    let hp_color = if hp >= 60 { Color::Green } else if hp >= 30 { Color::Yellow } else { Color::Red };
    cl.push(Line::from(vec![
        Span::styled(format!(" 体力: {hp}/100 "), Style::default().fg(hp_color)),
        Span::styled(
            "\u{2588}".repeat((hp / 5) as usize) + &"\u{2591}".repeat(((100 - hp) / 5) as usize),
            Style::default().fg(hp_color),
        ),
    ]));
    cl.push(Line::from(""));

    // Training choices
    cl.push(Line::from(Span::styled(" 訓練を選ぶ:", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))));
    let trainings = [
        (TrainingType::Service, PRODUCE_TRAIN_SERVICE, "接客", "+20", Color::Red),
        (TrainingType::Cooking, PRODUCE_TRAIN_COOKING, "調理", "+20", Color::Blue),
        (TrainingType::Atmosphere, PRODUCE_TRAIN_ATMOSPHERE, "雰囲気", "+20", Color::Green),
        (TrainingType::Rest, PRODUCE_TRAIN_REST, "休憩", "+HP回復", Color::Cyan),
    ];
    for (tt, id, name, bonus, color) in &trainings {
        let (s, c, a) = tt.base_gains();
        cl.push_clickable(
            Line::from(vec![
                Span::styled(format!(" ▶ {name} "), Style::default().fg(*color).add_modifier(Modifier::BOLD)),
                Span::styled(format!("({bonus}) "), Style::default().fg(Color::Yellow)),
                Span::styled(
                    format!("[接客+{s} 調理+{c} 雰囲気+{a}]"),
                    Style::default().fg(Color::DarkGray),
                ),
            ]),
            *id,
        );
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Magenta))
        .title(" 訓練 ");
    {
        let mut cs = click_state.borrow_mut();
        cl.render(f, area, block, &mut cs, false, 0);
    }
}

pub(super) fn render_produce_turn_result(
    state: &CafeState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
    training: TrainingType,
) {
    let produce = match &state.produce {
        Some(p) => p,
        None => return,
    };

    let mut cl = ClickableList::new();
    cl.push(Line::from(Span::styled(
        format!(" 「{}」を実施！", training.name()),
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
    )));
    cl.push(Line::from(""));

    // Show stat changes
    cl.push(Line::from(vec![
        Span::styled(" 接客: ", Style::default().fg(Color::Red)),
        Span::styled(format!("{}", produce.stats.service), Style::default().fg(Color::White)),
        Span::styled("  調理: ", Style::default().fg(Color::Blue)),
        Span::styled(format!("{}", produce.stats.cooking), Style::default().fg(Color::White)),
        Span::styled("  雰囲気: ", Style::default().fg(Color::Green)),
        Span::styled(format!("{}", produce.stats.atmosphere), Style::default().fg(Color::White)),
    ]));
    cl.push(Line::from(""));

    // Show event if any
    if let Some(event) = &produce.current_event {
        cl.push(Line::from(Span::styled(
            format!(" イベント: {}", event.name),
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        )));
        cl.push(Line::from(Span::styled(
            format!("  {}", event.description),
            Style::default().fg(Color::White),
        )));
        let mut bonus_parts = Vec::new();
        if event.bonus_service > 0 { bonus_parts.push(format!("接客+{}", event.bonus_service)); }
        if event.bonus_cooking > 0 { bonus_parts.push(format!("調理+{}", event.bonus_cooking)); }
        if event.bonus_atmosphere > 0 { bonus_parts.push(format!("雰囲気+{}", event.bonus_atmosphere)); }
        if !bonus_parts.is_empty() {
            cl.push(Line::from(Span::styled(
                format!("  ボーナス: {}", bonus_parts.join(" ")),
                Style::default().fg(Color::Green),
            )));
        }
        cl.push(Line::from(""));
    }

    cl.push_clickable(
        Line::from(Span::styled(" ▶ 次へ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))),
        PRODUCE_CONTINUE,
    );

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow))
        .title(" ターン結果 ");
    {
        let mut cs = click_state.borrow_mut();
        cl.render(f, area, block, &mut cs, false, 0);
    }
}

pub(super) fn render_produce_result(
    state: &CafeState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let produce = match &state.produce {
        Some(p) => p,
        None => return,
    };

    let rank = produce.final_rank.unwrap_or(ProduceRank::C);
    let mut cl = ClickableList::new();

    cl.push(Line::from(""));
    cl.push(Line::from(Span::styled(
        " プロデュース完了！",
        Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD),
    )));
    cl.push(Line::from(""));

    // Final stats
    cl.push(Line::from(vec![
        Span::styled(" 接客: ", Style::default().fg(Color::Red)),
        Span::styled(format!("{}", produce.stats.service), Style::default().fg(Color::White)),
        Span::styled("  調理: ", Style::default().fg(Color::Blue)),
        Span::styled(format!("{}", produce.stats.cooking), Style::default().fg(Color::White)),
        Span::styled("  雰囲気: ", Style::default().fg(Color::Green)),
        Span::styled(format!("{}", produce.stats.atmosphere), Style::default().fg(Color::White)),
    ]));
    cl.push(Line::from(Span::styled(
        format!(" 合計: {}", produce.stats.total()),
        Style::default().fg(Color::White),
    )));
    cl.push(Line::from(""));

    // Rank display
    let rank_color = match rank {
        ProduceRank::C => Color::DarkGray,
        ProduceRank::B => Color::White,
        ProduceRank::A => Color::Green,
        ProduceRank::S => Color::Yellow,
        ProduceRank::SS => Color::Magenta,
    };
    cl.push(Line::from(Span::styled(
        format!(" 評価ランク: {}", rank.label()),
        Style::default().fg(rank_color).add_modifier(Modifier::BOLD),
    )));
    cl.push(Line::from(""));

    // Rewards
    cl.push(Line::from(Span::styled(" 報酬:", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))));
    cl.push(Line::from(Span::styled(
        format!("  ¥{}", 200 * rank.credit_multiplier()),
        Style::default().fg(Color::Green),
    )));
    cl.push(Line::from(Span::styled(
        format!("  💎{}", rank.gem_reward()),
        Style::default().fg(Color::Cyan),
    )));
    if rank.shard_reward() > 0 {
        cl.push(Line::from(Span::styled(
            format!("  {}の欠片 x{}", produce.character.short_name(), rank.shard_reward()),
            Style::default().fg(Color::Magenta),
        )));
    }
    cl.push(Line::from(Span::styled(
        format!("  キャラEXP +{}", rank.exp_reward()),
        Style::default().fg(Color::White),
    )));

    cl.push(Line::from(""));
    cl.push_clickable(
        Line::from(Span::styled(" ▶ OK", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))),
        PRODUCE_FINISH,
    );

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Magenta))
        .title(" プロデュース結果 ");
    {
        let mut cs = click_state.borrow_mut();
        cl.render(f, area, block, &mut cs, false, 0);
    }
}
