//! Pure functions for Café game state transitions.
//!
//! Handles story, AP actions, business, produce, and memory unlocks.

use super::characters::affinity;
use super::characters::{ActionType, CharacterId};
use super::gacha;
use super::produce::{ProduceRank, ProduceState, TrainingType, PRODUCE_STAMINA_COST};
use super::scenario::{get_chapter_scenes, PROLOGUE_SCENES};
use super::social_sys::MissionType;
use super::state::{CafeState, CustomerVisit, GamePhase, AP_MAX};

// ── Story Progression ─────────────────────────────────────

fn current_scenes(state: &CafeState) -> &'static [&'static super::state::StoryScene] {
    if state.current_chapter == 0 {
        PROLOGUE_SCENES
    } else {
        get_chapter_scenes(state.current_chapter)
    }
}

pub fn advance_story(state: &mut CafeState) -> bool {
    if state.phase != GamePhase::Story {
        return false;
    }

    let scenes = current_scenes(state);
    let scene_count = scenes.len();
    if scene_count == 0 || state.current_scene_index >= scene_count {
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

fn finish_chapter(state: &mut CafeState) {
    if state.current_chapter > state.chapters_completed {
        state.chapters_completed = state.current_chapter;
    }
    if state.current_chapter == 0 && state.chapters_completed == 0 {
        state.chapters_completed = 0;
    }

    unlock_characters_for_chapter(state, state.current_chapter);

    // Gem reward for chapter completion
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

fn unlock_characters_for_chapter(state: &mut CafeState, chapter: u32) {
    for &ch in CharacterId::ALL {
        if ch.unlock_chapter() <= chapter {
            if let Some(data) = state.character_data.get_mut(&ch) {
                data.unlocked = true;
            }
        }
    }
}

pub fn start_chapter(state: &mut CafeState, chapter: u32) {
    state.current_chapter = chapter;
    state.current_scene_index = 0;
    state.current_line_index = 0;
    state.phase = GamePhase::Story;
}

pub fn next_available_chapter(state: &CafeState) -> Option<u32> {
    let next = state.chapters_completed + 1;
    if next <= state.player_rank.max_chapter() {
        let scenes = get_chapter_scenes(next);
        if !scenes.is_empty() {
            return Some(next);
        }
    }
    None
}

// ── AP Actions ────────────────────────────────────────────

pub fn perform_action(state: &mut CafeState, target: CharacterId, action: ActionType) -> bool {
    let cost = action.ap_cost();
    if state.ap_current < cost {
        return false;
    }

    // Check character is unlocked
    let char_data = match state.character_data.get(&target) {
        Some(d) if d.unlocked => d.clone(),
        _ => return false,
    };

    // Special requires affinity star rank >= 2
    if action == ActionType::Special {
        if let Some(aff) = state.affinities.get(&target) {
            if aff.axes.star_rank() < 2 {
                return false;
            }
        }
    }

    // Consume AP
    state.ap_current -= cost;
    state.actions_today += 1;

    // Calculate gains
    let base = affinity::base_gains(action);

    // Card multiplier
    let card_mult = state.card_state.equipped_multiplier();
    let mut gains = base.multiply(card_mult);

    // Card bonus axis: +50% to matching axis
    if let Some(bonus_axis) = state.card_state.equipped_bonus_axis() {
        match bonus_axis {
            gacha::BonusAxis::Trust => gains.trust = (gains.trust as f64 * 1.5) as u32,
            gacha::BonusAxis::Understanding => {
                gains.understanding = (gains.understanding as f64 * 1.5) as u32
            }
            gacha::BonusAxis::Empathy => gains.empathy = (gains.empathy as f64 * 1.5) as u32,
            gacha::BonusAxis::Balanced => {
                gains.trust = (gains.trust as f64 * 1.15) as u32;
                gains.understanding = (gains.understanding as f64 * 1.15) as u32;
                gains.empathy = (gains.empathy as f64 * 1.15) as u32;
            }
        }
    }

    // Character level bonus: +level_bonus to all axes
    let level_bonus = char_data.level_bonus();
    gains.trust += level_bonus;
    gains.understanding += level_bonus;
    gains.empathy += level_bonus;

    // Memory bonuses (flat add)
    let (mem_trust, mem_understanding, mem_empathy) = state.memory_bonuses();
    gains.trust += mem_trust;
    gains.understanding += mem_understanding;
    gains.empathy += mem_empathy;

    // Apply gains to affinity
    if let Some(aff) = state.affinities.get_mut(&target) {
        aff.axes.trust += gains.trust;
        aff.axes.understanding += gains.understanding;
        aff.axes.empathy += gains.empathy;
    }

    // Player rank EXP
    state.player_rank.add_exp(10 + cost * 5);

    // Character EXP
    if let Some(data) = state.character_data.get_mut(&target) {
        data.add_exp(15 + cost * 5);
    }

    // Coins from action
    state.card_state.coins += 5 + state.player_rank.level;

    // Mission tracking
    state.daily_missions.record(MissionType::Interact(1));
    state.weekly_missions.record(MissionType::Interact(1));

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

pub fn daily_reset(state: &mut CafeState) {
    state.ap_current = AP_MAX;
    state.actions_today = 0;
    state.today_business_runs = 0;
}

// ── Business Day ──────────────────────────────────────────

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

    // Rewards
    state.player_rank.add_exp(15);
    state.card_state.gems += 10;

    state.phase = GamePhase::DayResult;
}

pub fn next_day(state: &mut CafeState) {
    state.day += 1;
    state.today_visits.clear();
    state.phase = GamePhase::Hub;
}

// ── Produce Mode ──────────────────────────────────────────

/// Start a produce run with the given character.
pub fn start_produce(state: &mut CafeState, character: CharacterId) -> bool {
    let now = super::social_sys::now_ms();
    if !state.stamina.consume(PRODUCE_STAMINA_COST, now) {
        return false;
    }
    state.produce = Some(ProduceState::new(character));
    state.phase = GamePhase::ProduceTraining;
    true
}

/// Execute a training choice in produce.
pub fn produce_train(state: &mut CafeState, training: TrainingType) -> bool {
    let seed = (super::social_sys::now_ms() as u32).wrapping_mul(2654435761);

    let produce = match state.produce.as_mut() {
        Some(p) if p.is_active() => p,
        _ => return false,
    };

    produce.do_training(training, seed);

    if produce.finished {
        // Produce complete — apply rewards
        apply_produce_rewards(state);
        state.phase = GamePhase::ProduceResult;
    } else {
        state.phase = GamePhase::ProduceTurnResult { training };
    }
    true
}

/// Apply produce completion rewards.
fn apply_produce_rewards(state: &mut CafeState) {
    let produce = match &state.produce {
        Some(p) => p,
        None => return,
    };

    let rank = produce.final_rank.unwrap_or(ProduceRank::C);
    let character = produce.character;

    // Credits
    let base_credits = 200;
    state.money += (base_credits * rank.credit_multiplier()) as i64;

    // Gems
    state.card_state.gems += rank.gem_reward();

    // Character shards
    if let Some(data) = state.character_data.get_mut(&character) {
        data.shards += rank.shard_reward();
    }

    // Character EXP
    if let Some(data) = state.character_data.get_mut(&character) {
        data.add_exp(rank.exp_reward());
    }

    // Affinity gains from produce
    let aff_bonus = produce.stats.total() / 10;
    if let Some(aff) = state.affinities.get_mut(&character) {
        aff.axes.trust += aff_bonus / 3 + 1;
        aff.axes.understanding += aff_bonus / 3 + 1;
        aff.axes.empathy += aff_bonus / 3 + 1;
    }

    // Player rank EXP
    state.player_rank.add_exp(20 + rank.exp_reward() / 3);

    // Mission tracking
    state.total_produce_completions += 1;
    state.daily_missions.record(MissionType::ProduceComplete(1));
    state.weekly_missions.record(MissionType::ProduceComplete(1));
    if rank >= ProduceRank::S {
        state.daily_missions.record(MissionType::ProduceScore(1));
        state.weekly_missions.record(MissionType::ProduceScore(1));
    }
}

/// Finish viewing produce results, return to hub.
pub fn finish_produce(state: &mut CafeState) {
    state.produce = None;
    state.phase = GamePhase::Hub;
}

// ── Character Management ──────────────────────────────────

/// Try to promote a character's star rank.
pub fn try_promote_character(state: &mut CafeState, target: CharacterId) -> bool {
    if let Some(data) = state.character_data.get_mut(&target) {
        data.try_promote()
    } else {
        false
    }
}

/// Add EXP to a character (from EXP items, etc.).
#[allow(dead_code)] // Phase 2+: EXP item UI
pub fn add_character_exp(state: &mut CafeState, target: CharacterId, amount: u32) -> u32 {
    if let Some(data) = state.character_data.get_mut(&target) {
        data.add_exp(amount)
    } else {
        0
    }
}

// ── Memory Unlock ─────────────────────────────────────────

pub fn check_memory_unlocks(state: &mut CafeState) {
    // Memory 1: 最初の常連 — Sakura affinity star_rank >= 2
    if !state.memories.iter().any(|m| m.id == 1) {
        if let Some(aff) = state.affinities.get(&CharacterId::Sakura) {
            if aff.axes.star_rank() >= 2 {
                state.memories.push(super::state::Memory {
                    id: 1,
                    name: "最初の常連".into(),
                    description: "佐倉さんが初めて来た日の記憶".into(),
                    trust_bonus: 2, understanding_bonus: 1, empathy_bonus: 1,
                });
            }
        }
    }

    // Memory 2: 商店街の朝 — player rank >= 3
    if !state.memories.iter().any(|m| m.id == 2) && state.player_rank.level >= 3 {
        state.memories.push(super::state::Memory {
            id: 2,
            name: "商店街の朝".into(),
            description: "あかつき通りの活気を感じた朝".into(),
            trust_bonus: 1, understanding_bonus: 2, empathy_bonus: 1,
        });
    }

    // Memory 3: レシピの閃き — 10 customers served
    if !state.memories.iter().any(|m| m.id == 3) && state.total_customers_served >= 10 {
        state.memories.push(super::state::Memory {
            id: 3,
            name: "レシピの閃き".into(),
            description: "お客様の声からメニューを思いつく".into(),
            trust_bonus: 1, understanding_bonus: 1, empathy_bonus: 2,
        });
    }

    // Memory 4: 月灯りの夕暮れ — chapter 2 complete
    if !state.memories.iter().any(|m| m.id == 4) && state.chapters_completed >= 2 {
        state.memories.push(super::state::Memory {
            id: 4,
            name: "月灯りの夕暮れ".into(),
            description: "ステンドグラスに夕日が差す一瞬".into(),
            trust_bonus: 2, understanding_bonus: 2, empathy_bonus: 2,
        });
    }

    // Memory 5: プロデューサーの勲章 — 5 produce completions
    if !state.memories.iter().any(|m| m.id == 5) && state.total_produce_completions >= 5 {
        state.memories.push(super::state::Memory {
            id: 5,
            name: "プロデューサーの勲章".into(),
            description: "5回のプロデュースを乗り越えた証".into(),
            trust_bonus: 2, understanding_bonus: 2, empathy_bonus: 3,
        });
    }
}
