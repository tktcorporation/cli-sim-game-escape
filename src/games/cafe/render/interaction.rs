//! Character interaction rendering (select, action, result, detail).

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
use super::super::characters::affinity;
use super::super::characters::skills;
use super::super::characters::{ActionType, CharacterId};
use super::super::state::{CafeState, AP_MAX};

pub(super) fn render_character_select(state: &CafeState, f: &mut Frame, area: Rect, click_state: &Rc<RefCell<ClickState>>) {
    let mut cl = ClickableList::new();
    cl.push(Line::from(Span::styled(format!(" AP: {}/{}", state.ap_current, AP_MAX), Style::default().fg(Color::Green))));
    cl.push(Line::from(""));
    cl.push(Line::from(Span::styled(" 誰と交流しますか？", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))));
    cl.push(Line::from(""));

    let unlocked = state.unlocked_characters();
    for (i, ch) in unlocked.iter().enumerate() {
        let data = state.character_data.get(ch);
        let stars = data.map(|d| d.stars).unwrap_or(1);
        let level = data.map(|d| d.level).unwrap_or(1);
        let star_str = "★".repeat(stars as usize);
        cl.push_clickable(Line::from(vec![
            Span::styled(format!(" [{}] ", i + 1), Style::default().fg(Color::Yellow)),
            Span::styled(ch.name(), Style::default().fg(Color::White)),
            Span::styled(format!("  {star_str} Lv.{level}"), Style::default().fg(Color::Cyan)),
        ]), CHARACTER_BASE + i as u16);
    }
    cl.push(Line::from(""));
    cl.push_clickable(Line::from(Span::styled(" ◀ 戻る", Style::default().fg(Color::DarkGray))), CHARACTER_BACK);

    let block = Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::Cyan)).title(" 常連客を選ぶ ");
    {
        let mut cs = click_state.borrow_mut();
        cl.render(f, area, block, &mut cs, false, 0);
    }
}

pub(super) fn render_action_select(state: &CafeState, f: &mut Frame, area: Rect, click_state: &Rc<RefCell<ClickState>>, target: CharacterId) {
    let mut cl = ClickableList::new();
    cl.push(Line::from(Span::styled(format!(" {} への行動", target.name()), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))));
    cl.push(Line::from(Span::styled(format!(" AP: {}/{}", state.ap_current, AP_MAX), Style::default().fg(Color::Green))));
    cl.push(Line::from(""));

    let actions = [
        (ActionType::Eat, ACTION_EAT),
        (ActionType::Observe, ACTION_OBSERVE),
        (ActionType::Talk, ACTION_TALK),
        (ActionType::Special, ACTION_SPECIAL),
    ];

    for (action, id) in &actions {
        let cost = action.ap_cost();
        let can_do = state.ap_current >= cost;
        let special_locked = *action == ActionType::Special
            && state.affinities.get(&target).map(|a| a.axes.star_rank()).unwrap_or(1) < 2;
        let style = if !can_do || special_locked {
            Style::default().fg(Color::DarkGray)
        } else {
            Style::default().fg(Color::White)
        };
        let gains = affinity::base_gains(*action);
        cl.push_clickable(Line::from(vec![
            Span::styled(format!(" ▶ {} ", action.name()), style.add_modifier(Modifier::BOLD)),
            Span::styled(format!("(AP-{}) ", cost), Style::default().fg(Color::Yellow)),
            Span::styled(action.description(), Style::default().fg(Color::DarkGray)),
        ]), *id);
        cl.push(Line::from(Span::styled(
            format!("   信頼+{} 理解+{} 共感+{}", gains.trust, gains.understanding, gains.empathy),
            Style::default().fg(Color::DarkGray),
        )));
    }

    cl.push(Line::from(""));
    cl.push_clickable(Line::from(Span::styled(" ◀ 戻る", Style::default().fg(Color::DarkGray))), ACTION_BACK);

    let block = Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::Yellow)).title(" 行動を選ぶ ");
    {
        let mut cs = click_state.borrow_mut();
        cl.render(f, area, block, &mut cs, false, 0);
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn render_action_result(f: &mut Frame, area: Rect, click_state: &Rc<RefCell<ClickState>>, target: CharacterId, action: ActionType, trust: u32, understanding: u32, empathy: u32) {
    let mut cl = ClickableList::new();
    cl.push(Line::from(""));
    cl.push(Line::from(Span::styled(
        format!(" {} に「{}」を行った！", target.name(), action.name()),
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
    )));
    cl.push(Line::from(""));
    cl.push(Line::from(Span::styled(format!("  信頼  +{trust}"), Style::default().fg(Color::Red))));
    cl.push(Line::from(Span::styled(format!("  理解  +{understanding}"), Style::default().fg(Color::Blue))));
    cl.push(Line::from(Span::styled(format!("  共感  +{empathy}"), Style::default().fg(Color::Green))));
    cl.push(Line::from(""));
    cl.push_clickable(Line::from(Span::styled(" ▶ OK", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))), RESULT_OK);

    let block = Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::Yellow)).title(" 結果 ");
    {
        let mut cs = click_state.borrow_mut();
        cl.render(f, area, block, &mut cs, false, 0);
    }
}

pub(super) fn render_character_detail(state: &CafeState, f: &mut Frame, area: Rect, click_state: &Rc<RefCell<ClickState>>, target: CharacterId) {
    let mut cl = ClickableList::new();
    cl.push(Line::from(Span::styled(format!(" {}", target.name()), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))));
    cl.push(Line::from(""));

    // Character data (level, stars, shards)
    if let Some(data) = state.character_data.get(&target) {
        let star_str = "★".repeat(data.stars as usize);
        cl.push(Line::from(Span::styled(
            format!(" {star_str} Lv.{}/{}", data.level, data.level_cap()),
            Style::default().fg(Color::Yellow),
        )));
        cl.push(Line::from(Span::styled(
            format!(" EXP: {}/{}", data.exp, data.exp_to_next_level()),
            Style::default().fg(Color::DarkGray),
        )));

        // Shards / promotion
        if let Some(cost) = data.shards_to_promote() {
            let can_promote = data.shards >= cost;
            cl.push(Line::from(Span::styled(
                format!(" 欠片: {}/{}", data.shards, cost),
                Style::default().fg(if can_promote { Color::Green } else { Color::White }),
            )));
            if can_promote {
                cl.push_clickable(Line::from(Span::styled(
                    " ▶ ★昇格する",
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                )), DETAIL_PROMOTE);
            }
        } else {
            cl.push(Line::from(Span::styled(" ★MAX", Style::default().fg(Color::Yellow))));
        }
        cl.push(Line::from(""));

        // Skills
        cl.push(Line::from(Span::styled(" スキル:", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD))));
        let skill_defs = skills::character_skills(target);
        for (i, skill) in skill_defs.iter().enumerate() {
            let sl = data.skill_levels[i];
            if sl > 0 {
                cl.push(Line::from(Span::styled(
                    format!("  {} Lv.{} — {}", skill.name, sl, skill.description),
                    Style::default().fg(Color::White),
                )));
            } else {
                cl.push(Line::from(Span::styled(
                    format!("  ??? (★{}で解放)", skill.unlock_stars),
                    Style::default().fg(Color::DarkGray),
                )));
            }
        }
    }

    cl.push(Line::from(""));

    // Affinity
    if let Some(aff) = state.affinities.get(&target) {
        cl.push(Line::from(Span::styled(
            format!(" 好感度 Lv.{} (次まで{}pt)", aff.axes.level(), aff.axes.points_to_next_level()),
            Style::default().fg(Color::Magenta),
        )));
        cl.push(Line::from(Span::styled(format!("  信頼: {}", aff.axes.trust), Style::default().fg(Color::Red))));
        cl.push(Line::from(Span::styled(format!("  理解: {}", aff.axes.understanding), Style::default().fg(Color::Blue))));
        cl.push(Line::from(Span::styled(format!("  共感: {}", aff.axes.empathy), Style::default().fg(Color::Green))));
    }

    cl.push(Line::from(""));
    cl.push_clickable(Line::from(Span::styled(" ◀ 戻る", Style::default().fg(Color::DarkGray))), DETAIL_BACK);

    let block = Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::Cyan)).title(" キャラクター詳細 ");
    {
        let mut cs = click_state.borrow_mut();
        cl.render(f, area, block, &mut cs, false, 0);
    }
}
