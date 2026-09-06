//! 遠征団 — 行軍糧を貯めて短い遠征へ出し、結果でのみ絆が育つ。
//!
//! コアループ:
//! 1. 拠点で行軍糧（と下調べメモ）が自然回復する
//! 2. 3人を編成して出撃し、オート戦闘の遠征ランを進める
//! 3. 遠征は基本オート完走。任意で「援護」すると有利になる。クリア報酬の絆だけが永続成長

pub mod actions;
pub mod logic;
pub mod render;
pub mod save;
pub mod state;

#[cfg(test)]
mod simulator;

use std::cell::RefCell;
use std::rc::Rc;

use ratzilla::ratatui::layout::Rect;
use ratzilla::ratatui::Frame;

use crate::games::{Game, GameChoice};
use crate::input::{ClickScope, ClickState, InputEvent};

use actions::{
    hero_id_from_toggle, ACK_RESULT, CANCEL_FORMING, LAUNCH, LAUNCH_WITH_SCOUT, OPEN_FORMING,
    START_FORMING, TAB_CAMP, TAB_ROSTER, USE_AID,
};
use state::{ExpeditionState, HubTab, Screen};

pub struct ExpeditionGame {
    pub state: ExpeditionState,
    save_countdown: u32,
}

impl Default for ExpeditionGame {
    fn default() -> Self {
        Self::new()
    }
}

impl ExpeditionGame {
    pub fn new() -> Self {
        #[allow(unused_mut)]
        let mut state = ExpeditionState::new();
        #[cfg(target_arch = "wasm32")]
        {
            save::load_game(&mut state);
            if let Some(now) = crate::time::now_ms() {
                state.last_wall_ms = now as u64;
            }
        }
        Self {
            state,
            save_countdown: save::AUTOSAVE_INTERVAL,
        }
    }

    fn handle_action(&mut self, id: u16) -> bool {
        if let Some(hero_id) = hero_id_from_toggle(id) {
            return logic::toggle_forming_hero(&mut self.state, hero_id);
        }
        match id {
            START_FORMING => logic::primary_depart(&mut self.state),
            OPEN_FORMING => logic::begin_forming(&mut self.state),
            CANCEL_FORMING => logic::cancel_forming(&mut self.state),
            LAUNCH => logic::launch_sortie(&mut self.state, false),
            LAUNCH_WITH_SCOUT => logic::launch_sortie(&mut self.state, true),
            USE_AID => logic::use_aid(&mut self.state),
            ACK_RESULT => logic::acknowledge_result(&mut self.state),
            TAB_CAMP => logic::set_hub_tab(&mut self.state, HubTab::Camp),
            TAB_ROSTER => logic::set_hub_tab(&mut self.state, HubTab::Roster),
            _ => false,
        }
    }

    fn handle_key(&mut self, key: char) -> bool {
        match (self.state.screen, key) {
            (Screen::Camp, ' ' | 'e' | 'E') => logic::primary_depart(&mut self.state),
            (Screen::Camp, 'f' | 'F') => logic::begin_forming(&mut self.state),
            (Screen::Forming, ' ') => logic::launch_sortie(&mut self.state, false),
            (Screen::Forming, 's' | 'S') => logic::launch_sortie(&mut self.state, true),
            (Screen::Forming, 'q' | 'Q' | 'b' | 'B') => logic::cancel_forming(&mut self.state),
            (Screen::Forming, '1'..='4') => {
                let id = key as u8 - b'1';
                logic::toggle_forming_hero(&mut self.state, id)
            }
            (Screen::Running, 'a' | 'A' | ' ') => logic::use_aid(&mut self.state),
            (Screen::Result, ' ' | '\n') => logic::acknowledge_result(&mut self.state),
            (_, '{') => logic::set_hub_tab(&mut self.state, HubTab::Camp),
            (_, '|') => logic::set_hub_tab(&mut self.state, HubTab::Roster),
            _ => false,
        }
    }
}

impl Game for ExpeditionGame {
    fn choice(&self) -> GameChoice {
        GameChoice::Expedition
    }

    fn handle_input(&mut self, event: &InputEvent) -> bool {
        match event {
            InputEvent::Key(k) => self.handle_key(*k),
            InputEvent::Click(ClickScope::Game(GameChoice::Expedition), id) => {
                self.handle_action(*id)
            }
            _ => false,
        }
    }

    fn tick(&mut self, delta_ticks: u32) {
        logic::tick(&mut self.state, delta_ticks);
        self.save_countdown = self.save_countdown.saturating_sub(delta_ticks);
        if self.save_countdown == 0 {
            #[cfg(target_arch = "wasm32")]
            {
                if let Some(now) = crate::time::now_ms() {
                    self.state.last_wall_ms = now as u64;
                }
                save::save_game(&self.state);
            }
            self.save_countdown = save::AUTOSAVE_INTERVAL;
        }
    }

    fn on_leave(&mut self) {
        #[cfg(target_arch = "wasm32")]
        {
            if let Some(now) = crate::time::now_ms() {
                self.state.last_wall_ms = now as u64;
            }
            save::save_game(&self.state);
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
    fn camp_start_via_key() {
        let mut game = ExpeditionGame::new();
        assert!(game.handle_input(&InputEvent::Key('e')));
        assert_eq!(game.state.screen, Screen::Running);
    }

    #[test]
    fn camp_open_forming_via_key() {
        let mut game = ExpeditionGame::new();
        assert!(game.handle_input(&InputEvent::Key('f')));
        assert_eq!(game.state.screen, Screen::Forming);
    }

    #[test]
    fn hub_tab_switches_via_key() {
        let mut game = ExpeditionGame::new();
        assert!(game.handle_input(&InputEvent::Key('|')));
        assert_eq!(game.state.hub_tab, HubTab::Roster);
        assert!(game.handle_input(&InputEvent::Key('{')));
        assert_eq!(game.state.hub_tab, HubTab::Camp);
    }

    #[test]
    fn tick_regenerates_rations() {
        let mut game = ExpeditionGame::new();
        game.state.rations = 0;
        let need = game.state.ration_regen_ticks();
        game.tick(need);
        assert!(game.state.rations >= 1);
    }
}
