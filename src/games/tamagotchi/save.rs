//! たまごっち風育成ゲームの localStorage 保存。

#[cfg(any(target_arch = "wasm32", test))]
use serde::{Deserialize, Serialize};

#[cfg(any(target_arch = "wasm32", test))]
use super::state::{Stage, Stats, TamaState};

#[cfg(any(target_arch = "wasm32", test))]
const SAVE_VERSION: u32 = 1;

/// ロード時に許容する最小バージョン。これより古い save は破棄。
/// 将来フィールドを追加して SAVE_VERSION を bump しても、`serde(default)` が
/// 旧 save の欠落フィールドを埋めてくれるので、ここを据え置けば既存プレイヤーの
/// 世代記録 (best_age_ticks / generation) は失われない。
#[cfg(target_arch = "wasm32")]
const MIN_COMPATIBLE_VERSION: u32 = 1;

#[cfg(target_arch = "wasm32")]
const STORAGE_KEY: &str = "tamagotchi_save";

/// オートセーブ間隔 (tick)。10 ticks/sec → 30 秒に 1 度。
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
    stage: u8,
    hunger: u8,
    happiness: u8,
    cleanliness: u8,
    health: u8,
    age_ticks: u64,
    sleeping: bool,
    generation: u32,
    best_age_ticks: u64,
    total_ticks: u64,
    poop_count: u8,
}

#[cfg(any(target_arch = "wasm32", test))]
fn extract_save(state: &TamaState) -> SaveData {
    SaveData {
        version: SAVE_VERSION,
        game: GameSave {
            stage: state.stage.to_save_id(),
            hunger: state.stats.hunger,
            happiness: state.stats.happiness,
            cleanliness: state.stats.cleanliness,
            health: state.stats.health,
            age_ticks: state.age_ticks,
            sleeping: state.sleeping,
            generation: state.generation.max(1),
            best_age_ticks: state.best_age_ticks,
            total_ticks: state.total_ticks,
            poop_count: state.poop_count,
        },
    }
}

#[cfg(any(target_arch = "wasm32", test))]
fn apply_save(state: &mut TamaState, save: &GameSave) {
    state.stage = Stage::from_save_id(save.stage);
    state.stats = Stats {
        hunger: save.hunger.min(100),
        happiness: save.happiness.min(100),
        cleanliness: save.cleanliness.min(100),
        health: save.health.min(100),
    };
    state.age_ticks = save.age_ticks;
    state.sleeping = save.sleeping;
    state.generation = save.generation.max(1);
    state.best_age_ticks = save.best_age_ticks;
    state.total_ticks = save.total_ticks;
    state.poop_count = save.poop_count.min(5);
    // 演出 state は永続化しない: ロード直後はクリーン状態から再開する。
    state.last_action = None;
    state.action_flash = 0;
    state.anim_frame = 0;
    state.log.clear();
}

#[cfg(target_arch = "wasm32")]
fn get_storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok()?
}

#[cfg(target_arch = "wasm32")]
pub fn save_game(state: &TamaState) {
    let save_data = extract_save(state);
    let json = match serde_json::to_string(&save_data) {
        Ok(j) => j,
        Err(e) => {
            web_sys::console::warn_1(
                &format!("たまごっち: セーブのシリアライズに失敗: {e}").into(),
            );
            return;
        }
    };
    if let Some(storage) = get_storage() {
        if let Err(e) = storage.set_item(STORAGE_KEY, &json) {
            web_sys::console::warn_1(
                &format!("たまごっち: localStorage への保存に失敗: {e:?}").into(),
            );
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub fn load_game(state: &mut TamaState) -> bool {
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
                &format!("たまごっち: セーブのパースに失敗 (破棄): {e}").into(),
            );
            let _ = storage.remove_item(STORAGE_KEY);
            return false;
        }
    };
    if save_data.version < MIN_COMPATIBLE_VERSION {
        let _ = storage.remove_item(STORAGE_KEY);
        return false;
    }
    // 新しい version は `serde(default)` のフォールバックで吸収する想定 —
    // version が大きい場合に弾くと、別タブで先行プレイした save を消して
    // しまう事故になる。
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
    use super::super::logic;

    #[test]
    fn extract_then_apply_roundtrips() {
        let mut original = TamaState::new();
        logic::hatch(&mut original);
        original.stats.hunger = 42;
        original.stats.happiness = 30;
        original.age_ticks = 1234;
        original.generation = 7;
        original.best_age_ticks = 5000;
        original.poop_count = 3;
        original.sleeping = true;

        let saved = extract_save(&original);
        let mut restored = TamaState::new();
        apply_save(&mut restored, &saved.game);

        assert_eq!(restored.stage, original.stage);
        assert_eq!(restored.stats.hunger, 42);
        assert_eq!(restored.stats.happiness, 30);
        assert_eq!(restored.age_ticks, 1234);
        assert_eq!(restored.generation, 7);
        assert_eq!(restored.best_age_ticks, 5000);
        assert_eq!(restored.poop_count, 3);
        assert!(restored.sleeping);
    }

    #[test]
    fn dead_state_roundtrips() {
        // 「死亡したまま閉じる → 翌日開く」流れで Dead/best_age が消えないこと。
        let mut original = TamaState::new();
        logic::hatch(&mut original);
        original.age_ticks = 8000;
        original.best_age_ticks = 8000;
        original.stage = Stage::Dead;
        original.generation = 4;

        let saved = extract_save(&original);
        let mut restored = TamaState::new();
        apply_save(&mut restored, &saved.game);

        assert_eq!(restored.stage, Stage::Dead);
        assert_eq!(restored.best_age_ticks, 8000);
        assert_eq!(restored.generation, 4);
        assert!(restored.is_dead());
    }

    #[test]
    fn apply_save_clamps_oversized_values() {
        let save = GameSave {
            stage: 1,
            hunger: 250,
            happiness: 250,
            cleanliness: 250,
            health: 250,
            age_ticks: 0,
            sleeping: false,
            generation: 0,
            best_age_ticks: 0,
            total_ticks: 0,
            poop_count: 99,
        };
        let mut s = TamaState::new();
        apply_save(&mut s, &save);
        assert_eq!(s.stats.hunger, 100);
        assert_eq!(s.stats.happiness, 100);
        assert_eq!(s.stats.cleanliness, 100);
        assert_eq!(s.stats.health, 100);
        assert_eq!(s.poop_count, 5);
        // 0 generation は 1 に修正
        assert_eq!(s.generation, 1);
    }
}
