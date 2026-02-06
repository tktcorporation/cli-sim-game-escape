//! Tiny Factory — a grid-based factory automation game.

pub mod actions;
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

use actions::*;
use grid::VIEW_W;
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

    fn handle_click(&mut self, action_id: u16) -> bool {
        match action_id {
            SELECT_MINER => {
                self.state.tool = PlacementTool::Miner;
                true
            }
            SELECT_SMELTER => {
                self.state.tool = PlacementTool::Smelter;
                true
            }
            SELECT_ASSEMBLER => {
                self.state.tool = PlacementTool::Assembler;
                true
            }
            SELECT_EXPORTER => {
                self.state.tool = PlacementTool::Exporter;
                true
            }
            SELECT_FABRICATOR => {
                self.state.tool = PlacementTool::Fabricator;
                true
            }
            SELECT_BELT => {
                self.state.tool = PlacementTool::Belt;
                true
            }
            SELECT_DELETE => {
                self.state.tool = PlacementTool::Delete;
                true
            }
            TOGGLE_MINER_MODE => {
                logic::toggle_miner_mode(&mut self.state);
                true
            }
            id if id >= GRID_CLICK_BASE => {
                let offset = (id - GRID_CLICK_BASE) as usize;
                let vy_offset = offset / VIEW_W;
                let vx_offset = offset % VIEW_W;
                let target_x = self.state.viewport_x + vx_offset;
                let target_y = self.state.viewport_y + vy_offset;
                self.state.cursor_x = target_x;
                self.state.cursor_y = target_y;
                logic::place(&mut self.state);
                true
            }
            _ => false,
        }
    }

    fn handle_key(&mut self, key: char) -> bool {
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
            '5' => {
                self.state.tool = PlacementTool::Fabricator;
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
            't' => {
                logic::toggle_miner_mode(&mut self.state);
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
}

impl Game for FactoryGame {
    fn handle_input(&mut self, event: &InputEvent) -> bool {
        match event {
            InputEvent::Key(c) => self.handle_key(*c),
            InputEvent::Click(id) => self.handle_click(*id),
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
    fn factory_game_belt_direction_follows_cursor() {
        let mut game = FactoryGame::new();
        assert_eq!(game.state.belt_direction, grid::Direction::Right);
        game.handle_input(&InputEvent::Key('j')); // move down
        assert_eq!(game.state.belt_direction, grid::Direction::Down);
    }

    // ── Click action tests ──────────────────────────────────────

    #[test]
    fn click_action_select_tool() {
        let mut game = FactoryGame::new();
        game.handle_input(&InputEvent::Click(SELECT_MINER));
        assert_eq!(game.state.tool, PlacementTool::Miner);
        game.handle_input(&InputEvent::Click(SELECT_BELT));
        assert_eq!(game.state.tool, PlacementTool::Belt);
        game.handle_input(&InputEvent::Click(SELECT_DELETE));
        assert_eq!(game.state.tool, PlacementTool::Delete);
    }
}
