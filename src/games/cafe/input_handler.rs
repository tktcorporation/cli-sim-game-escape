//! Input handling for the Café game — keyboard and click dispatch.

use super::actions::*;
use super::characters::ActionType;
use super::gacha::{self, GACHA_SINGLE_COST, GACHA_TEN_COST};
use super::logic;
use super::produce::TrainingType;
use super::save;
use super::social_sys::{self, MissionType, BUSINESS_DAY_COST};
use super::state::{CafeState, GamePhase, HubTab};

/// `CARDS_VISIBLE` の u16 化版 — `Range<u16>` で contains に使うため。
const CARDS_VISIBLE_U16: u16 = CARDS_VISIBLE as u16;

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
            '1' => { switch_hub_tab(state, HubTab::Home); true }
            '2' => { switch_hub_tab(state, HubTab::Characters); true }
            '3' => { switch_hub_tab(state, HubTab::Cards); true }
            '4' => { switch_hub_tab(state, HubTab::Produce); true }
            '5' => { switch_hub_tab(state, HubTab::Missions); true }
            'j' if state.hub_tab == HubTab::Cards => { scroll_cards(state, SCROLL_STEP as i32); true }
            'k' if state.hub_tab == HubTab::Cards => { scroll_cards(state, -(SCROLL_STEP as i32)); true }
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
            'p' => { state.phase = GamePhase::ProduceCharSelect; true }
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
        GamePhase::CharacterDetail { .. } => match ch {
            'p' => {
                // Try star promotion
                if let GamePhase::CharacterDetail { target } = &state.phase {
                    let target = *target;
                    logic::try_promote_character(state, target);
                }
                true
            }
            'q' => { state.phase = GamePhase::Hub; true }
            _ => false,
        },
        GamePhase::CardScreen => match ch {
            'd' => try_daily_draw(state),
            'g' => try_gacha_single(state),
            'j' => { scroll_cards(state, SCROLL_STEP as i32); true }
            'k' => { scroll_cards(state, -(SCROLL_STEP as i32)); true }
            'q' => { state.phase = GamePhase::Hub; true }
            _ => false,
        },
        GamePhase::GachaResult { card_ids } => match ch {
            ' ' | 'l' => {
                let ids = card_ids.clone();
                handle_gacha_result_ok(state, &ids)
            }
            'j' => { scroll_gacha_result(state, SCROLL_STEP as i32); true }
            'k' => { scroll_gacha_result(state, -(SCROLL_STEP as i32)); true }
            _ => false,
        },
        // ── Produce ──────────────────────────────────
        GamePhase::ProduceCharSelect => match ch {
            '1'..='5' => {
                let idx = (ch as u8 - b'1') as usize;
                let unlocked = state.unlocked_characters();
                if idx < unlocked.len() {
                    logic::start_produce(state, unlocked[idx])
                } else {
                    false
                }
            }
            'q' => { state.phase = GamePhase::Hub; true }
            _ => false,
        },
        GamePhase::ProduceTraining => match ch {
            '1' => logic::produce_train(state, TrainingType::Service),
            '2' => logic::produce_train(state, TrainingType::Cooking),
            '3' => logic::produce_train(state, TrainingType::Atmosphere),
            '4' => logic::produce_train(state, TrainingType::Rest),
            _ => false,
        },
        GamePhase::ProduceTurnResult { .. } => match ch {
            ' ' | 'l' => {
                state.phase = GamePhase::ProduceTraining;
                true
            }
            _ => false,
        },
        GamePhase::ProduceResult => match ch {
            ' ' | 'l' => {
                logic::finish_produce(state);
                save::save_game(state);
                true
            }
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
            TAB_HOME => { switch_hub_tab(state, HubTab::Home); true }
            TAB_CHARACTERS => { switch_hub_tab(state, HubTab::Characters); true }
            TAB_CARDS => { switch_hub_tab(state, HubTab::Cards); true }
            TAB_PRODUCE => { switch_hub_tab(state, HubTab::Produce); true }
            TAB_MISSIONS => { switch_hub_tab(state, HubTab::Missions); true }
            CARD_SCROLL_UP => { scroll_cards(state, -(SCROLL_STEP as i32)); true }
            CARD_SCROLL_DOWN => { scroll_cards(state, SCROLL_STEP as i32); true }
            HUB_STORY => {
                if let Some(ch_num) = logic::next_available_chapter(state) {
                    logic::start_chapter(state, ch_num);
                    true
                } else {
                    false
                }
            }
            HUB_BUSINESS => try_run_business(state),
            HUB_PRODUCE => { state.phase = GamePhase::ProduceCharSelect; true }
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
        GamePhase::CharacterDetail { target } => {
            let target = *target;
            if id == DETAIL_BACK { state.phase = GamePhase::Hub; return true; }
            if id == DETAIL_PROMOTE {
                logic::try_promote_character(state, target);
                save::save_game(state);
                return true;
            }
            false
        }
        GamePhase::CardScreen => match id {
            CARD_DAILY_DRAW => try_daily_draw(state),
            CARD_GACHA_SINGLE => try_gacha_single(state),
            CARD_GACHA_TEN => try_gacha_ten(state),
            CARD_SCROLL_UP => { scroll_cards(state, -(SCROLL_STEP as i32)); true }
            CARD_SCROLL_DOWN => { scroll_cards(state, SCROLL_STEP as i32); true }
            CARD_BACK => { state.phase = GamePhase::Hub; true }
            id if (CARD_EQUIP_BASE..CARD_EQUIP_BASE + CARDS_VISIBLE_U16).contains(&id) => {
                equip_card_by_display_offset(state, (id - CARD_EQUIP_BASE) as usize)
            }
            _ => false,
        },
        GamePhase::GachaResult { card_ids } => {
            if id == GACHA_RESULT_OK {
                let ids = card_ids.clone();
                return handle_gacha_result_ok(state, &ids);
            }
            if id == GACHA_RESULT_SCROLL_UP {
                scroll_gacha_result(state, -(SCROLL_STEP as i32));
                return true;
            }
            if id == GACHA_RESULT_SCROLL_DOWN {
                scroll_gacha_result(state, SCROLL_STEP as i32);
                return true;
            }
            false
        }
        // ── Produce ──────────────────────────────────
        GamePhase::ProduceCharSelect => {
            if id == PRODUCE_BACK { state.phase = GamePhase::Hub; return true; }
            if (PRODUCE_CHAR_BASE..PRODUCE_CHAR_BASE + 5).contains(&id) {
                let idx = (id - PRODUCE_CHAR_BASE) as usize;
                let unlocked = state.unlocked_characters();
                if idx < unlocked.len() {
                    return logic::start_produce(state, unlocked[idx]);
                }
            }
            false
        }
        GamePhase::ProduceTraining => match id {
            PRODUCE_TRAIN_SERVICE => logic::produce_train(state, TrainingType::Service),
            PRODUCE_TRAIN_COOKING => logic::produce_train(state, TrainingType::Cooking),
            PRODUCE_TRAIN_ATMOSPHERE => logic::produce_train(state, TrainingType::Atmosphere),
            PRODUCE_TRAIN_REST => logic::produce_train(state, TrainingType::Rest),
            _ => false,
        },
        GamePhase::ProduceTurnResult { .. } => {
            if id == PRODUCE_CONTINUE {
                state.phase = GamePhase::ProduceTraining;
                return true;
            }
            false
        }
        GamePhase::ProduceResult => {
            if id == PRODUCE_FINISH {
                logic::finish_produce(state);
                save::save_game(state);
                return true;
            }
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

/// Tap or arrow-key step for the Cards-tab scroll. Same magnitude as the
/// Abyss tab (`SCROLL_STEP = 3`) — the upper bound is clamped at render time
/// so saturating-add is safe here.
const SCROLL_STEP: u16 = 3;

/// Switch the active hub tab. Resets `cards_scroll` so re-entering Cards
/// starts at the top instead of stranding the user mid-list.
fn switch_hub_tab(state: &mut CafeState, tab: HubTab) {
    state.hub_tab = tab;
    state.cards_scroll.set(0);
}

/// Apply a signed delta to `cards_scroll`. Lower bound is 0; upper bound is
/// clamped against actual content height in `render_hub_cards`.
fn scroll_cards(state: &mut CafeState, delta: i32) {
    let cur = state.cards_scroll.get() as i32;
    let next = (cur + delta).max(0) as u16;
    state.cards_scroll.set(next);
}

/// Same shape as `scroll_cards` but for the GachaResult screen — clamping
/// against `max_scroll` happens in `render_gacha_result` since the line
/// count grows per anim frame.
fn scroll_gacha_result(state: &mut CafeState, delta: i32) {
    let cur = state.gacha_result_scroll.get() as i32;
    let next = (cur + delta).max(0) as u16;
    state.gacha_result_scroll.set(next);
}

/// Resolve a Cards-tab display row (0..CARDS_VISIBLE) back to the actual
/// `cards` vec index. Renderer shows the **last** `CARDS_VISIBLE` cards so
/// recently-pulled cards are reachable; this inverse mapping is the
/// counterpart and is the single place that knows the policy.
fn equip_card_by_display_offset(state: &mut CafeState, offset: usize) -> bool {
    let total = state.card_state.cards.len();
    let start = total.saturating_sub(CARDS_VISIBLE);
    let actual_idx = start + offset;
    if actual_idx >= total {
        return false;
    }
    state.card_state.equipped_card = Some(actual_idx);
    save::save_game(state);
    true
}

fn try_run_business(state: &mut CafeState) -> bool {
    let now = social_sys::now_ms();
    if !state.stamina.consume(BUSINESS_DAY_COST, now) {
        return false;
    }
    logic::run_business_day(state);
    state.today_business_runs += 1;
    state.daily_missions.record(MissionType::RunBusiness(state.today_business_runs));
    state.weekly_missions.record(MissionType::RunBusiness(state.today_business_runs));
    save::save_game(state);
    true
}

fn try_daily_draw(state: &mut CafeState) -> bool {
    if state.card_state.daily_draw_used { return false; }
    let seed = (social_sys::now_ms() as u32).wrapping_mul(2654435761);
    let card_ids = gacha::daily_draw(&mut state.card_state, seed);
    enter_gacha_result(state, card_ids);
    save::save_game(state);
    true
}

fn try_gacha_single(state: &mut CafeState) -> bool {
    if state.card_state.gems < GACHA_SINGLE_COST { return false; }
    state.card_state.gems -= GACHA_SINGLE_COST;
    let seed = (social_sys::now_ms() as u32).wrapping_mul(2654435761);
    let banners = gacha::active_banners();
    let banner = &banners[0]; // Standard banner
    let card_id = gacha::gacha_pull(&mut state.card_state, seed, banner);
    // Mission tracking
    state.daily_missions.record(MissionType::GachaPull(1));
    state.weekly_missions.record(MissionType::GachaPull(1));
    enter_gacha_result(state, vec![card_id]);
    save::save_game(state);
    true
}

fn try_gacha_ten(state: &mut CafeState) -> bool {
    if state.card_state.gems < GACHA_TEN_COST { return false; }
    state.card_state.gems -= GACHA_TEN_COST;
    let base_seed = (social_sys::now_ms() as u32).wrapping_mul(2654435761);
    let banners = gacha::active_banners();
    let banner = &banners[0];
    let mut card_ids = Vec::new();
    for i in 0..10u32 {
        let seed = base_seed.wrapping_add(i * 37);
        let card_id = gacha::gacha_pull(&mut state.card_state, seed, banner);
        card_ids.push(card_id);
    }
    // Mission tracking
    for _ in 0..10 {
        state.daily_missions.record(MissionType::GachaPull(1));
        state.weekly_missions.record(MissionType::GachaPull(1));
    }
    enter_gacha_result(state, card_ids);
    save::save_game(state);
    true
}

/// Transition into `GachaResult` and arm the reveal animation.
fn enter_gacha_result(state: &mut CafeState, card_ids: Vec<u32>) {
    state.gacha_anim_frame = 0;
    state.gacha_result_scroll.set(0);
    state.phase = GamePhase::GachaResult { card_ids };
}

/// First click on OK during the reveal anim skips to the end (showing all
/// cards instantly); a second click dismisses the result and saves.
/// Returns true if the click was handled.
fn handle_gacha_result_ok(state: &mut CafeState, card_ids: &[u32]) -> bool {
    let total = card_ids.len();
    if !gacha::gacha_anim_is_complete(state.gacha_anim_frame, total) {
        // Skip the staged reveal — jump straight to "all cards visible, OK ready".
        state.gacha_anim_frame = gacha::gacha_anim_complete_frame(total);
        true
    } else {
        state.phase = GamePhase::CardScreen;
        save::save_game(state);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::gacha::state::OwnedCard;

    fn push_dummy_cards(state: &mut CafeState, n: u32) {
        for id in 1..=n {
            state.card_state.cards.push(OwnedCard {
                card_id: id,
                level: 1,
                rank_ups: 0,
                duplicates: 0,
            });
        }
    }

    /// 所持枚数 > CARDS_VISIBLE の時、display offset 14 のクリックは
    /// 最新カード (= 末尾) を装備する。レンダラの「最新 15 枚を表示」
    /// 仕様と一貫してないと、正しいカードが装備されない回帰の検出。
    #[test]
    fn equip_by_display_offset_resolves_to_latest_card() {
        let mut state = CafeState::new();
        push_dummy_cards(&mut state, 20);
        // display offset 14 = 最新表示行 → cards[19]
        let ok = equip_card_by_display_offset(&mut state, 14);
        assert!(ok);
        assert_eq!(state.card_state.equipped_card, Some(19));
    }

    /// 所持 <= CARDS_VISIBLE では display offset = 元 index と一致 (start=0)。
    /// 既存セーブとの後方互換性を保証する条件。
    #[test]
    fn equip_by_display_offset_matches_index_when_under_cap() {
        let mut state = CafeState::new();
        push_dummy_cards(&mut state, 5);
        let ok = equip_card_by_display_offset(&mut state, 3);
        assert!(ok);
        assert_eq!(state.card_state.equipped_card, Some(3));
    }

    /// 表示行外の display offset は何もしない (range 外 ID は struct
    /// match で弾かれるが、念のため helper レベルでも安全に)。
    #[test]
    fn equip_by_display_offset_returns_false_when_out_of_range() {
        let mut state = CafeState::new();
        push_dummy_cards(&mut state, 5);
        // 5 枚しか無いのに offset 10 を要求 → start=0+10=10 > 5
        let ok = equip_card_by_display_offset(&mut state, 10);
        assert!(!ok);
        assert_eq!(state.card_state.equipped_card, None);
    }
}

pub fn dismiss_popup(state: &mut CafeState) -> bool {
    if let Some(reward) = state.pending_login_reward.take() {
        state.money += reward;
        // Also claim gems
        if let Some(gems) = state.pending_login_gems.take() {
            state.card_state.gems += gems;
        }
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
