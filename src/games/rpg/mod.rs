//! Dungeon Dive — grid-based dungeon crawler with first-person 3D view.
//!
//! Game trait implementation with arrow-key movement and interactive events.
//! Movement: Arrow keys (1 step), map tap (auto-walk through corridors).
//! Events: [1]-[5] choices.  Overlays: [I] inventory, [S] status.

pub mod actions;
pub mod dungeon_map;
pub mod dungeon_view;
pub mod events;
pub mod logic;
pub mod lore;
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
    handle_overlay_open_click(state, id)
}

// ── Dungeon Explore (arrow key / map tap movement) ─────────

fn handle_dungeon_explore_key(state: &mut RpgState, ch: char) -> bool {
    match ch {
        // WASD / hjkl / arrow keys — all direct cardinal movement
        'W' | 'w' | 'k' => logic::try_move(state, state::Facing::North),
        'A' | 'a' | 'h' => logic::try_move(state, state::Facing::West),
        'S' | 's' | 'j' => logic::try_move(state, state::Facing::South),
        'D' | 'd' | 'l' => logic::try_move(state, state::Facing::East),
        // Overlays
        'I' | 'i' => {
            state.overlay = Some(Overlay::Inventory);
            true
        }
        'X' | 'x' => {
            state.overlay = Some(Overlay::Status);
            true
        }
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
        Some(d) => logic::try_move(state, d),
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
        Some(dir) => logic::move_direction(state, dir),
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
        return logic::resolve_event_choice(state, idx);
    }

    match ch {
        'I' | 'i' => {
            state.overlay = Some(Overlay::Inventory);
            true
        }
        _ => false,
    }
}

fn handle_dungeon_event_click(state: &mut RpgState, id: u16) -> bool {
    if (EVENT_CHOICE_BASE..EVENT_CHOICE_BASE + 10).contains(&id) {
        let index = (id - EVENT_CHOICE_BASE) as usize;
        return logic::resolve_event_choice(state, index);
    }
    handle_overlay_open_click(state, id)
}

// ── Dungeon Result ─────────────────────────────────────────

fn handle_dungeon_result_key(state: &mut RpgState, ch: char) -> bool {
    match ch {
        '1' | ' ' => {
            logic::continue_exploration(state);
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

fn handle_dungeon_result_click(state: &mut RpgState, id: u16) -> bool {
    if id == CHOICE_BASE {
        logic::continue_exploration(state);
        return true;
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

// ── Overlay open (shared by town / dungeon) ─────────────────

fn handle_overlay_open_click(state: &mut RpgState, id: u16) -> bool {
    match id {
        OPEN_INVENTORY => {
            state.overlay = Some(Overlay::Inventory);
            true
        }
        OPEN_STATUS => {
            state.overlay = Some(Overlay::Status);
            true
        }
        _ => false,
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

    #[test]
    fn click_town_open_inventory() {
        let mut g = make_game();
        g.handle_input(&InputEvent::Key('1'));
        g.handle_input(&InputEvent::Key('1'));
        assert_eq!(g.state.scene, Scene::Town);
        let result = g.handle_input(&InputEvent::Click(OPEN_INVENTORY));
        assert!(result);
        assert_eq!(g.state.overlay, Some(Overlay::Inventory));
    }

    #[test]
    fn click_town_open_status() {
        let mut g = make_game();
        g.handle_input(&InputEvent::Key('1'));
        g.handle_input(&InputEvent::Key('1'));
        assert_eq!(g.state.scene, Scene::Town);
        let result = g.handle_input(&InputEvent::Click(OPEN_STATUS));
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
        let _result = g.handle_input(&InputEvent::Click(south_id));
    }

    #[test]
    fn click_dungeon_open_inventory() {
        let mut g = make_game();
        g.handle_input(&InputEvent::Key('1'));
        g.handle_input(&InputEvent::Key('1'));
        g.handle_input(&InputEvent::Key('1')); // Enter dungeon
        assert_eq!(g.state.scene, Scene::DungeonExplore);
        let result = g.handle_input(&InputEvent::Click(OPEN_INVENTORY));
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
            let result = g.handle_input(&InputEvent::Click(north_tap_id));
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
            let result = g.handle_input(&InputEvent::Click(south_id));
            assert!(result);
            let map = g.state.dungeon.as_ref().unwrap();
            assert_eq!(map.last_dir, state::Facing::South);
            // D-pad moves exactly 1 step (no auto-walk)
            assert_eq!(map.player_x, start_x);
            assert_eq!(map.player_y, start_y + 1);
        }
    }
}
