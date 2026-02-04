//! Cookie Factory — an incremental cookie clicker game.

pub mod logic;
pub mod render;
pub mod save;
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
    /// オートセーブまでの残り tick 数。
    save_countdown: u32,
}

impl CookieGame {
    pub fn new() -> Self {
        let state = CookieState::new();

        #[cfg(target_arch = "wasm32")]
        let state = {
            let mut s = state;
            if save::load_game(&mut s) {
                s.add_log("セーブデータをロードしました", true);
            }
            s
        };

        Self {
            state,
            save_countdown: save::AUTOSAVE_INTERVAL,
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
                self.state.show_research = false;
                self.state.show_milestones = false;
                self.state.show_prestige = false;
                true
            }
            'r' => {
                self.state.show_research = !self.state.show_research;
                self.state.show_upgrades = false;
                self.state.show_milestones = false;
                self.state.show_prestige = false;
                true
            }
            'm' => {
                self.state.show_milestones = !self.state.show_milestones;
                self.state.show_upgrades = false;
                self.state.show_research = false;
                self.state.show_prestige = false;
                true
            }
            // Tab direct-set keys (used by click targets, not toggling)
            '{' => {
                // Go to Producers tab
                self.state.show_upgrades = false;
                self.state.show_research = false;
                self.state.show_milestones = false;
                self.state.show_prestige = false;
                true
            }
            '|' => {
                // Go to Upgrades tab
                self.state.show_upgrades = true;
                self.state.show_research = false;
                self.state.show_milestones = false;
                self.state.show_prestige = false;
                true
            }
            '\\' => {
                // Go to Research tab
                self.state.show_research = true;
                self.state.show_upgrades = false;
                self.state.show_milestones = false;
                self.state.show_prestige = false;
                true
            }
            '}' => {
                // Go to Milestones tab
                self.state.show_milestones = true;
                self.state.show_upgrades = false;
                self.state.show_research = false;
                self.state.show_prestige = false;
                true
            }
            '~' => {
                // Go to Prestige tab
                self.state.show_prestige = true;
                self.state.show_upgrades = false;
                self.state.show_research = false;
                self.state.show_milestones = false;
                true
            }
            'p' if self.state.show_prestige => {
                // Perform prestige reset
                logic::perform_prestige(&mut self.state);
                true
            }
            '1'..='8' if self.state.show_prestige => {
                // In prestige mode: feed producer to dragon
                let kind = match key {
                    '1' => ProducerKind::Cursor,
                    '2' => ProducerKind::Grandma,
                    '3' => ProducerKind::Farm,
                    '4' => ProducerKind::Mine,
                    '5' => ProducerKind::Factory,
                    '6' => ProducerKind::Temple,
                    '7' => ProducerKind::WizardTower,
                    '8' => ProducerKind::Shipment,
                    _ => unreachable!(),
                };
                logic::feed_dragon(&mut self.state, &kind, 1);
                true
            }
            '9' if self.state.show_prestige => {
                // Cycle dragon aura
                let auras = state::DragonAura::all();
                let current_idx = auras.iter().position(|a| *a == self.state.dragon_aura);
                let next = match current_idx {
                    Some(i) => auras[(i + 1) % auras.len()].clone(),
                    None => auras[0].clone(),
                };
                logic::set_dragon_aura(&mut self.state, next);
                true
            }
            '1'..='8' if !self.state.show_upgrades && !self.state.show_research && !self.state.show_milestones && !self.state.show_prestige => {
                let kind = match key {
                    '1' => ProducerKind::Cursor,
                    '2' => ProducerKind::Grandma,
                    '3' => ProducerKind::Farm,
                    '4' => ProducerKind::Mine,
                    '5' => ProducerKind::Factory,
                    '6' => ProducerKind::Temple,
                    '7' => ProducerKind::WizardTower,
                    '8' => ProducerKind::Shipment,
                    _ => unreachable!(),
                };
                logic::buy_producer(&mut self.state, &kind);
                true
            }
            'a'..='z' if self.state.show_prestige => {
                let idx = (key as u8 - b'a') as usize;
                logic::buy_prestige_upgrade(&mut self.state, idx);
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
                let display_idx = (key as u8 - b'a') as usize;
                let available_upgrades: Vec<usize> = self
                    .state
                    .upgrades
                    .iter()
                    .enumerate()
                    .filter(|(_, u)| !u.purchased)
                    .map(|(i, _)| i)
                    .collect();

                if let Some(&real_idx) = available_upgrades.get(display_idx) {
                    logic::buy_upgrade(&mut self.state, real_idx);
                }
                true
            }
            'a'..='z' if self.state.show_research => {
                let display_idx = (key as u8 - b'a') as usize;
                let visible_research: Vec<usize> = self
                    .state
                    .research_nodes
                    .iter()
                    .enumerate()
                    .filter(|(_, n)| {
                        if self.state.research_path != state::ResearchPath::None
                            && n.path != self.state.research_path
                        {
                            return false;
                        }
                        !n.purchased
                    })
                    .map(|(i, _)| i)
                    .collect();

                if let Some(&real_idx) = visible_research.get(display_idx) {
                    logic::buy_research(&mut self.state, real_idx);
                }
                true
            }
            _ => false,
        }
    }

    fn tick(&mut self, delta_ticks: u32) {
        logic::tick(&mut self.state, delta_ticks);

        // オートセーブ (WASM環境のみ)
        self.save_countdown = self.save_countdown.saturating_sub(delta_ticks);
        if self.save_countdown == 0 {
            #[cfg(target_arch = "wasm32")]
            save::save_game(&self.state);
            self.save_countdown = save::AUTOSAVE_INTERVAL;
        }
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

    #[test]
    fn tab_direct_set_producers() {
        let mut game = CookieGame::new();
        game.state.show_upgrades = true;
        game.handle_input(&InputEvent::Key('{'));
        assert!(!game.state.show_upgrades);
        assert!(!game.state.show_milestones);
    }

    #[test]
    fn tab_direct_set_upgrades() {
        let mut game = CookieGame::new();
        game.handle_input(&InputEvent::Key('|'));
        assert!(game.state.show_upgrades);
        assert!(!game.state.show_milestones);
        // Clicking again stays on upgrades (no toggle)
        game.handle_input(&InputEvent::Key('|'));
        assert!(game.state.show_upgrades);
    }

    #[test]
    fn tab_direct_set_milestones() {
        let mut game = CookieGame::new();
        game.handle_input(&InputEvent::Key('}'));
        assert!(game.state.show_milestones);
        assert!(!game.state.show_upgrades);
        // Clicking again stays on milestones
        game.handle_input(&InputEvent::Key('}'));
        assert!(game.state.show_milestones);
    }

    #[test]
    fn tab_direct_set_prestige() {
        let mut game = CookieGame::new();
        game.handle_input(&InputEvent::Key('~'));
        assert!(game.state.show_prestige);
        assert!(!game.state.show_upgrades);
        assert!(!game.state.show_milestones);
    }

    #[test]
    fn prestige_tab_mutually_exclusive() {
        let mut game = CookieGame::new();
        game.handle_input(&InputEvent::Key('~'));
        assert!(game.state.show_prestige);
        game.handle_input(&InputEvent::Key('{'));
        assert!(!game.state.show_prestige);
        game.handle_input(&InputEvent::Key('~'));
        game.handle_input(&InputEvent::Key('|'));
        assert!(!game.state.show_prestige);
        assert!(game.state.show_upgrades);
    }

    #[test]
    fn prestige_upgrade_via_input() {
        let mut game = CookieGame::new();
        game.state.heavenly_chips = 10;
        game.handle_input(&InputEvent::Key('~')); // go to prestige tab
        game.handle_input(&InputEvent::Key('a')); // buy first prestige upgrade
        assert!(game.state.prestige_upgrades[0].purchased);
    }

    #[test]
    fn producer_keys_ignored_in_prestige_mode() {
        let mut game = CookieGame::new();
        game.state.cookies = 1000.0;
        game.state.show_prestige = true;
        game.handle_input(&InputEvent::Key('1'));
        assert_eq!(game.state.producers[0].count, 0);
    }

    #[test]
    fn toggle_research() {
        let mut game = CookieGame::new();
        assert!(!game.state.show_research);
        game.handle_input(&InputEvent::Key('r'));
        assert!(game.state.show_research);
        assert!(!game.state.show_upgrades);
        assert!(!game.state.show_milestones);
        assert!(!game.state.show_prestige);
        game.handle_input(&InputEvent::Key('r'));
        assert!(!game.state.show_research);
    }

    #[test]
    fn tab_direct_set_research() {
        let mut game = CookieGame::new();
        game.handle_input(&InputEvent::Key('\\'));
        assert!(game.state.show_research);
        assert!(!game.state.show_upgrades);
        assert!(!game.state.show_milestones);
        assert!(!game.state.show_prestige);
    }

    #[test]
    fn research_tab_mutually_exclusive() {
        let mut game = CookieGame::new();
        game.handle_input(&InputEvent::Key('r'));
        assert!(game.state.show_research);
        game.handle_input(&InputEvent::Key('u'));
        assert!(game.state.show_upgrades);
        assert!(!game.state.show_research);
        game.handle_input(&InputEvent::Key('r'));
        assert!(game.state.show_research);
        game.handle_input(&InputEvent::Key('m'));
        assert!(game.state.show_milestones);
        assert!(!game.state.show_research);
    }

    #[test]
    fn research_purchase_via_input() {
        let mut game = CookieGame::new();
        game.state.cookies = 1e12;
        game.handle_input(&InputEvent::Key('r'));
        game.handle_input(&InputEvent::Key('a'));
        let purchased_count = game.state.research_nodes.iter().filter(|n| n.purchased).count();
        assert!(purchased_count > 0);
    }

    #[test]
    fn new_producers_buyable() {
        let mut game = CookieGame::new();
        game.state.cookies = 1e12;
        game.handle_input(&InputEvent::Key('6')); // Temple
        assert_eq!(game.state.producers[5].count, 1);
        game.handle_input(&InputEvent::Key('7')); // WizardTower
        assert_eq!(game.state.producers[6].count, 1);
        game.handle_input(&InputEvent::Key('8')); // Shipment
        assert_eq!(game.state.producers[7].count, 1);
    }
}
