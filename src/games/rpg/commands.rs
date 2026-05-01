//! `PlayerAction` — the single set of actions a player (human or AI) can
//! actually take in Dungeon Dive. Both the main game's input handlers
//! (`mod.rs::handle_input`) and the simulator (`sim.rs`) construct values
//! of this enum and feed them through `apply_action`. That's the only
//! supported entry point; talking to `logic.rs` directly is reserved for
//! the rpg module itself.
//!
//! ## Why
//!
//! - **No divergence**: simulation and main game share the same dispatch
//!   function, so balance numbers measured by the simulator transfer
//!   1:1 to the playable build.
//! - **Honest policy testing**: the simulator's policy can only do what a
//!   human player can do. There is no "retreat instantly" shortcut — the
//!   AI must reach the entrance and accept the `ReturnToTown` event, just
//!   like a real player.
//! - **Future-proof**: when new player actions are added, they show up
//!   here and both call sites pick them up automatically.

use super::logic;
use super::state::{BattlePhase, Facing, Overlay, RpgState, Scene};

/// Every action a player can take, regardless of the input device.
///
/// New actions go here. The dispatcher (`apply_action`) is the only place
/// that ties actions to logic functions, so the mapping is centralized.
///
/// A few variants (e.g. `OpenShop`, `RetreatToTown`, `AcknowledgeGameClear`)
/// are exercised today only by the `#[cfg(test)]`-gated simulator — UI
/// buttons that dispatch them will be added in a follow-up PR. Keep them
/// in the enum so the simulator and future UI agree on the action surface.
#[derive(Clone, Debug, PartialEq)]
#[allow(dead_code)]
pub enum PlayerAction {
    // ── Intro ──
    AdvanceIntro,

    // ── Town ──
    /// Pick a town menu option by visible index (matches `logic::town_choices`).
    TownChoice(usize),

    // ── Overlays (any scene) ──
    OpenInventory,
    OpenStatus,
    OpenShop,
    CloseOverlay,
    UseInventoryItem(usize),
    BuyShopItem(usize),

    // ── Dungeon explore ──
    /// Single-step move (D-pad / WASD / arrow key).
    Move(Facing),
    /// Auto-walk move (map tap): walks through corridors until junction/event.
    MoveAuto(Facing),
    /// Voluntarily return to town from the dungeon, banking the return
    /// bonus. The HUD already advertises the bonus amount (帰還+XG); this
    /// command is what a future "give up" button will dispatch. The
    /// simulator uses it as the player's "I've had enough" decision so
    /// that AI policies match the intended player verb.
    RetreatToTown,

    // ── Dungeon event / result ──
    PickEventChoice(usize),
    ContinueExploration,

    // ── Battle ──
    BattleAttack,
    BattleOpenSkillMenu,
    BattleOpenItemMenu,
    BattleBackToActions,
    BattleUseSkill(usize),
    BattleUseItem(usize),
    BattleFlee,
    /// Acknowledge the post-battle phase (Victory/Defeat/Fled).
    BattleAcknowledgeOutcome,

    // ── Game clear ──
    AcknowledgeGameClear,
}

/// Dispatch `cmd` against the current state. Returns `true` if the command
/// was applicable in the current scene/overlay; `false` otherwise.
///
/// This is the single seam between input layer and game logic. Tests can
/// replay a sequence of commands deterministically; the simulator drives
/// the game with the same vocabulary as `mod.rs::handle_input`.
pub fn apply_action(state: &mut RpgState, cmd: PlayerAction) -> bool {
    use PlayerAction as C;

    // Overlay-level commands are valid only when an overlay is open.
    if state.overlay.is_some() {
        return apply_overlay_command(state, cmd);
    }

    match cmd {
        C::AdvanceIntro => match state.scene {
            Scene::Intro(_) => {
                logic::advance_intro(state);
                true
            }
            _ => false,
        },

        C::TownChoice(idx) => match state.scene {
            Scene::Town => logic::execute_town_choice(state, idx),
            _ => false,
        },

        C::OpenInventory => {
            if matches!(
                state.scene,
                Scene::Town
                    | Scene::DungeonExplore
                    | Scene::DungeonEvent
                    | Scene::DungeonResult
            ) {
                state.overlay = Some(Overlay::Inventory);
                true
            } else {
                false
            }
        }
        C::OpenStatus => {
            if matches!(state.scene, Scene::Town | Scene::DungeonExplore | Scene::DungeonResult) {
                state.overlay = Some(Overlay::Status);
                true
            } else {
                false
            }
        }
        C::OpenShop => match state.scene {
            Scene::Town => {
                state.overlay = Some(Overlay::Shop);
                true
            }
            _ => false,
        },
        C::CloseOverlay => {
            // No-op when no overlay is open.
            false
        }

        C::UseInventoryItem(_) | C::BuyShopItem(_) => {
            // These require an overlay; rejected here.
            false
        }

        C::Move(dir) => match state.scene {
            Scene::DungeonExplore => logic::try_move(state, dir),
            _ => false,
        },
        C::MoveAuto(dir) => match state.scene {
            Scene::DungeonExplore => logic::move_direction(state, dir),
            _ => false,
        },
        C::RetreatToTown => match state.scene {
            Scene::DungeonExplore | Scene::DungeonResult => {
                logic::retreat_to_town(state);
                true
            }
            _ => false,
        },

        C::PickEventChoice(idx) => match state.scene {
            Scene::DungeonEvent => logic::resolve_event_choice(state, idx),
            _ => false,
        },
        C::ContinueExploration => match state.scene {
            Scene::DungeonResult => {
                logic::continue_exploration(state);
                true
            }
            _ => false,
        },

        C::BattleAttack => battle_action(state, logic::battle_attack),
        C::BattleOpenSkillMenu => battle_phase_change(state, BattlePhase::SelectSkill, |s| {
            !logic::available_skills(s.level).is_empty()
        }),
        C::BattleOpenItemMenu => battle_phase_change(state, BattlePhase::SelectItem, |s| {
            !logic::battle_consumables(s).is_empty()
        }),
        C::BattleBackToActions => {
            if let Some(b) = &mut state.battle {
                if matches!(b.phase, BattlePhase::SelectSkill | BattlePhase::SelectItem) {
                    b.phase = BattlePhase::SelectAction;
                    return true;
                }
            }
            false
        }
        C::BattleUseSkill(idx) => {
            if state
                .battle
                .as_ref()
                .map(|b| b.phase == BattlePhase::SelectSkill || b.phase == BattlePhase::SelectAction)
                .unwrap_or(false)
            {
                logic::battle_use_skill(state, idx)
            } else {
                false
            }
        }
        C::BattleUseItem(idx) => {
            if state
                .battle
                .as_ref()
                .map(|b| b.phase == BattlePhase::SelectItem || b.phase == BattlePhase::SelectAction)
                .unwrap_or(false)
            {
                logic::battle_use_item(state, idx)
            } else {
                false
            }
        }
        C::BattleFlee => battle_action(state, logic::battle_flee),
        C::BattleAcknowledgeOutcome => {
            let phase = state.battle.as_ref().map(|b| b.phase);
            match phase {
                Some(BattlePhase::Victory) => {
                    logic::process_victory(state);
                    true
                }
                Some(BattlePhase::Defeat) => {
                    logic::process_defeat(state);
                    true
                }
                Some(BattlePhase::Fled) => {
                    logic::process_fled(state);
                    true
                }
                _ => false,
            }
        }

        C::AcknowledgeGameClear => matches!(state.scene, Scene::GameClear),
    }
}

fn apply_overlay_command(state: &mut RpgState, cmd: PlayerAction) -> bool {
    use PlayerAction as C;

    match cmd {
        C::CloseOverlay => {
            state.overlay = None;
            true
        }
        C::UseInventoryItem(idx) => {
            if state.overlay == Some(Overlay::Inventory) {
                logic::use_item(state, idx)
            } else {
                false
            }
        }
        C::BuyShopItem(idx) => {
            if state.overlay == Some(Overlay::Shop) {
                logic::buy_item(state, idx)
            } else {
                false
            }
        }
        // Opening another overlay while one is open: ignored, matching the
        // existing input handlers (no nested overlays in the current UI).
        _ => false,
    }
}

fn battle_action<F: FnOnce(&mut RpgState) -> bool>(state: &mut RpgState, f: F) -> bool {
    let phase = state.battle.as_ref().map(|b| b.phase);
    if matches!(phase, Some(BattlePhase::SelectAction)) {
        f(state)
    } else {
        false
    }
}

fn battle_phase_change<G: FnOnce(&RpgState) -> bool>(
    state: &mut RpgState,
    target: BattlePhase,
    guard: G,
) -> bool {
    if state.battle.as_ref().map(|b| b.phase) != Some(BattlePhase::SelectAction) {
        return false;
    }
    if !guard(state) {
        return false;
    }
    if let Some(b) = &mut state.battle {
        b.phase = target;
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::games::rpg::state::{Scene, Overlay};

    fn fresh() -> RpgState {
        let mut s = RpgState::new();
        // Skip intro for convenience.
        apply_action(&mut s, PlayerAction::AdvanceIntro);
        apply_action(&mut s, PlayerAction::AdvanceIntro);
        s
    }

    #[test]
    fn intro_advances_via_command() {
        let mut s = RpgState::new();
        assert!(apply_action(&mut s, PlayerAction::AdvanceIntro));
        assert_eq!(s.scene, Scene::Intro(1));
    }

    #[test]
    fn town_enter_dungeon_via_command() {
        let mut s = fresh();
        assert!(apply_action(&mut s, PlayerAction::TownChoice(0)));
        assert_eq!(s.scene, Scene::DungeonExplore);
    }

    #[test]
    fn shop_opens_and_buys() {
        let mut s = fresh();
        s.gold = 200;
        assert!(apply_action(&mut s, PlayerAction::OpenShop));
        assert_eq!(s.overlay, Some(Overlay::Shop));
        assert!(apply_action(&mut s, PlayerAction::BuyShopItem(0)));
        assert!(s.gold < 200);
        assert!(apply_action(&mut s, PlayerAction::CloseOverlay));
        assert!(s.overlay.is_none());
    }

    #[test]
    fn invalid_command_for_scene_returns_false() {
        let mut s = RpgState::new();
        // Town commands should not work in Intro.
        assert!(!apply_action(&mut s, PlayerAction::TownChoice(0)));
    }

    #[test]
    fn battle_attack_via_command() {
        use crate::games::rpg::logic;
        use crate::games::rpg::state::EnemyKind;

        let mut s = fresh();
        apply_action(&mut s, PlayerAction::TownChoice(0));
        logic::start_battle(&mut s, EnemyKind::Slime, false);
        assert_eq!(s.scene, Scene::Battle);
        assert!(apply_action(&mut s, PlayerAction::BattleAttack));
    }

    /// Equivalent input sequences via the input handler and via direct
    /// `apply_action` calls must produce identical state. This guards
    /// against `mod.rs::handle_input` regressing into direct `logic::*`
    /// calls and silently diverging from the simulator's view of the game.
    #[test]
    fn input_handler_and_apply_action_agree() {
        use crate::games::rpg::RpgGame;
        use crate::games::Game;
        use crate::input::InputEvent;

        // Path A: drive RpgGame via InputEvents (the runtime path).
        let mut g = RpgGame::new();
        // Intro x2 → Town → enter dungeon.
        g.handle_input(&InputEvent::Key('1'));
        g.handle_input(&InputEvent::Key('1'));
        g.handle_input(&InputEvent::Key('1'));

        // Path B: drive RpgState via PlayerActions (the simulator path).
        let mut s = RpgState::new();
        apply_action(&mut s, PlayerAction::AdvanceIntro);
        apply_action(&mut s, PlayerAction::AdvanceIntro);
        apply_action(&mut s, PlayerAction::TownChoice(0));

        // Both should land in DungeonExplore with the same starting stats.
        assert_eq!(g.state.scene, s.scene);
        assert_eq!(g.state.hp, s.hp);
        assert_eq!(g.state.max_hp, s.max_hp);
        assert_eq!(g.state.gold, s.gold);
        assert_eq!(g.state.weapon, s.weapon);
        assert_eq!(g.state.armor, s.armor);
        assert_eq!(g.state.inventory.len(), s.inventory.len());
    }
}
