//! Cookie Factory — an incremental cookie clicker game.

pub mod actions;
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

use actions::*;
use state::{CookieState, DragonAura, ProducerKind, SugarBoostKind};

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

    /// Handle a click action by semantic action ID (direct dispatch, no context ambiguity).
    fn handle_click(&mut self, action_id: u16) -> bool {
        match action_id {
            CLICK_COOKIE => {
                logic::click(&mut self.state);
                true
            }
            CLAIM_GOLDEN => {
                logic::claim_golden(&mut self.state);
                true
            }
            TAB_PRODUCERS => {
                self.state.show_upgrades = false;
                self.state.show_research = false;
                self.state.show_milestones = false;
                self.state.show_prestige = false;
                true
            }
            TAB_UPGRADES => {
                self.state.show_upgrades = true;
                self.state.show_research = false;
                self.state.show_milestones = false;
                self.state.show_prestige = false;
                true
            }
            TAB_RESEARCH => {
                self.state.show_research = true;
                self.state.show_upgrades = false;
                self.state.show_milestones = false;
                self.state.show_prestige = false;
                true
            }
            TAB_MILESTONES => {
                self.state.show_milestones = true;
                self.state.show_upgrades = false;
                self.state.show_research = false;
                self.state.show_prestige = false;
                true
            }
            TAB_PRESTIGE => {
                self.state.show_prestige = true;
                self.state.show_upgrades = false;
                self.state.show_research = false;
                self.state.show_milestones = false;
                true
            }
            id if (BUY_PRODUCER_BASE..BUY_PRODUCER_BASE + 12).contains(&id) => {
                let idx = (id - BUY_PRODUCER_BASE) as usize;
                if let Some(kind) = ProducerKind::from_index(idx) {
                    logic::buy_producer(&mut self.state, &kind);
                }
                true
            }
            id if (BUY_UPGRADE_BASE..BUY_UPGRADE_BASE + 26).contains(&id) => {
                let display_idx = (id - BUY_UPGRADE_BASE) as usize;
                let available: Vec<usize> = self.state.upgrades.iter().enumerate()
                    .filter(|(_, u)| !u.purchased)
                    .map(|(i, _)| i)
                    .collect();
                if let Some(&real_idx) = available.get(display_idx) {
                    logic::buy_upgrade(&mut self.state, real_idx);
                }
                true
            }
            id if (BUY_RESEARCH_BASE..BUY_RESEARCH_BASE + 26).contains(&id) => {
                let display_idx = (id - BUY_RESEARCH_BASE) as usize;
                let visible: Vec<usize> = self.state.research_nodes.iter().enumerate()
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
                if let Some(&real_idx) = visible.get(display_idx) {
                    logic::buy_research(&mut self.state, real_idx);
                }
                true
            }
            id if (CLAIM_MILESTONE_BASE..CLAIM_MILESTONE_BASE + 26).contains(&id) => {
                let display_idx = (id - CLAIM_MILESTONE_BASE) as usize;
                let ready: Vec<usize> = self.state.milestones.iter().enumerate()
                    .filter(|(_, m)| m.status == state::MilestoneStatus::Ready)
                    .map(|(i, _)| i)
                    .collect();
                if let Some(&real_idx) = ready.get(display_idx) {
                    logic::claim_milestone(&mut self.state, real_idx);
                }
                true
            }
            CLAIM_ALL_MILESTONES => {
                logic::claim_all_milestones(&mut self.state);
                true
            }
            PRESTIGE_RESET => {
                logic::perform_prestige(&mut self.state);
                true
            }
            id if (BUY_PRESTIGE_UPGRADE_BASE..BUY_PRESTIGE_UPGRADE_BASE + 26).contains(&id) => {
                let idx = (id - BUY_PRESTIGE_UPGRADE_BASE) as usize;
                logic::buy_prestige_upgrade(&mut self.state, idx);
                true
            }
            id if (DRAGON_FEED_BASE..DRAGON_FEED_BASE + 12).contains(&id) => {
                let idx = (id - DRAGON_FEED_BASE) as usize;
                if let Some(kind) = ProducerKind::from_index(idx) {
                    logic::feed_dragon(&mut self.state, &kind, 1);
                }
                true
            }
            DRAGON_CYCLE_AURA => {
                let auras = DragonAura::all();
                let current_idx = auras.iter().position(|a| *a == self.state.dragon_aura);
                let next = match current_idx {
                    Some(i) => auras[(i + 1) % auras.len()].clone(),
                    None => auras[0].clone(),
                };
                logic::set_dragon_aura(&mut self.state, next);
                true
            }
            SUGAR_RUSH => {
                logic::activate_sugar_boost(&mut self.state, SugarBoostKind::Rush);
                true
            }
            SUGAR_FEVER => {
                logic::activate_sugar_boost(&mut self.state, SugarBoostKind::Fever);
                true
            }
            SUGAR_FRENZY => {
                logic::activate_sugar_boost(&mut self.state, SugarBoostKind::Frenzy);
                true
            }
            TOGGLE_AUTO_CLICKER => {
                logic::toggle_auto_clicker(&mut self.state);
                true
            }
            _ => false,
        }
    }

    /// Handle a keyboard key press (context-dependent, as before).
    fn handle_key(&mut self, key: char) -> bool {
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
            // Tab direct-set keys (used by keyboard shortcuts)
            '{' => {
                self.state.show_upgrades = false;
                self.state.show_research = false;
                self.state.show_milestones = false;
                self.state.show_prestige = false;
                true
            }
            '|' => {
                self.state.show_upgrades = true;
                self.state.show_research = false;
                self.state.show_milestones = false;
                self.state.show_prestige = false;
                true
            }
            '\\' => {
                self.state.show_research = true;
                self.state.show_upgrades = false;
                self.state.show_milestones = false;
                self.state.show_prestige = false;
                true
            }
            '}' => {
                self.state.show_milestones = true;
                self.state.show_upgrades = false;
                self.state.show_research = false;
                self.state.show_prestige = false;
                true
            }
            '~' => {
                self.state.show_prestige = true;
                self.state.show_upgrades = false;
                self.state.show_research = false;
                self.state.show_milestones = false;
                true
            }
            'p' if self.state.show_prestige => {
                logic::perform_prestige(&mut self.state);
                true
            }
            '1'..='8' if self.state.show_prestige => {
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
                let auras = DragonAura::all();
                let current_idx = auras.iter().position(|a| *a == self.state.dragon_aura);
                let next = match current_idx {
                    Some(i) => auras[(i + 1) % auras.len()].clone(),
                    None => auras[0].clone(),
                };
                logic::set_dragon_aura(&mut self.state, next);
                true
            }
            '1'..='9' | '0' | '-' | '=' if !self.state.show_upgrades && !self.state.show_research && !self.state.show_milestones && !self.state.show_prestige => {
                let kind = match key {
                    '1' => ProducerKind::Cursor,
                    '2' => ProducerKind::Grandma,
                    '3' => ProducerKind::Farm,
                    '4' => ProducerKind::Mine,
                    '5' => ProducerKind::Factory,
                    '6' => ProducerKind::Temple,
                    '7' => ProducerKind::WizardTower,
                    '8' => ProducerKind::Shipment,
                    '9' => ProducerKind::AlchemyLab,
                    '0' => ProducerKind::Portal,
                    '-' => ProducerKind::TimeMachine,
                    '=' => ProducerKind::AntimatterCondenser,
                    _ => unreachable!(),
                };
                logic::buy_producer(&mut self.state, &kind);
                true
            }
            // Sugar boost activation (Shift+R=Rush, Shift+F=Fever, Shift+Z=Frenzy)
            'R' if self.state.show_prestige => {
                logic::activate_sugar_boost(&mut self.state, SugarBoostKind::Rush);
                true
            }
            'F' if self.state.show_prestige => {
                logic::activate_sugar_boost(&mut self.state, SugarBoostKind::Fever);
                true
            }
            'Z' if self.state.show_prestige => {
                logic::activate_sugar_boost(&mut self.state, SugarBoostKind::Frenzy);
                true
            }
            // Auto-clicker toggle (Shift+A)
            'A' if self.state.show_prestige => {
                logic::toggle_auto_clicker(&mut self.state);
                true
            }
            'a'..='z' if self.state.show_prestige => {
                let idx = (key as u8 - b'a') as usize;
                logic::buy_prestige_upgrade(&mut self.state, idx);
                true
            }
            'a'..='z' if self.state.show_milestones => {
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
}

impl Game for CookieGame {
    fn handle_input(&mut self, event: &InputEvent) -> bool {
        match event {
            InputEvent::Key(c) => self.handle_key(*c),
            InputEvent::Click(id) => self.handle_click(*id),
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

    // ── Keyboard input tests (same as before) ────────────────────

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

    #[test]
    fn late_producers_buyable() {
        let mut game = CookieGame::new();
        game.state.cookies = 1e18;
        game.handle_input(&InputEvent::Key('9')); // AlchemyLab
        assert_eq!(game.state.producers[8].count, 1);
        game.handle_input(&InputEvent::Key('0')); // Portal
        assert_eq!(game.state.producers[9].count, 1);
        game.handle_input(&InputEvent::Key('-')); // TimeMachine
        assert_eq!(game.state.producers[10].count, 1);
        game.handle_input(&InputEvent::Key('=')); // AntimatterCondenser
        assert_eq!(game.state.producers[11].count, 1);
    }

    // ── Click action tests (new: semantic action IDs) ────────────

    #[test]
    fn click_action_produces_cookies() {
        let mut game = CookieGame::new();
        game.handle_input(&InputEvent::Click(CLICK_COOKIE));
        assert!((game.state.cookies - 1.0).abs() < 0.001);
    }

    #[test]
    fn click_action_buy_producer() {
        let mut game = CookieGame::new();
        game.state.cookies = 100.0;
        game.handle_input(&InputEvent::Click(BUY_PRODUCER_BASE)); // Cursor
        assert_eq!(game.state.producers[0].count, 1);
    }

    #[test]
    fn click_action_buy_producer_no_context_dependency() {
        let mut game = CookieGame::new();
        game.state.cookies = 100.0;
        // Even with upgrades tab open, click action directly buys producer
        game.state.show_upgrades = true;
        game.handle_input(&InputEvent::Click(BUY_PRODUCER_BASE));
        assert_eq!(game.state.producers[0].count, 1);
    }

    #[test]
    fn click_action_tab_navigation() {
        let mut game = CookieGame::new();
        game.handle_input(&InputEvent::Click(TAB_UPGRADES));
        assert!(game.state.show_upgrades);
        game.handle_input(&InputEvent::Click(TAB_PRODUCERS));
        assert!(!game.state.show_upgrades);
    }

    #[test]
    fn click_action_golden_cookie() {
        let mut game = CookieGame::new();
        game.state.producers[1].count = 5;
        game.state.golden_event = Some(state::GoldenCookieEvent {
            appear_ticks_left: 50,
            claimed: false,
        });
        game.handle_input(&InputEvent::Click(CLAIM_GOLDEN));
        assert!(game.state.golden_event.is_none());
    }

    #[test]
    fn click_action_prestige_reset() {
        let mut game = CookieGame::new();
        game.state.cookies = 1e15;
        game.state.cookies_all_time = 1e15;
        game.handle_input(&InputEvent::Click(PRESTIGE_RESET));
        // After prestige, cookies should be reset
        assert!(game.state.cookies < 1.0);
    }
}
