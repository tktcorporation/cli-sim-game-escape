//! 神の戦場 (God Field 風) — turn-based card battle royal.
//!
//! 4 players (you + 3 CPU), each with 30 HP.  Draw 5 cards each turn,
//! attack with weapons, defend with armor.  Last one standing wins.
//!
//! See `state::Phase` for the input state machine.

pub mod actions;
pub mod effects;
pub mod logic;
pub mod render;
pub mod state;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use ratzilla::ratatui::layout::Rect;
use ratzilla::ratatui::Frame;

use crate::effects::{FlashTimer, FrameClock};
use crate::games::{Game, GameChoice};
use crate::input::{ClickState, InputEvent};
use crate::sound;
use crate::time;

use actions::*;
use effects::GfEffects;
use state::{Card, CardKind, GfState, Phase, NUM_PLAYERS};

pub struct GodFieldGame {
    state: GfState,
    effects: RefCell<GfEffects>,
    prev: Cell<PrevSnapshot>,
    frame_clock: FrameClock,
}

/// 前フレームの state スナップショット。render 毎に差分を見て演出をトリガする
/// (`detect_transitions`)。
#[derive(Clone, Copy, Default)]
struct PrevSnapshot {
    is_victory: bool,
    is_defeat: bool,
    hurt_flashes: [FlashTimer; NUM_PLAYERS],
    alive: [bool; NUM_PLAYERS],
}

impl GodFieldGame {
    pub fn new() -> Self {
        let state = GfState::new(initial_seed());
        let prev = Self::snapshot(&state);
        Self {
            state,
            effects: RefCell::new(GfEffects::new()),
            prev: Cell::new(prev),
            frame_clock: FrameClock::new(),
        }
    }

    fn snapshot(s: &GfState) -> PrevSnapshot {
        let mut hurt_flashes = [FlashTimer::new(); NUM_PLAYERS];
        let mut alive = [false; NUM_PLAYERS];
        for (i, p) in s.players.iter().enumerate() {
            hurt_flashes[i] = p.hurt_flash;
            alive[i] = p.alive;
        }
        PrevSnapshot {
            is_victory: s.phase == Phase::Victory,
            is_defeat: s.phase == Phase::Defeat,
            hurt_flashes,
            alive,
        }
    }

    fn detect_transitions(&self, area: Rect) {
        let prev = self.prev.get();
        let mut effects = self.effects.borrow_mut();
        let s = &self.state;
        let layout = render::compute_layout(area, &s.phase);

        for (i, p) in s.players.iter().enumerate() {
            if !prev.hurt_flashes[i].is_active() && p.hurt_flash.is_active() {
                effects.push_player_hit(layout.status);
                // CPU同士の被弾も毎回鳴らすと耳障りなので、自分が被弾した時だけ音を出す
                // (abyss の enemy_hurt_flash が無音なのと同じ配慮)。
                if p.is_human {
                    sound::play(sound::DAMAGE);
                }
            }
            if prev.alive[i] && !p.alive {
                effects.push_death(layout.status);
            }
        }

        if !prev.is_victory && s.phase == Phase::Victory {
            effects.push_victory(area);
            sound::play(sound::VICTORY);
        }
        if !prev.is_defeat && s.phase == Phase::Defeat {
            effects.push_defeat(area);
            sound::play(sound::DEFEAT);
        }

        self.prev.set(Self::snapshot(s));
    }
}

/// Best-effort entropy for a fresh deal: high-resolution wall clock when
/// available (browser), otherwise a constant fallback.
fn initial_seed() -> u32 {
    #[cfg(target_arch = "wasm32")]
    {
        if let Some(w) = web_sys::window() {
            if let Some(p) = w.performance() {
                return (p.now() as u64) as u32;
            }
        }
    }
    0xCAFE_BABE
}

impl Game for GodFieldGame {
    fn choice(&self) -> GameChoice {
        GameChoice::Godfield
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
        self.detect_transitions(area);
        render::render(&self.state, f, area, click_state);
        let elapsed = self.frame_clock.elapsed(time::now_ms().unwrap_or(0.0));
        self.effects.borrow_mut().process(elapsed, f.buffer_mut(), area);
    }
}

// ── Key dispatch ───────────────────────────────────────────────

fn handle_key(state: &mut GfState, ch: char) -> bool {
    match state.phase {
        Phase::Intro => {
            if matches!(ch, '1' | ' ' | '\n') {
                logic::begin_turn(state);
                sound::play(sound::CLICK);
                return true;
            }
            false
        }
        Phase::Victory | Phase::Defeat => {
            if matches!(ch, '1' | ' ' | '\n') {
                *state = GfState::new(state.rng_seed.wrapping_add(1));
                logic::begin_turn(state);
                sound::play(sound::CLICK);
                return true;
            }
            false
        }
        Phase::PlayerAction => match ch {
            'A' | 'a' => {
                let ok = enter_attack_phase(state);
                sound::play(if ok { sound::CLICK } else { sound::ERROR });
                ok
            }
            'H' | 'h' => {
                let ok = enter_heal_phase(state);
                sound::play(if ok { sound::CLICK } else { sound::ERROR });
                ok
            }
            'S' | 's' => {
                let ok = enter_special_phase(state);
                sound::play(if ok { sound::CLICK } else { sound::ERROR });
                ok
            }
            'P' | 'p' => {
                logic::human_pass(state);
                sound::play(sound::CLICK);
                true
            }
            _ => false,
        },
        Phase::PlayerSelectWeapons => match ch {
            '0' | '-' => {
                state.selected_weapons.clear();
                state.phase = Phase::PlayerAction;
                sound::play(sound::CLICK);
                true
            }
            ' ' | '\n' => {
                let ok = logic::confirm_weapons(state);
                sound::play(if ok { sound::CLICK } else { sound::ERROR });
                ok
            }
            d if d.is_ascii_digit() => {
                let idx = (d as u32 - '1' as u32) as usize;
                logic::toggle_weapon_selection(state, idx);
                sound::play(sound::CLICK);
                true
            }
            _ => false,
        },
        Phase::PlayerSelectTarget => match ch {
            '0' | '-' => {
                state.phase = Phase::PlayerSelectWeapons;
                sound::play(sound::CLICK);
                true
            }
            d if d.is_ascii_digit() => {
                // Map '1'..='3' to player indices 1..=3 (skipping self).
                let n = (d as u32 - '0' as u32) as usize;
                if n >= 1 && n < state.players.len() {
                    let ok = logic::human_attack(state, n);
                    sound::play(if ok { sound::CLICK } else { sound::ERROR });
                    return ok;
                }
                false
            }
            _ => false,
        },
        Phase::PlayerSelectHeal => match ch {
            '0' | '-' => {
                state.phase = Phase::PlayerAction;
                sound::play(sound::CLICK);
                true
            }
            d if d.is_ascii_digit() => {
                let idx = (d as u32 - '1' as u32) as usize;
                let ok = logic::human_heal(state, idx);
                sound::play(if ok { sound::CLICK } else { sound::ERROR });
                ok
            }
            _ => false,
        },
        Phase::PlayerSelectSpecial => match ch {
            '0' | '-' => {
                state.phase = Phase::PlayerAction;
                sound::play(sound::CLICK);
                true
            }
            d if d.is_ascii_digit() => {
                let idx = (d as u32 - '1' as u32) as usize;
                let ok = logic::human_use_special(state, idx);
                sound::play(if ok { sound::CLICK } else { sound::ERROR });
                ok
            }
            _ => false,
        },
        Phase::CpuTurn { .. } | Phase::BetweenTurns { .. } => false,
    }
}

// ── Click dispatch ─────────────────────────────────────────────

fn handle_click(state: &mut GfState, id: u16) -> bool {
    match state.phase {
        Phase::Intro => {
            if id == ACTION_START {
                logic::begin_turn(state);
                sound::play(sound::CLICK);
                return true;
            }
            false
        }
        Phase::Victory | Phase::Defeat => {
            if id == ACTION_RESTART {
                *state = GfState::new(state.rng_seed.wrapping_add(1));
                logic::begin_turn(state);
                sound::play(sound::CLICK);
                return true;
            }
            false
        }
        Phase::PlayerAction => match id {
            ACTION_ATTACK => {
                let ok = enter_attack_phase(state);
                sound::play(if ok { sound::CLICK } else { sound::ERROR });
                ok
            }
            ACTION_HEAL => {
                let ok = enter_heal_phase(state);
                sound::play(if ok { sound::CLICK } else { sound::ERROR });
                ok
            }
            ACTION_SPECIAL => {
                let ok = enter_special_phase(state);
                sound::play(if ok { sound::CLICK } else { sound::ERROR });
                ok
            }
            ACTION_PASS => {
                logic::human_pass(state);
                sound::play(sound::CLICK);
                true
            }
            _ => false,
        },
        Phase::PlayerSelectWeapons => {
            if id == ACTION_CANCEL {
                state.selected_weapons.clear();
                state.phase = Phase::PlayerAction;
                sound::play(sound::CLICK);
                return true;
            }
            if id == ACTION_CONFIRM_WEAPONS {
                let ok = logic::confirm_weapons(state);
                sound::play(if ok { sound::CLICK } else { sound::ERROR });
                return ok;
            }
            if (HAND_BASE..HAND_BASE + 16).contains(&id) {
                let idx = (id - HAND_BASE) as usize;
                logic::toggle_weapon_selection(state, idx);
                sound::play(sound::CLICK);
                return true;
            }
            false
        }
        Phase::PlayerSelectTarget => {
            if id == ACTION_CANCEL {
                state.phase = Phase::PlayerSelectWeapons;
                sound::play(sound::CLICK);
                return true;
            }
            if (TARGET_BASE..TARGET_BASE + 16).contains(&id) {
                let idx = (id - TARGET_BASE) as usize;
                let ok = logic::human_attack(state, idx);
                sound::play(if ok { sound::CLICK } else { sound::ERROR });
                return ok;
            }
            false
        }
        Phase::PlayerSelectHeal => {
            if id == ACTION_CANCEL {
                state.phase = Phase::PlayerAction;
                sound::play(sound::CLICK);
                return true;
            }
            if (HAND_BASE..HAND_BASE + 16).contains(&id) {
                let idx = (id - HAND_BASE) as usize;
                let ok = logic::human_heal(state, idx);
                sound::play(if ok { sound::CLICK } else { sound::ERROR });
                return ok;
            }
            false
        }
        Phase::PlayerSelectSpecial => {
            if id == ACTION_CANCEL {
                state.phase = Phase::PlayerAction;
                sound::play(sound::CLICK);
                return true;
            }
            if (HAND_BASE..HAND_BASE + 16).contains(&id) {
                let idx = (id - HAND_BASE) as usize;
                let ok = logic::human_use_special(state, idx);
                sound::play(if ok { sound::CLICK } else { sound::ERROR });
                return ok;
            }
            false
        }
        Phase::CpuTurn { .. } | Phase::BetweenTurns { .. } => false,
    }
}

// ── Phase-entry helpers ────────────────────────────────────────

fn enter_attack_phase(state: &mut GfState) -> bool {
    let h = state.human_idx();
    let has_weapon = state.players[h].hand.iter().any(|c| c.kind() == CardKind::Weapon);
    if !has_weapon { return false; }
    state.selected_weapons.clear();
    state.phase = Phase::PlayerSelectWeapons;
    true
}

fn enter_heal_phase(state: &mut GfState) -> bool {
    let h = state.human_idx();
    let has_heal = state.players[h].hand.iter().any(|c| c.kind() == CardKind::Heal);
    if !has_heal { return false; }
    state.phase = Phase::PlayerSelectHeal;
    true
}

fn enter_special_phase(state: &mut GfState) -> bool {
    let h = state.human_idx();
    let has_special = state.players[h].hand.iter()
        .any(|c| c.kind() == CardKind::Special && *c != Card::Reflect);
    if !has_special { return false; }
    state.phase = Phase::PlayerSelectSpecial;
    true
}

// ── Tests ──────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::ClickScope;

    fn make_game() -> GodFieldGame {
        let mut g = GodFieldGame::new();
        // Skip intro
        g.state.phase = Phase::PlayerAction;
        g
    }

    fn click(id: u16) -> InputEvent {
        InputEvent::Click(ClickScope::Game(GameChoice::Godfield), id)
    }

    #[test]
    fn intro_to_player_action() {
        let mut g = GodFieldGame::new();
        assert_eq!(g.state.phase, Phase::Intro);
        g.handle_input(&click(ACTION_START));
        assert_eq!(g.state.phase, Phase::PlayerAction);
    }

    #[test]
    fn intro_key_starts_game() {
        let mut g = GodFieldGame::new();
        g.handle_input(&InputEvent::Key(' '));
        assert_eq!(g.state.phase, Phase::PlayerAction);
    }

    #[test]
    fn attack_path_full() {
        let mut g = make_game();
        g.state.players[0].hand = vec![Card::Sword, Card::Shield];
        // Enter weapon picker
        g.handle_input(&click(ACTION_ATTACK));
        assert_eq!(g.state.phase, Phase::PlayerSelectWeapons);
        // Select weapon
        g.handle_input(&click(HAND_BASE));
        assert_eq!(g.state.selected_weapons, vec![0]);
        // Confirm
        g.handle_input(&click(ACTION_CONFIRM_WEAPONS));
        assert_eq!(g.state.phase, Phase::PlayerSelectTarget);
        // Pick target (player 1)
        let hp_before = g.state.players[1].hp;
        g.handle_input(&click(TARGET_BASE + 1));
        assert!(g.state.players[1].hp <= hp_before);
        // Turn moved off human.
        assert_ne!(g.state.turn, 0);
    }

    #[test]
    fn attack_disabled_without_weapons() {
        let mut g = make_game();
        g.state.players[0].hand = vec![Card::Shield, Card::Herb];
        let consumed = g.handle_input(&click(ACTION_ATTACK));
        assert!(!consumed);
        assert_eq!(g.state.phase, Phase::PlayerAction);
    }

    #[test]
    fn cancel_returns_to_action() {
        let mut g = make_game();
        g.state.players[0].hand = vec![Card::Sword];
        g.handle_input(&click(ACTION_ATTACK));
        g.handle_input(&click(ACTION_CANCEL));
        assert_eq!(g.state.phase, Phase::PlayerAction);
        assert!(g.state.selected_weapons.is_empty());
    }

    #[test]
    fn heal_path() {
        let mut g = make_game();
        g.state.players[0].hp = 10;
        g.state.players[0].hand = vec![Card::Herb];
        g.handle_input(&click(ACTION_HEAL));
        assert_eq!(g.state.phase, Phase::PlayerSelectHeal);
        g.handle_input(&click(HAND_BASE));
        assert!(g.state.players[0].hp > 10);
        assert_ne!(g.state.turn, 0);
    }

    #[test]
    fn pass_advances_turn() {
        let mut g = make_game();
        g.handle_input(&click(ACTION_PASS));
        assert_ne!(g.state.turn, 0);
    }

    #[test]
    fn restart_after_victory() {
        let mut g = make_game();
        g.state.phase = Phase::Victory;
        g.handle_input(&click(ACTION_RESTART));
        assert_eq!(g.state.phase, Phase::PlayerAction);
    }

    #[test]
    fn key_attack_target_uses_player_index() {
        let mut g = make_game();
        g.state.players[0].hand = vec![Card::Sword];
        g.handle_input(&InputEvent::Key('a'));
        g.handle_input(&InputEvent::Key('1')); // select sword
        g.handle_input(&InputEvent::Key(' ')); // confirm
        assert_eq!(g.state.phase, Phase::PlayerSelectTarget);
        let hp_before = g.state.players[1].hp;
        g.handle_input(&InputEvent::Key('1')); // attack player 1
        assert!(g.state.players[1].hp <= hp_before);
    }
}
