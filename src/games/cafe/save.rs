//! Save/load for the Café game using localStorage.

#[cfg(target_arch = "wasm32")]
use serde::{Deserialize, Serialize};

#[cfg(target_arch = "wasm32")]
use super::social::{DailyMissionState, LoginBonusState, StaminaState};
#[cfg(target_arch = "wasm32")]
use super::state::{CafeState, GamePhase};
#[cfg(not(target_arch = "wasm32"))]
use super::state::CafeState;

#[cfg(target_arch = "wasm32")]
const STORAGE_KEY: &str = "cafe_save";
#[cfg(target_arch = "wasm32")]
const SAVE_VERSION: u32 = 1;
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
    story_complete: bool,
    scene_index: usize,
    line_index: usize,
    day: u32,
    money: i64,
    total_customers: u32,

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
            story_complete: state.story_complete,
            scene_index: state.current_scene_index,
            line_index: state.current_line_index,
            day: state.day,
            money: state.money,
            total_customers: state.total_customers_served,
            stamina: state.stamina.clone(),
            daily_missions: state.daily_missions.clone(),
            login_bonus: state.login_bonus.clone(),
        },
    }
}

/// Apply saved data to game state.
#[cfg(target_arch = "wasm32")]
fn apply_save(state: &mut CafeState, save: GameSave) {
    state.story_complete = save.story_complete;
    state.current_scene_index = save.scene_index;
    state.current_line_index = save.line_index;
    state.day = save.day;
    state.money = save.money;
    state.total_customers_served = save.total_customers;
    state.stamina = save.stamina;
    state.daily_missions = save.daily_missions;
    state.login_bonus = save.login_bonus;

    // Set phase based on saved state
    if state.story_complete {
        state.phase = GamePhase::Business;
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
