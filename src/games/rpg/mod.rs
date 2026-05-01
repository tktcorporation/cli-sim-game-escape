//! Dungeon Dive — grid-based dungeon crawler with first-person 3D view.
//!
//! Game trait implementation with arrow-key movement and interactive events.
//! Movement: Arrow keys (1 step), map tap (auto-walk through corridors).
//! Events: numbered choices.  Overlays: inventory / status.

pub mod actions;
pub mod balance;
pub mod commands;
pub mod dungeon_map;
pub mod dungeon_view;
pub mod events;
pub mod logic;
pub mod lore;
pub mod render;
pub mod simulator;
pub mod state;

use std::cell::RefCell;
use std::rc::Rc;

use ratzilla::ratatui::layout::Rect;
use ratzilla::ratatui::Frame;

use crate::games::{Game, GameChoice};
use crate::input::{ClickState, InputEvent};

use actions::*;
use commands::{apply_action, PlayerAction};
use state::{BattlePhase, Overlay, RpgState, Scene};

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
    fn choice(&self) -> GameChoice {
        GameChoice::Rpg
    }

    fn handle_input(&mut self, event: &InputEvent) -> bool {
        match event {
            InputEvent::Key(ch) => handle_key(&mut self.state, *ch),
            InputEvent::Click(_, id) => handle_click(&mut self.state, *id),
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
        Scene::DungeonExplore => handle_dungeon_explore_key(state, ch),
        Scene::DungeonEvent => handle_dungeon_event_key(state, ch),
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
        Scene::DungeonExplore => handle_dungeon_explore_click(state, id),
        Scene::DungeonEvent => handle_dungeon_event_click(state, id),
        Scene::DungeonResult => handle_dungeon_result_click(state, id),
        Scene::Battle => handle_battle_click(state, id),
        Scene::GameClear => handle_game_clear_click(state, id),
    }
}

// ── Intro ──────────────────────────────────────────────────

fn handle_intro_key(state: &mut RpgState, ch: char) -> bool {
    match ch {
        '1' | '2' | ' ' => apply_action(state, PlayerAction::AdvanceIntro),
        _ => false,
    }
}

fn handle_intro_click(state: &mut RpgState, id: u16) -> bool {
    if (CHOICE_BASE..CHOICE_BASE + 5).contains(&id) {
        return apply_action(state, PlayerAction::AdvanceIntro);
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
        return apply_action(state, PlayerAction::TownChoice(idx));
    }

    match ch {
        'I' | 'i' => apply_action(state, PlayerAction::OpenInventory),
        'S' | 's' => apply_action(state, PlayerAction::OpenStatus),
        _ => false,
    }
}

fn handle_town_click(state: &mut RpgState, id: u16) -> bool {
    if (CHOICE_BASE..CHOICE_BASE + 10).contains(&id) {
        let index = (id - CHOICE_BASE) as usize;
        return apply_action(state, PlayerAction::TownChoice(index));
    }
    handle_overlay_open_click(state, id)
}

// ── Dungeon Explore (arrow key / map tap movement) ─────────

fn handle_dungeon_explore_key(state: &mut RpgState, ch: char) -> bool {
    match ch {
        // WASD / hjkl / arrow keys — all direct cardinal movement
        'W' | 'w' | 'k' => apply_action(state, PlayerAction::Move(state::Facing::North)),
        'A' | 'a' | 'h' => apply_action(state, PlayerAction::Move(state::Facing::West)),
        'S' | 's' | 'j' => apply_action(state, PlayerAction::Move(state::Facing::South)),
        'D' | 'd' | 'l' => apply_action(state, PlayerAction::Move(state::Facing::East)),
        // Overlays
        'I' | 'i' => apply_action(state, PlayerAction::OpenInventory),
        'X' | 'x' => apply_action(state, PlayerAction::OpenStatus),
        _ => false,
    }
}

fn handle_dungeon_explore_click(state: &mut RpgState, id: u16) -> bool {
    handle_dpad_tap(state, id)
        || handle_map_tap(state, id)
        || handle_overlay_open_click(state, id)
}

/// Handle a tap on the on-screen D-pad (1 step, no auto-walk).
fn handle_dpad_tap(state: &mut RpgState, id: u16) -> bool {
    use crate::widgets::ClickableGrid;
    let Some((col, row)) = ClickableGrid::decode(DPAD_BASE, 3, id) else {
        return false;
    };
    let dir = match (col, row) {
        (1, 0) => Some(state::Facing::North),
        (0, 1) => Some(state::Facing::West),
        (2, 1) => Some(state::Facing::East),
        (1, 2) => Some(state::Facing::South),
        _ => None,
    };
    match dir {
        Some(d) => apply_action(state, PlayerAction::Move(d)),
        None => false,
    }
}

/// Handle a tap on the 2D map. Decode the 3×3 grid position and move in
/// the corresponding absolute cardinal direction (with auto-walk).
fn handle_map_tap(state: &mut RpgState, id: u16) -> bool {
    use crate::widgets::ClickableGrid;
    let Some((col, row)) = ClickableGrid::decode(MAP_TAP_BASE, 3, id) else {
        return false;
    };
    // Map grid (col, row) to screen cardinal direction
    let screen_dir = match (col, row) {
        (_, 0) => Some(state::Facing::North),     // top row
        (0, 1) => Some(state::Facing::West),      // middle-left
        (2, 1) => Some(state::Facing::East),      // middle-right
        (_, 2) => Some(state::Facing::South),     // bottom row
        _ => None,                                 // center (1,1): no-op
    };
    match screen_dir {
        Some(dir) => apply_action(state, PlayerAction::MoveAuto(dir)),
        None => false,
    }
}

// ── Dungeon Event (Interactive Choices) ─────────────────────

fn handle_dungeon_event_key(state: &mut RpgState, ch: char) -> bool {
    let choice_index = match ch {
        '1' => Some(0),
        '2' => Some(1),
        '3' => Some(2),
        '4' => Some(3),
        '5' => Some(4),
        _ => None,
    };
    if let Some(idx) = choice_index {
        return apply_action(state, PlayerAction::PickEventChoice(idx));
    }

    match ch {
        'I' | 'i' => apply_action(state, PlayerAction::OpenInventory),
        _ => false,
    }
}

fn handle_dungeon_event_click(state: &mut RpgState, id: u16) -> bool {
    if (EVENT_CHOICE_BASE..EVENT_CHOICE_BASE + 10).contains(&id) {
        let index = (id - EVENT_CHOICE_BASE) as usize;
        return apply_action(state, PlayerAction::PickEventChoice(index));
    }
    handle_overlay_open_click(state, id)
}

// ── Dungeon Result ─────────────────────────────────────────

fn handle_dungeon_result_key(state: &mut RpgState, ch: char) -> bool {
    match ch {
        '1' | ' ' => apply_action(state, PlayerAction::ContinueExploration),
        'I' | 'i' => apply_action(state, PlayerAction::OpenInventory),
        'S' | 's' => apply_action(state, PlayerAction::OpenStatus),
        _ => false,
    }
}

fn handle_dungeon_result_click(state: &mut RpgState, id: u16) -> bool {
    if id == CHOICE_BASE {
        return apply_action(state, PlayerAction::ContinueExploration);
    }
    handle_overlay_open_click(state, id)
}

// ── Battle ──────────────────────────────────────────────────

fn handle_battle_key(state: &mut RpgState, ch: char) -> bool {
    let battle = match &state.battle {
        Some(b) => b,
        None => return false,
    };

    match battle.phase {
        BattlePhase::SelectAction => match ch {
            '1' => apply_action(state, PlayerAction::BattleAttack),
            '2' => apply_action(state, PlayerAction::BattleOpenSkillMenu),
            '3' => apply_action(state, PlayerAction::BattleOpenItemMenu),
            '4' => apply_action(state, PlayerAction::BattleFlee),
            _ => false,
        },
        BattlePhase::SelectSkill => match ch {
            '0' | '-' => apply_action(state, PlayerAction::BattleBackToActions),
            '1'..='9' => {
                let idx = (ch as u32 - '1' as u32) as usize;
                apply_action(state, PlayerAction::BattleUseSkill(idx))
            }
            _ => false,
        },
        BattlePhase::SelectItem => match ch {
            '0' | '-' => apply_action(state, PlayerAction::BattleBackToActions),
            '1'..='9' => {
                let idx = (ch as u32 - '1' as u32) as usize;
                apply_action(state, PlayerAction::BattleUseItem(idx))
            }
            _ => false,
        },
        BattlePhase::Victory | BattlePhase::Defeat | BattlePhase::Fled => {
            if ch == '1' || ch == ' ' {
                apply_action(state, PlayerAction::BattleAcknowledgeOutcome)
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
                return apply_action(state, PlayerAction::BattleAttack);
            }
            if id == CHOICE_BASE + 1 {
                return apply_action(state, PlayerAction::BattleOpenSkillMenu);
            }
            if id == CHOICE_BASE + 2 {
                return apply_action(state, PlayerAction::BattleOpenItemMenu);
            }
            if id == CHOICE_BASE + 3 {
                return apply_action(state, PlayerAction::BattleFlee);
            }
            false
        }
        BattlePhase::SelectSkill => {
            if id == BATTLE_BACK {
                return apply_action(state, PlayerAction::BattleBackToActions);
            }
            if (SKILL_BASE..SKILL_BASE + 10).contains(&id) {
                return apply_action(
                    state,
                    PlayerAction::BattleUseSkill((id - SKILL_BASE) as usize),
                );
            }
            false
        }
        BattlePhase::SelectItem => {
            if id == BATTLE_BACK {
                return apply_action(state, PlayerAction::BattleBackToActions);
            }
            if (BATTLE_ITEM_BASE..BATTLE_ITEM_BASE + 10).contains(&id) {
                return apply_action(
                    state,
                    PlayerAction::BattleUseItem((id - BATTLE_ITEM_BASE) as usize),
                );
            }
            false
        }
        BattlePhase::Victory | BattlePhase::Defeat | BattlePhase::Fled => {
            if id == CHOICE_BASE {
                apply_action(state, PlayerAction::BattleAcknowledgeOutcome)
            } else {
                false
            }
        }
    }
}

// ── Overlay open (shared by town / dungeon) ─────────────────

fn handle_overlay_open_click(state: &mut RpgState, id: u16) -> bool {
    match id {
        OPEN_INVENTORY => apply_action(state, PlayerAction::OpenInventory),
        OPEN_STATUS => apply_action(state, PlayerAction::OpenStatus),
        _ => false,
    }
}

// ── Overlays ────────────────────────────────────────────────

fn handle_overlay_key(state: &mut RpgState, ch: char) -> bool {
    match state.overlay {
        Some(Overlay::Inventory) => match ch {
            '0' | '-' => apply_action(state, PlayerAction::CloseOverlay),
            '1'..='9' => {
                let idx = (ch as u32 - '1' as u32) as usize;
                apply_action(state, PlayerAction::UseInventoryItem(idx))
            }
            _ => false,
        },
        Some(Overlay::Shop) => match ch {
            '0' | '-' => apply_action(state, PlayerAction::CloseOverlay),
            '1'..='9' => {
                let idx = (ch as u32 - '1' as u32) as usize;
                apply_action(state, PlayerAction::BuyShopItem(idx))
            }
            _ => false,
        },
        Some(Overlay::Status) => {
            if ch == '0' || ch == '-' {
                apply_action(state, PlayerAction::CloseOverlay)
            } else {
                false
            }
        }
        None => false,
    }
}

fn handle_overlay_click(state: &mut RpgState, id: u16) -> bool {
    if id == CLOSE_OVERLAY {
        return apply_action(state, PlayerAction::CloseOverlay);
    }

    match state.overlay {
        Some(Overlay::Inventory) => {
            if (INV_USE_BASE..INV_USE_BASE + 20).contains(&id) {
                return apply_action(
                    state,
                    PlayerAction::UseInventoryItem((id - INV_USE_BASE) as usize),
                );
            }
            false
        }
        Some(Overlay::Shop) => {
            if (SHOP_BUY_BASE..SHOP_BUY_BASE + 20).contains(&id) {
                return apply_action(
                    state,
                    PlayerAction::BuyShopItem((id - SHOP_BUY_BASE) as usize),
                );
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
    use crate::input::ClickScope;

    fn make_game() -> RpgGame {
        RpgGame::new()
    }

    /// Build a `Click` event scoped to this game.
    fn click(id: u16) -> InputEvent {
        InputEvent::Click(ClickScope::Game(GameChoice::Rpg), id)
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
        assert_eq!(g.state.scene, Scene::DungeonExplore);
        assert!(g.state.dungeon.is_some());
    }

    #[test]
    fn dungeon_wasd_movement() {
        let mut g = make_game();
        g.handle_input(&InputEvent::Key('1'));
        g.handle_input(&InputEvent::Key('1'));
        g.handle_input(&InputEvent::Key('1')); // Enter dungeon
        assert_eq!(g.state.scene, Scene::DungeonExplore);

        // Try moving in any direction (WASD are now direct cardinal movement)
        // Try all directions, at least one should work (player starts in a room)
        g.handle_input(&InputEvent::Key('W'));
        g.handle_input(&InputEvent::Key('D'));
        g.handle_input(&InputEvent::Key('A'));
        g.handle_input(&InputEvent::Key('S'));
        // Just verify we didn't crash; exact position depends on layout
    }

    #[test]
    fn dungeon_retreat_via_logic() {
        let mut g = make_game();
        g.handle_input(&InputEvent::Key('1'));
        g.handle_input(&InputEvent::Key('1'));
        g.handle_input(&InputEvent::Key('1')); // Enter dungeon

        // Test retreat via logic directly
        logic::retreat_to_town(&mut g.state);
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
        g.handle_input(&click(CHOICE_BASE));
        assert_eq!(g.state.scene, Scene::Intro(1));
    }

    #[test]
    fn click_town_choice() {
        let mut g = make_game();
        g.handle_input(&InputEvent::Key('1'));
        g.handle_input(&InputEvent::Key('1'));
        let result = g.handle_input(&click(CHOICE_BASE));
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

    #[test]
    fn click_town_open_inventory() {
        let mut g = make_game();
        g.handle_input(&InputEvent::Key('1'));
        g.handle_input(&InputEvent::Key('1'));
        assert_eq!(g.state.scene, Scene::Town);
        let result = g.handle_input(&click(OPEN_INVENTORY));
        assert!(result);
        assert_eq!(g.state.overlay, Some(Overlay::Inventory));
    }

    #[test]
    fn click_town_open_status() {
        let mut g = make_game();
        g.handle_input(&InputEvent::Key('1'));
        g.handle_input(&InputEvent::Key('1'));
        assert_eq!(g.state.scene, Scene::Town);
        let result = g.handle_input(&click(OPEN_STATUS));
        assert!(result);
        assert_eq!(g.state.overlay, Some(Overlay::Status));
    }

    #[test]
    fn click_dungeon_dpad_movement() {
        let mut g = make_game();
        g.handle_input(&InputEvent::Key('1'));
        g.handle_input(&InputEvent::Key('1'));
        g.handle_input(&InputEvent::Key('1')); // Enter dungeon
        assert_eq!(g.state.scene, Scene::DungeonExplore);

        // D-pad south (row=2, col=1 in 3x3 grid = DPAD_BASE + 2*3 + 1)
        let south_id = DPAD_BASE + 7;
        // May or may not move depending on map layout, just check no crash
        let _result = g.handle_input(&click(south_id));
    }

    #[test]
    fn click_dungeon_open_inventory() {
        let mut g = make_game();
        g.handle_input(&InputEvent::Key('1'));
        g.handle_input(&InputEvent::Key('1'));
        g.handle_input(&InputEvent::Key('1')); // Enter dungeon
        assert_eq!(g.state.scene, Scene::DungeonExplore);
        let result = g.handle_input(&click(OPEN_INVENTORY));
        assert!(result);
        assert_eq!(g.state.overlay, Some(Overlay::Inventory));
    }

    #[test]
    fn dungeon_result_continues() {
        let mut g = make_game();
        g.handle_input(&InputEvent::Key('1'));
        g.handle_input(&InputEvent::Key('1'));
        logic::enter_dungeon(&mut g.state, 1);
        g.state.scene = Scene::DungeonResult;
        g.state.room_result = Some(state::RoomResult {
            description: vec!["test".into()],
        });
        g.handle_input(&InputEvent::Key('1'));
        assert_eq!(g.state.scene, Scene::DungeonExplore);
    }

    #[test]
    fn arrow_key_moves_in_absolute_direction() {
        let mut g = make_game();
        g.handle_input(&InputEvent::Key('1'));
        g.handle_input(&InputEvent::Key('1'));
        g.handle_input(&InputEvent::Key('1')); // Enter dungeon
        assert_eq!(g.state.scene, Scene::DungeonExplore);

        // 'k' = North (absolute direction, 1 step)
        let map = g.state.dungeon.as_ref().unwrap();
        let px = map.player_x;
        let py = map.player_y;
        let nx = px as i32 + state::Facing::North.dx();
        let ny = py as i32 + state::Facing::North.dy();
        let north_open = map.in_bounds(nx, ny)
            && map.cell(nx as usize, ny as usize).is_walkable();
        if north_open {
            let old_y = py;
            g.handle_input(&InputEvent::Key('k'));
            let map = g.state.dungeon.as_ref().unwrap();
            assert_eq!(map.last_dir, state::Facing::North);
            assert_eq!(map.player_y, old_y - 1);
        }
    }

    #[test]
    fn map_tap_moves_absolute_direction() {
        let mut g = make_game();
        g.handle_input(&InputEvent::Key('1'));
        g.handle_input(&InputEvent::Key('1'));
        g.handle_input(&InputEvent::Key('1')); // Enter dungeon
        assert_eq!(g.state.scene, Scene::DungeonExplore);

        // Tap north on map (row=0, col=1 in 3x3 grid = MAP_TAP_BASE + 0*3 + 1)
        let north_tap_id = MAP_TAP_BASE + 1; // col=1, row=0
        let map = g.state.dungeon.as_ref().unwrap();
        let px = map.player_x;
        let py = map.player_y;
        let nx = px as i32 + state::Facing::North.dx();
        let ny = py as i32 + state::Facing::North.dy();
        let north_open = map.in_bounds(nx, ny)
            && map.cell(nx as usize, ny as usize).is_walkable();
        if north_open {
            let result = g.handle_input(&click(north_tap_id));
            assert!(result);
        }
    }

    #[test]
    fn dpad_moves_one_step() {
        let mut g = make_game();
        g.handle_input(&InputEvent::Key('1'));
        g.handle_input(&InputEvent::Key('1'));
        g.handle_input(&InputEvent::Key('1')); // Enter dungeon
        assert_eq!(g.state.scene, Scene::DungeonExplore);

        let map = g.state.dungeon.as_ref().unwrap();
        let start_x = map.player_x;
        let start_y = map.player_y;

        // D-pad south (row=2, col=1 in 3x3 grid = DPAD_BASE + 2*3 + 1)
        let south_id = DPAD_BASE + 7; // col=1, row=2
        let sx = start_x as i32 + state::Facing::South.dx();
        let sy = start_y as i32 + state::Facing::South.dy();
        let south_open = map.in_bounds(sx, sy)
            && map.cell(sx as usize, sy as usize).is_walkable();
        if south_open {
            let result = g.handle_input(&click(south_id));
            assert!(result);
            let map = g.state.dungeon.as_ref().unwrap();
            assert_eq!(map.last_dir, state::Facing::South);
            // D-pad moves exactly 1 step (no auto-walk)
            assert_eq!(map.player_x, start_x);
            assert_eq!(map.player_y, start_y + 1);
        }
    }
}
