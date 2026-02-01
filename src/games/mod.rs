/// Game trait and game selection logic.

pub mod cookie;
pub mod factory;

use std::cell::RefCell;
use std::rc::Rc;

use ratzilla::ratatui::layout::Rect;
use ratzilla::ratatui::Frame;

use crate::input::{ClickState, InputEvent};

/// Trait that all games implement.
pub trait Game {
    /// Handle an input event. Returns true if the event was consumed.
    fn handle_input(&mut self, event: &InputEvent) -> bool;

    /// Advance game logic by `delta_ticks` discrete ticks.
    fn tick(&mut self, delta_ticks: u32);

    /// Render the game into the given area.
    fn render(&self, f: &mut Frame, area: Rect, click_state: &Rc<RefCell<ClickState>>);
}

/// Which game the player has selected (or is choosing).
#[derive(Clone, Debug, PartialEq)]
pub enum GameChoice {
    Cookie,
    Factory,
}

/// Top-level application state.
pub enum AppState {
    /// Showing game selection menu.
    Menu,
    /// Playing a game.
    Playing {
        game: Box<dyn Game>,
    },
}

/// Create a game instance from a choice.
pub fn create_game(choice: &GameChoice) -> Box<dyn Game> {
    match choice {
        GameChoice::Cookie => Box::new(cookie::CookieGame::new()),
        GameChoice::Factory => Box::new(factory::FactoryGame::new()),
    }
}
