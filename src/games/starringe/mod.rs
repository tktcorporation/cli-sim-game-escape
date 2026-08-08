//! 星環 — 漂う鉱石を公転砲台と武装で砕く放置ゲーム。
//!
//! 中心の採掘コアを囲む砲台が楕円軌道を回り、外周を螺旋漂流する鉱石を
//! レーザー・脈動・穿光で砕いて星屑を得る。脅威の増加は層 (depth) 進行が
//! 担い、プレイヤー強化は純粋な火力・武装・収率に寄せる。

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

use actions::{upgrade_for_buy_id, TAB_CODEX, TAB_UPGRADES, TAP_STRIKE};
use state::{StarRingState, Tab, UpgradeKind};

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
            '1' => logic::purchase_upgrade(&mut self.state, UpgradeKind::Turrets),
            '2' => logic::purchase_upgrade(&mut self.state, UpgradeKind::Damage),
            '3' => logic::purchase_upgrade(&mut self.state, UpgradeKind::FireRate),
            '4' => logic::purchase_upgrade(&mut self.state, UpgradeKind::Pulse),
            '5' => logic::purchase_upgrade(&mut self.state, UpgradeKind::Lance),
            '6' => logic::purchase_upgrade(&mut self.state, UpgradeKind::Yield),
            '{' | 'u' | 'U' => {
                self.state.tab = Tab::Upgrades;
                true
            }
            '|' | 'c' | 'C' => {
                self.state.tab = Tab::Codex;
                true
            }
            ' ' | 'a' | 'A' => {
                logic::manual_strike(&mut self.state);
                true
            }
            _ => false,
        }
    }

    fn handle_click(&mut self, action_id: u16) -> bool {
        match action_id {
            TAB_UPGRADES => {
                self.state.tab = Tab::Upgrades;
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
            id => {
                if let Some(kind) = upgrade_for_buy_id(id) {
                    logic::purchase_upgrade(&mut self.state, kind);
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
    fn buy_upgrade_via_key() {
        let mut game = StarRingGame::new();
        game.state.shards = 1000.0;
        // 2 = 火力
        assert!(game.handle_input(&InputEvent::Key('2')));
        assert_eq!(game.state.level(UpgradeKind::Damage), 1);
    }

    #[test]
    fn tab_switch_via_click() {
        let mut game = StarRingGame::new();
        assert_eq!(game.state.tab, Tab::Upgrades);
        game.handle_input(&click(TAB_CODEX));
        assert_eq!(game.state.tab, Tab::Codex);
        game.handle_input(&click(TAB_UPGRADES));
        assert_eq!(game.state.tab, Tab::Upgrades);
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
}
