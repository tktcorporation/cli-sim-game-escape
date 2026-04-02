//! 廃墟カフェ復興記 — Ruined Café Revival
//!
//! A story-driven café management game with novel-ADV style narrative.

mod actions;
mod logic;
mod render;
mod scenario;
mod state;

use std::cell::RefCell;
use std::rc::Rc;

use ratzilla::ratatui::layout::Rect;
use ratzilla::ratatui::Frame;

use crate::input::{ClickState, InputEvent};

use actions::*;
use state::{CafeState, GamePhase};

pub struct CafeGame {
    state: CafeState,
}

impl CafeGame {
    pub fn new() -> Self {
        Self {
            state: CafeState::new(),
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

    fn tick(&mut self, _delta_ticks: u32) {
        // No continuous simulation for now.
        // Future: stamina recovery, time-based events.
    }

    fn render(&self, f: &mut Frame, area: Rect, click_state: &Rc<RefCell<ClickState>>) {
        render::render(&self.state, f, area, click_state);
    }
}

fn handle_key(state: &mut CafeState, ch: char) -> bool {
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
            ' ' => {
                logic::run_business_day(state);
                true
            }
            // 'q' is handled by main.rs for back-to-menu
            'q' => false,
            _ => false,
        },
        GamePhase::DayResult => match ch {
            ' ' | 'l' => {
                logic::next_day(state);
                true
            }
            _ => false,
        },
    }
}

fn handle_click(state: &mut CafeState, id: u16) -> bool {
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
                logic::run_business_day(state);
                return true;
            }
            false
        }
        GamePhase::DayResult => {
            if id == SERVE_CONFIRM {
                logic::next_day(state);
                return true;
            }
            false
        }
    }
}
