//! RPG Quest — a short-form RPG with extensible quest system.

pub mod actions;
pub mod logic;
pub mod render;
pub mod state;

use std::cell::RefCell;
use std::rc::Rc;

use ratzilla::ratatui::layout::Rect;
use ratzilla::ratatui::Frame;

use crate::input::{ClickState, InputEvent};

use super::Game;
use actions::*;
use state::{BattleAction, RpgState, Screen};

pub struct RpgGame {
    pub state: RpgState,
}

impl RpgGame {
    pub fn new() -> Self {
        Self {
            state: RpgState::new(),
        }
    }

    fn handle_key(&mut self, key: char) -> bool {
        match self.state.screen {
            Screen::World => self.handle_world_key(key),
            Screen::Battle => self.handle_battle_key(key),
            Screen::Inventory => self.handle_inventory_key(key),
            Screen::QuestLog => self.handle_back_key(key, Screen::World),
            Screen::Shop => self.handle_shop_key(key),
            Screen::Status => self.handle_back_key(key, Screen::World),
            Screen::Dialogue => self.handle_dialogue_key(key),
            Screen::GameClear => self.handle_game_clear_key(key),
        }
    }

    fn handle_click(&mut self, action_id: u16) -> bool {
        match self.state.screen {
            Screen::World => self.handle_world_click(action_id),
            Screen::Battle => self.handle_battle_click(action_id),
            Screen::Inventory => self.handle_inventory_click(action_id),
            Screen::QuestLog => self.handle_quest_log_click(action_id),
            Screen::Shop => self.handle_shop_click(action_id),
            Screen::Status => self.handle_status_click(action_id),
            Screen::Dialogue => self.handle_dialogue_click(action_id),
            Screen::GameClear => self.handle_game_clear_click(action_id),
        }
    }

    // ── World ────────────────────────────────────────────────

    fn handle_world_key(&mut self, key: char) -> bool {
        match key {
            '1' => logic::explore(&mut self.state),
            '2' => logic::talk_npc(&mut self.state),
            '3' => {
                if state::location_info(self.state.location).has_shop {
                    self.state.screen = Screen::Shop;
                    true
                } else {
                    false
                }
            }
            '4' => logic::rest(&mut self.state),
            '7' => {
                self.state.screen = Screen::Inventory;
                true
            }
            '8' => {
                self.state.screen = Screen::QuestLog;
                true
            }
            '9' => {
                self.state.screen = Screen::Status;
                true
            }
            // Travel keys: A=first, B=second, C=third, D=fourth
            'A' | 'a' => logic::travel(&mut self.state, 0),
            'B' | 'b' => logic::travel(&mut self.state, 1),
            'C' | 'c' => logic::travel(&mut self.state, 2),
            'D' | 'd' => logic::travel(&mut self.state, 3),
            _ => false,
        }
    }

    fn handle_world_click(&mut self, action_id: u16) -> bool {
        match action_id {
            EXPLORE => logic::explore(&mut self.state),
            TALK_NPC => logic::talk_npc(&mut self.state),
            GO_SHOP => {
                self.state.screen = Screen::Shop;
                true
            }
            REST => logic::rest(&mut self.state),
            GO_INVENTORY => {
                self.state.screen = Screen::Inventory;
                true
            }
            GO_QUEST_LOG => {
                self.state.screen = Screen::QuestLog;
                true
            }
            GO_STATUS => {
                self.state.screen = Screen::Status;
                true
            }
            id if (TRAVEL_BASE..TRAVEL_BASE + 10).contains(&id) => {
                let idx = (id - TRAVEL_BASE) as usize;
                logic::travel(&mut self.state, idx)
            }
            _ => false,
        }
    }

    // ── Battle ───────────────────────────────────────────────

    fn handle_battle_key(&mut self, key: char) -> bool {
        let action = self
            .state
            .battle
            .as_ref()
            .map(|b| b.action)
            .unwrap_or(BattleAction::SelectAction);

        match action {
            BattleAction::SelectAction => match key {
                '1' => logic::battle_attack(&mut self.state),
                '2' => {
                    if let Some(b) = &mut self.state.battle {
                        b.action = BattleAction::SelectSkill;
                    }
                    true
                }
                '3' => {
                    if let Some(b) = &mut self.state.battle {
                        b.action = BattleAction::SelectItem;
                    }
                    true
                }
                '4' => logic::battle_flee(&mut self.state),
                _ => false,
            },
            BattleAction::SelectSkill => match key {
                '1'..='9' => {
                    let idx = (key as u8 - b'1') as usize;
                    if let Some(b) = &mut self.state.battle {
                        b.action = BattleAction::SelectAction;
                    }
                    logic::battle_use_skill(&mut self.state, idx)
                }
                '-' => {
                    if let Some(b) = &mut self.state.battle {
                        b.action = BattleAction::SelectAction;
                    }
                    true
                }
                _ => false,
            },
            BattleAction::SelectItem => match key {
                '1'..='9' => {
                    let idx = (key as u8 - b'1') as usize;
                    if let Some(b) = &mut self.state.battle {
                        b.action = BattleAction::SelectAction;
                    }
                    logic::battle_use_item(&mut self.state, idx)
                }
                '-' => {
                    if let Some(b) = &mut self.state.battle {
                        b.action = BattleAction::SelectAction;
                    }
                    true
                }
                _ => false,
            },
            BattleAction::Victory => {
                if key == '0' {
                    logic::process_victory(&mut self.state);
                    true
                } else {
                    false
                }
            }
            BattleAction::Defeat => {
                if key == '0' {
                    logic::process_defeat(&mut self.state);
                    true
                } else {
                    false
                }
            }
            BattleAction::Fled => {
                if key == '0' {
                    logic::process_fled(&mut self.state);
                    true
                } else {
                    false
                }
            }
            BattleAction::EnemyTurn => false,
        }
    }

    fn handle_battle_click(&mut self, action_id: u16) -> bool {
        let action = self
            .state
            .battle
            .as_ref()
            .map(|b| b.action)
            .unwrap_or(BattleAction::SelectAction);

        match action {
            BattleAction::SelectAction => match action_id {
                BATTLE_ATTACK => logic::battle_attack(&mut self.state),
                BATTLE_SKILL => {
                    if let Some(b) = &mut self.state.battle {
                        b.action = BattleAction::SelectSkill;
                    }
                    true
                }
                BATTLE_ITEM => {
                    if let Some(b) = &mut self.state.battle {
                        b.action = BattleAction::SelectItem;
                    }
                    true
                }
                BATTLE_FLEE => logic::battle_flee(&mut self.state),
                _ => false,
            },
            BattleAction::SelectSkill => {
                if action_id == BACK_FROM_SKILL {
                    if let Some(b) = &mut self.state.battle {
                        b.action = BattleAction::SelectAction;
                    }
                    return true;
                }
                if (SKILL_SELECT_BASE..SKILL_SELECT_BASE + 10).contains(&action_id) {
                    let idx = (action_id - SKILL_SELECT_BASE) as usize;
                    if let Some(b) = &mut self.state.battle {
                        b.action = BattleAction::SelectAction;
                    }
                    return logic::battle_use_skill(&mut self.state, idx);
                }
                false
            }
            BattleAction::SelectItem => {
                if action_id == BACK_FROM_BATTLE_ITEM {
                    if let Some(b) = &mut self.state.battle {
                        b.action = BattleAction::SelectAction;
                    }
                    return true;
                }
                if (BATTLE_ITEM_BASE..BATTLE_ITEM_BASE + 10).contains(&action_id) {
                    let idx = (action_id - BATTLE_ITEM_BASE) as usize;
                    if let Some(b) = &mut self.state.battle {
                        b.action = BattleAction::SelectAction;
                    }
                    return logic::battle_use_item(&mut self.state, idx);
                }
                false
            }
            BattleAction::Victory => {
                if action_id == BATTLE_CONTINUE {
                    logic::process_victory(&mut self.state);
                    true
                } else {
                    false
                }
            }
            BattleAction::Defeat => {
                if action_id == BATTLE_CONTINUE {
                    logic::process_defeat(&mut self.state);
                    true
                } else {
                    false
                }
            }
            BattleAction::Fled => {
                if action_id == BATTLE_CONTINUE {
                    logic::process_fled(&mut self.state);
                    true
                } else {
                    false
                }
            }
            BattleAction::EnemyTurn => false,
        }
    }

    // ── Inventory ────────────────────────────────────────────

    fn handle_inventory_key(&mut self, key: char) -> bool {
        match key {
            '1'..='9' => {
                let idx = (key as u8 - b'1') as usize;
                logic::use_item(&mut self.state, idx)
            }
            '-' => {
                self.state.screen = Screen::World;
                true
            }
            _ => false,
        }
    }

    fn handle_inventory_click(&mut self, action_id: u16) -> bool {
        if action_id == BACK_FROM_INVENTORY {
            self.state.screen = Screen::World;
            return true;
        }
        if (INV_USE_BASE..INV_USE_BASE + 30).contains(&action_id) {
            let idx = (action_id - INV_USE_BASE) as usize;
            return logic::use_item(&mut self.state, idx);
        }
        false
    }

    // ── Shop ─────────────────────────────────────────────────

    fn handle_shop_key(&mut self, key: char) -> bool {
        match key {
            '1'..='9' => {
                let idx = (key as u8 - b'1') as usize;
                logic::buy_item(&mut self.state, idx)
            }
            '-' => {
                self.state.screen = Screen::World;
                true
            }
            _ => false,
        }
    }

    fn handle_shop_click(&mut self, action_id: u16) -> bool {
        if action_id == BACK_FROM_SHOP {
            self.state.screen = Screen::World;
            return true;
        }
        if (SHOP_BUY_BASE..SHOP_BUY_BASE + 20).contains(&action_id) {
            let idx = (action_id - SHOP_BUY_BASE) as usize;
            return logic::buy_item(&mut self.state, idx);
        }
        false
    }

    // ── Quest Log / Status (back only) ───────────────────────

    fn handle_quest_log_click(&mut self, action_id: u16) -> bool {
        if action_id == BACK_FROM_QUEST_LOG {
            self.state.screen = Screen::World;
            return true;
        }
        false
    }

    fn handle_status_click(&mut self, action_id: u16) -> bool {
        if action_id == BACK_FROM_STATUS {
            self.state.screen = Screen::World;
            return true;
        }
        false
    }

    fn handle_back_key(&mut self, key: char, target: Screen) -> bool {
        if key == '-' {
            self.state.screen = target;
            true
        } else {
            false
        }
    }

    // ── Dialogue ─────────────────────────────────────────────

    fn handle_dialogue_key(&mut self, key: char) -> bool {
        if key == '0' || key == ' ' {
            logic::advance_dialogue(&mut self.state)
        } else {
            false
        }
    }

    fn handle_dialogue_click(&mut self, action_id: u16) -> bool {
        if action_id == DIALOGUE_NEXT {
            logic::advance_dialogue(&mut self.state)
        } else {
            false
        }
    }

    // ── Game Clear ───────────────────────────────────────────

    fn handle_game_clear_key(&mut self, key: char) -> bool {
        if key == '0' {
            // Return false to go back to menu
            false
        } else {
            false
        }
    }

    fn handle_game_clear_click(&mut self, action_id: u16) -> bool {
        if action_id == GAME_CLEAR_CONTINUE {
            // Return false to go back to menu
            false
        } else {
            false
        }
    }
}

impl Game for RpgGame {
    fn handle_input(&mut self, event: &InputEvent) -> bool {
        match event {
            InputEvent::Key(c) => self.handle_key(*c),
            InputEvent::Click(id) => self.handle_click(*id),
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
    fn rpg_game_new() {
        let game = RpgGame::new();
        assert_eq!(game.state.level, 1);
        assert_eq!(game.state.screen, Screen::World);
    }

    #[test]
    fn rpg_game_talk_npc_key() {
        let mut game = RpgGame::new();
        // Talk to village elder - should complete MainPrepare
        assert!(game.handle_input(&InputEvent::Key('2')));
    }

    #[test]
    fn rpg_game_navigate_screens() {
        let mut game = RpgGame::new();
        // Go to inventory
        assert!(game.handle_input(&InputEvent::Key('7')));
        assert_eq!(game.state.screen, Screen::Inventory);
        // Go back
        assert!(game.handle_input(&InputEvent::Key('-')));
        assert_eq!(game.state.screen, Screen::World);
    }

    #[test]
    fn rpg_game_navigate_quest_log() {
        let mut game = RpgGame::new();
        assert!(game.handle_input(&InputEvent::Key('8')));
        assert_eq!(game.state.screen, Screen::QuestLog);
        assert!(game.handle_input(&InputEvent::Key('-')));
        assert_eq!(game.state.screen, Screen::World);
    }

    #[test]
    fn rpg_game_click_travel() {
        let mut game = RpgGame::new();
        game.state.rng_seed = 999; // Avoid encounters
        // Travel to forest (index 0)
        assert!(game.handle_input(&InputEvent::Click(TRAVEL_BASE)));
        assert_eq!(game.state.location, state::LocationId::Forest);
    }

    #[test]
    fn rpg_game_click_inventory() {
        let mut game = RpgGame::new();
        assert!(game.handle_input(&InputEvent::Click(GO_INVENTORY)));
        assert_eq!(game.state.screen, Screen::Inventory);
        assert!(game.handle_input(&InputEvent::Click(BACK_FROM_INVENTORY)));
        assert_eq!(game.state.screen, Screen::World);
    }

    #[test]
    fn rpg_game_shop() {
        let mut game = RpgGame::new();
        game.state.gold = 100;
        // Open shop
        assert!(game.handle_input(&InputEvent::Key('3')));
        assert_eq!(game.state.screen, Screen::Shop);
        // Buy herb (index 0, 20G)
        assert!(game.handle_input(&InputEvent::Key('1')));
        assert_eq!(game.state.gold, 80);
        // Back
        assert!(game.handle_input(&InputEvent::Key('-')));
        assert_eq!(game.state.screen, Screen::World);
    }

    #[test]
    fn rpg_game_battle_flow() {
        let mut game = RpgGame::new();
        game.state.weapon = Some(state::ItemKind::HolySword);
        logic::start_battle(&mut game.state, state::EnemyKind::Slime, false);
        assert_eq!(game.state.screen, Screen::Battle);

        // Attack
        assert!(game.handle_input(&InputEvent::Key('1')));
        // Slime should be dead with holy sword
        let action = game.state.battle.as_ref().map(|b| b.action);
        assert_eq!(action, Some(BattleAction::Victory));

        // Continue
        assert!(game.handle_input(&InputEvent::Key('0')));
        assert_eq!(game.state.screen, Screen::World);
    }
}
