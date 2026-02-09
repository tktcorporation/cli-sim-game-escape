//! RPG Quest — scene-based story RPG.
//!
//! Game trait implementation with simplified input dispatch.
//! All choices use [1]-[5], overlays use [I]/[S]/[Q], back uses [0].

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
use state::{BattlePhase, Overlay, RpgState, Scene};

pub struct RpgGame {
    state: RpgState,
}

impl RpgGame {
    pub fn new() -> Self {
        Self { state: RpgState::new() }
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
        Scene::Prologue(_) => handle_prologue_key(state, ch),
        Scene::World => handle_world_key(state, ch),
        Scene::Battle => handle_battle_key(state, ch),
        Scene::GameClear => handle_game_clear_key(state, ch),
    }
}

fn handle_click(state: &mut RpgState, id: u16) -> bool {
    // Overlay close
    if state.overlay.is_some() {
        return handle_overlay_click(state, id);
    }

    match state.scene {
        Scene::Prologue(_) => handle_prologue_click(state, id),
        Scene::World => handle_world_click(state, id),
        Scene::Battle => handle_battle_click(state, id),
        Scene::GameClear => handle_game_clear_click(state, id),
    }
}

// ── Prologue ────────────────────────────────────────────────

fn handle_prologue_key(state: &mut RpgState, ch: char) -> bool {
    match ch {
        '1' | '2' | ' ' => { logic::advance_prologue(state); true }
        _ => false,
    }
}

fn handle_prologue_click(state: &mut RpgState, id: u16) -> bool {
    if (CHOICE_BASE..CHOICE_BASE + 5).contains(&id) {
        logic::advance_prologue(state);
        return true;
    }
    false
}

// ── World ───────────────────────────────────────────────────

fn handle_world_key(state: &mut RpgState, ch: char) -> bool {
    // Number choices 1-5
    let choice_index = match ch {
        '1' => Some(0), '2' => Some(1), '3' => Some(2),
        '4' => Some(3), '5' => Some(4),
        _ => None,
    };
    if let Some(idx) = choice_index {
        return logic::execute_world_choice(state, idx);
    }

    // Overlay shortcuts
    match ch {
        'I' | 'i' => {
            if state.unlocks.inventory_shortcut {
                state.overlay = Some(Overlay::Inventory);
                return true;
            }
        }
        'S' | 's' => {
            if state.unlocks.status_shortcut {
                state.overlay = Some(Overlay::Status);
                return true;
            }
        }
        'Q' | 'q' => {
            if state.unlocks.quest_log_shortcut {
                state.overlay = Some(Overlay::QuestLog);
                return true;
            }
        }
        _ => {}
    }
    false
}

fn handle_world_click(state: &mut RpgState, id: u16) -> bool {
    if (CHOICE_BASE..CHOICE_BASE + 10).contains(&id) {
        let index = (id - CHOICE_BASE) as usize;
        return logic::execute_world_choice(state, index);
    }
    false
}

// ── Battle ──────────────────────────────────────────────────

fn handle_battle_key(state: &mut RpgState, ch: char) -> bool {
    let battle = match &state.battle { Some(b) => b, None => return false };

    match battle.phase {
        BattlePhase::SelectAction => {
            match ch {
                '1' => logic::battle_attack(state),
                '2' => {
                    if !logic::available_skills(state.level).is_empty() {
                        if let Some(b) = &mut state.battle {
                            b.phase = BattlePhase::SelectSkill;
                        }
                        true
                    } else { false }
                }
                '3' => {
                    if !logic::battle_consumables(state).is_empty() {
                        if let Some(b) = &mut state.battle {
                            b.phase = BattlePhase::SelectItem;
                        }
                        true
                    } else { false }
                }
                '4' => logic::battle_flee(state),
                _ => false,
            }
        }
        BattlePhase::SelectSkill => {
            match ch {
                '0' | '-' => {
                    if let Some(b) = &mut state.battle { b.phase = BattlePhase::SelectAction; }
                    true
                }
                '1'..='9' => {
                    let idx = (ch as u32 - '1' as u32) as usize;
                    logic::battle_use_skill(state, idx)
                }
                _ => false,
            }
        }
        BattlePhase::SelectItem => {
            match ch {
                '0' | '-' => {
                    if let Some(b) = &mut state.battle { b.phase = BattlePhase::SelectAction; }
                    true
                }
                '1'..='9' => {
                    let idx = (ch as u32 - '1' as u32) as usize;
                    logic::battle_use_item(state, idx)
                }
                _ => false,
            }
        }
        BattlePhase::Victory => {
            if ch == '1' || ch == ' ' { logic::process_victory(state); true }
            else { false }
        }
        BattlePhase::Defeat => {
            if ch == '1' || ch == ' ' { logic::process_defeat(state); true }
            else { false }
        }
        BattlePhase::Fled => {
            if ch == '1' || ch == ' ' { logic::process_fled(state); true }
            else { false }
        }
    }
}

fn handle_battle_click(state: &mut RpgState, id: u16) -> bool {
    let battle = match &state.battle { Some(b) => b, None => return false };

    match battle.phase {
        BattlePhase::SelectAction => {
            if id == CHOICE_BASE { return logic::battle_attack(state); }
            if id == CHOICE_BASE + 1
                && !logic::available_skills(state.level).is_empty()
            {
                if let Some(b) = &mut state.battle { b.phase = BattlePhase::SelectSkill; }
                return true;
            }
            if id == CHOICE_BASE + 2
                && !logic::battle_consumables(state).is_empty()
            {
                if let Some(b) = &mut state.battle { b.phase = BattlePhase::SelectItem; }
                return true;
            }
            if id == CHOICE_BASE + 3 { return logic::battle_flee(state); }
            false
        }
        BattlePhase::SelectSkill => {
            if id == BATTLE_BACK {
                if let Some(b) = &mut state.battle { b.phase = BattlePhase::SelectAction; }
                return true;
            }
            if (SKILL_BASE..SKILL_BASE + 10).contains(&id) {
                return logic::battle_use_skill(state, (id - SKILL_BASE) as usize);
            }
            false
        }
        BattlePhase::SelectItem => {
            if id == BATTLE_BACK {
                if let Some(b) = &mut state.battle { b.phase = BattlePhase::SelectAction; }
                return true;
            }
            if (BATTLE_ITEM_BASE..BATTLE_ITEM_BASE + 10).contains(&id) {
                return logic::battle_use_item(state, (id - BATTLE_ITEM_BASE) as usize);
            }
            false
        }
        BattlePhase::Victory => {
            if id == CHOICE_BASE { logic::process_victory(state); true }
            else { false }
        }
        BattlePhase::Defeat => {
            if id == CHOICE_BASE { logic::process_defeat(state); true }
            else { false }
        }
        BattlePhase::Fled => {
            if id == CHOICE_BASE { logic::process_fled(state); true }
            else { false }
        }
    }
}

// ── Overlays ────────────────────────────────────────────────

fn handle_overlay_key(state: &mut RpgState, ch: char) -> bool {
    match state.overlay {
        Some(Overlay::Inventory) => {
            match ch {
                '0' | '-' => { state.overlay = None; true }
                '1'..='9' => {
                    let idx = (ch as u32 - '1' as u32) as usize;
                    logic::use_item(state, idx)
                }
                _ => false,
            }
        }
        Some(Overlay::Shop) => {
            match ch {
                '0' | '-' => { state.overlay = None; true }
                '1'..='9' => {
                    let idx = (ch as u32 - '1' as u32) as usize;
                    logic::buy_item(state, idx)
                }
                _ => false,
            }
        }
        Some(Overlay::QuestLog) | Some(Overlay::Status) => {
            if ch == '0' || ch == '-' { state.overlay = None; true }
            else { false }
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
    fn prologue_sequence() {
        let mut g = make_game();
        assert_eq!(g.state.scene, Scene::Prologue(0));
        g.handle_input(&InputEvent::Key('1')); // step 0 -> 1
        assert_eq!(g.state.scene, Scene::Prologue(1));
        g.handle_input(&InputEvent::Key('1')); // step 1 -> 2
        assert_eq!(g.state.scene, Scene::Prologue(2));
        g.handle_input(&InputEvent::Key('1')); // step 2 -> World
        assert_eq!(g.state.scene, Scene::World);
        assert!(g.state.unlocks.status_bar);
    }

    #[test]
    fn world_choice_talk_npc() {
        let mut g = make_game();
        // Skip prologue
        for _ in 0..3 { g.handle_input(&InputEvent::Key('1')); }
        assert_eq!(g.state.scene, Scene::World);
        // First choice at village should be talk NPC
        g.handle_input(&InputEvent::Key('1'));
        // Should trigger some action
        assert!(!g.state.log.is_empty());
    }

    #[test]
    fn overlay_open_close() {
        let mut g = make_game();
        for _ in 0..3 { g.handle_input(&InputEvent::Key('1')); }
        g.state.unlocks.inventory_shortcut = true;
        g.handle_input(&InputEvent::Key('I'));
        assert_eq!(g.state.overlay, Some(Overlay::Inventory));
        g.handle_input(&InputEvent::Key('0'));
        assert_eq!(g.state.overlay, None);
    }

    #[test]
    fn battle_flow() {
        let mut g = make_game();
        for _ in 0..3 { g.handle_input(&InputEvent::Key('1')); }
        // Force a battle
        logic::start_battle(&mut g.state, state::EnemyKind::Slime, false);
        assert_eq!(g.state.scene, Scene::Battle);
        // Attack
        g.handle_input(&InputEvent::Key('1'));
        // Battle should have progressed
        assert!(g.state.battle.as_ref().unwrap().log.len() > 1);
    }

    #[test]
    fn click_prologue() {
        let mut g = make_game();
        g.handle_input(&InputEvent::Click(CHOICE_BASE)); // step 0 -> 1
        assert_eq!(g.state.scene, Scene::Prologue(1));
    }

    #[test]
    fn click_world_choice() {
        let mut g = make_game();
        for _ in 0..3 { g.handle_input(&InputEvent::Key('1')); }
        let result = g.handle_input(&InputEvent::Click(CHOICE_BASE));
        assert!(result);
    }

    #[test]
    fn shop_overlay_buy() {
        let mut g = make_game();
        for _ in 0..3 { g.handle_input(&InputEvent::Key('1')); }
        g.state.overlay = Some(Overlay::Shop);
        g.state.gold = 200;
        g.handle_input(&InputEvent::Key('1'));
        assert!(g.state.gold < 200);
    }
}
