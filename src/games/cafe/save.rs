//! Save/load for the Café game using localStorage.
//!
//! Version 2: Adds affinity, cards, player rank, memories, AP.

#[cfg(target_arch = "wasm32")]
use serde::{Deserialize, Serialize};

#[cfg(target_arch = "wasm32")]
use std::collections::HashMap;

#[cfg(target_arch = "wasm32")]
use super::affinity::{CharacterAffinity, CharacterId};
#[cfg(target_arch = "wasm32")]
use super::cards::CardState;
#[cfg(target_arch = "wasm32")]
use super::social::{DailyMissionState, LoginBonusState, StaminaState};
#[cfg(target_arch = "wasm32")]
use super::state::{CafeState, GamePhase, Memory, PlayerRank};
#[cfg(not(target_arch = "wasm32"))]
use super::state::CafeState;

#[cfg(target_arch = "wasm32")]
const STORAGE_KEY: &str = "cafe_save";
#[cfg(target_arch = "wasm32")]
const SAVE_VERSION: u32 = 2;
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

    // Affinities
    affinities: HashMap<CharacterId, CharacterAffinity>,

    // Cards
    card_state: CardState,

    // Memories
    memories: Vec<Memory>,
    equipped_memories: Vec<usize>,

    // Social systems
    stamina: StaminaState,
    daily_missions: DailyMissionState,
    login_bonus: LoginBonusState,
}

/// Extract saveable data from game state.
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
            affinities: state.affinities.clone(),
            card_state: state.card_state.clone(),
            memories: state.memories.clone(),
            equipped_memories: state.equipped_memories.clone(),
            stamina: state.stamina.clone(),
            daily_missions: state.daily_missions.clone(),
            login_bonus: state.login_bonus.clone(),
        },
    }
}

/// Apply saved data to game state.
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
    state.login_bonus = save.login_bonus;

    // Merge affinities (keep defaults for any missing characters)
    for (ch, aff) in save.affinities {
        state.affinities.insert(ch, aff);
    }

    // Set phase based on saved state
    if state.chapters_completed > 0 || state.day > 1 {
        state.phase = GamePhase::Hub;
    } else {
        state.phase = GamePhase::Story;
    }
}

/// Save game to localStorage.
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

/// Load game from localStorage.
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

/// Delete save data.
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
