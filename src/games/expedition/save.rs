//! 遠征団のセーブ / ロード。
//!
//! 永続対象: 行軍糧・下調べメモ・レベル・到達層。
//! 遠征中の進行は保存しない（拠点から再開）。

#[cfg(any(target_arch = "wasm32", test))]
use serde::{Deserialize, Serialize};

#[cfg(any(target_arch = "wasm32", test))]
use super::state::{ExpeditionState, Screen};

/// オートセーブ間隔 (tick)。10 tick/秒 × 30秒。
pub const AUTOSAVE_INTERVAL: u32 = 300;

#[cfg(any(target_arch = "wasm32", test))]
const SAVE_VERSION: u32 = 1;
#[cfg(target_arch = "wasm32")]
const MIN_COMPATIBLE_VERSION: u32 = 1;

#[cfg(target_arch = "wasm32")]
const STORAGE_KEY: &str = "expedition_save_v1";

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Serialize, Deserialize)]
struct SaveData {
    version: u32,
    game: GameSave,
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct GameSave {
    rations: u32,
    ration_progress: u32,
    scout_memos: u32,
    scout_progress: u32,
    best_depth: u32,
    #[serde(alias = "bonds")]
    levels: Vec<u32>,
    elapsed_ticks: u64,
    last_wall_ms: u64,
}

#[cfg(any(target_arch = "wasm32", test))]
fn extract_save(state: &ExpeditionState) -> SaveData {
    SaveData {
        version: SAVE_VERSION,
        game: GameSave {
            rations: state.rations,
            ration_progress: state.ration_progress,
            scout_memos: state.scout_memos,
            scout_progress: state.scout_progress,
            best_depth: state.best_depth,
            levels: state.roster.iter().map(|h| h.level).collect(),
            elapsed_ticks: state.elapsed_ticks,
            last_wall_ms: state.last_wall_ms,
        },
    }
}

#[cfg(any(target_arch = "wasm32", test))]
fn apply_save(state: &mut ExpeditionState, save: &GameSave) {
    state.rations = save.rations;
    state.ration_progress = save.ration_progress;
    state.scout_memos = save.scout_memos;
    state.scout_progress = save.scout_progress;
    state.best_depth = save.best_depth.max(1);
    state.elapsed_ticks = save.elapsed_ticks;
    state.last_wall_ms = save.last_wall_ms;
    for (hero, level) in state.roster.iter_mut().zip(save.levels.iter()) {
        hero.level = *level;
        hero.refresh_max_hp();
        hero.hp = hero.max_hp;
    }
    state.screen = Screen::Camp;
    state.sortie = None;
    state.result_summary.clear();
}

#[cfg(target_arch = "wasm32")]
fn get_storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok()?
}

#[cfg(target_arch = "wasm32")]
pub fn save_game(state: &ExpeditionState) {
    let save_data = extract_save(state);
    let json = match serde_json::to_string(&save_data) {
        Ok(j) => j,
        Err(e) => {
            web_sys::console::warn_1(&format!("遠征団: セーブのシリアライズに失敗: {e}").into());
            return;
        }
    };
    if let Some(storage) = get_storage() {
        if let Err(e) = storage.set_item(STORAGE_KEY, &json) {
            web_sys::console::warn_1(
                &format!("遠征団: localStorage への保存に失敗: {e:?}").into(),
            );
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub fn load_game(state: &mut ExpeditionState) -> bool {
    let Some(storage) = get_storage() else {
        return false;
    };
    let Ok(Some(json)) = storage.get_item(STORAGE_KEY) else {
        return false;
    };
    let Ok(save) = serde_json::from_str::<SaveData>(&json) else {
        let _ = storage.remove_item(STORAGE_KEY);
        return false;
    };
    if save.version < MIN_COMPATIBLE_VERSION {
        let _ = storage.remove_item(STORAGE_KEY);
        return false;
    }
    apply_save(state, &save.game);
    if save.game.last_wall_ms > 0 {
        if let Some(now) = crate::time::now_ms() {
            let elapsed_ms = now - save.game.last_wall_ms as f64;
            if elapsed_ms > 0.0 {
                let ticks = (elapsed_ms / 100.0) as u64;
                super::logic::apply_offline_regen(state, ticks);
            }
            state.last_wall_ms = now as u64;
        }
    }
    true
}

#[cfg(target_arch = "wasm32")]
pub fn delete_save() {
    if let Some(storage) = get_storage() {
        let _ = storage.remove_item(STORAGE_KEY);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_preserves_levels_and_rations() {
        let mut state = ExpeditionState::new();
        state.rations = 2;
        state.roster[0].level = 4;
        state.best_depth = 3;
        let save = extract_save(&state);
        let mut loaded = ExpeditionState::new();
        apply_save(&mut loaded, &save.game);
        assert_eq!(loaded.rations, 2);
        assert_eq!(loaded.roster[0].level, 4);
        assert_eq!(loaded.best_depth, 3);
        assert_eq!(loaded.screen, Screen::Camp);
    }
}
