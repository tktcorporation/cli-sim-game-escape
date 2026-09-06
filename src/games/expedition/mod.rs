//! 遠征団 — マップを進め、詰まったら拠点で育てる。
//!
//! コアループ:
//! 1. 拠点で行軍糧が自然回復する（放置は燃料だけ）
//! 2. 章マップ（1-1, 1-2, …）をオート探索で切り拓き、補給を得る
//! 3. 詰まったら育成タブで補給を使いレベルを上げ、またマップへ戻る

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
    hero_id_from_toggle, hero_id_from_upgrade, ACK_RESULT, CANCEL_FORMING, LAUNCH, LAUNCH_WITH_SCOUT,
    OPEN_FORMING, START_FORMING, TAB_CAMP, TAB_TRAIN, USE_AID,
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
        if let Some(hero_id) = hero_id_from_upgrade(id) {
            return logic::upgrade_hero(&mut self.state, hero_id);
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
            TAB_TRAIN => {
                if self.state.screen == Screen::Result {
                    let _ = logic::acknowledge_result(&mut self.state);
                }
                logic::set_hub_tab(&mut self.state, HubTab::Train)
            }
            _ => false,
        }
    }

    fn handle_key(&mut self, key: char) -> bool {
        match (self.state.screen, key) {
            (Screen::Camp, ' ' | 'e' | 'E') => logic::primary_depart(&mut self.state),
            (Screen::Camp, 'f' | 'F') => logic::begin_forming(&mut self.state),
            (Screen::Camp, '1'..='4') if self.state.hub_tab == HubTab::Train => {
                let id = key as u8 - b'1';
                logic::upgrade_hero(&mut self.state, id)
            }
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
            (_, '|' | 't' | 'T') => logic::set_hub_tab(&mut self.state, HubTab::Train),
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
        assert_eq!(game.state.hub_tab, HubTab::Train);
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
