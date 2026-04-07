//! Save/load for the Café game using localStorage.
//!
//! Version 3: Adds character data (levels/stars/shards), weekly missions,
//! produce completions, login gems.

#[cfg(target_arch = "wasm32")]
use serde::{Deserialize, Serialize};

#[cfg(target_arch = "wasm32")]
use std::collections::HashMap;

#[cfg(target_arch = "wasm32")]
use super::characters::affinity::CharacterAffinity;
#[cfg(target_arch = "wasm32")]
use super::characters::{CharacterData, CharacterId};
#[cfg(target_arch = "wasm32")]
use super::gacha::CardState;
#[cfg(target_arch = "wasm32")]
use super::social_sys::{DailyMissionState, LoginBonusState, StaminaState, WeeklyMissionState};
#[cfg(target_arch = "wasm32")]
use super::state::{CafeState, GamePhase, Memory, PlayerRank};
#[cfg(not(target_arch = "wasm32"))]
use super::state::CafeState;

#[cfg(target_arch = "wasm32")]
const STORAGE_KEY: &str = "cafe_save";
#[cfg(target_arch = "wasm32")]
const SAVE_VERSION: u32 = 3;
#[cfg(target_arch = "wasm32")]
const MIN_COMPATIBLE_VERSION: u32 = 1;

#[cfg(target_arch = "wasm32")]
#[derive(Serialize, Deserialize)]
struct SaveData {
    version: u32,
    game: GameSave,
}

#[cfg(target_arch = "wasm32")]
#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct GameSave {
    // Progress
    chapters_completed: u32,
    current_chapter: u32,
    scene_index: usize,
    line_index: usize,
    day: u32,
    money: i64,
    total_customers: u32,

    // AP
    ap_current: u32,
    actions_today: u32,

    // Player rank
    player_rank: PlayerRank,

    // Characters (v3: separate data + affinity)
    character_data: HashMap<CharacterId, CharacterData>,
    affinities: HashMap<CharacterId, CharacterAffinity>,

    // Cards
    card_state: CardState,

    // Memories
    memories: Vec<Memory>,
    equipped_memories: Vec<usize>,

    // Social systems
    stamina: StaminaState,
    daily_missions: DailyMissionState,
    weekly_missions: WeeklyMissionState,
    login_bonus: LoginBonusState,

    // Produce
    total_produce_completions: u32,
}

#[cfg(target_arch = "wasm32")]
fn extract_save(state: &CafeState) -> SaveData {
    SaveData {
        version: SAVE_VERSION,
        game: GameSave {
            chapters_completed: state.chapters_completed,
            current_chapter: state.current_chapter,
            scene_index: state.current_scene_index,
            line_index: state.current_line_index,
            day: state.day,
            money: state.money,
            total_customers: state.total_customers_served,
            ap_current: state.ap_current,
            actions_today: state.actions_today,
            player_rank: state.player_rank.clone(),
            character_data: state.character_data.clone(),
            affinities: state.affinities.clone(),
            card_state: state.card_state.clone(),
            memories: state.memories.clone(),
            equipped_memories: state.equipped_memories.clone(),
            stamina: state.stamina.clone(),
            daily_missions: state.daily_missions.clone(),
            weekly_missions: state.weekly_missions.clone(),
            login_bonus: state.login_bonus.clone(),
            total_produce_completions: state.total_produce_completions,
        },
    }
}

#[cfg(target_arch = "wasm32")]
fn apply_save(state: &mut CafeState, save: GameSave) {
    state.chapters_completed = save.chapters_completed;
    state.current_chapter = save.current_chapter;
    state.current_scene_index = save.scene_index;
    state.current_line_index = save.line_index;
    state.day = save.day;
    state.money = save.money;
    state.total_customers_served = save.total_customers;
    state.ap_current = save.ap_current;
    state.actions_today = save.actions_today;
    state.player_rank = save.player_rank;
    state.card_state = save.card_state;
    state.memories = save.memories;
    state.equipped_memories = save.equipped_memories;
    state.stamina = save.stamina;
    state.daily_missions = save.daily_missions;
    state.weekly_missions = save.weekly_missions;
    state.login_bonus = save.login_bonus;
    state.total_produce_completions = save.total_produce_completions;

    // Merge character data (keep defaults for missing)
    for (ch, data) in save.character_data {
        state.character_data.insert(ch, data);
    }
    for (ch, aff) in save.affinities {
        state.affinities.insert(ch, aff);
    }

    // Set phase
    if state.chapters_completed > 0 || state.day > 1 {
        state.phase = GamePhase::Hub;
    } else {
        state.phase = GamePhase::Story;
    }
}

pub fn save_game(state: &CafeState) {
    #[cfg(target_arch = "wasm32")]
    {
        let save = extract_save(state);
        if let Ok(json) = serde_json::to_string(&save) {
            if let Some(storage) = web_sys::window()
                .and_then(|w| w.local_storage().ok())
                .flatten()
            {
                let _ = storage.set_item(STORAGE_KEY, &json);
            }
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    let _ = state;
}

pub fn load_game(state: &mut CafeState) -> bool {
    #[cfg(target_arch = "wasm32")]
    {
        let json = web_sys::window()
            .and_then(|w| w.local_storage().ok())
            .flatten()
            .and_then(|s| s.get_item(STORAGE_KEY).ok())
            .flatten();

        if let Some(json) = json {
            if let Ok(save) = serde_json::from_str::<SaveData>(&json) {
                if save.version >= MIN_COMPATIBLE_VERSION {
                    apply_save(state, save.game);
                    return true;
                }
            }
        }
        false
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = state;
        false
    }
}

#[allow(dead_code)]
pub fn delete_save() {
    #[cfg(target_arch = "wasm32")]
    {
        if let Some(storage) = web_sys::window()
            .and_then(|w| w.local_storage().ok())
            .flatten()
        {
            let _ = storage.remove_item(STORAGE_KEY);
        }
    }
}
