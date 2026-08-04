//! 常夜灯 セーブ/ロード。
//!
//! 永続資源 (残光・拠点強化・自己ベスト・乱数シード) のみを保存する。
//! 夜番中の進行 (敵・弾・宝箱・現在の装備・灯の残量) は対象外 —
//! リロードすれば拠点からやり直しになる。死亡/撤退と同じ非対称性を
//! リロードにも一貫させるための意図的な設計 (loopmarchと同じ方針)。

#[cfg(any(target_arch = "wasm32", test))]
use serde::{Deserialize, Serialize};

#[cfg(any(target_arch = "wasm32", test))]
use super::state::{CampUpgrades, EverlightState, LanternType, WeaponKind};

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
    extra_weapon_slot_level: u32,
    rng_state: u32,
    /// 挑戦を許された最大の夜番ランク。旧セーブ (フィールド無し) は
    /// serdeのデフォルト(0)で読み込まれるが、`apply_save` 側で1未満を
    /// 1へ補正する (ランク1は常に挑戦可能なため)。
    max_unlocked_rank: u32,
    /// 拠点で選択中の挑戦ランク。保存しないとリロードのたびに選択が
    /// 第1夜へ戻ってしまう。
    selected_rank: u32,
    /// 解放済み武器の `WeaponKind::save_id()` 一覧。旧セーブ (フィールド
    /// 無し) は空Vecで読み込まれるが、`apply_save` 側で「光弾のみ解放」
    /// (`CampUpgrades::default()`相当) へ補正する — 空Vecのまま扱うと
    /// 光弾すら使えない状態になってしまう。
    unlocked_weapon_ids: Vec<u8>,
    /// 夜番開始時の初期武器 (`WeaponKind::save_id()`)。デフォルト0=光弾は
    /// 旧セーブの欠損値としても自然に成立する。
    starting_weapon_id: u8,
    /// 灯のタイプ (`LanternType::save_id()`)。デフォルト0=常灯は旧セーブの
    /// 欠損値 (=挙動が変わらない) としても自然に成立する。
    lantern_type_id: u8,
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
            extra_weapon_slot_level: state.camp.extra_weapon_slot_level,
            rng_state: state.rng_state,
            max_unlocked_rank: state.camp.max_unlocked_rank,
            selected_rank: state.camp.selected_rank,
            unlocked_weapon_ids: state.camp.unlocked_weapons.iter().map(|k| k.save_id()).collect(),
            starting_weapon_id: state.camp.starting_weapon.save_id(),
            lantern_type_id: state.camp.lantern_type.save_id(),
        },
    }
}

#[cfg(any(target_arch = "wasm32", test))]
fn apply_save(state: &mut EverlightState, save: &GameSave) {
    state.ember = save.ember;
    state.best_wave = save.best_wave;
    state.best_survival_ticks = save.best_survival_ticks;
    // 旧セーブ (フィールド無し→空Vec) は「光弾のみ解放」のデフォルトへ
    // 補正する。未知のidは将来のバージョン間往復を安全にするため無視する。
    let mut unlocked_weapons: Vec<WeaponKind> =
        save.unlocked_weapon_ids.iter().filter_map(|&id| WeaponKind::from_save_id(id)).collect();
    if unlocked_weapons.is_empty() {
        unlocked_weapons.push(WeaponKind::Bolt);
    } else if !unlocked_weapons.contains(&WeaponKind::Bolt) {
        // 光弾は常に無料解放済みという不変条件を、手動編集されたセーブ
        // からの復元でも保つ。
        unlocked_weapons.push(WeaponKind::Bolt);
    }
    let starting_weapon = WeaponKind::from_save_id(save.starting_weapon_id).unwrap_or(WeaponKind::Bolt);
    let lantern_type = LanternType::from_save_id(save.lantern_type_id).unwrap_or(LanternType::Steady);
    state.camp = CampUpgrades {
        light_level: save.light_level,
        power_level: save.power_level,
        extra_slot_level: save.extra_slot_level,
        extra_weapon_slot_level: save.extra_weapon_slot_level,
        max_unlocked_rank: save.max_unlocked_rank.max(1),
        // 保存されたランクが (バージョン違いや手動編集で) 解放範囲外に
        // なっていても安全に読めるよう、旧セーブと同じ経路でクランプする。
        selected_rank: save.selected_rank.clamp(1, save.max_unlocked_rank.max(1)),
        unlocked_weapons,
        starting_weapon,
        lantern_type,
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
        original.camp.extra_weapon_slot_level = 1;
        original.camp.max_unlocked_rank = 3;
        original.camp.selected_rank = 2;
        original.camp.unlocked_weapons = vec![WeaponKind::Bolt, WeaponKind::Spray, WeaponKind::Chain];
        original.camp.starting_weapon = WeaponKind::Chain;
        original.camp.lantern_type = LanternType::Warden;
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
        assert_eq!(restored.camp.extra_weapon_slot_level, 1);
        assert_eq!(restored.camp.max_unlocked_rank, 3, "解放済みランクも保存/復元されるはず");
        assert_eq!(restored.camp.selected_rank, 2, "選択中ランクも保存/復元され、リロードで第1夜に戻らないはず");
        assert_eq!(restored.lantern.light_max, restored.camp.light_max());
        assert_eq!(
            restored.rng_state, 999_999,
            "rng_state を保存しないとリロードのたびに同じ乱数列を再生してしまう"
        );
        assert_eq!(
            restored.camp.unlocked_weapons, original.camp.unlocked_weapons,
            "解放済み武器も保存/復元されるはず"
        );
        assert_eq!(restored.camp.starting_weapon, WeaponKind::Chain, "初期武器も保存/復元されるはず");
        assert_eq!(restored.camp.lantern_type, LanternType::Warden, "灯のタイプも保存/復元されるはず");
    }

    #[test]
    fn save_without_new_unlock_fields_loads_with_bolt_only_defaults() {
        // 武器解放システム導入前の旧セーブ (該当キーが無いJSON) を読み込んでも
        // panicせず、「光弾のみ解放・初期武器は光弾・灯は常灯」という
        // 導入前と同じ挙動になることを保証する。
        let json = r#"{"version":1,"game":{"ember":10,"best_wave":2}}"#;
        let loaded: SaveData = serde_json::from_str(json).unwrap();

        let mut restored = EverlightState::new();
        apply_save(&mut restored, &loaded.game);

        assert_eq!(restored.camp.unlocked_weapons, vec![WeaponKind::Bolt]);
        assert_eq!(restored.camp.starting_weapon, WeaponKind::Bolt);
        assert_eq!(restored.camp.lantern_type, LanternType::Steady);
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
        assert_eq!(restored.camp.extra_weapon_slot_level, 0, "旧セーブに無いフィールドはデフォルト(未購入)になるはず");
        assert_eq!(
            restored.camp.max_unlocked_rank, 1,
            "旧セーブにmax_unlocked_rankが無くてもランク1は挑戦可能でなければならない"
        );
        assert_eq!(
            restored.camp.selected_rank, 1,
            "旧セーブにselected_rankが無くても範囲内(1)へ補正されるはず"
        );
    }
}
