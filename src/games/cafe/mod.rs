//! 廃墟カフェ復興記 — Ruined Café Revival
//!
//! A story-driven café management game with novel-ADV style narrative.

mod actions;
mod logic;
mod render;
pub mod save;
mod scenario;
pub mod social;
mod state;

use std::cell::RefCell;
use std::rc::Rc;

use ratzilla::ratatui::layout::Rect;
use ratzilla::ratatui::Frame;

use crate::input::{ClickState, InputEvent};

use actions::*;
use social::{MissionType, BUSINESS_DAY_COST};
use state::{CafeState, GamePhase};

pub struct CafeGame {
    state: CafeState,
    initialized: bool,
    /// Tick counter for periodic save (every ~10 seconds = 100 ticks).
    save_tick_counter: u32,
}

impl CafeGame {
    pub fn new() -> Self {
        let mut state = CafeState::new();
        save::load_game(&mut state);

        Self {
            state,
            initialized: false,
            save_tick_counter: 0,
        }
    }
}

impl super::Game for CafeGame {
    fn handle_input(&mut self, event: &InputEvent) -> bool {
        match event {
            InputEvent::Key(ch) => handle_key(&mut self.state, *ch),
            InputEvent::Click(id) => handle_click(&mut self.state, *id),
        }
    }

    fn tick(&mut self, delta_ticks: u32) {
        let now = social::now_ms();

        // First tick: process login, recover stamina, check daily reset
        if !self.initialized && now > 0.0 {
            self.initialized = true;
            // Stamina recovery for offline time
            self.state.stamina.recover(now);
            // Login bonus
            if let Some(reward) = self.state.login_bonus.process_login(now) {
                self.state.pending_login_reward = Some(reward.money);
            }
            // Recovery bonus
            if self.state.login_bonus.has_recovery_bonus() {
                let bonus = self.state.login_bonus.recovery_bonus_money();
                self.state.pending_recovery_bonus = Some(bonus);
                self.state.login_bonus.recovery_shown = true;
            }
            // Daily mission reset
            self.state.daily_missions.check_reset(now);
        }

        // Periodic stamina recovery (every tick)
        if now > 0.0 {
            self.state.stamina.recover(now);
        }

        // Periodic save (~every 10 seconds)
        self.save_tick_counter += delta_ticks;
        if self.save_tick_counter >= 100 {
            self.save_tick_counter = 0;
            save::save_game(&self.state);
        }
    }

    fn render(&self, f: &mut Frame, area: Rect, click_state: &Rc<RefCell<ClickState>>) {
        render::render(&self.state, f, area, click_state);
    }
}

fn handle_key(state: &mut CafeState, ch: char) -> bool {
    // Dismiss popups first
    if state.pending_login_reward.is_some() || state.pending_recovery_bonus.is_some() {
        return dismiss_popup(state);
    }

    match state.phase {
        GamePhase::Story => match ch {
            ' ' | 'l' => logic::advance_story(state),
            _ => false,
        },
        GamePhase::Business => match ch {
            '1'..='9' => {
                let idx = (ch as u8 - b'1') as usize;
                if idx < state.menu.len() {
                    state.selected_menu_item = idx;
                    true
                } else {
                    false
                }
            }
            ' ' => try_run_business(state),
            // 'q' is handled by main.rs for back-to-menu
            'q' => {
                save::save_game(state);
                false
            }
            _ => false,
        },
        GamePhase::DayResult => match ch {
            ' ' | 'l' => {
                logic::next_day(state);
                save::save_game(state);
                true
            }
            _ => false,
        },
    }
}

fn handle_click(state: &mut CafeState, id: u16) -> bool {
    // Dismiss popups first
    if (state.pending_login_reward.is_some() || state.pending_recovery_bonus.is_some())
        && id == STORY_ADVANCE
    {
        return dismiss_popup(state);
    }

    match state.phase {
        GamePhase::Story => {
            if id == STORY_ADVANCE {
                return logic::advance_story(state);
            }
            false
        }
        GamePhase::Business => {
            if (MENU_ITEM_BASE..MENU_ITEM_BASE + 20).contains(&id) {
                let idx = (id - MENU_ITEM_BASE) as usize;
                if idx < state.menu.len() {
                    state.selected_menu_item = idx;
                    return true;
                }
            }
            if id == SERVE_CONFIRM {
                return try_run_business(state);
            }
            false
        }
        GamePhase::DayResult => {
            if id == SERVE_CONFIRM {
                logic::next_day(state);
                save::save_game(state);
                return true;
            }
            false
        }
    }
}

/// Try to run business (stamina check + mission tracking).
fn try_run_business(state: &mut CafeState) -> bool {
    let now = social::now_ms();
    if !state.stamina.consume(BUSINESS_DAY_COST, now) {
        return false; // Not enough stamina
    }

    logic::run_business_day(state);
    state.today_business_runs += 1;

    // Track mission progress
    state
        .daily_missions
        .record(MissionType::RunBusiness(state.today_business_runs));

    save::save_game(state);
    true
}

/// Dismiss the current popup and apply rewards.
fn dismiss_popup(state: &mut CafeState) -> bool {
    if let Some(reward) = state.pending_login_reward.take() {
        state.money += reward;
        // Claim today's bonus
        state.login_bonus.today_claimed = true;
        save::save_game(state);
        return true;
    }
    if let Some(bonus) = state.pending_recovery_bonus.take() {
        state.money += bonus;
        save::save_game(state);
        return true;
    }
    false
}
