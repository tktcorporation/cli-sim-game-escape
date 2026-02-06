//! Career Simulator — climb the career ladder, invest, and grow your influence.

pub mod logic;
pub mod render;
pub mod state;

use std::cell::RefCell;
use std::rc::Rc;

use ratzilla::ratatui::layout::Rect;
use ratzilla::ratatui::Frame;

use crate::games::Game;
use crate::input::{ClickState, InputEvent};

use state::{CareerState, InvestKind, Screen};

pub struct CareerGame {
    pub state: CareerState,
}

impl CareerGame {
    pub fn new() -> Self {
        Self {
            state: CareerState::new(),
        }
    }
}

impl Game for CareerGame {
    fn handle_input(&mut self, event: &InputEvent) -> bool {
        let key = match event {
            InputEvent::Key(c) => *c,
        };

        match self.state.screen {
            Screen::Main => match key {
                '1' => {
                    logic::buy_training(&mut self.state, 0);
                    true
                }
                '2' => {
                    logic::buy_training(&mut self.state, 1);
                    true
                }
                '3' => {
                    logic::buy_training(&mut self.state, 2);
                    true
                }
                '4' => {
                    logic::buy_training(&mut self.state, 3);
                    true
                }
                '5' => {
                    logic::buy_training(&mut self.state, 4);
                    true
                }
                '6' => {
                    self.state.screen = Screen::JobMarket;
                    true
                }
                '7' => {
                    self.state.screen = Screen::Invest;
                    true
                }
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
                '-' => {
                    self.state.screen = Screen::Main;
                    true
                }
                _ => false,
            },
            Screen::Invest => match key {
                '1' => { logic::invest(&mut self.state, InvestKind::Savings); true }
                '2' => { logic::invest(&mut self.state, InvestKind::Stocks); true }
                '3' => { logic::invest(&mut self.state, InvestKind::RealEstate); true }
                '-' => {
                    self.state.screen = Screen::Main;
                    true
                }
                _ => false,
            },
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
    fn career_game_training() {
        let mut game = CareerGame::new();
        // Free self-study (key '4')
        game.handle_input(&InputEvent::Key('4'));
        assert_eq!(game.state.knowledge, 1.0);
    }

    #[test]
    fn career_game_training_needs_money() {
        let mut game = CareerGame::new();
        // Programming course costs ¥3,000 but we have ¥0
        game.handle_input(&InputEvent::Key('1'));
        assert_eq!(game.state.technical, 0.0);
    }

    #[test]
    fn career_game_screen_navigation() {
        let mut game = CareerGame::new();
        assert_eq!(game.state.screen, Screen::Main);

        game.handle_input(&InputEvent::Key('6'));
        assert_eq!(game.state.screen, Screen::JobMarket);

        game.handle_input(&InputEvent::Key('-'));
        assert_eq!(game.state.screen, Screen::Main);

        game.handle_input(&InputEvent::Key('7'));
        assert_eq!(game.state.screen, Screen::Invest);

        game.handle_input(&InputEvent::Key('-'));
        assert_eq!(game.state.screen, Screen::Main);
    }

    #[test]
    fn career_game_job_change() {
        let mut game = CareerGame::new();
        game.state.knowledge = 10.0;
        game.handle_input(&InputEvent::Key('6')); // go to job market
        game.handle_input(&InputEvent::Key('2')); // apply for office clerk
        assert_eq!(game.state.job, state::JobKind::OfficeClerk);
        assert_eq!(game.state.screen, Screen::Main); // returns to main after job change
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
    fn career_game_tick_earns_money() {
        let mut game = CareerGame::new();
        game.tick(10);
        assert!(game.state.money > 0.0);
    }

    #[test]
    fn career_game_full_progression() {
        let mut game = CareerGame::new();

        // Self-study 5 times to get knowledge >= 5
        for _ in 0..5 {
            game.handle_input(&InputEvent::Key('4'));
        }
        assert!(game.state.knowledge >= 5.0);

        // Switch to office clerk
        game.handle_input(&InputEvent::Key('6'));
        game.handle_input(&InputEvent::Key('2'));
        assert_eq!(game.state.job, state::JobKind::OfficeClerk);

        // Earn money and buy programming courses
        game.state.money = 15_000.0;
        for _ in 0..5 {
            game.handle_input(&InputEvent::Key('1')); // programming course, tech+3
        }
        assert!(game.state.technical >= 15.0);

        // Switch to programmer
        game.handle_input(&InputEvent::Key('6'));
        game.handle_input(&InputEvent::Key('3'));
        assert_eq!(game.state.job, state::JobKind::Programmer);
    }
}
