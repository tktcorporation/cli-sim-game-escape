//! Career Simulator — climb the career ladder, invest, and grow your influence.

pub mod actions;
pub mod logic;
pub mod render;
pub mod save;
pub mod state;

use std::cell::RefCell;
use std::rc::Rc;

use ratzilla::ratatui::layout::Rect;
use ratzilla::ratatui::Frame;

use crate::games::Game;
use crate::input::{ClickState, InputEvent};

use actions::*;
use state::{CareerState, InvestKind, Screen};

pub struct CareerGame {
    pub state: CareerState,
}

impl CareerGame {
    pub fn new() -> Self {
        let state = CareerState::new();

        #[cfg(target_arch = "wasm32")]
        let state = {
            let mut s = state;
            if save::load_game(&mut s) {
                s.add_log("セーブデータをロードしました");
            }
            s
        };

        Self { state }
    }

    fn handle_click(&mut self, action_id: u16) -> bool {
        match action_id {
            // Main screen
            ADVANCE_MONTH => {
                logic::advance_month(&mut self.state);
                true
            }
            GO_TRAINING => {
                self.state.screen = Screen::Training;
                true
            }
            DO_NETWORKING => {
                logic::do_networking(&mut self.state);
                true
            }
            DO_SIDE_JOB => {
                logic::do_side_job(&mut self.state);
                true
            }
            GO_JOB_MARKET => {
                self.state.screen = Screen::JobMarket;
                true
            }
            GO_INVEST => {
                self.state.screen = Screen::Invest;
                true
            }
            GO_BUDGET => {
                self.state.screen = Screen::Budget;
                true
            }
            GO_LIFESTYLE => {
                self.state.screen = Screen::Lifestyle;
                true
            }
            // Training screen
            id if (TRAINING_BASE..TRAINING_BASE + 5).contains(&id) => {
                logic::buy_training(&mut self.state, (id - TRAINING_BASE) as usize);
                true
            }
            BACK_FROM_TRAINING => {
                self.state.screen = Screen::Main;
                true
            }
            // Job Market screen
            id if (APPLY_JOB_BASE..APPLY_JOB_BASE + 10).contains(&id) => {
                logic::apply_job(&mut self.state, (id - APPLY_JOB_BASE) as usize);
                true
            }
            BACK_FROM_JOBS | BACK_FROM_INVEST | BACK_FROM_BUDGET | BACK_FROM_LIFESTYLE => {
                self.state.screen = Screen::Main;
                true
            }
            // Invest screen
            INVEST_SAVINGS => {
                logic::invest(&mut self.state, InvestKind::Savings);
                true
            }
            INVEST_STOCKS => {
                logic::invest(&mut self.state, InvestKind::Stocks);
                true
            }
            INVEST_REAL_ESTATE => {
                logic::invest(&mut self.state, InvestKind::RealEstate);
                true
            }
            // Lifestyle screen
            id if (LIFESTYLE_BASE..LIFESTYLE_BASE + 5).contains(&id) => {
                logic::change_lifestyle(&mut self.state, (id - LIFESTYLE_BASE) as usize);
                true
            }
            _ => false,
        }
    }

    fn handle_key(&mut self, key: char) -> bool {
        match self.state.screen {
            Screen::Main => match key {
                '1' => { self.state.screen = Screen::Training; true }
                '2' => { logic::do_networking(&mut self.state); true }
                '3' => { logic::do_side_job(&mut self.state); true }
                '6' => { self.state.screen = Screen::JobMarket; true }
                '7' => { self.state.screen = Screen::Invest; true }
                '8' => { self.state.screen = Screen::Budget; true }
                '9' => { self.state.screen = Screen::Lifestyle; true }
                '0' => { logic::advance_month(&mut self.state); true }
                _ => false,
            },
            Screen::Training => match key {
                '1' => { logic::buy_training(&mut self.state, 0); true }
                '2' => { logic::buy_training(&mut self.state, 1); true }
                '3' => { logic::buy_training(&mut self.state, 2); true }
                '4' => { logic::buy_training(&mut self.state, 3); true }
                '5' => { logic::buy_training(&mut self.state, 4); true }
                '-' => { self.state.screen = Screen::Main; true }
                _ => false,
            },
            Screen::JobMarket => match key {
                '1' => { logic::apply_job(&mut self.state, 0); true }
                '2' => { logic::apply_job(&mut self.state, 1); true }
                '3' => { logic::apply_job(&mut self.state, 2); true }
                '4' => { logic::apply_job(&mut self.state, 3); true }
                '5' => { logic::apply_job(&mut self.state, 4); true }
                '6' => { logic::apply_job(&mut self.state, 5); true }
                '7' => { logic::apply_job(&mut self.state, 6); true }
                '8' => { logic::apply_job(&mut self.state, 7); true }
                '9' => { logic::apply_job(&mut self.state, 8); true }
                '0' => { logic::apply_job(&mut self.state, 9); true }
                '-' => { self.state.screen = Screen::Main; true }
                _ => false,
            },
            Screen::Invest => match key {
                '1' => { logic::invest(&mut self.state, InvestKind::Savings); true }
                '2' => { logic::invest(&mut self.state, InvestKind::Stocks); true }
                '3' => { logic::invest(&mut self.state, InvestKind::RealEstate); true }
                '-' => { self.state.screen = Screen::Main; true }
                _ => false,
            },
            Screen::Budget => match key {
                '-' => { self.state.screen = Screen::Main; true }
                _ => false,
            },
            Screen::Lifestyle => match key {
                '1' => { logic::change_lifestyle(&mut self.state, 0); true }
                '2' => { logic::change_lifestyle(&mut self.state, 1); true }
                '3' => { logic::change_lifestyle(&mut self.state, 2); true }
                '4' => { logic::change_lifestyle(&mut self.state, 3); true }
                '5' => { logic::change_lifestyle(&mut self.state, 4); true }
                '-' => { self.state.screen = Screen::Main; true }
                _ => false,
            },
        }
    }
}

impl Game for CareerGame {
    fn handle_input(&mut self, event: &InputEvent) -> bool {
        let consumed = match event {
            InputEvent::Key(c) => self.handle_key(*c),
            InputEvent::Click(id) => self.handle_click(*id),
        };
        if consumed {
            #[cfg(target_arch = "wasm32")]
            save::save_game(&self.state);
        }
        consumed
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
    fn career_game_training_via_subscreen() {
        let mut game = CareerGame::new();
        // Navigate to training screen
        game.handle_input(&InputEvent::Key('1'));
        assert_eq!(game.state.screen, Screen::Training);
        // Free self-study (key '4' in training screen)
        game.handle_input(&InputEvent::Key('4'));
        assert_eq!(game.state.knowledge, 1.0);
        assert_eq!(game.state.ap, 1); // used 1 AP
    }

    #[test]
    fn career_game_training_needs_money() {
        let mut game = CareerGame::new();
        game.handle_input(&InputEvent::Key('1')); // go to training
        // Programming course costs ¥3,000 but we have ¥0
        game.handle_input(&InputEvent::Key('1'));
        assert_eq!(game.state.technical, 0.0);
    }

    #[test]
    fn career_game_training_needs_ap() {
        let mut game = CareerGame::new();
        game.state.money = 50_000.0;
        game.state.ap = 0;
        game.handle_input(&InputEvent::Key('1')); // go to training
        game.handle_input(&InputEvent::Key('1')); // try programming course
        assert_eq!(game.state.technical, 0.0);
        assert_eq!(game.state.money, 50_000.0); // unchanged
    }

    #[test]
    fn career_game_screen_navigation() {
        let mut game = CareerGame::new();
        assert_eq!(game.state.screen, Screen::Main);

        game.handle_input(&InputEvent::Key('1'));
        assert_eq!(game.state.screen, Screen::Training);

        game.handle_input(&InputEvent::Key('-'));
        assert_eq!(game.state.screen, Screen::Main);

        game.handle_input(&InputEvent::Key('6'));
        assert_eq!(game.state.screen, Screen::JobMarket);

        game.handle_input(&InputEvent::Key('-'));
        assert_eq!(game.state.screen, Screen::Main);

        game.handle_input(&InputEvent::Key('7'));
        assert_eq!(game.state.screen, Screen::Invest);

        game.handle_input(&InputEvent::Key('-'));
        assert_eq!(game.state.screen, Screen::Main);

        game.handle_input(&InputEvent::Key('8'));
        assert_eq!(game.state.screen, Screen::Budget);

        game.handle_input(&InputEvent::Key('-'));
        assert_eq!(game.state.screen, Screen::Main);

        game.handle_input(&InputEvent::Key('9'));
        assert_eq!(game.state.screen, Screen::Lifestyle);

        game.handle_input(&InputEvent::Key('-'));
        assert_eq!(game.state.screen, Screen::Main);
    }

    #[test]
    fn career_game_networking() {
        let mut game = CareerGame::new();
        game.handle_input(&InputEvent::Key('2')); // networking
        assert_eq!(game.state.social, 2.0);
        assert_eq!(game.state.reputation, 3.0);
        assert_eq!(game.state.ap, 1);
    }

    #[test]
    fn career_game_side_job() {
        let mut game = CareerGame::new();
        game.state.technical = 10.0;
        game.handle_input(&InputEvent::Key('3')); // side job
        assert!(game.state.money > 0.0);
        assert_eq!(game.state.ap, 1);
    }

    #[test]
    fn career_game_job_change() {
        let mut game = CareerGame::new();
        game.state.knowledge = 10.0;
        game.handle_input(&InputEvent::Key('6')); // go to job market
        game.handle_input(&InputEvent::Key('2')); // apply for office clerk
        assert_eq!(game.state.job, state::JobKind::OfficeClerk);
        assert_eq!(game.state.screen, Screen::Main);
        assert_eq!(game.state.ap, 1); // used 1 AP
    }

    #[test]
    fn career_game_invest() {
        let mut game = CareerGame::new();
        game.state.money = 5_000.0;
        game.handle_input(&InputEvent::Key('7')); // go to invest
        game.handle_input(&InputEvent::Key('1')); // savings +¥1,000
        assert_eq!(game.state.savings, 1_000.0);
        assert_eq!(game.state.money, 4_000.0);
    }

    #[test]
    fn career_game_advance_month_earns_money() {
        let mut game = CareerGame::new();
        game.handle_input(&InputEvent::Key('0')); // advance month
        assert!(game.state.money > 0.0);
        assert_eq!(game.state.months_elapsed, 1);
    }

    #[test]
    fn career_game_advance_month_via_click() {
        let mut game = CareerGame::new();
        game.handle_input(&InputEvent::Click(ADVANCE_MONTH));
        assert!(game.state.money > 0.0);
        assert_eq!(game.state.months_elapsed, 1);
    }

    #[test]
    fn career_game_tick_is_noop() {
        let mut game = CareerGame::new();
        game.tick(1000);
        assert_eq!(game.state.money, 0.0);
        assert_eq!(game.state.months_elapsed, 0);
    }

    #[test]
    fn career_game_full_progression() {
        let mut game = CareerGame::new();

        // Self-study to get knowledge >= 5 (uses AP, need to advance months)
        game.handle_input(&InputEvent::Key('1')); // training screen
        game.handle_input(&InputEvent::Key('4')); // 独学 (AP: 2→1)
        game.handle_input(&InputEvent::Key('4')); // 独学 (AP: 1→0)
        game.handle_input(&InputEvent::Key('-')); // back to main
        game.handle_input(&InputEvent::Key('0')); // advance month (AP reset)
        game.handle_input(&InputEvent::Key('1')); // training screen
        game.handle_input(&InputEvent::Key('4')); // 独学 (AP: 2→1)
        game.handle_input(&InputEvent::Key('4')); // 独学 (AP: 1→0)
        game.handle_input(&InputEvent::Key('-')); // back to main
        game.handle_input(&InputEvent::Key('0')); // advance month
        game.handle_input(&InputEvent::Key('1')); // training screen
        game.handle_input(&InputEvent::Key('4')); // 独学 (AP: 2→1)
        game.handle_input(&InputEvent::Key('-')); // back
        assert!(game.state.knowledge >= 5.0);

        // Switch to office clerk
        game.handle_input(&InputEvent::Key('6'));
        game.handle_input(&InputEvent::Key('2'));
        assert_eq!(game.state.job, state::JobKind::OfficeClerk);

        // Earn money and buy programming courses
        game.state.money = 15_000.0;
        game.state.ap = 3; // give enough AP
        game.state.ap_max = 3;
        game.handle_input(&InputEvent::Key('1')); // training screen
        game.handle_input(&InputEvent::Key('1')); // programming course, tech+3
        game.handle_input(&InputEvent::Key('1')); // tech+3
        game.handle_input(&InputEvent::Key('1')); // tech+3 (no AP left)
        game.handle_input(&InputEvent::Key('-')); // back
        game.handle_input(&InputEvent::Key('0')); // advance month
        game.state.money = 15_000.0;
        game.handle_input(&InputEvent::Key('1')); // training screen
        game.handle_input(&InputEvent::Key('1')); // tech+3
        game.handle_input(&InputEvent::Key('1')); // tech+3
        game.handle_input(&InputEvent::Key('-'));
        assert!(game.state.technical >= 15.0);

        // Need AP for job change — advance month to reset
        game.handle_input(&InputEvent::Key('0'));

        // Switch to programmer
        game.handle_input(&InputEvent::Key('6'));
        game.handle_input(&InputEvent::Key('3'));
        assert_eq!(game.state.job, state::JobKind::Programmer);
    }

    // ── Click action tests ──────────────────────────────────────

    #[test]
    fn click_action_training() {
        let mut game = CareerGame::new();
        // Navigate to training via click
        game.handle_input(&InputEvent::Click(GO_TRAINING));
        assert_eq!(game.state.screen, Screen::Training);
        // Free self-study via click (index 3)
        game.handle_input(&InputEvent::Click(TRAINING_BASE + 3));
        assert_eq!(game.state.knowledge, 1.0);
    }

    #[test]
    fn click_action_screen_navigation() {
        let mut game = CareerGame::new();
        assert_eq!(game.state.screen, Screen::Main);

        game.handle_input(&InputEvent::Click(GO_TRAINING));
        assert_eq!(game.state.screen, Screen::Training);

        game.handle_input(&InputEvent::Click(BACK_FROM_TRAINING));
        assert_eq!(game.state.screen, Screen::Main);

        game.handle_input(&InputEvent::Click(GO_JOB_MARKET));
        assert_eq!(game.state.screen, Screen::JobMarket);

        game.handle_input(&InputEvent::Click(BACK_FROM_JOBS));
        assert_eq!(game.state.screen, Screen::Main);

        game.handle_input(&InputEvent::Click(GO_INVEST));
        assert_eq!(game.state.screen, Screen::Invest);

        game.handle_input(&InputEvent::Click(BACK_FROM_INVEST));
        assert_eq!(game.state.screen, Screen::Main);

        game.handle_input(&InputEvent::Click(GO_BUDGET));
        assert_eq!(game.state.screen, Screen::Budget);

        game.handle_input(&InputEvent::Click(BACK_FROM_BUDGET));
        assert_eq!(game.state.screen, Screen::Main);

        game.handle_input(&InputEvent::Click(GO_LIFESTYLE));
        assert_eq!(game.state.screen, Screen::Lifestyle);

        game.handle_input(&InputEvent::Click(BACK_FROM_LIFESTYLE));
        assert_eq!(game.state.screen, Screen::Main);
    }

    #[test]
    fn click_action_networking() {
        let mut game = CareerGame::new();
        game.handle_input(&InputEvent::Click(DO_NETWORKING));
        assert_eq!(game.state.social, 2.0);
        assert_eq!(game.state.reputation, 3.0);
    }

    #[test]
    fn click_action_side_job() {
        let mut game = CareerGame::new();
        game.state.technical = 10.0;
        game.handle_input(&InputEvent::Click(DO_SIDE_JOB));
        assert!(game.state.money > 0.0);
    }

    #[test]
    fn click_action_job_apply() {
        let mut game = CareerGame::new();
        game.state.knowledge = 10.0;
        game.handle_input(&InputEvent::Click(APPLY_JOB_BASE + 1)); // office clerk
        assert_eq!(game.state.job, state::JobKind::OfficeClerk);
    }

    #[test]
    fn click_action_invest() {
        let mut game = CareerGame::new();
        game.state.money = 5_000.0;
        game.handle_input(&InputEvent::Click(INVEST_SAVINGS));
        assert_eq!(game.state.savings, 1_000.0);
        assert_eq!(game.state.money, 4_000.0);
    }

    #[test]
    fn click_action_lifestyle() {
        let mut game = CareerGame::new();
        game.handle_input(&InputEvent::Click(GO_LIFESTYLE));
        assert_eq!(game.state.screen, Screen::Lifestyle);

        game.handle_input(&InputEvent::Click(LIFESTYLE_BASE + 1)); // Normal
        assert_eq!(game.state.lifestyle, state::LifestyleLevel::Normal);
        assert_eq!(game.state.screen, Screen::Main);
    }

    #[test]
    fn key_action_lifestyle() {
        let mut game = CareerGame::new();
        game.handle_input(&InputEvent::Key('9')); // go to lifestyle
        assert_eq!(game.state.screen, Screen::Lifestyle);

        game.handle_input(&InputEvent::Key('2')); // Normal
        assert_eq!(game.state.lifestyle, state::LifestyleLevel::Normal);
        assert_eq!(game.state.screen, Screen::Main);
    }

    #[test]
    fn monthly_cycle_integration() {
        let mut game = CareerGame::new();
        game.handle_input(&InputEvent::Key('0')); // advance 1 month
        assert_eq!(game.state.months_elapsed, 1);
        assert!(game.state.money > 0.0);
        assert!(game.state.money < 2_400.0);
        // AP should be reset
        assert_eq!(game.state.ap, game.state.ap_max);
    }

    #[test]
    fn ap_limits_actions_per_month() {
        let mut game = CareerGame::new();
        // Freeter: 2 AP
        assert_eq!(game.state.ap, 2);

        // Use both AP
        game.handle_input(&InputEvent::Key('2')); // networking (AP: 2→1)
        game.handle_input(&InputEvent::Key('2')); // networking (AP: 1→0)
        assert_eq!(game.state.ap, 0);

        // Can't do more actions
        game.handle_input(&InputEvent::Key('2')); // should fail
        assert_eq!(game.state.ap, 0);

        // Advance month resets AP
        game.handle_input(&InputEvent::Key('0'));
        assert_eq!(game.state.ap, 2);
    }
}
