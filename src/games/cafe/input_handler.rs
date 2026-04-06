//! Input handling for the Café game — keyboard and click dispatch.

use super::actions::*;
use super::affinity::ActionType;
use super::cards::{self, GACHA_SINGLE_COST, GACHA_TEN_COST};
use super::logic;
use super::save;
use super::social::{self, MissionType, BUSINESS_DAY_COST};
use super::state::{CafeState, GamePhase, HubTab};

// ── Keyboard ──────────────────────────────────────────────

pub fn handle_key(state: &mut CafeState, ch: char) -> bool {
    // Dismiss popups first
    if state.pending_login_reward.is_some() || state.pending_recovery_bonus.is_some() {
        return dismiss_popup(state);
    }

    match &state.phase {
        GamePhase::Story => match ch {
            ' ' | 'l' => logic::advance_story(state),
            _ => false,
        },
        GamePhase::Hub => match ch {
            '1' => { state.hub_tab = HubTab::Home; true }
            '2' => { state.hub_tab = HubTab::Characters; true }
            '3' => { state.hub_tab = HubTab::Cards; true }
            '4' => { state.hub_tab = HubTab::Missions; true }
            's' => {
                if let Some(ch_num) = logic::next_available_chapter(state) {
                    logic::start_chapter(state, ch_num);
                    true
                } else {
                    false
                }
            }
            'b' => try_run_business(state),
            'c' => { state.phase = GamePhase::CharacterSelect; true }
            'g' => { state.phase = GamePhase::CardScreen; true }
            'q' => { save::save_game(state); false }
            _ => false,
        },
        GamePhase::CharacterSelect => match ch {
            '1'..='5' => {
                let idx = (ch as u8 - b'1') as usize;
                let unlocked = state.unlocked_characters();
                if idx < unlocked.len() {
                    state.phase = GamePhase::ActionSelect { target: unlocked[idx] };
                    true
                } else {
                    false
                }
            }
            'q' => { state.phase = GamePhase::Hub; true }
            _ => false,
        },
        GamePhase::ActionSelect { target } => {
            let target = *target;
            match ch {
                'e' | '1' => logic::perform_action(state, target, ActionType::Eat),
                'o' | '2' => logic::perform_action(state, target, ActionType::Observe),
                't' | '3' => logic::perform_action(state, target, ActionType::Talk),
                's' | '4' => logic::perform_action(state, target, ActionType::Special),
                'q' => { state.phase = GamePhase::CharacterSelect; true }
                _ => false,
            }
        }
        GamePhase::ActionResult { .. } => match ch {
            ' ' | 'l' => {
                state.phase = GamePhase::Hub;
                save::save_game(state);
                logic::check_memory_unlocks(state);
                true
            }
            _ => false,
        },
        GamePhase::CardScreen => match ch {
            'd' => try_daily_draw(state),
            'g' => try_gacha_single(state),
            'q' => { state.phase = GamePhase::Hub; true }
            _ => false,
        },
        GamePhase::GachaResult { .. } => match ch {
            ' ' | 'l' => {
                state.phase = GamePhase::CardScreen;
                save::save_game(state);
                true
            }
            _ => false,
        },
        GamePhase::CharacterDetail { .. } => match ch {
            'q' => { state.phase = GamePhase::Hub; true }
            _ => false,
        },
        GamePhase::DayResult => match ch {
            ' ' | 'l' => {
                logic::next_day(state);
                save::save_game(state);
                true
            }
            _ => false,
        },
    }
}

// ── Click ─────────────────────────────────────────────────

pub fn handle_click(state: &mut CafeState, id: u16) -> bool {
    if (state.pending_login_reward.is_some() || state.pending_recovery_bonus.is_some())
        && id == STORY_ADVANCE
    {
        return dismiss_popup(state);
    }

    match &state.phase {
        GamePhase::Story => {
            if id == STORY_ADVANCE { return logic::advance_story(state); }
            false
        }
        GamePhase::Hub => match id {
            TAB_HOME => { state.hub_tab = HubTab::Home; true }
            TAB_CHARACTERS => { state.hub_tab = HubTab::Characters; true }
            TAB_CARDS => { state.hub_tab = HubTab::Cards; true }
            TAB_MISSIONS => { state.hub_tab = HubTab::Missions; true }
            HUB_STORY => {
                if let Some(ch_num) = logic::next_available_chapter(state) {
                    logic::start_chapter(state, ch_num);
                    true
                } else {
                    false
                }
            }
            HUB_BUSINESS => try_run_business(state),
            id if (CHARACTER_BASE..CHARACTER_BASE + 5).contains(&id) => {
                let idx = (id - CHARACTER_BASE) as usize;
                let unlocked = state.unlocked_characters();
                if idx < unlocked.len() {
                    state.phase = GamePhase::CharacterSelect;
                    true
                } else {
                    false
                }
            }
            CARD_DAILY_DRAW => { state.phase = GamePhase::CardScreen; true }
            _ => false,
        },
        GamePhase::CharacterSelect => {
            if id == CHARACTER_BACK { state.phase = GamePhase::Hub; return true; }
            if (CHARACTER_BASE..CHARACTER_BASE + 5).contains(&id) {
                let idx = (id - CHARACTER_BASE) as usize;
                let unlocked = state.unlocked_characters();
                if idx < unlocked.len() {
                    state.phase = GamePhase::ActionSelect { target: unlocked[idx] };
                    return true;
                }
            }
            if (DETAIL_EPISODE_BASE..DETAIL_EPISODE_BASE + 5).contains(&id) {
                let idx = (id - DETAIL_EPISODE_BASE) as usize;
                let unlocked = state.unlocked_characters();
                if idx < unlocked.len() {
                    state.phase = GamePhase::CharacterDetail { target: unlocked[idx] };
                    return true;
                }
            }
            false
        }
        GamePhase::ActionSelect { target } => {
            let target = *target;
            match id {
                ACTION_EAT => logic::perform_action(state, target, ActionType::Eat),
                ACTION_OBSERVE => logic::perform_action(state, target, ActionType::Observe),
                ACTION_TALK => logic::perform_action(state, target, ActionType::Talk),
                ACTION_SPECIAL => logic::perform_action(state, target, ActionType::Special),
                ACTION_BACK => { state.phase = GamePhase::CharacterSelect; true }
                _ => false,
            }
        }
        GamePhase::ActionResult { .. } => {
            if id == RESULT_OK {
                state.phase = GamePhase::Hub;
                save::save_game(state);
                logic::check_memory_unlocks(state);
                return true;
            }
            false
        }
        GamePhase::CardScreen => match id {
            CARD_DAILY_DRAW => try_daily_draw(state),
            CARD_GACHA_SINGLE => try_gacha_single(state),
            CARD_GACHA_TEN => try_gacha_ten(state),
            CARD_BACK => { state.phase = GamePhase::Hub; true }
            id if (CARD_EQUIP_BASE..CARD_EQUIP_BASE + 20).contains(&id) => {
                let idx = (id - CARD_EQUIP_BASE) as usize;
                if idx < state.card_state.cards.len() {
                    state.card_state.equipped_card = Some(idx);
                    save::save_game(state);
                    true
                } else {
                    false
                }
            }
            _ => false,
        },
        GamePhase::GachaResult { .. } => {
            if id == GACHA_RESULT_OK {
                state.phase = GamePhase::CardScreen;
                save::save_game(state);
                return true;
            }
            false
        }
        GamePhase::CharacterDetail { .. } => {
            if id == DETAIL_BACK { state.phase = GamePhase::Hub; return true; }
            false
        }
        GamePhase::DayResult => {
            if id == SERVE_CONFIRM || id == DAY_RESULT_OK {
                logic::next_day(state);
                save::save_game(state);
                return true;
            }
            false
        }
    }
}

// ── Helpers ───────────────────────────────────────────────

fn try_run_business(state: &mut CafeState) -> bool {
    let now = social::now_ms();
    if !state.stamina.consume(BUSINESS_DAY_COST, now) {
        return false;
    }
    logic::run_business_day(state);
    state.today_business_runs += 1;
    state.daily_missions.record(MissionType::RunBusiness(state.today_business_runs));
    save::save_game(state);
    true
}

fn try_daily_draw(state: &mut CafeState) -> bool {
    if state.card_state.daily_draw_used { return false; }
    let seed = (social::now_ms() as u32).wrapping_mul(2654435761);
    let card_ids = cards::daily_draw(&mut state.card_state, seed);
    state.phase = GamePhase::GachaResult { card_ids };
    save::save_game(state);
    true
}

fn try_gacha_single(state: &mut CafeState) -> bool {
    if state.card_state.gems < GACHA_SINGLE_COST { return false; }
    state.card_state.gems -= GACHA_SINGLE_COST;
    let seed = (social::now_ms() as u32).wrapping_mul(2654435761);
    let card_id = cards::gacha_pull(&mut state.card_state, seed);
    state.phase = GamePhase::GachaResult { card_ids: vec![card_id] };
    save::save_game(state);
    true
}

fn try_gacha_ten(state: &mut CafeState) -> bool {
    if state.card_state.gems < GACHA_TEN_COST { return false; }
    state.card_state.gems -= GACHA_TEN_COST;
    let base_seed = (social::now_ms() as u32).wrapping_mul(2654435761);
    let mut card_ids = Vec::new();
    for i in 0..10u32 {
        let seed = base_seed.wrapping_add(i * 37);
        let card_id = cards::gacha_pull(&mut state.card_state, seed);
        card_ids.push(card_id);
    }
    state.phase = GamePhase::GachaResult { card_ids };
    save::save_game(state);
    true
}

pub fn dismiss_popup(state: &mut CafeState) -> bool {
    if let Some(reward) = state.pending_login_reward.take() {
        state.money += reward;
        state.login_bonus.today_claimed = true;
        save::save_game(state);
        return true;
    }
    if let Some(bonus) = state.pending_recovery_bonus.take() {
        state.money += bonus;
        save::save_game(state);
        return true;
    }
    false
}
