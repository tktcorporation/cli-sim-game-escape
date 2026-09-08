//! 遠征団 — 戦役はTD配置、レベルは遊技場のメダルプッシャー。
//!
//! コアループ:
//! 1. 拠点で行軍糧が自然回復する（放置は燃料だけ）
//! 2. 章マップ（1-1, 1-2, …）を道への配置防衛で切り拓き、メダルを得る
//! 3. 遊技場でメダルを落として光珠を稼ぎ、レベルを上げてまた戦役へ戻る

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
    hero_id_from_level, hero_id_from_toggle, lane_from_drop, slot_from_place, ACK_RESULT,
    CANCEL_FORMING, CONFIRM_PLACEMENT, LAUNCH, OPEN_FORMING, START_FORMING, TAB_ARCADE, TAB_CAMP,
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
        if let Some(hero_id) = hero_id_from_level(id) {
            return logic::pick_level_hero(&mut self.state, hero_id);
        }
        if let Some(slot) = slot_from_place(id) {
            return logic::place_on_slot(&mut self.state, slot as usize);
        }
        if let Some(lane) = lane_from_drop(id) {
            return logic::drop_medal(&mut self.state, lane as usize);
        }
        match id {
            START_FORMING => logic::primary_depart(&mut self.state),
            OPEN_FORMING => logic::begin_forming(&mut self.state),
            CANCEL_FORMING => logic::cancel_forming(&mut self.state),
            LAUNCH => logic::launch_sortie(&mut self.state),
            CONFIRM_PLACEMENT => logic::confirm_placement(&mut self.state),
            ACK_RESULT => logic::acknowledge_result(&mut self.state),
            TAB_CAMP => logic::set_hub_tab(&mut self.state, HubTab::Camp),
            TAB_ARCADE => {
                if self.state.screen == Screen::Result {
                    let _ = logic::acknowledge_result(&mut self.state);
                }
                logic::set_hub_tab(&mut self.state, HubTab::Arcade)
            }
            _ => false,
        }
    }

    fn handle_key(&mut self, key: char) -> bool {
        match (self.state.screen, key) {
            (Screen::Camp, ' ' | 'e' | 'E') if self.state.hub_tab == HubTab::Camp => {
                logic::primary_depart(&mut self.state)
            }
            (Screen::Camp, '0'..='4') if self.state.hub_tab == HubTab::Arcade => {
                if self.state.pending_level_pick {
                    false
                } else {
                    let lane = (key as u8 - b'0') as usize;
                    logic::drop_medal(&mut self.state, lane)
                }
            }
            (Screen::Camp, '1'..='4')
                if self.state.hub_tab == HubTab::Arcade && self.state.pending_level_pick =>
            {
                let id = key as u8 - b'1';
                logic::pick_level_hero(&mut self.state, id)
            }
            (Screen::Camp, 'f' | 'F') => logic::begin_forming(&mut self.state),
            (Screen::Forming, ' ') => logic::launch_sortie(&mut self.state),
            (Screen::Forming, 'q' | 'Q' | 'b' | 'B') => logic::cancel_forming(&mut self.state),
            (Screen::Forming, '1'..='4') => {
                let id = key as u8 - b'1';
                logic::toggle_forming_hero(&mut self.state, id)
            }
            (Screen::Placing, ' ') => logic::confirm_placement(&mut self.state),
            (Screen::Placing, '0'..='4') => {
                let slot = (key as u8 - b'0') as usize;
                logic::place_on_slot(&mut self.state, slot)
            }
            (Screen::Placing, 'q' | 'Q' | 'b' | 'B') => logic::cancel_forming(&mut self.state),
            (Screen::Result, ' ' | '\n') => logic::acknowledge_result(&mut self.state),
            (_, '{') => logic::set_hub_tab(&mut self.state, HubTab::Camp),
            (_, '|' | 't' | 'T' | 'a' | 'A') => {
                logic::set_hub_tab(&mut self.state, HubTab::Arcade)
            }
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
        assert_eq!(game.state.screen, Screen::Placing);
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
        assert_eq!(game.state.hub_tab, HubTab::Arcade);
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

    #[test]
    fn arcade_drop_via_key() {
        let mut game = ExpeditionGame::new();
        game.state.hub_tab = HubTab::Arcade;
        game.state.medals = 10;
        let before = game.state.medals;
        assert!(game.handle_input(&InputEvent::Key('2')));
        assert_eq!(game.state.medals, before - 1);
    }
}
