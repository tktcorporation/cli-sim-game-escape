/// Tiny Factory — a grid-based factory automation game.

pub mod grid;
pub mod logic;
pub mod render;
pub mod state;

use std::cell::RefCell;
use std::rc::Rc;

use ratzilla::ratatui::layout::Rect;
use ratzilla::ratatui::Frame;

use crate::games::Game;
use crate::input::{ClickState, InputEvent};

use state::{FactoryState, PlacementTool};

pub struct FactoryGame {
    pub state: FactoryState,
}

impl FactoryGame {
    pub fn new() -> Self {
        Self {
            state: FactoryState::new(),
        }
    }
}

impl Game for FactoryGame {
    fn handle_input(&mut self, event: &InputEvent) -> bool {
        let key = match event {
            InputEvent::Key(c) => *c,
        };

        match key {
            // Tool selection
            '1' => {
                self.state.tool = PlacementTool::Miner;
                true
            }
            '2' => {
                self.state.tool = PlacementTool::Smelter;
                true
            }
            '3' => {
                self.state.tool = PlacementTool::Assembler;
                true
            }
            '4' => {
                self.state.tool = PlacementTool::Exporter;
                true
            }
            'b' => {
                self.state.tool = PlacementTool::Belt;
                true
            }
            'd' => {
                self.state.tool = PlacementTool::Delete;
                true
            }
            'r' => {
                logic::rotate_belt(&mut self.state);
                true
            }
            // Cursor movement (WASD-style + arrow-like)
            'h' => {
                self.state.move_cursor(-1, 0);
                true
            }
            'l' => {
                self.state.move_cursor(1, 0);
                true
            }
            'k' => {
                self.state.move_cursor(0, -1);
                true
            }
            'j' => {
                self.state.move_cursor(0, 1);
                true
            }
            // Place
            ' ' => {
                logic::place(&mut self.state);
                true
            }
            _ => false,
        }
    }

    fn tick(&mut self, delta_ticks: u32) {
        logic::tick_n(&mut self.state, delta_ticks);
    }

    fn render(&self, f: &mut Frame, area: Rect, click_state: &Rc<RefCell<ClickState>>) {
        render::render(&self.state, f, area, click_state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn factory_game_select_tool() {
        let mut game = FactoryGame::new();
        game.handle_input(&InputEvent::Key('1'));
        assert_eq!(game.state.tool, PlacementTool::Miner);
        game.handle_input(&InputEvent::Key('b'));
        assert_eq!(game.state.tool, PlacementTool::Belt);
    }

    #[test]
    fn factory_game_move_cursor() {
        let mut game = FactoryGame::new();
        game.handle_input(&InputEvent::Key('l'));
        assert_eq!(game.state.cursor_x, 1);
        game.handle_input(&InputEvent::Key('j'));
        assert_eq!(game.state.cursor_y, 1);
    }

    #[test]
    fn factory_game_place_and_tick() {
        let mut game = FactoryGame::new();
        game.handle_input(&InputEvent::Key('1')); // select miner
        game.handle_input(&InputEvent::Key(' ')); // place

        assert!(matches!(
            game.state.grid[0][0],
            grid::Cell::Machine(_)
        ));

        game.tick(10);
        if let grid::Cell::Machine(m) = &game.state.grid[0][0] {
            assert_eq!(m.output_buffer.len(), 1);
        }
    }

    #[test]
    fn factory_game_rotate_belt() {
        let mut game = FactoryGame::new();
        assert_eq!(game.state.belt_direction, grid::Direction::Right);
        game.handle_input(&InputEvent::Key('r'));
        assert_eq!(game.state.belt_direction, grid::Direction::Down);
    }
}
