//! Cookie Factory — an incremental cookie clicker game.

pub mod logic;
pub mod render;
#[cfg(test)]
mod simulator;
pub mod state;

use std::cell::RefCell;
use std::rc::Rc;

use ratzilla::ratatui::layout::Rect;
use ratzilla::ratatui::Frame;

use crate::input::{ClickState, InputEvent};
use crate::games::Game;

use state::{CookieState, ProducerKind};

pub struct CookieGame {
    pub state: CookieState,
}

impl CookieGame {
    pub fn new() -> Self {
        Self {
            state: CookieState::new(),
        }
    }
}

impl Game for CookieGame {
    fn handle_input(&mut self, event: &InputEvent) -> bool {
        let key = match event {
            InputEvent::Key(c) => *c,
        };

        match key {
            'c' => {
                logic::click(&mut self.state);
                true
            }
            'g' => {
                logic::claim_golden(&mut self.state);
                true
            }
            'u' => {
                self.state.show_upgrades = !self.state.show_upgrades;
                self.state.show_milestones = false;
                true
            }
            'm' => {
                self.state.show_milestones = !self.state.show_milestones;
                self.state.show_upgrades = false;
                true
            }
            '1' | '2' | '3' | '4' | '5' if !self.state.show_upgrades && !self.state.show_milestones => {
                let kind = match key {
                    '1' => ProducerKind::Cursor,
                    '2' => ProducerKind::Grandma,
                    '3' => ProducerKind::Farm,
                    '4' => ProducerKind::Mine,
                    '5' => ProducerKind::Factory,
                    _ => unreachable!(),
                };
                logic::buy_producer(&mut self.state, &kind);
                true
            }
            'a'..='z' if self.state.show_milestones => {
                // Map 'a'..'z' to ready milestone indices
                let display_idx = (key as u8 - b'a') as usize;
                let ready: Vec<usize> = self
                    .state
                    .milestones
                    .iter()
                    .enumerate()
                    .filter(|(_, m)| m.status == state::MilestoneStatus::Ready)
                    .map(|(i, _)| i)
                    .collect();
                if let Some(&real_idx) = ready.get(display_idx) {
                    logic::claim_milestone(&mut self.state, real_idx);
                }
                true
            }
            '!' if self.state.show_milestones => {
                // Claim all ready milestones at once
                logic::claim_all_milestones(&mut self.state);
                true
            }
            'a'..='z' if self.state.show_upgrades => {
                // Map 'a'..'z' to available upgrade indices
                let display_idx = (key as u8 - b'a') as usize;
                let available: Vec<usize> = self
                    .state
                    .upgrades
                    .iter()
                    .enumerate()
                    .filter(|(_, u)| !u.purchased)
                    .map(|(i, _)| i)
                    .collect();
                if let Some(&real_idx) = available.get(display_idx) {
                    logic::buy_upgrade(&mut self.state, real_idx);
                }
                true
            }
            _ => false,
        }
    }

    fn tick(&mut self, delta_ticks: u32) {
        logic::tick(&mut self.state, delta_ticks);
    }

    fn render(&self, f: &mut Frame, area: Rect, click_state: &Rc<RefCell<ClickState>>) {
        render::render(&self.state, f, area, click_state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cookie_game_click_produces_cookies() {
        let mut game = CookieGame::new();
        game.handle_input(&InputEvent::Key('c'));
        assert!((game.state.cookies - 1.0).abs() < 0.001);
    }

    #[test]
    fn cookie_game_buy_producer_via_input() {
        let mut game = CookieGame::new();
        game.state.cookies = 100.0;
        game.handle_input(&InputEvent::Key('1')); // buy cursor
        assert_eq!(game.state.producers[0].count, 1);
    }

    #[test]
    fn cookie_game_toggle_upgrades() {
        let mut game = CookieGame::new();
        assert!(!game.state.show_upgrades);
        game.handle_input(&InputEvent::Key('u'));
        assert!(game.state.show_upgrades);
        game.handle_input(&InputEvent::Key('u'));
        assert!(!game.state.show_upgrades);
    }

    #[test]
    fn cookie_game_buy_upgrade_via_input() {
        let mut game = CookieGame::new();
        game.state.cookies = 200.0;
        game.handle_input(&InputEvent::Key('u')); // show upgrades
        game.handle_input(&InputEvent::Key('a')); // buy first available
        assert!(game.state.upgrades[0].purchased);
    }

    #[test]
    fn cookie_game_tick_advances() {
        let mut game = CookieGame::new();
        game.state.producers[0].count = 10; // 1.0 cps
        game.tick(10);
        assert!((game.state.cookies - 1.0).abs() < 0.01);
    }

    #[test]
    fn producer_keys_ignored_in_upgrade_mode() {
        let mut game = CookieGame::new();
        game.state.cookies = 100.0;
        game.state.show_upgrades = true;
        game.handle_input(&InputEvent::Key('1'));
        // Should NOT buy a producer when in upgrade mode
        assert_eq!(game.state.producers[0].count, 0);
    }

    #[test]
    fn golden_cookie_claim_via_input() {
        let mut game = CookieGame::new();
        game.state.producers[1].count = 5;
        game.state.golden_event = Some(state::GoldenCookieEvent {
            appear_ticks_left: 50,
            claimed: false,
        });
        game.handle_input(&InputEvent::Key('g'));
        assert!(game.state.golden_event.is_none());
        assert_eq!(game.state.golden_cookies_claimed, 1);
    }

    #[test]
    fn toggle_milestones() {
        let mut game = CookieGame::new();
        assert!(!game.state.show_milestones);
        game.handle_input(&InputEvent::Key('m'));
        assert!(game.state.show_milestones);
        assert!(!game.state.show_upgrades);
        game.handle_input(&InputEvent::Key('m'));
        assert!(!game.state.show_milestones);
    }

    #[test]
    fn milestones_and_upgrades_mutually_exclusive() {
        let mut game = CookieGame::new();
        game.handle_input(&InputEvent::Key('u'));
        assert!(game.state.show_upgrades);
        game.handle_input(&InputEvent::Key('m'));
        assert!(game.state.show_milestones);
        assert!(!game.state.show_upgrades);
        game.handle_input(&InputEvent::Key('u'));
        assert!(game.state.show_upgrades);
        assert!(!game.state.show_milestones);
    }
}
