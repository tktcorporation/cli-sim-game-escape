//! 星環 セーブ/ロード。
//!
//! 永続対象: shards・各強化レベル・累計撃破・層進行・rng。
//! 一時演出 (鉱石・ビーム・パーティクル・ブースト) は保存しない。

#[cfg(any(target_arch = "wasm32", test))]
use serde::{Deserialize, Serialize};

#[cfg(any(target_arch = "wasm32", test))]
use super::state::{StarRingState, UPGRADE_COUNT};

#[cfg(any(target_arch = "wasm32", test))]
const SAVE_VERSION: u32 = 2;

#[cfg(any(target_arch = "wasm32", test))]
const MIN_COMPATIBLE_VERSION: u32 = 1;

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
    shards_leaked: f64,
    total_kills: u64,
    leak_count: u64,
    depth: u32,
    depth_kills: u64,
    best_depth: u32,
    /// v2: [砲台, 火力, 連射, 脈動, 穿光, 収率]
    upgrade_levels: [u32; UPGRADE_COUNT],
    rng_state: u32,
}

#[cfg(any(target_arch = "wasm32", test))]
fn extract_save(state: &StarRingState) -> SaveData {
    SaveData {
        version: SAVE_VERSION,
        game: GameSave {
            shards: state.shards,
            shards_earned: state.shards_earned,
            shards_leaked: state.shards_leaked,
            total_kills: state.total_kills,
            leak_count: state.leak_count,
            depth: state.depth,
            depth_kills: state.depth_kills,
            best_depth: state.best_depth,
            upgrade_levels: state.upgrade_levels,
            rng_state: state.rng_state,
        },
    }
}

/// v1: [Turrets, OrbitSpeed, Damage, FireRate, Density, Yield]
/// v2: [Turrets, Damage, FireRate, Pulse, Lance, Yield]
#[cfg(any(target_arch = "wasm32", test))]
fn migrate_v1_levels(old: [u32; 6]) -> [u32; UPGRADE_COUNT] {
    [
        old[0], // Turrets
        old[2], // Damage
        old[3], // FireRate
        0,      // Pulse (新設)
        0,      // Lance (新設)
        old[5], // Yield
                // OrbitSpeed / Density は意図的に捨てる
    ]
}

#[cfg(any(target_arch = "wasm32", test))]
fn apply_save(state: &mut StarRingState, save: &GameSave, version: u32) {
    state.shards = save.shards.max(0.0);
    state.shards_earned = save.shards_earned.max(0.0);
    state.shards_leaked = save.shards_leaked.max(0.0);
    state.total_kills = save.total_kills;
    state.leak_count = save.leak_count;

    if version <= 1 {
        // v1 は upgrade_levels に旧並びが入っている
        state.upgrade_levels = migrate_v1_levels(save.upgrade_levels);
        // 撃破数からおおまかに層を復元
        state.depth = depth_from_legacy_kills(save.total_kills);
        state.depth_kills = 0;
        state.best_depth = state.depth;
    } else {
        state.upgrade_levels = save.upgrade_levels;
        state.depth = save.depth.max(1);
        state.depth_kills = save.depth_kills;
        state.best_depth = save.best_depth.max(state.depth);
    }

    let max_turrets = super::state::UpgradeKind::Turrets
        .max_level()
        .unwrap_or(u32::MAX);
    if state.upgrade_levels[0] > max_turrets {
        state.upgrade_levels[0] = max_turrets;
    }
    state.rng_state = if save.rng_state == 0 {
        0xC0FFEE42
    } else {
        save.rng_state
    };
}

#[cfg(any(target_arch = "wasm32", test))]
fn depth_from_legacy_kills(kills: u64) -> u32 {
    // 旧解放閾値のおおまかな対応: 12/60/180/450
    if kills >= 450 {
        8
    } else if kills >= 180 {
        5
    } else if kills >= 60 {
        3
    } else if kills >= 12 {
        2
    } else {
        1
    }
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
        let _ = storage.remove_item(STORAGE_KEY);
        return false;
    }
    apply_save(state, &save_data.game, save_data.version);
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
    use crate::games::starringe::state::UpgradeKind;

    #[test]
    fn extract_and_apply_roundtrip() {
        let mut original = StarRingState::new();
        original.shards = 1234.5;
        original.shards_earned = 5000.0;
        original.shards_leaked = 12.0;
        original.total_kills = 88;
        original.leak_count = 3;
        original.depth = 4;
        original.depth_kills = 7;
        original.best_depth = 4;
        original.upgrade_levels = [2, 3, 1, 4, 0, 5];
        original.rng_state = 42;

        let save = extract_save(&original);
        let json = serde_json::to_string(&save).unwrap();
        let loaded: SaveData = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.version, SAVE_VERSION);

        let mut restored = StarRingState::new();
        apply_save(&mut restored, &loaded.game, loaded.version);

        assert!((restored.shards - 1234.5).abs() < 0.001);
        assert!((restored.shards_earned - 5000.0).abs() < 0.001);
        assert!((restored.shards_leaked - 12.0).abs() < 0.001);
        assert_eq!(restored.total_kills, 88);
        assert_eq!(restored.leak_count, 3);
        assert_eq!(restored.depth, 4);
        assert_eq!(restored.depth_kills, 7);
        assert_eq!(restored.upgrade_levels, [2, 3, 1, 4, 0, 5]);
        assert_eq!(restored.rng_state, 42);
        assert_eq!(restored.level(UpgradeKind::Turrets), 2);
        assert_eq!(restored.level(UpgradeKind::Pulse), 4);
    }

    #[test]
    fn empty_state_roundtrip() {
        let state = StarRingState::new();
        let save = extract_save(&state);
        let json = serde_json::to_string(&save).unwrap();
        let loaded: SaveData = serde_json::from_str(&json).unwrap();
        let mut restored = StarRingState::new();
        apply_save(&mut restored, &loaded.game, loaded.version);
        assert!((restored.shards - state.shards).abs() < 0.001);
        assert_eq!(restored.total_kills, 0);
        assert_eq!(restored.depth, 1);
    }

    #[test]
    fn migrate_v1_drops_density_and_orbit() {
        // old: Turrets=2, Orbit=5, Damage=3, Fire=1, Density=9, Yield=4
        let migrated = migrate_v1_levels([2, 5, 3, 1, 9, 4]);
        assert_eq!(migrated, [2, 3, 1, 0, 0, 4]);
    }

    #[test]
    fn apply_v1_save_restores_depth_from_kills() {
        let save = GameSave {
            shards: 10.0,
            total_kills: 60,
            upgrade_levels: [1, 2, 3, 1, 4, 2],
            rng_state: 7,
            ..GameSave::default()
        };
        let mut state = StarRingState::new();
        apply_save(&mut state, &save, 1);
        assert_eq!(state.depth, 3);
        assert_eq!(state.level(UpgradeKind::Turrets), 1);
        assert_eq!(state.level(UpgradeKind::Damage), 3);
        assert_eq!(state.level(UpgradeKind::FireRate), 1);
        assert_eq!(state.level(UpgradeKind::Yield), 2);
        assert_eq!(state.level(UpgradeKind::Pulse), 0);
        assert_eq!(state.level(UpgradeKind::Lance), 0);
    }

    #[test]
    fn save_version_is_compatible() {
        const { assert!(SAVE_VERSION >= MIN_COMPATIBLE_VERSION) };
    }
}
