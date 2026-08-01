//! 周回討伐 セーブ/ロード。
//!
//! 永続資源 (魂・拠点強化・自己ベスト周回数) のみを保存する。遠征中の
//! 進行 (道の配置・勇者HP・木材/石材) は対象外 — リロードすれば拠点から
//! やり直しになる。「死んだら失うもの」と同じ非対称性をリロードにも
//! 一貫させるための意図的な設計。

#[cfg(any(target_arch = "wasm32", test))]
use serde::{Deserialize, Serialize};

#[cfg(any(target_arch = "wasm32", test))]
use super::state::{CampUpgrades, LoopMarchState};

/// セーブデータのフォーマットバージョン。フィールド追加時にインクリメントすること。
#[cfg(any(target_arch = "wasm32", test))]
const SAVE_VERSION: u32 = 1;

/// 互換性を維持できる最小バージョン。
#[cfg(any(target_arch = "wasm32", test))]
const MIN_COMPATIBLE_VERSION: u32 = 1;

#[cfg(target_arch = "wasm32")]
const STORAGE_KEY: &str = "loopmarch_save";

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
    soul: u32,
    best_lap: u32,
    max_hp_level: u32,
    attack_level: u32,
    extra_card_level: u32,
    /// 乱数シード。保存しないとリロードのたびに固定シードへ戻り、
    /// 初期手札や湧き判定が毎回同じ列を再生してしまう。
    rng_state: u32,
}

#[cfg(any(target_arch = "wasm32", test))]
fn extract_save(state: &LoopMarchState) -> SaveData {
    SaveData {
        version: SAVE_VERSION,
        game: GameSave {
            soul: state.soul,
            best_lap: state.best_lap,
            max_hp_level: state.camp.max_hp_level,
            attack_level: state.camp.attack_level,
            extra_card_level: state.camp.extra_card_level,
            rng_state: state.rng_state,
        },
    }
}

#[cfg(any(target_arch = "wasm32", test))]
fn apply_save(state: &mut LoopMarchState, save: &GameSave) {
    state.soul = save.soul;
    state.best_lap = save.best_lap;
    state.camp = CampUpgrades {
        max_hp_level: save.max_hp_level,
        attack_level: save.attack_level,
        extra_card_level: save.extra_card_level,
    };
    // 0 は rng_next 側で固定値に補正されるだけなので、未保存(旧セーブ)の
    // 0 をそのまま許容してよい。
    state.rng_state = save.rng_state;
    // 拠点強化を反映した状態で、遠征前の勇者ステータスを作り直す。
    state.hero.max_hp = state.camp.hero_max_hp();
    state.hero.hp = state.hero.max_hp;
    state.hero.attack = state.camp.hero_attack();
}

#[cfg(target_arch = "wasm32")]
fn get_storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok()?
}

/// ゲーム状態を localStorage に保存する。失敗時はサイレントに無視 (コンソールに警告)。
#[cfg(target_arch = "wasm32")]
pub fn save_game(state: &LoopMarchState) {
    let save_data = extract_save(state);
    let json = match serde_json::to_string(&save_data) {
        Ok(j) => j,
        Err(e) => {
            web_sys::console::warn_1(&format!("周回討伐: セーブのシリアライズに失敗: {e}").into());
            return;
        }
    };

    if let Some(storage) = get_storage() {
        if let Err(e) = storage.set_item(STORAGE_KEY, &json) {
            web_sys::console::warn_1(
                &format!("周回討伐: localStorage への保存に失敗: {e:?}").into(),
            );
        }
    }
}

/// localStorage からゲーム状態を復元する。失敗時は false を返す (新規ゲームになる)。
#[cfg(target_arch = "wasm32")]
pub fn load_game(state: &mut LoopMarchState) -> bool {
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
                &format!("周回討伐: セーブデータのパースに失敗（破棄します）: {e}").into(),
            );
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
        let mut original = LoopMarchState::new();
        original.soul = 42;
        original.best_lap = 7;
        original.camp.max_hp_level = 2;
        original.camp.attack_level = 1;
        original.camp.extra_card_level = 1;
        original.rng_state = 999_999;

        let save = extract_save(&original);
        let json = serde_json::to_string(&save).unwrap();
        let loaded: SaveData = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.version, SAVE_VERSION);

        let mut restored = LoopMarchState::new();
        apply_save(&mut restored, &loaded.game);

        assert_eq!(restored.soul, 42);
        assert_eq!(restored.best_lap, 7);
        assert_eq!(restored.camp.max_hp_level, 2);
        assert_eq!(restored.camp.attack_level, 1);
        assert_eq!(restored.camp.extra_card_level, 1);
        assert_eq!(restored.hero.max_hp, restored.camp.hero_max_hp());
        assert_eq!(restored.hero.attack, restored.camp.hero_attack());
        assert_eq!(
            restored.rng_state, 999_999,
            "rng_state を保存しないとリロードのたびに同じ乱数列を再生してしまう"
        );
    }

    #[test]
    fn version_below_min_compatible_is_rejected() {
        let save_data = SaveData {
            version: 0,
            game: GameSave::default(),
        };
        assert!(save_data.version < MIN_COMPATIBLE_VERSION);
    }

    #[test]
    fn empty_state_roundtrip() {
        let state = LoopMarchState::new();
        let save = extract_save(&state);
        let json = serde_json::to_string(&save).unwrap();
        let loaded: SaveData = serde_json::from_str(&json).unwrap();

        let mut restored = LoopMarchState::new();
        apply_save(&mut restored, &loaded.game);

        assert_eq!(restored.soul, 0);
        assert_eq!(restored.best_lap, 0);
    }
}
