//! 星環 — 螺旋漂流する鉱石を公転武装の連射で砕く放置ゲーム。
//!
//! 外周を漂う鉱石を軌道上の武装で刈り取り、星屑を得る。
//! 層は撃破条件を満たしたうえで星屑を払って開放する。進むと敵の数・強さ・報酬が
//! 段で切り替わり、新しい武装と環武装が解放される。
//! 各武装は弾数・連射・威力を個別に強化できる。

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
use crate::input::{ClickState, InputEvent};

use actions::{
    ring_for_buy_id, weapon_for_select_id, weapon_stat_for_buy_id, OPEN_LAYER, RING_SCROLL_DOWN,
    RING_SCROLL_UP, TAB_ARMORY, TAB_CODEX, TAB_RING, TAP_STRIKE, WEAPON_NEXT, WEAPON_PREV,
};

const RING_SCROLL_STEP: i32 = 3;
use state::{RingUpgrade, StarRingState, Tab, WeaponKind, WeaponStat};

pub struct StarRingGame {
    pub state: StarRingState,
    save_countdown: u32,
}

impl Default for StarRingGame {
    fn default() -> Self {
        Self::new()
    }
}

impl StarRingGame {
    pub fn new() -> Self {
        #[allow(unused_mut)]
        let mut state = StarRingState::new();
        #[cfg(target_arch = "wasm32")]
        {
            save::load_game(&mut state);
        }
        Self {
            state,
            save_countdown: save::AUTOSAVE_INTERVAL,
        }
    }

    fn handle_key(&mut self, key: char) -> bool {
        match key {
            '{' | 'u' | 'U' => {
                self.state.tab = Tab::Armory;
                true
            }
            '|' | 'r' | 'R' => {
                self.state.tab = Tab::Ring;
                self.state.ring_scroll.set(0);
                true
            }
            '}' | 'c' | 'C' => {
                self.state.tab = Tab::Codex;
                true
            }
            'k' | 'K' if self.state.tab == Tab::Ring => {
                self.state.scroll_ring(-RING_SCROLL_STEP);
                true
            }
            'j' | 'J' if self.state.tab == Tab::Ring => {
                self.state.scroll_ring(RING_SCROLL_STEP);
                true
            }
            ' ' | 't' | 'T' => {
                logic::manual_strike(&mut self.state);
                true
            }
            '[' | ',' => {
                logic::cycle_selected_weapon(&mut self.state, -1);
                true
            }
            ']' | '.' => {
                logic::cycle_selected_weapon(&mut self.state, 1);
                true
            }
            'a' | 'A' if self.state.tab == Tab::Armory => {
                let w = self.state.selected_weapon;
                logic::purchase_weapon_stat(&mut self.state, w, WeaponStat::Count)
            }
            's' | 'S' if self.state.tab == Tab::Armory => {
                let w = self.state.selected_weapon;
                logic::purchase_weapon_stat(&mut self.state, w, WeaponStat::Rate)
            }
            'd' | 'D' if self.state.tab == Tab::Armory => {
                let w = self.state.selected_weapon;
                logic::purchase_weapon_stat(&mut self.state, w, WeaponStat::Power)
            }
            '1' if self.state.tab == Tab::Armory => {
                logic::select_weapon(&mut self.state, WeaponKind::Pulse)
            }
            '2' if self.state.tab == Tab::Armory => {
                logic::select_weapon(&mut self.state, WeaponKind::Ray)
            }
            '3' if self.state.tab == Tab::Armory => {
                logic::select_weapon(&mut self.state, WeaponKind::Scatter)
            }
            '4' if self.state.tab == Tab::Armory => {
                logic::select_weapon(&mut self.state, WeaponKind::Arc)
            }
            '5' if self.state.tab == Tab::Armory => {
                logic::select_weapon(&mut self.state, WeaponKind::Nova)
            }
            '1' if self.state.tab == Tab::Ring => {
                logic::purchase_ring_upgrade(&mut self.state, RingUpgrade::Yield)
            }
            '2' if self.state.tab == Tab::Ring => {
                logic::purchase_ring_upgrade(&mut self.state, RingUpgrade::CorePulse)
            }
            '!' => logic::unlock_next_layer(&mut self.state),
            _ => false,
        }
    }

    fn handle_click(&mut self, action_id: u16) -> bool {
        match action_id {
            TAB_ARMORY => {
                self.state.tab = Tab::Armory;
                true
            }
            TAB_RING => {
                self.state.tab = Tab::Ring;
                self.state.ring_scroll.set(0);
                true
            }
            TAB_CODEX => {
                self.state.tab = Tab::Codex;
                true
            }
            TAP_STRIKE => {
                logic::manual_strike(&mut self.state);
                true
            }
            OPEN_LAYER => logic::unlock_next_layer(&mut self.state),
            RING_SCROLL_UP => {
                self.state.scroll_ring(-RING_SCROLL_STEP);
                true
            }
            RING_SCROLL_DOWN => {
                self.state.scroll_ring(RING_SCROLL_STEP);
                true
            }
            WEAPON_PREV => {
                logic::cycle_selected_weapon(&mut self.state, -1);
                true
            }
            WEAPON_NEXT => {
                logic::cycle_selected_weapon(&mut self.state, 1);
                true
            }
            id => {
                if let Some(w) = weapon_for_select_id(id) {
                    logic::select_weapon(&mut self.state, w);
                    true
                } else if let Some((w, s)) = weapon_stat_for_buy_id(id) {
                    logic::purchase_weapon_stat(&mut self.state, w, s);
                    true
                } else if let Some(kind) = ring_for_buy_id(id) {
                    logic::purchase_ring_upgrade(&mut self.state, kind);
                    true
                } else {
                    false
                }
            }
        }
    }
}

impl Game for StarRingGame {
    fn choice(&self) -> GameChoice {
        GameChoice::StarRing
    }

    fn handle_input(&mut self, event: &InputEvent) -> bool {
        match event {
            InputEvent::Key(c) => self.handle_key(*c),
            InputEvent::Click(_, id) => self.handle_click(*id),
        }
    }

    fn tick(&mut self, delta_ticks: u32) {
        logic::tick(&mut self.state, delta_ticks);
        self.save_countdown = self.save_countdown.saturating_sub(delta_ticks);
        if self.save_countdown == 0 {
            #[cfg(target_arch = "wasm32")]
            save::save_game(&self.state);
            self.save_countdown = save::AUTOSAVE_INTERVAL;
        }
    }

    fn on_leave(&mut self) {
        #[cfg(target_arch = "wasm32")]
        save::save_game(&self.state);
    }

    fn render(&self, f: &mut Frame, area: Rect, click_state: &Rc<RefCell<ClickState>>) {
        render::render(&self.state, f, area, click_state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::ClickScope;

    fn click(id: u16) -> InputEvent {
        InputEvent::Click(ClickScope::Game(GameChoice::StarRing), id)
    }

    #[test]
    fn buy_weapon_stat_via_key() {
        let mut game = StarRingGame::new();
        game.state.shards = 1000.0;
        game.state.tab = Tab::Armory;
        assert!(game.handle_input(&InputEvent::Key('d')));
        assert_eq!(
            game.state.weapon_stat(WeaponKind::Pulse, WeaponStat::Power),
            1
        );
    }

    #[test]
    fn tab_switch_via_click() {
        let mut game = StarRingGame::new();
        assert_eq!(game.state.tab, Tab::Armory);
        game.handle_input(&click(TAB_CODEX));
        assert_eq!(game.state.tab, Tab::Codex);
        game.handle_input(&click(TAB_RING));
        assert_eq!(game.state.tab, Tab::Ring);
        game.handle_input(&click(TAB_ARMORY));
        assert_eq!(game.state.tab, Tab::Armory);
    }

    #[test]
    fn strike_via_space() {
        let mut game = StarRingGame::new();
        game.handle_input(&InputEvent::Key(' '));
        assert!(game.state.boost_ticks > 0);
    }

    #[test]
    fn tick_advances() {
        let mut game = StarRingGame::new();
        game.tick(50);
        assert_eq!(game.state.elapsed_ticks, 50);
    }

    #[test]
    fn weapon_cycle_via_brackets() {
        let mut game = StarRingGame::new();
        game.state.shards = 1e9;
        game.state.total_kills = state::Layer::THRESHOLDS[2];
        assert!(logic::unlock_next_layer(&mut game.state));
        assert!(logic::unlock_next_layer(&mut game.state));
        game.state.selected_weapon = WeaponKind::Pulse;
        game.handle_input(&InputEvent::Key(']'));
        assert_eq!(game.state.selected_weapon, WeaponKind::Ray);
    }

    #[test]
    fn open_layer_via_key_and_click() {
        let mut game = StarRingGame::new();
        game.state.total_kills = state::Layer::THRESHOLDS[1];
        game.state.shards = 1e9;
        assert!(game.handle_input(&InputEvent::Key('!')));
        assert_eq!(game.state.layer(), 2);

        game.state.total_kills = state::Layer::THRESHOLDS[2];
        game.state.shards = 1e9;
        assert!(game.handle_input(&click(OPEN_LAYER)));
        assert_eq!(game.state.layer(), 3);
    }
}
