//! 常夜灯 セーブ/ロード。
//!
//! 永続資源 (残光・拠点強化・自己ベスト・乱数シード) のみを保存する。
//! 夜番中の進行 (敵・弾・宝箱・現在の装備・灯の残量) は対象外 —
//! リロードすれば拠点からやり直しになる。死亡/撤退と同じ非対称性を
//! リロードにも一貫させるための意図的な設計 (loopmarchと同じ方針)。

#[cfg(any(target_arch = "wasm32", test))]
use serde::{Deserialize, Serialize};

#[cfg(any(target_arch = "wasm32", test))]
use super::state::{CampUpgrades, EverlightState};

/// セーブデータのフォーマットバージョン。フィールド追加時にインクリメントすること。
#[cfg(any(target_arch = "wasm32", test))]
const SAVE_VERSION: u32 = 1;

/// 互換性を維持できる最小バージョン。
#[cfg(any(target_arch = "wasm32", test))]
const MIN_COMPATIBLE_VERSION: u32 = 1;

#[cfg(target_arch = "wasm32")]
const STORAGE_KEY: &str = "everlight_save";

/// オートセーブの間隔 (tick数)。10 ticks/sec × 30秒 = 300 ticks。
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
    ember: u32,
    best_wave: u32,
    best_survival_ticks: u64,
    light_level: u32,
    power_level: u32,
    extra_slot_level: u32,
    rng_state: u32,
    /// 挑戦を許された最大の夜番ランク。旧セーブ (フィールド無し) は
    /// serdeのデフォルト(0)で読み込まれるが、`apply_save` 側で1未満を
    /// 1へ補正する (ランク1は常に挑戦可能なため)。
    max_unlocked_rank: u32,
}

#[cfg(any(target_arch = "wasm32", test))]
fn extract_save(state: &EverlightState) -> SaveData {
    SaveData {
        version: SAVE_VERSION,
        game: GameSave {
            ember: state.ember,
            best_wave: state.best_wave,
            best_survival_ticks: state.best_survival_ticks,
            light_level: state.camp.light_level,
            power_level: state.camp.power_level,
            extra_slot_level: state.camp.extra_slot_level,
            rng_state: state.rng_state,
            max_unlocked_rank: state.camp.max_unlocked_rank,
        },
    }
}

#[cfg(any(target_arch = "wasm32", test))]
fn apply_save(state: &mut EverlightState, save: &GameSave) {
    state.ember = save.ember;
    state.best_wave = save.best_wave;
    state.best_survival_ticks = save.best_survival_ticks;
    state.camp = CampUpgrades {
        light_level: save.light_level,
        power_level: save.power_level,
        extra_slot_level: save.extra_slot_level,
        max_unlocked_rank: save.max_unlocked_rank.max(1),
        selected_rank: 1,
    };
    // 0 は rng_next 側で固定値に補正されるだけなので、未保存(旧セーブ)の
    // 0 をそのまま許容してよい。
    state.rng_state = save.rng_state;
    // 拠点強化を反映した状態で灯を作り直す (夜番に出ていなければ満タン)。
    state.lantern.light_max = state.camp.light_max();
    state.lantern.light = state.lantern.light_max;
}

#[cfg(target_arch = "wasm32")]
fn get_storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok()?
}

/// ゲーム状態を localStorage に保存する。失敗時はサイレントに無視 (コンソールに警告)。
#[cfg(target_arch = "wasm32")]
pub fn save_game(state: &EverlightState) {
    let save_data = extract_save(state);
    let json = match serde_json::to_string(&save_data) {
        Ok(j) => j,
        Err(e) => {
            web_sys::console::warn_1(&format!("常夜灯: セーブのシリアライズに失敗: {e}").into());
            return;
        }
    };

    if let Some(storage) = get_storage() {
        if let Err(e) = storage.set_item(STORAGE_KEY, &json) {
            web_sys::console::warn_1(&format!("常夜灯: localStorage への保存に失敗: {e:?}").into());
        }
    }
}

/// localStorage からゲーム状態を復元する。失敗時は false を返す (新規ゲームになる)。
#[cfg(target_arch = "wasm32")]
pub fn load_game(state: &mut EverlightState) -> bool {
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
            web_sys::console::warn_1(&format!("常夜灯: セーブデータのパースに失敗（破棄します）: {e}").into());
            let _ = storage.remove_item(STORAGE_KEY);
            return false;
        }
    };

    if save_data.version < MIN_COMPATIBLE_VERSION {
        let _ = storage.remove_item(STORAGE_KEY);
        return false;
    }

    apply_save(state, &save_data.game);
    true
}

/// セーブデータを削除する。
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
    fn extract_and_apply_roundtrip() {
        let mut original = EverlightState::new();
        original.ember = 42;
        original.best_wave = 7;
        original.best_survival_ticks = 12345;
        original.camp.light_level = 2;
        original.camp.power_level = 1;
        original.camp.extra_slot_level = 1;
        original.camp.max_unlocked_rank = 3;
        original.rng_state = 999_999;

        let save = extract_save(&original);
        let json = serde_json::to_string(&save).unwrap();
        let loaded: SaveData = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.version, SAVE_VERSION);

        let mut restored = EverlightState::new();
        apply_save(&mut restored, &loaded.game);

        assert_eq!(restored.ember, 42);
        assert_eq!(restored.best_wave, 7);
        assert_eq!(restored.best_survival_ticks, 12345);
        assert_eq!(restored.camp.light_level, 2);
        assert_eq!(restored.camp.power_level, 1);
        assert_eq!(restored.camp.extra_slot_level, 1);
        assert_eq!(restored.camp.max_unlocked_rank, 3, "解放済みランクも保存/復元されるはず");
        assert_eq!(restored.lantern.light_max, restored.camp.light_max());
        assert_eq!(
            restored.rng_state, 999_999,
            "rng_state を保存しないとリロードのたびに同じ乱数列を再生してしまう"
        );
    }

    #[test]
    fn version_below_min_compatible_is_rejected() {
        let save_data = SaveData { version: 0, game: GameSave::default() };
        assert!(save_data.version < MIN_COMPATIBLE_VERSION);
    }

    #[test]
    fn empty_state_roundtrip() {
        let state = EverlightState::new();
        let save = extract_save(&state);
        let json = serde_json::to_string(&save).unwrap();
        let loaded: SaveData = serde_json::from_str(&json).unwrap();

        let mut restored = EverlightState::new();
        apply_save(&mut restored, &loaded.game);

        assert_eq!(restored.ember, 0);
        assert_eq!(restored.best_wave, 0);
    }

    #[test]
    fn save_without_new_fields_loads_with_defaults() {
        // フィールド追加前の旧セーブ (該当キーが無いJSON) を読み込んでも
        // panicせず、デフォルト値として扱えることを保証する。
        let json = r#"{"version":1,"game":{"ember":10,"best_wave":2}}"#;
        let loaded: SaveData = serde_json::from_str(json).unwrap();

        let mut restored = EverlightState::new();
        apply_save(&mut restored, &loaded.game);

        assert_eq!(restored.ember, 10);
        assert_eq!(restored.best_wave, 2);
        assert_eq!(restored.camp.light_level, 0);
        assert_eq!(
            restored.camp.max_unlocked_rank, 1,
            "旧セーブにmax_unlocked_rankが無くてもランク1は挑戦可能でなければならない"
        );
    }
}
