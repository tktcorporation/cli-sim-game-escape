//! 星環 セーブ/ロード。
//!
//! 永続対象: shards・武器/環レベル・累計撃破・rng・層同期。
//! 一時演出 (鉱石・弾・パーティクル・ブースト) は保存しない。

#[cfg(any(target_arch = "wasm32", test))]
use serde::{Deserialize, Serialize};

#[cfg(any(target_arch = "wasm32", test))]
use super::state::{StarRingState, WeaponKind, WEAPON_COUNT};

#[cfg(any(target_arch = "wasm32", test))]
const SAVE_VERSION: u32 = 2;

#[cfg(any(target_arch = "wasm32", test))]
const MIN_COMPATIBLE_VERSION: u32 = 2;

#[cfg(target_arch = "wasm32")]
const STORAGE_KEY: &str = "starringe_save";

/// オートセーブ間隔 (tick)。10 ticks/sec × 30秒 = 300。
pub const AUTOSAVE_INTERVAL: u32 = 300;

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
    shards: f64,
    shards_earned: f64,
    total_kills: u64,
    missed_count: u64,
    weapon_levels: [[u32; 3]; WEAPON_COUNT],
    ring_levels: [u32; 2],
    selected_weapon: u8,
    rng_state: u32,
}

#[cfg(any(target_arch = "wasm32", test))]
fn extract_save(state: &StarRingState) -> SaveData {
    SaveData {
        version: SAVE_VERSION,
        game: GameSave {
            shards: state.shards,
            shards_earned: state.shards_earned,
            total_kills: state.total_kills,
            missed_count: state.missed_count,
            weapon_levels: state.weapon_levels,
            ring_levels: state.ring_levels,
            selected_weapon: state.selected_weapon.index() as u8,
            rng_state: state.rng_state,
        },
    }
}

#[cfg(any(target_arch = "wasm32", test))]
fn apply_save(state: &mut StarRingState, save: &GameSave) {
    state.shards = save.shards.max(0.0);
    state.shards_earned = save.shards_earned.max(0.0);
    state.total_kills = save.total_kills;
    state.missed_count = save.missed_count;
    state.weapon_levels = save.weapon_levels;
    state.ring_levels = save.ring_levels;
    state.selected_weapon =
        WeaponKind::from_index(save.selected_weapon as usize).unwrap_or(WeaponKind::Pulse);
    if !state.is_weapon_unlocked(state.selected_weapon) {
        state.selected_weapon = WeaponKind::Pulse;
    }
    state.last_layer = state.layer();
    state.rng_state = if save.rng_state == 0 {
        0xC0FFEE42
    } else {
        save.rng_state
    };
}

#[cfg(target_arch = "wasm32")]
fn get_storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok()?
}

#[cfg(target_arch = "wasm32")]
pub fn save_game(state: &StarRingState) {
    let save_data = extract_save(state);
    let json = match serde_json::to_string(&save_data) {
        Ok(j) => j,
        Err(e) => {
            web_sys::console::warn_1(&format!("星環: セーブのシリアライズに失敗: {e}").into());
            return;
        }
    };
    if let Some(storage) = get_storage() {
        if let Err(e) = storage.set_item(STORAGE_KEY, &json) {
            web_sys::console::warn_1(&format!("星環: localStorage への保存に失敗: {e:?}").into());
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub fn load_game(state: &mut StarRingState) -> bool {
    let storage = match get_storage() {
        Some(s) => s,
        None => return false,
    };
    let json = match storage.get_item(STORAGE_KEY) {
        Ok(Some(j)) => j,
        _ => return false,
    };
    let save_data: SaveData = match serde_json::from_str(&json) {
        Ok(d) => d,
        Err(e) => {
            web_sys::console::warn_1(
                &format!("星環: セーブデータのパースに失敗（破棄します）: {e}").into(),
            );
            let _ = storage.remove_item(STORAGE_KEY);
            return false;
        }
    };
    if save_data.version < MIN_COMPATIBLE_VERSION {
        // v1 (漏洩・共通強化) はスキーマ非互換のため破棄
        let _ = storage.remove_item(STORAGE_KEY);
        return false;
    }
    apply_save(state, &save_data.game);
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
    use crate::games::starringe::state::{WeaponKind, WeaponStat};

    #[test]
    fn extract_and_apply_roundtrip() {
        let mut original = StarRingState::new();
        original.shards = 1234.5;
        original.shards_earned = 5000.0;
        original.total_kills = 300;
        original.missed_count = 7;
        original.weapon_levels[0] = [2, 3, 1];
        original.weapon_levels[1] = [1, 0, 2];
        original.ring_levels = [2, 4];
        original.selected_weapon = WeaponKind::Ray;
        original.rng_state = 42;

        let save = extract_save(&original);
        let json = serde_json::to_string(&save).unwrap();
        let loaded: SaveData = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.version, SAVE_VERSION);

        let mut restored = StarRingState::new();
        apply_save(&mut restored, &loaded.game);

        assert!((restored.shards - 1234.5).abs() < 0.001);
        assert!((restored.shards_earned - 5000.0).abs() < 0.001);
        assert_eq!(restored.total_kills, 300);
        assert_eq!(restored.missed_count, 7);
        assert_eq!(
            restored.weapon_stat(WeaponKind::Pulse, WeaponStat::Count),
            2
        );
        assert_eq!(restored.ring_levels, [2, 4]);
        assert_eq!(restored.selected_weapon, WeaponKind::Ray);
        assert_eq!(restored.rng_state, 42);
        assert_eq!(restored.last_layer, restored.layer());
    }

    #[test]
    fn empty_state_roundtrip() {
        let state = StarRingState::new();
        let save = extract_save(&state);
        let json = serde_json::to_string(&save).unwrap();
        let loaded: SaveData = serde_json::from_str(&json).unwrap();
        let mut restored = StarRingState::new();
        apply_save(&mut restored, &loaded.game);
        assert!((restored.shards - state.shards).abs() < 0.001);
        assert_eq!(restored.total_kills, 0);
    }

    #[test]
    fn save_version_is_compatible() {
        const { assert!(SAVE_VERSION >= MIN_COMPATIBLE_VERSION) };
    }
}
