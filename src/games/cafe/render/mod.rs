//! Rendering for the Café game — all phases.

mod day_result;
mod gacha;
mod hub;
mod interaction;
mod produce;
mod story;

use std::cell::RefCell;
use std::rc::Rc;

use ratzilla::ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratzilla::ratatui::style::{Color, Modifier, Style};
use ratzilla::ratatui::text::{Line, Span};
use ratzilla::ratatui::widgets::{Block, Borders};
use ratzilla::ratatui::Frame;

use crate::input::ClickState;
use crate::widgets::ClickableList;

use super::actions::*;
use super::state::{CafeState, GamePhase};

/// Main render dispatcher.
pub fn render(state: &CafeState, f: &mut Frame, area: Rect, click_state: &Rc<RefCell<ClickState>>) {
    if state.pending_login_reward.is_some() || state.pending_recovery_bonus.is_some() {
        render_popup(state, f, area, click_state);
        return;
    }

    match &state.phase {
        GamePhase::Story => story::render_story(state, f, area, click_state),
        GamePhase::Hub => hub::render_hub(state, f, area, click_state),
        GamePhase::CharacterSelect => interaction::render_character_select(state, f, area, click_state),
        GamePhase::ActionSelect { target } => interaction::render_action_select(state, f, area, click_state, *target),
        GamePhase::ActionResult { target, action, trust_gain, understanding_gain, empathy_gain } => {
            interaction::render_action_result(f, area, click_state, *target, *action, *trust_gain, *understanding_gain, *empathy_gain);
        }
        GamePhase::CharacterDetail { target } => interaction::render_character_detail(state, f, area, click_state, *target),
        GamePhase::CardScreen => gacha::render_card_screen(state, f, area, click_state),
        GamePhase::GachaResult { card_ids } => gacha::render_gacha_result(f, area, click_state, card_ids),
        GamePhase::ProduceCharSelect => produce::render_produce_char_select(state, f, area, click_state),
        GamePhase::ProduceTraining => produce::render_produce_training(state, f, area, click_state),
        GamePhase::ProduceTurnResult { training } => produce::render_produce_turn_result(state, f, area, click_state, *training),
        GamePhase::ProduceResult => produce::render_produce_result(state, f, area, click_state),
        GamePhase::DayResult => day_result::render_day_result(state, f, area, click_state),
    }
}

fn render_popup(
    state: &CafeState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),
            Constraint::Length(9),
            Constraint::Min(3),
        ])
        .split(area);

    let popup_area = chunks[1];
    let mut cl = ClickableList::new();

    if let Some(reward) = state.pending_login_reward {
        let day = state.login_bonus.total_login_days;
        let gems = state.pending_login_gems.unwrap_or(0);
        cl.push(Line::from(""));
        cl.push(Line::from(Span::styled(
            "  ログインボーナス",
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        )));
        cl.push(Line::from(""));
        cl.push(Line::from(Span::styled(
            format!("  ログイン {day}日目"),
            Style::default().fg(Color::White),
        )));
        cl.push(Line::from(Span::styled(
            format!("  報酬: ¥{reward} + 💎{gems}"),
            Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
        )));
        cl.push(Line::from(""));
        cl.push_clickable(
            Line::from(Span::styled(
                "  ▶ 受け取る",
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            )),
            STORY_ADVANCE,
        );
    } else if let Some(bonus) = state.pending_recovery_bonus {
        let days = state.login_bonus.absence_days;
        cl.push(Line::from(""));
        cl.push(Line::from(Span::styled(
            "  おかえりなさい！",
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        )));
        cl.push(Line::from(""));
        cl.push(Line::from(Span::styled(
            format!("  {days}日ぶりのご来店です"),
            Style::default().fg(Color::White),
        )));
        cl.push(Line::from(Span::styled(
            format!("  復帰ボーナス: ¥{bonus}"),
            Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
        )));
        cl.push(Line::from(""));
        cl.push_clickable(
            Line::from(Span::styled(
                "  ▶ 受け取る",
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            )),
            STORY_ADVANCE,
        );
    }

    let popup_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow))
        .title(" お知らせ ");
    {
        let mut cs = click_state.borrow_mut();
        cl.render(f, popup_area, popup_block, &mut cs, false, 0);
    }
}
