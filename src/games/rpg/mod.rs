//! Dungeon Dive — grid-based dungeon crawler with inline combat.
//!
//! Roguelike gameplay: monsters live on the same grid as the player.
//! Movement against a monster tile = attack. Each player action triggers
//! a monster turn (chase + attack). No separate battle screen.

pub mod actions;
pub mod dungeon_map;
pub mod dungeon_view;
pub mod events;
pub mod logic;
pub mod lore;
pub mod render;
pub mod state;
#[cfg(test)]
pub mod simulator;

use std::cell::RefCell;
use std::rc::Rc;

use ratzilla::ratatui::layout::Rect;
use ratzilla::ratatui::Frame;

use crate::games::{Game, GameChoice};
use crate::input::{ClickState, InputEvent};

use actions::*;
use state::{Overlay, RpgState, Scene};

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

/// Whether the current scene/overlay treats the arrow keys as cursor
/// navigation. In dungeon explore (with no popup or overlay), arrows are
/// player movement instead.
fn arrows_navigate_cursor(state: &RpgState) -> bool {
    if state.overlay.is_some() {
        return true;
    }
    matches!(state.scene, Scene::Town | Scene::Intro(_) | Scene::GameClear)
        || (state.scene == Scene::DungeonExplore && state.active_event.is_some())
}

fn handle_key(state: &mut RpgState, ch: char) -> bool {
    // Keep the cursor inside the current menu's bounds before any handler
    // reads it (menus may have shrunk since the last input — e.g. consumed
    // an inventory item).
    logic::cursor_clamp(state);

    // Arrow-key cursor navigation, applied uniformly across scenes/overlays
    // that have a selectable list. In dungeon explore (no popup) the same
    // keys fall through to player movement.
    if arrows_navigate_cursor(state) {
        match ch {
            'j' => {
                logic::cursor_move(state, 1);
                return true;
            }
            'k' => {
                logic::cursor_move(state, -1);
                return true;
            }
            _ => {}
        }
    }

    if state.overlay.is_some() {
        return handle_overlay_key(state, ch);
    }

    match state.scene {
        Scene::Intro(_) => handle_intro_key(state, ch),
        Scene::Town => handle_town_key(state, ch),
        Scene::DungeonExplore => {
            // When an event popup is active, route input there first.
            if state.active_event.is_some() {
                handle_dungeon_event_key(state, ch)
            } else {
                handle_dungeon_explore_key(state, ch)
            }
        }
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
        Scene::DungeonExplore => {
            if state.active_event.is_some() {
                handle_dungeon_event_click(state, id)
            } else {
                handle_dungeon_explore_click(state, id)
            }
        }
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
    // Number keys still work as direct shortcuts (backward compat for PC
    // keyboard users), but the canonical UX is arrow + A button.
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
        // A button — confirm cursor's town choice.
        ' ' | 'A' | 'a' => logic::execute_town_choice(state, state.cursor),
        'I' | 'i' => {
            state.open_overlay(Overlay::Inventory);
            true
        }
        'S' | 's' => {
            state.open_overlay(Overlay::Status);
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

// ── Dungeon Explore ───────────────────────────────────────

fn handle_dungeon_explore_key(state: &mut RpgState, ch: char) -> bool {
    match ch {
        // Movement: arrow keys (h/j/k/l after KeyCode mapping) and WASD.
        // 'a' is reserved for the A button; use 'h' or arrow-left for west.
        'W' | 'w' | 'k' => logic::try_move(state, state::Facing::North),
        'h' => logic::try_move(state, state::Facing::West),
        'S' | 's' | 'j' => logic::try_move(state, state::Facing::South),
        'D' | 'd' | 'l' => logic::try_move(state, state::Facing::East),
        // A button — context-sensitive primary action.
        ' ' | 'A' | 'a' => trigger_a_button(state),
        // B button — unified menu (持ち物 / スキル / ステータス).
        'b' | 'B' | 'I' | 'i' => {
            state.open_overlay(Overlay::Inventory);
            true
        }
        // Skill / Status shortcuts kept for keyboard users.
        'Z' | 'z' => {
            state.open_overlay(Overlay::SkillMenu);
            true
        }
        'X' | 'x' => {
            state.open_overlay(Overlay::Status);
            true
        }
        _ => false,
    }
}

/// Implements the A button:
/// foot event → confirm cursor's choice (was hardcoded to choice 0 before
///   the cursor unification — now respects whichever option the player
///   has highlighted with arrow keys),
/// adjacent enemy → open skill menu,
/// otherwise → wait one turn.
fn trigger_a_button(state: &mut RpgState) -> bool {
    if state.active_event.is_some() {
        return logic::resolve_event_choice(state, state.cursor);
    }
    if let Some(map) = &state.dungeon {
        let px = map.player_x as i32;
        let py = map.player_y as i32;
        if map
            .monsters
            .iter()
            .any(|m| m.hp > 0 && (m.x as i32 - px).abs() + (m.y as i32 - py).abs() == 1)
        {
            state.open_overlay(Overlay::SkillMenu);
            return true;
        }
    }
    logic::wait_in_place(state)
}

fn handle_dungeon_explore_click(state: &mut RpgState, id: u16) -> bool {
    if id == AB_A_BUTTON {
        return trigger_a_button(state);
    }
    if id == AB_B_BUTTON {
        state.open_overlay(Overlay::Inventory);
        return true;
    }
    handle_dpad_tap(state, id)
        || handle_map_tap(state, id)
        || handle_overlay_open_click(state, id)
}

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

fn handle_map_tap(state: &mut RpgState, id: u16) -> bool {
    use crate::widgets::ClickableGrid;
    let Some((col, row)) = ClickableGrid::decode(MAP_TAP_BASE, 3, id) else {
        return false;
    };
    let screen_dir = match (col, row) {
        (_, 0) => Some(state::Facing::North),
        (0, 1) => Some(state::Facing::West),
        (2, 1) => Some(state::Facing::East),
        (_, 2) => Some(state::Facing::South),
        _ => None,
    };
    match screen_dir {
        Some(dir) => logic::move_direction(state, dir),
        None => false,
    }
}

// ── Dungeon Event ─────────────────────────────────────────

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
        // A button — confirm cursor's choice in the popup.
        ' ' | 'A' | 'a' => logic::resolve_event_choice(state, state.cursor),
        // B button — skip / "explore on" (last choice, conventionally Ignore).
        'b' | 'B' => {
            let last = state
                .active_event
                .as_ref()
                .map(|e| e.choices.len().saturating_sub(1))
                .unwrap_or(0);
            logic::resolve_event_choice(state, last)
        }
        'I' | 'i' => {
            state.open_overlay(Overlay::Inventory);
            true
        }
        _ => false,
    }
}

fn handle_dungeon_event_click(state: &mut RpgState, id: u16) -> bool {
    if id == AB_A_BUTTON {
        return logic::resolve_event_choice(state, state.cursor);
    }
    if id == AB_B_BUTTON {
        let last = state
            .active_event
            .as_ref()
            .map(|e| e.choices.len().saturating_sub(1))
            .unwrap_or(0);
        return logic::resolve_event_choice(state, last);
    }
    if (EVENT_CHOICE_BASE..EVENT_CHOICE_BASE + 10).contains(&id) {
        let index = (id - EVENT_CHOICE_BASE) as usize;
        return logic::resolve_event_choice(state, index);
    }
    handle_overlay_open_click(state, id)
}

// ── Overlay open (shared) ─────────────────────────────────

fn handle_overlay_open_click(state: &mut RpgState, id: u16) -> bool {
    match id {
        OPEN_INVENTORY => {
            state.open_overlay(Overlay::Inventory);
            true
        }
        OPEN_STATUS => {
            state.open_overlay(Overlay::Status);
            true
        }
        OPEN_SKILL_MENU => {
            state.open_overlay(Overlay::SkillMenu);
            true
        }
        _ => false,
    }
}

// ── Overlays ───────────────────────────────────────────────

fn handle_overlay_key(state: &mut RpgState, ch: char) -> bool {
    // B button / common close shortcuts work for every overlay.
    if matches!(ch, 'b' | 'B' | '0' | '-') {
        state.close_overlay();
        return true;
    }

    // Tab cycle (h/l) when on a menu tab.
    if state.overlay.map(|o| o.is_menu_tab()).unwrap_or(false) {
        match ch {
            'l' => {
                let next = match state.overlay.unwrap() {
                    Overlay::Inventory => Overlay::SkillMenu,
                    Overlay::SkillMenu => Overlay::Status,
                    _ => Overlay::Inventory,
                };
                state.open_overlay(next);
                return true;
            }
            'h' => {
                let next = match state.overlay.unwrap() {
                    Overlay::Status => Overlay::SkillMenu,
                    Overlay::SkillMenu => Overlay::Inventory,
                    _ => Overlay::Status,
                };
                state.open_overlay(next);
                return true;
            }
            _ => {}
        }
    }
    match state.overlay {
        Some(Overlay::Inventory) => match ch {
            // A button — use the highlighted item.
            ' ' | 'A' | 'a' => logic::use_item(state, state.cursor),
            // Number-key shortcut still works for direct access.
            '1'..='9' => {
                let idx = (ch as u32 - '1' as u32) as usize;
                logic::use_item(state, idx)
            }
            _ => false,
        },
        Some(Overlay::Shop) => match ch {
            ' ' | 'A' | 'a' => logic::buy_item(state, state.cursor),
            '1'..='9' => {
                let idx = (ch as u32 - '1' as u32) as usize;
                logic::buy_item(state, idx)
            }
            _ => false,
        },
        Some(Overlay::Status) => false, // status has no clickable items
        Some(Overlay::SkillMenu) => match ch {
            ' ' | 'A' | 'a' => logic::use_skill(state, state.cursor),
            '1'..='9' => {
                let idx = (ch as u32 - '1' as u32) as usize;
                logic::use_skill(state, idx)
            }
            _ => false,
        },
        Some(Overlay::QuestBoard) => match ch {
            ' ' | 'A' | 'a' => {
                if state.active_quest.is_some() {
                    logic::abandon_quest(state)
                } else {
                    logic::accept_quest(state, state.cursor)
                }
            }
            '1'..='9' => {
                let idx = (ch as u32 - '1' as u32) as usize;
                if state.active_quest.is_some() {
                    logic::abandon_quest(state)
                } else {
                    logic::accept_quest(state, idx)
                }
            }
            _ => false,
        },
        Some(Overlay::PrayMenu) => match ch {
            ' ' | '1' | 'A' | 'a' => logic::pray(state),
            _ => false,
        },
        None => false,
    }
}

fn handle_overlay_click(state: &mut RpgState, id: u16) -> bool {
    if id == CLOSE_OVERLAY {
        state.close_overlay();
        return true;
    }

    // Tab switch within the unified menu (Inventory / SkillMenu / Status).
    if state.overlay.map(|o| o.is_menu_tab()).unwrap_or(false) {
        match id {
            MENU_TAB_INVENTORY => {
                state.open_overlay(Overlay::Inventory);
                return true;
            }
            MENU_TAB_SKILL => {
                state.open_overlay(Overlay::SkillMenu);
                return true;
            }
            MENU_TAB_STATUS => {
                state.open_overlay(Overlay::Status);
                return true;
            }
            _ => {}
        }
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
        Some(Overlay::SkillMenu) => {
            if (SKILL_BASE..SKILL_BASE + 10).contains(&id) {
                return logic::use_skill(state, (id - SKILL_BASE) as usize);
            }
            false
        }
        Some(Overlay::QuestBoard) => {
            if (QUEST_ACCEPT_BASE..QUEST_ACCEPT_BASE + 5).contains(&id) {
                return logic::accept_quest(state, (id - QUEST_ACCEPT_BASE) as usize);
            }
            if id == QUEST_ABANDON {
                return logic::abandon_quest(state);
            }
            false
        }
        Some(Overlay::PrayMenu) => {
            if id == PRAY_CONFIRM {
                return logic::pray(state);
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

    fn click(id: u16) -> InputEvent {
        InputEvent::Click(ClickScope::Game(GameChoice::Rpg), id)
    }

    #[test]
    fn intro_sequence() {
        let mut g = make_game();
        assert_eq!(g.state.scene, Scene::Intro(0));
        g.handle_input(&InputEvent::Key('1'));
        assert_eq!(g.state.scene, Scene::Intro(1));
        g.handle_input(&InputEvent::Key('1'));
        assert_eq!(g.state.scene, Scene::Town);
        assert!(g.state.weapon().is_some());
    }

    #[test]
    fn town_enter_dungeon() {
        let mut g = make_game();
        g.handle_input(&InputEvent::Key('1'));
        g.handle_input(&InputEvent::Key('1'));
        assert_eq!(g.state.scene, Scene::Town);
        g.handle_input(&InputEvent::Key('1'));
        assert_eq!(g.state.scene, Scene::DungeonExplore);
        assert!(g.state.dungeon.is_some());
    }

    #[test]
    fn dungeon_wasd_movement() {
        let mut g = make_game();
        g.handle_input(&InputEvent::Key('1'));
        g.handle_input(&InputEvent::Key('1'));
        g.handle_input(&InputEvent::Key('1'));
        assert_eq!(g.state.scene, Scene::DungeonExplore);
        g.handle_input(&InputEvent::Key('W'));
        g.handle_input(&InputEvent::Key('D'));
        g.handle_input(&InputEvent::Key('A'));
        g.handle_input(&InputEvent::Key('S'));
    }

    #[test]
    fn dungeon_retreat_via_logic() {
        let mut g = make_game();
        g.handle_input(&InputEvent::Key('1'));
        g.handle_input(&InputEvent::Key('1'));
        g.handle_input(&InputEvent::Key('1'));
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
    fn skill_overlay_opens_in_dungeon() {
        let mut g = make_game();
        g.handle_input(&InputEvent::Key('1'));
        g.handle_input(&InputEvent::Key('1'));
        g.handle_input(&InputEvent::Key('1'));
        assert_eq!(g.state.scene, Scene::DungeonExplore);
        g.handle_input(&InputEvent::Key('Z'));
        assert_eq!(g.state.overlay, Some(Overlay::SkillMenu));
    }

    #[test]
    fn quest_board_opens() {
        let mut g = make_game();
        g.handle_input(&InputEvent::Key('1'));
        g.handle_input(&InputEvent::Key('1'));
        // Town menu: 1=enter, 2=shop, 3=quests
        g.handle_input(&InputEvent::Key('3'));
        assert_eq!(g.state.overlay, Some(Overlay::QuestBoard));
    }

    #[test]
    fn pray_menu_opens() {
        let mut g = make_game();
        g.handle_input(&InputEvent::Key('1'));
        g.handle_input(&InputEvent::Key('1'));
        g.handle_input(&InputEvent::Key('4'));
        assert_eq!(g.state.overlay, Some(Overlay::PrayMenu));
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
    fn a_button_waits_when_no_event_no_enemy() {
        let mut g = make_game();
        g.handle_input(&InputEvent::Key('1'));
        g.handle_input(&InputEvent::Key('1'));
        g.handle_input(&InputEvent::Key('1'));
        assert_eq!(g.state.scene, Scene::DungeonExplore);
        // Clear all monsters so A button just waits.
        g.state.dungeon.as_mut().unwrap().monsters.clear();
        let turns_before = g.state.turn_count;
        g.handle_input(&click(AB_A_BUTTON));
        // Wait consumes a turn → turn_count increments.
        assert_eq!(g.state.turn_count, turns_before + 1);
        assert!(g.state.overlay.is_none());
    }

    #[test]
    fn a_button_opens_skill_when_enemy_adjacent() {
        let mut g = make_game();
        g.handle_input(&InputEvent::Key('1'));
        g.handle_input(&InputEvent::Key('1'));
        g.handle_input(&InputEvent::Key('1'));
        let map = g.state.dungeon.as_mut().unwrap();
        let px = map.player_x;
        let py = map.player_y;
        // Place a slime adjacent to the player.
        for &dir in &[
            state::Facing::North,
            state::Facing::East,
            state::Facing::South,
            state::Facing::West,
        ] {
            let nx = px as i32 + dir.dx();
            let ny = py as i32 + dir.dy();
            if !map.in_bounds(nx, ny) { continue; }
            let ux = nx as usize; let uy = ny as usize;
            if !map.cell(ux, uy).is_walkable() { continue; }
            map.monsters.clear();
            map.monsters.push(state::Monster {
                kind: state::EnemyKind::Slime,
                x: ux, y: uy, hp: 12, max_hp: 12,
                awake: true, charging: false,
            });
            break;
        }
        g.handle_input(&click(AB_A_BUTTON));
        assert_eq!(g.state.overlay, Some(Overlay::SkillMenu));
    }

    #[test]
    fn b_button_opens_unified_menu() {
        let mut g = make_game();
        g.handle_input(&InputEvent::Key('1'));
        g.handle_input(&InputEvent::Key('1'));
        g.handle_input(&InputEvent::Key('1'));
        g.handle_input(&click(AB_B_BUTTON));
        assert_eq!(g.state.overlay, Some(Overlay::Inventory));
    }

    #[test]
    fn menu_tab_switch_via_click() {
        let mut g = make_game();
        g.state.overlay = Some(Overlay::Inventory);
        g.handle_input(&click(MENU_TAB_SKILL));
        assert_eq!(g.state.overlay, Some(Overlay::SkillMenu));
        g.handle_input(&click(MENU_TAB_STATUS));
        assert_eq!(g.state.overlay, Some(Overlay::Status));
        g.handle_input(&click(MENU_TAB_INVENTORY));
        assert_eq!(g.state.overlay, Some(Overlay::Inventory));
    }

    #[test]
    fn failed_peddler_purchase_keeps_event_alive() {
        // Codex P1 (#95): a failed buy (insufficient gold) must NOT consume
        // the peddler tile, so the player can pick a different choice.
        let mut g = make_game();
        g.handle_input(&InputEvent::Key('1'));
        g.handle_input(&InputEvent::Key('1'));
        g.handle_input(&InputEvent::Key('1'));
        g.state.gold = 0; // can't afford anything
        g.state.active_event = Some(state::DungeonEvent {
            description: vec!["peddler".into()],
            choices: vec![state::EventChoice {
                label: "buy".into(),
                action: state::EventAction::PeddlerBuyHerb,
            }],
        });
        // Force the player tile cell_type = Peddler with event_done=false so
        // resolve_event matches and we can assert it stays unresolved.
        let map = g.state.dungeon.as_mut().unwrap();
        let (px, py) = (map.player_x, map.player_y);
        map.grid[py][px].cell_type = state::CellType::Peddler;
        map.grid[py][px].event_done = false;
        let resolved = logic::resolve_event_choice(&mut g.state, 0);
        assert!(!resolved, "failed purchase should report failure");
        assert!(g.state.active_event.is_some(), "event should remain open");
        let map = g.state.dungeon.as_ref().unwrap();
        assert!(
            !map.grid[py][px].event_done,
            "tile should not be marked done after failed purchase"
        );
    }

    #[test]
    fn dungeon_event_stays_in_explore_scene() {
        // Issue #89: events become popups; the scene must not switch.
        let mut g = make_game();
        g.handle_input(&InputEvent::Key('1'));
        g.handle_input(&InputEvent::Key('1'));
        g.handle_input(&InputEvent::Key('1'));
        // Manually trigger an event by injecting it.
        g.state.active_event = Some(state::DungeonEvent {
            description: vec!["test".into()],
            choices: vec![state::EventChoice {
                label: "ok".into(),
                action: state::EventAction::Continue,
            }],
        });
        assert_eq!(g.state.scene, Scene::DungeonExplore);
        g.handle_input(&click(AB_A_BUTTON));
        // After resolving, scene is still DungeonExplore, event cleared.
        assert_eq!(g.state.scene, Scene::DungeonExplore);
        assert!(g.state.active_event.is_none());
    }

    #[test]
    fn arrow_keys_navigate_cursor_in_town() {
        // Arrow + A unification: arrows move cursor; A confirms selection.
        let mut g = make_game();
        g.handle_input(&InputEvent::Key('1'));
        g.handle_input(&InputEvent::Key('1'));
        assert_eq!(g.state.scene, Scene::Town);
        assert_eq!(g.state.cursor, 0);

        g.handle_input(&InputEvent::Key('j')); // down
        assert_eq!(g.state.cursor, 1);
        g.handle_input(&InputEvent::Key('j'));
        assert_eq!(g.state.cursor, 2);
        g.handle_input(&InputEvent::Key('k')); // up
        assert_eq!(g.state.cursor, 1);

        // A button confirms cursor (idx 1 = ショップ → opens Shop overlay).
        g.handle_input(&InputEvent::Key(' '));
        assert_eq!(g.state.overlay, Some(Overlay::Shop));
        assert_eq!(g.state.cursor, 0, "cursor resets when overlay opens");
    }

    #[test]
    fn arrow_keys_navigate_event_popup() {
        // Cursor should walk through event choices and A picks the
        // highlighted one — replacing the old "A always picks index 0"
        // behavior so the player has agency.
        let mut g = make_game();
        g.handle_input(&InputEvent::Key('1'));
        g.handle_input(&InputEvent::Key('1'));
        g.handle_input(&InputEvent::Key('1'));
        // Inject a 3-choice event, pick the middle one.
        g.state.active_event = Some(state::DungeonEvent {
            description: vec!["x".into()],
            choices: vec![
                state::EventChoice { label: "a".into(), action: state::EventAction::Continue },
                state::EventChoice { label: "b".into(), action: state::EventAction::Continue },
                state::EventChoice { label: "c".into(), action: state::EventAction::Continue },
            ],
        });
        g.state.cursor = 0;
        g.handle_input(&InputEvent::Key('j'));
        assert_eq!(g.state.cursor, 1);
        // A on cursor=1 → resolves choice 1 (and clears event).
        g.handle_input(&InputEvent::Key(' '));
        assert!(g.state.active_event.is_none());
    }

    #[test]
    fn cursor_wraps_around_in_menu() {
        // Wrapping is intentional — small lists feel snappier when ↑ from
        // the top jumps to the bottom rather than getting stuck.
        let mut g = make_game();
        g.handle_input(&InputEvent::Key('1'));
        g.handle_input(&InputEvent::Key('1'));
        g.state.cursor = 0;
        g.handle_input(&InputEvent::Key('k')); // up at 0 → wraps to last
        let last = logic::cursor_count(&g.state) - 1;
        assert_eq!(g.state.cursor, last);
    }

    #[test]
    fn b_button_closes_overlay_from_keyboard() {
        let mut g = make_game();
        g.handle_input(&InputEvent::Key('1'));
        g.handle_input(&InputEvent::Key('1'));
        g.state.overlay = Some(Overlay::Inventory);
        g.handle_input(&InputEvent::Key('b'));
        assert!(g.state.overlay.is_none());
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
