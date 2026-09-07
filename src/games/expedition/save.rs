//! 遠征団のセーブ / ロード。
//!
//! 永続対象: 行軍糧・レベル・章節・メダル・光珠ゲージ。
//! プッシャー盤面と遠征中進行は保存しない（拠点から再開）。

#[cfg(any(target_arch = "wasm32", test))]
use serde::{Deserialize, Serialize};

#[cfg(any(target_arch = "wasm32", test))]
use super::state::{ExpeditionState, Screen};

/// オートセーブ間隔 (tick)。10 tick/秒 × 30秒。
pub const AUTOSAVE_INTERVAL: u32 = 300;

#[cfg(any(target_arch = "wasm32", test))]
const SAVE_VERSION: u32 = 4;
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
    #[serde(default = "default_chapter")]
    chapter: u32,
    #[serde(default = "default_stage")]
    stage: u32,
    #[serde(default)]
    medals: u32,
    /// 旧セーブ互換: 補給 → メダルへ移行。
    #[serde(default)]
    supplies: u32,
    #[serde(default)]
    orb_gauge: u32,
    /// 旧すごろく互換フィールド（無視）。
    #[serde(default)]
    board_pos: u32,
    #[serde(default)]
    board_goal: u32,
    #[serde(default)]
    best_depth: u32,
    #[serde(alias = "bonds")]
    levels: Vec<u32>,
    elapsed_ticks: u64,
    last_wall_ms: u64,
}

#[cfg(any(target_arch = "wasm32", test))]
fn default_chapter() -> u32 {
    1
}
#[cfg(any(target_arch = "wasm32", test))]
fn default_stage() -> u32 {
    1
}

#[cfg(any(target_arch = "wasm32", test))]
fn extract_save(state: &ExpeditionState) -> SaveData {
    SaveData {
        version: SAVE_VERSION,
        game: GameSave {
            rations: state.rations,
            ration_progress: state.ration_progress,
            chapter: state.chapter,
            stage: state.stage,
            medals: state.medals,
            supplies: 0,
            orb_gauge: state.orb_gauge,
            board_pos: 0,
            board_goal: 0,
            best_depth: 0,
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
    if save.chapter > 0 {
        state.chapter = save.chapter.max(1);
        state.stage = save.stage.clamp(1, 4);
    } else if save.best_depth > 0 {
        let d = save.best_depth.max(1) - 1;
        state.chapter = d / 4 + 1;
        state.stage = d % 4 + 1;
    } else {
        state.chapter = 1;
        state.stage = 1;
    }
    state.medals = if save.medals > 0 {
        save.medals
    } else {
        save.supplies
    };
    state.orb_gauge = save.orb_gauge;
    state.pending_level_pick = false;
    state.elapsed_ticks = save.elapsed_ticks;
    state.last_wall_ms = save.last_wall_ms;
    for (hero, level) in state.roster.iter_mut().zip(save.levels.iter()) {
        hero.level = (*level).max(1);
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
    fn roundtrip_preserves_levels_map_and_medals() {
        let mut state = ExpeditionState::new();
        state.rations = 2;
        state.roster[0].level = 4;
        state.chapter = 1;
        state.stage = 3;
        state.medals = 9;
        state.orb_gauge = 2;
        let save = extract_save(&state);
        let mut loaded = ExpeditionState::new();
        apply_save(&mut loaded, &save.game);
        assert_eq!(loaded.rations, 2);
        assert_eq!(loaded.roster[0].level, 4);
        assert_eq!(loaded.chapter, 1);
        assert_eq!(loaded.stage, 3);
        assert_eq!(loaded.medals, 9);
        assert_eq!(loaded.orb_gauge, 2);
        assert_eq!(loaded.screen, Screen::Camp);
    }

    #[test]
    fn migrates_supplies_to_medals() {
        let save = GameSave {
            supplies: 7,
            medals: 0,
            ..GameSave::default()
        };
        let mut loaded = ExpeditionState::new();
        apply_save(&mut loaded, &save);
        assert_eq!(loaded.medals, 7);
    }
}
