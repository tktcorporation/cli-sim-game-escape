//! Dungeon Dive — room-by-room dungeon crawler.
//!
//! Game trait implementation with simplified input dispatch.
//! All choices use [1]-[5], overlays use [I]/[S], back uses [0].

pub mod actions;
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
use state::{BattlePhase, Overlay, RoomKind, RpgState, Scene};

pub struct RpgGame {
    state: RpgState,
}

impl RpgGame {
    pub fn new() -> Self {
        Self {
            state: RpgState::new(),
        }
    }
}

impl Game for RpgGame {
    fn handle_input(&mut self, event: &InputEvent) -> bool {
        match event {
            InputEvent::Key(ch) => handle_key(&mut self.state, *ch),
            InputEvent::Click(id) => handle_click(&mut self.state, *id),
        }
    }

    fn tick(&mut self, delta_ticks: u32) {
        logic::tick(&mut self.state, delta_ticks);
    }

    fn render(&self, f: &mut Frame, area: Rect, click_state: &Rc<RefCell<ClickState>>) {
        render::render(&self.state, f, area, click_state);
    }
}

// ── Input Handling ──────────────────────────────────────────

fn handle_key(state: &mut RpgState, ch: char) -> bool {
    // Overlay close
    if state.overlay.is_some() {
        return handle_overlay_key(state, ch);
    }

    match state.scene {
        Scene::Intro(_) => handle_intro_key(state, ch),
        Scene::Town => handle_town_key(state, ch),
        Scene::Dungeon => handle_dungeon_key(state, ch),
        Scene::DungeonResult => handle_dungeon_result_key(state, ch),
        Scene::Battle => handle_battle_key(state, ch),
        Scene::GameClear => handle_game_clear_key(state, ch),
    }
}

fn handle_click(state: &mut RpgState, id: u16) -> bool {
    if state.overlay.is_some() {
        return handle_overlay_click(state, id);
    }

    match state.scene {
        Scene::Intro(_) => handle_intro_click(state, id),
        Scene::Town => handle_town_click(state, id),
        Scene::Dungeon => handle_dungeon_click(state, id),
        Scene::DungeonResult => handle_dungeon_result_click(state, id),
        Scene::Battle => handle_battle_click(state, id),
        Scene::GameClear => handle_game_clear_click(state, id),
    }
}

// ── Intro ──────────────────────────────────────────────────

fn handle_intro_key(state: &mut RpgState, ch: char) -> bool {
    match ch {
        '1' | '2' | ' ' => {
            logic::advance_intro(state);
            true
        }
        _ => false,
    }
}

fn handle_intro_click(state: &mut RpgState, id: u16) -> bool {
    if (CHOICE_BASE..CHOICE_BASE + 5).contains(&id) {
        logic::advance_intro(state);
        return true;
    }
    false
}

// ── Town ───────────────────────────────────────────────────

fn handle_town_key(state: &mut RpgState, ch: char) -> bool {
    let choice_index = match ch {
        '1' => Some(0),
        '2' => Some(1),
        '3' => Some(2),
        '4' => Some(3),
        '5' => Some(4),
        _ => None,
    };
    if let Some(idx) = choice_index {
        return logic::execute_town_choice(state, idx);
    }

    match ch {
        'I' | 'i' => {
            state.overlay = Some(Overlay::Inventory);
            true
        }
        'S' | 's' => {
            state.overlay = Some(Overlay::Status);
            true
        }
        _ => false,
    }
}

fn handle_town_click(state: &mut RpgState, id: u16) -> bool {
    if (CHOICE_BASE..CHOICE_BASE + 10).contains(&id) {
        let index = (id - CHOICE_BASE) as usize;
        return logic::execute_town_choice(state, index);
    }
    false
}

// ── Dungeon ────────────────────────────────────────────────

fn handle_dungeon_key(state: &mut RpgState, ch: char) -> bool {
    match ch {
        '1' => {
            // Advance: enter the current room
            logic::enter_current_room(state);
            true
        }
        '2' => {
            // Retreat to town
            logic::retreat_to_town(state);
            true
        }
        'I' | 'i' => {
            state.overlay = Some(Overlay::Inventory);
            true
        }
        'S' | 's' => {
            state.overlay = Some(Overlay::Status);
            true
        }
        _ => false,
    }
}

fn handle_dungeon_click(state: &mut RpgState, id: u16) -> bool {
    if id == CHOICE_BASE {
        logic::enter_current_room(state);
        return true;
    }
    if id == CHOICE_BASE + 1 {
        logic::retreat_to_town(state);
        return true;
    }
    false
}

// ── Dungeon Result ─────────────────────────────────────────

fn handle_dungeon_result_key(state: &mut RpgState, ch: char) -> bool {
    let is_stairs = state
        .dungeon
        .as_ref()
        .and_then(|d| d.rooms.get(d.current_room))
        .map(|r| r.kind == RoomKind::Stairs)
        .unwrap_or(false);
    let is_dead = state.hp == 0;

    match ch {
        '1' => {
            if is_dead {
                logic::advance_room(state); // Will trigger death handling
            } else if is_stairs {
                logic::descend_floor(state);
            } else {
                logic::advance_room(state);
            }
            true
        }
        '2' => {
            if !is_dead {
                logic::retreat_to_town(state);
                true
            } else {
                false
            }
        }
        'I' | 'i' => {
            if !is_dead {
                state.overlay = Some(Overlay::Inventory);
                true
            } else {
                false
            }
        }
        'S' | 's' => {
            if !is_dead {
                state.overlay = Some(Overlay::Status);
                true
            } else {
                false
            }
        }
        _ => false,
    }
}

fn handle_dungeon_result_click(state: &mut RpgState, id: u16) -> bool {
    let is_stairs = state
        .dungeon
        .as_ref()
        .and_then(|d| d.rooms.get(d.current_room))
        .map(|r| r.kind == RoomKind::Stairs)
        .unwrap_or(false);
    let is_dead = state.hp == 0;

    if id == CHOICE_BASE {
        if is_dead {
            logic::advance_room(state);
        } else if is_stairs {
            logic::descend_floor(state);
        } else {
            logic::advance_room(state);
        }
        return true;
    }
    if id == CHOICE_BASE + 1 && !is_dead {
        logic::retreat_to_town(state);
        return true;
    }
    false
}

// ── Battle ──────────────────────────────────────────────────

fn handle_battle_key(state: &mut RpgState, ch: char) -> bool {
    let battle = match &state.battle {
        Some(b) => b,
        None => return false,
    };

    match battle.phase {
        BattlePhase::SelectAction => match ch {
            '1' => logic::battle_attack(state),
            '2' => {
                if !logic::available_skills(state.level).is_empty() {
                    if let Some(b) = &mut state.battle {
                        b.phase = BattlePhase::SelectSkill;
                    }
                    true
                } else {
                    false
                }
            }
            '3' => {
                if !logic::battle_consumables(state).is_empty() {
                    if let Some(b) = &mut state.battle {
                        b.phase = BattlePhase::SelectItem;
                    }
                    true
                } else {
                    false
                }
            }
            '4' => logic::battle_flee(state),
            _ => false,
        },
        BattlePhase::SelectSkill => match ch {
            '0' | '-' => {
                if let Some(b) = &mut state.battle {
                    b.phase = BattlePhase::SelectAction;
                }
                true
            }
            '1'..='9' => {
                let idx = (ch as u32 - '1' as u32) as usize;
                logic::battle_use_skill(state, idx)
            }
            _ => false,
        },
        BattlePhase::SelectItem => match ch {
            '0' | '-' => {
                if let Some(b) = &mut state.battle {
                    b.phase = BattlePhase::SelectAction;
                }
                true
            }
            '1'..='9' => {
                let idx = (ch as u32 - '1' as u32) as usize;
                logic::battle_use_item(state, idx)
            }
            _ => false,
        },
        BattlePhase::Victory => {
            if ch == '1' || ch == ' ' {
                logic::process_victory(state);
                true
            } else {
                false
            }
        }
        BattlePhase::Defeat => {
            if ch == '1' || ch == ' ' {
                logic::process_defeat(state);
                true
            } else {
                false
            }
        }
        BattlePhase::Fled => {
            if ch == '1' || ch == ' ' {
                logic::process_fled(state);
                true
            } else {
                false
            }
        }
    }
}

fn handle_battle_click(state: &mut RpgState, id: u16) -> bool {
    let battle = match &state.battle {
        Some(b) => b,
        None => return false,
    };

    match battle.phase {
        BattlePhase::SelectAction => {
            if id == CHOICE_BASE {
                return logic::battle_attack(state);
            }
            if id == CHOICE_BASE + 1 && !logic::available_skills(state.level).is_empty() {
                if let Some(b) = &mut state.battle {
                    b.phase = BattlePhase::SelectSkill;
                }
                return true;
            }
            if id == CHOICE_BASE + 2 && !logic::battle_consumables(state).is_empty() {
                if let Some(b) = &mut state.battle {
                    b.phase = BattlePhase::SelectItem;
                }
                return true;
            }
            if id == CHOICE_BASE + 3 {
                return logic::battle_flee(state);
            }
            false
        }
        BattlePhase::SelectSkill => {
            if id == BATTLE_BACK {
                if let Some(b) = &mut state.battle {
                    b.phase = BattlePhase::SelectAction;
                }
                return true;
            }
            if (SKILL_BASE..SKILL_BASE + 10).contains(&id) {
                return logic::battle_use_skill(state, (id - SKILL_BASE) as usize);
            }
            false
        }
        BattlePhase::SelectItem => {
            if id == BATTLE_BACK {
                if let Some(b) = &mut state.battle {
                    b.phase = BattlePhase::SelectAction;
                }
                return true;
            }
            if (BATTLE_ITEM_BASE..BATTLE_ITEM_BASE + 10).contains(&id) {
                return logic::battle_use_item(state, (id - BATTLE_ITEM_BASE) as usize);
            }
            false
        }
        BattlePhase::Victory => {
            if id == CHOICE_BASE {
                logic::process_victory(state);
                true
            } else {
                false
            }
        }
        BattlePhase::Defeat => {
            if id == CHOICE_BASE {
                logic::process_defeat(state);
                true
            } else {
                false
            }
        }
        BattlePhase::Fled => {
            if id == CHOICE_BASE {
                logic::process_fled(state);
                true
            } else {
                false
            }
        }
    }
}

// ── Overlays ────────────────────────────────────────────────

fn handle_overlay_key(state: &mut RpgState, ch: char) -> bool {
    match state.overlay {
        Some(Overlay::Inventory) => match ch {
            '0' | '-' => {
                state.overlay = None;
                true
            }
            '1'..='9' => {
                let idx = (ch as u32 - '1' as u32) as usize;
                logic::use_item(state, idx)
            }
            _ => false,
        },
        Some(Overlay::Shop) => match ch {
            '0' | '-' => {
                state.overlay = None;
                true
            }
            '1'..='9' => {
                let idx = (ch as u32 - '1' as u32) as usize;
                logic::buy_item(state, idx)
            }
            _ => false,
        },
        Some(Overlay::Status) => {
            if ch == '0' || ch == '-' {
                state.overlay = None;
                true
            } else {
                false
            }
        }
        None => false,
    }
}

fn handle_overlay_click(state: &mut RpgState, id: u16) -> bool {
    if id == CLOSE_OVERLAY {
        state.overlay = None;
        return true;
    }

    match state.overlay {
        Some(Overlay::Inventory) => {
            if (INV_USE_BASE..INV_USE_BASE + 20).contains(&id) {
                return logic::use_item(state, (id - INV_USE_BASE) as usize);
            }
            false
        }
        Some(Overlay::Shop) => {
            if (SHOP_BUY_BASE..SHOP_BUY_BASE + 20).contains(&id) {
                return logic::buy_item(state, (id - SHOP_BUY_BASE) as usize);
            }
            false
        }
        _ => false,
    }
}

// ── Game Clear ──────────────────────────────────────────────

fn handle_game_clear_key(state: &mut RpgState, ch: char) -> bool {
    let _ = state;
    ch == '1' || ch == ' '
}

fn handle_game_clear_click(_state: &mut RpgState, id: u16) -> bool {
    id == CHOICE_BASE
}

// ── Tests ───────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_game() -> RpgGame {
        RpgGame::new()
    }

    #[test]
    fn intro_sequence() {
        let mut g = make_game();
        assert_eq!(g.state.scene, Scene::Intro(0));
        g.handle_input(&InputEvent::Key('1')); // step 0 -> 1
        assert_eq!(g.state.scene, Scene::Intro(1));
        g.handle_input(&InputEvent::Key('1')); // step 1 -> Town
        assert_eq!(g.state.scene, Scene::Town);
        assert!(g.state.weapon.is_some());
    }

    #[test]
    fn town_enter_dungeon() {
        let mut g = make_game();
        // Skip intro
        g.handle_input(&InputEvent::Key('1'));
        g.handle_input(&InputEvent::Key('1'));
        assert_eq!(g.state.scene, Scene::Town);
        // Enter dungeon
        g.handle_input(&InputEvent::Key('1'));
        assert_eq!(g.state.scene, Scene::Dungeon);
        assert!(g.state.dungeon.is_some());
    }

    #[test]
    fn dungeon_advance_room() {
        let mut g = make_game();
        g.handle_input(&InputEvent::Key('1'));
        g.handle_input(&InputEvent::Key('1'));
        g.handle_input(&InputEvent::Key('1')); // Enter dungeon
        assert_eq!(g.state.scene, Scene::Dungeon);
        // Press 1 to advance into room
        g.handle_input(&InputEvent::Key('1'));
        // Should be in battle, dungeon result, or still dungeon depending on room type
        assert_ne!(g.state.scene, Scene::Town);
    }

    #[test]
    fn dungeon_retreat() {
        let mut g = make_game();
        g.handle_input(&InputEvent::Key('1'));
        g.handle_input(&InputEvent::Key('1'));
        g.handle_input(&InputEvent::Key('1')); // Enter dungeon
        // Press 2 to retreat
        g.handle_input(&InputEvent::Key('2'));
        assert_eq!(g.state.scene, Scene::Town);
        assert!(g.state.dungeon.is_none());
    }

    #[test]
    fn overlay_open_close() {
        let mut g = make_game();
        g.handle_input(&InputEvent::Key('1'));
        g.handle_input(&InputEvent::Key('1'));
        g.handle_input(&InputEvent::Key('I'));
        assert_eq!(g.state.overlay, Some(Overlay::Inventory));
        g.handle_input(&InputEvent::Key('0'));
        assert_eq!(g.state.overlay, None);
    }

    #[test]
    fn battle_flow() {
        let mut g = make_game();
        g.handle_input(&InputEvent::Key('1'));
        g.handle_input(&InputEvent::Key('1'));
        // Force a battle
        logic::enter_dungeon(&mut g.state, 1);
        logic::start_battle(&mut g.state, state::EnemyKind::Slime, false);
        assert_eq!(g.state.scene, Scene::Battle);
        // Attack
        g.handle_input(&InputEvent::Key('1'));
        // Battle should have progressed
        assert!(g.state.battle.as_ref().unwrap().log.len() > 1);
    }

    #[test]
    fn click_intro() {
        let mut g = make_game();
        g.handle_input(&InputEvent::Click(CHOICE_BASE));
        assert_eq!(g.state.scene, Scene::Intro(1));
    }

    #[test]
    fn click_town_choice() {
        let mut g = make_game();
        g.handle_input(&InputEvent::Key('1'));
        g.handle_input(&InputEvent::Key('1'));
        let result = g.handle_input(&InputEvent::Click(CHOICE_BASE));
        assert!(result);
    }

    #[test]
    fn shop_overlay_buy() {
        let mut g = make_game();
        g.handle_input(&InputEvent::Key('1'));
        g.handle_input(&InputEvent::Key('1'));
        g.state.overlay = Some(Overlay::Shop);
        g.state.gold = 200;
        g.handle_input(&InputEvent::Key('1'));
        assert!(g.state.gold < 200);
    }
}
