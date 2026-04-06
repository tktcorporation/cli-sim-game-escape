//! Pure functions for Café game state transitions.
//!
//! Revamped with AP action system, affinity gains, and card effects.

use super::affinity::{ActionType, CharacterId};
use super::cards;
use super::scenario::{get_chapter_scenes, PROLOGUE_SCENES};
use super::state::{CafeState, CustomerVisit, GamePhase, AP_MAX};

// ── Story Progression ─────────────────────────────────────

/// Get scenes for the current chapter.
fn current_scenes(state: &CafeState) -> &'static [&'static super::state::StoryScene] {
    if state.current_chapter == 0 {
        PROLOGUE_SCENES
    } else {
        get_chapter_scenes(state.current_chapter)
    }
}

/// Advance the story by one line. Returns true if consumed.
pub fn advance_story(state: &mut CafeState) -> bool {
    if state.phase != GamePhase::Story {
        return false;
    }

    let scenes = current_scenes(state);
    let scene_count = scenes.len();
    if scene_count == 0 || state.current_scene_index >= scene_count {
        // Chapter done → go to hub
        finish_chapter(state);
        return true;
    }

    let scene = scenes[state.current_scene_index];
    if state.current_line_index + 1 < scene.lines.len() {
        state.current_line_index += 1;
    } else {
        state.current_scene_index += 1;
        state.current_line_index = 0;

        if state.current_scene_index >= scene_count {
            finish_chapter(state);
        }
    }
    true
}

/// Finish the current chapter and return to hub.
fn finish_chapter(state: &mut CafeState) {
    if state.current_chapter > state.chapters_completed {
        state.chapters_completed = state.current_chapter;
    }
    // First completion of Ch.0 → mark prologue done
    if state.current_chapter == 0 && state.chapters_completed == 0 {
        state.chapters_completed = 0; // Prologue complete = chapters_completed >= 0
    }

    // Unlock characters based on chapter
    unlock_characters_for_chapter(state, state.current_chapter);

    // Reward gems for chapter completion
    let gem_reward = match state.current_chapter {
        0 => 300,
        1 => 200,
        2 => 200,
        3 => 300,
        _ => 150,
    };
    state.card_state.gems += gem_reward;

    state.phase = GamePhase::Hub;
}

/// Unlock characters that appear in a given chapter.
fn unlock_characters_for_chapter(state: &mut CafeState, chapter: u32) {
    for &ch in CharacterId::ALL {
        if ch.unlock_chapter() <= chapter {
            if let Some(aff) = state.affinities.get_mut(&ch) {
                aff.unlocked = true;
            }
        }
    }
}

/// Start reading a chapter.
pub fn start_chapter(state: &mut CafeState, chapter: u32) {
    state.current_chapter = chapter;
    state.current_scene_index = 0;
    state.current_line_index = 0;
    state.phase = GamePhase::Story;
}

/// Get the next chapter available to start.
pub fn next_available_chapter(state: &CafeState) -> Option<u32> {
    let next = state.chapters_completed + 1;
    if next <= state.player_rank.max_chapter() {
        // Check if chapter content exists
        let scenes = get_chapter_scenes(next);
        if !scenes.is_empty() {
            return Some(next);
        }
    }
    None
}

// ── AP Actions ────────────────────────────────────────────

/// Perform an action on a character. Returns true if successful.
pub fn perform_action(state: &mut CafeState, target: CharacterId, action: ActionType) -> bool {
    let cost = action.ap_cost();
    if state.ap_current < cost {
        return false;
    }

    // Check character is unlocked
    let affinity = match state.affinities.get(&target) {
        Some(a) if a.unlocked => a.clone(),
        _ => return false,
    };

    // Special action requires star rank ≥ 2
    if action == ActionType::Special && affinity.axes.star_rank() < 2 {
        return false;
    }

    // Consume AP
    state.ap_current -= cost;
    state.actions_today += 1;

    // Calculate gains
    let base = action.base_gains();

    // Card multiplier
    let card_mult = state.card_state.equipped_multiplier();
    let mut gains = base.multiply(card_mult);

    // Card bonus axis: +50% to matching axis
    if let Some(bonus_axis) = state.card_state.equipped_bonus_axis() {
        match bonus_axis {
            cards::BonusAxis::Trust => gains.trust = (gains.trust as f64 * 1.5) as u32,
            cards::BonusAxis::Understanding => {
                gains.understanding = (gains.understanding as f64 * 1.5) as u32
            }
            cards::BonusAxis::Empathy => gains.empathy = (gains.empathy as f64 * 1.5) as u32,
            cards::BonusAxis::Balanced => {
                gains.trust = (gains.trust as f64 * 1.15) as u32;
                gains.understanding = (gains.understanding as f64 * 1.15) as u32;
                gains.empathy = (gains.empathy as f64 * 1.15) as u32;
            }
        }
    }

    // Memory bonuses (flat add)
    let (mem_trust, mem_understanding, mem_empathy) = state.memory_bonuses();
    gains.trust += mem_trust;
    gains.understanding += mem_understanding;
    gains.empathy += mem_empathy;

    // Apply gains to character
    if let Some(aff) = state.affinities.get_mut(&target) {
        aff.axes.trust += gains.trust;
        aff.axes.understanding += gains.understanding;
        aff.axes.empathy += gains.empathy;
    }

    // Player rank EXP: 10 base + 5 per AP cost
    state.player_rank.add_exp(10 + cost * 5);

    // Coins from action
    state.card_state.coins += 5 + state.player_rank.level;

    // Set phase to show result
    state.phase = GamePhase::ActionResult {
        target,
        action,
        trust_gain: gains.trust,
        understanding_gain: gains.understanding,
        empathy_gain: gains.empathy,
    };

    true
}

// ── Daily Reset ───────────────────────────────────────────

/// Reset daily state (AP, actions, etc.).
pub fn daily_reset(state: &mut CafeState) {
    state.ap_current = AP_MAX;
    state.actions_today = 0;
    state.today_business_runs = 0;
}

// ── Business Day (simplified, kept for backward compat) ──

/// Run a simplified day of business.
pub fn run_business_day(state: &mut CafeState) {
    state.today_visits.clear();

    let customer_count = 3 + (state.day as usize % 3);
    let customer_names = ["サラリーマン", "OL", "学生", "主婦", "老紳士"];

    for i in 0..customer_count {
        let name = customer_names[i % customer_names.len()];
        let menu_idx = i % state.menu.len();
        let item = &state.menu[menu_idx];

        let visit = CustomerVisit {
            name,
            order: item.name,
            satisfied: true,
            revenue: item.price,
        };
        state.today_visits.push(visit);
    }

    let revenue = state.today_revenue() as i64;
    let cost = state.today_cost() as i64;
    state.money += revenue - cost;
    state.total_customers_served += customer_count as u32;

    // Player rank EXP from business
    state.player_rank.add_exp(15);
    // Gems from business
    state.card_state.gems += 10;

    state.phase = GamePhase::DayResult;
}

/// End the day results and start the next day.
pub fn next_day(state: &mut CafeState) {
    state.day += 1;
    state.today_visits.clear();
    state.phase = GamePhase::Hub;
}

// ── Memory Unlock ─────────────────────────────────────────

/// Check and unlock new memories based on current state.
pub fn check_memory_unlocks(state: &mut CafeState) {
    // Memory: 最初の常連 — unlocked when Sakura reaches ★2
    if !state.memories.iter().any(|m| m.id == 1) {
        if let Some(aff) = state.affinities.get(&CharacterId::Sakura) {
            if aff.axes.star_rank() >= 2 {
                state.memories.push(super::state::Memory {
                    id: 1,
                    name: "最初の常連".into(),
                    description: "佐倉さんが初めて来た日の記憶".into(),
                    trust_bonus: 2,
                    understanding_bonus: 1,
                    empathy_bonus: 1,
                });
            }
        }
    }

    // Memory: 商店街の朝 — unlocked at player rank 3
    if !state.memories.iter().any(|m| m.id == 2) && state.player_rank.level >= 3 {
        state.memories.push(super::state::Memory {
            id: 2,
            name: "商店街の朝".into(),
            description: "あかつき通りの活気を感じた朝".into(),
            trust_bonus: 1,
            understanding_bonus: 2,
            empathy_bonus: 1,
        });
    }

    // Memory: レシピの閃き — unlocked after 10 total actions
    if !state.memories.iter().any(|m| m.id == 3) && state.total_customers_served >= 10 {
        state.memories.push(super::state::Memory {
            id: 3,
            name: "レシピの閃き".into(),
            description: "お客様の声からメニューを思いつく".into(),
            trust_bonus: 1,
            understanding_bonus: 1,
            empathy_bonus: 2,
        });
    }

    // Memory: 月灯りの夕暮れ — unlocked at chapter 2 complete
    if !state.memories.iter().any(|m| m.id == 4) && state.chapters_completed >= 2 {
        state.memories.push(super::state::Memory {
            id: 4,
            name: "月灯りの夕暮れ".into(),
            description: "ステンドグラスに夕日が差す一瞬".into(),
            trust_bonus: 2,
            understanding_bonus: 2,
            empathy_bonus: 2,
        });
    }
}
