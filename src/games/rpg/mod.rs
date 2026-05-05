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

fn handle_key(state: &mut RpgState, ch: char) -> bool {
    if state.overlay.is_some() {
        return handle_overlay_key(state, ch);
    }

    match state.scene {
        Scene::Intro(_) => handle_intro_key(state, ch),
        Scene::Town => handle_town_key(state, ch),
        Scene::DungeonExplore => handle_dungeon_explore_key(state, ch),
        Scene::DungeonEvent => handle_dungeon_event_key(state, ch),
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

// ── Dungeon Explore ───────────────────────────────────────

fn handle_dungeon_explore_key(state: &mut RpgState, ch: char) -> bool {
    match ch {
        'W' | 'w' | 'k' => logic::try_move(state, state::Facing::North),
        'A' | 'a' | 'h' => logic::try_move(state, state::Facing::West),
        'S' | 's' | 'j' => logic::try_move(state, state::Facing::South),
        'D' | 'd' | 'l' => logic::try_move(state, state::Facing::East),
        'I' | 'i' => {
            state.overlay = Some(Overlay::Inventory);
            true
        }
        'X' | 'x' => {
            state.overlay = Some(Overlay::Status);
            true
        }
        'Z' | 'z' => {
            state.overlay = Some(Overlay::SkillMenu);
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

// ── Overlay open (shared) ─────────────────────────────────

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
        OPEN_SKILL_MENU => {
            state.overlay = Some(Overlay::SkillMenu);
            true
        }
        _ => false,
    }
}

// ── Overlays ───────────────────────────────────────────────

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
        Some(Overlay::SkillMenu) => match ch {
            '0' | '-' => {
                state.overlay = None;
                true
            }
            '1'..='9' => {
                let idx = (ch as u32 - '1' as u32) as usize;
                logic::use_skill(state, idx)
            }
            _ => false,
        },
        Some(Overlay::QuestBoard) => match ch {
            '0' | '-' => {
                state.overlay = None;
                true
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
            '0' | '-' => {
                state.overlay = None;
                true
            }
            '1' | ' ' => logic::pray(state),
            _ => false,
        },
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
