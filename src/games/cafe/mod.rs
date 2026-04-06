//! 廃墟カフェ復興記 — Ruined Café Revival
//!
//! A story-driven café management game with social game systems:
//! - AP action system (5 AP/day, 4 action types)
//! - Multi-axis character affinity (Trust/Understanding/Empathy)
//! - Card collection & gacha
//! - Player rank & story progression
//! - Memory equipment system

mod actions;
pub mod affinity;
pub mod cards;
mod input_handler;
mod logic;
mod render;
pub mod save;
mod scenario;
pub mod social;
pub mod state;

use std::cell::RefCell;
use std::rc::Rc;

use ratzilla::ratatui::layout::Rect;
use ratzilla::ratatui::Frame;

use crate::input::{ClickState, InputEvent};

use state::CafeState;

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
            InputEvent::Key(ch) => input_handler::handle_key(&mut self.state, *ch),
            InputEvent::Click(id) => input_handler::handle_click(&mut self.state, *id),
        }
    }

    fn tick(&mut self, delta_ticks: u32) {
        let now = social::now_ms();

        // First tick: process login, recover stamina, check daily reset
        if !self.initialized && now > 0.0 {
            self.initialized = true;
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

            // AP daily reset
            let jst_day = social::current_jst_day(now);
            let mut ap_reset = social::ApResetState {
                last_reset_day: self.state.day,
            };
            if ap_reset.check_reset(now) {
                logic::daily_reset(&mut self.state);
            }

            // Card daily draw reset
            self.state.card_state.check_daily_reset(jst_day);

            // Check memory unlocks
            logic::check_memory_unlocks(&mut self.state);
        }

        // Periodic stamina recovery
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
