//! 深淵潜行 (Abyss Idle) セーブ/ロード機能。
//!
//! ## バージョニング方針
//!
//! - `SAVE_VERSION`: 現在のセーブ形式バージョン。フィールド追加時にインクリメントする。
//! - `MIN_COMPATIBLE_VERSION`: 互換性を維持できる最小バージョン。
//!   新フィールドの追加のみの場合はこの値を変えない (旧データを維持できる)。
//!   既存フィールドの意味変更や削除など破壊的変更を行った場合のみインクリメントする。

#[cfg(any(target_arch = "wasm32", test))]
use serde::{Deserialize, Serialize};

#[cfg(any(target_arch = "wasm32", test))]
use super::state::{AbyssState, FloorKind, Tab};

#[cfg(any(target_arch = "wasm32", test))]
const SAVE_VERSION: u32 = 1;

#[cfg(target_arch = "wasm32")]
const MIN_COMPATIBLE_VERSION: u32 = 1;

#[cfg(target_arch = "wasm32")]
const STORAGE_KEY: &str = "abyss_idle_save";

/// イベントベース保存の保険として走らせる定期セーブ間隔 (tick 数)。
/// 10 ticks/sec × 30 秒 = 300 ticks。auto_descend OFF で同フロア周回中も
/// gold/souls/keys が積み上がるので、ミルストーン以外のドリフトを救うため。
pub const AUTOSAVE_INTERVAL: u32 = 300;

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Serialize, Deserialize)]
struct SaveData {
    version: u32,
    game: GameSave,
}

/// 永続化対象フィールド。current_enemy / フラッシュ等の演出 state は保存しない
/// (敵は次 tick で再スポーン、演出は新規ロードで自然にリセットされるため)。
#[cfg(any(target_arch = "wasm32", test))]
#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct GameSave {
    upgrades: [u32; 7],
    soul_perks: [u32; 4],
    souls: u64,

    gold: u64,
    floor: u32,
    max_floor: u32,
    kills_on_floor: u32,
    run_kills: u64,
    run_gold_earned: u64,

    hero_hp: u64,
    combat_focus: u32,

    /// FloorKind の index: 0=Normal, 1=Treasure, 2=Elite, 3=Bonanza
    floor_kind: u8,
    auto_descend: bool,
    /// Tab の index: 0=Upgrades, 1=Souls, 2=Stats, 3=Gacha
    tab: u8,

    keys: u64,
    pulls_since_epic: u32,
    total_pulls: u64,

    deepest_floor_ever: u32,
    total_kills: u64,
    deaths: u64,
    total_ticks: u64,
    rng_state: u32,
}

#[cfg(any(target_arch = "wasm32", test))]
fn extract_save(state: &AbyssState) -> SaveData {
    SaveData {
        version: SAVE_VERSION,
        game: GameSave {
            upgrades: state.upgrades,
            soul_perks: state.soul_perks,
            souls: state.souls,
            gold: state.gold,
            floor: state.floor,
            max_floor: state.max_floor,
            kills_on_floor: state.kills_on_floor,
            run_kills: state.run_kills,
            run_gold_earned: state.run_gold_earned,
            hero_hp: state.hero_hp,
            combat_focus: state.combat_focus,
            floor_kind: match state.floor_kind {
                FloorKind::Normal => 0,
                FloorKind::Treasure => 1,
                FloorKind::Elite => 2,
                FloorKind::Bonanza => 3,
            },
            auto_descend: state.auto_descend,
            tab: match state.tab {
                Tab::Upgrades => 0,
                Tab::Souls => 1,
                Tab::Stats => 2,
                Tab::Gacha => 3,
            },
            keys: state.keys,
            pulls_since_epic: state.pulls_since_epic,
            total_pulls: state.total_pulls,
            deepest_floor_ever: state.deepest_floor_ever,
            total_kills: state.total_kills,
            deaths: state.deaths,
            total_ticks: state.total_ticks,
            rng_state: state.rng_state,
        },
    }
}

#[cfg(any(target_arch = "wasm32", test))]
fn apply_save(state: &mut AbyssState, save: &GameSave) {
    state.upgrades = save.upgrades;
    state.soul_perks = save.soul_perks;
    state.souls = save.souls;
    state.gold = save.gold;
    state.floor = save.floor.max(1);
    state.max_floor = save.max_floor.max(state.floor);
    state.kills_on_floor = save.kills_on_floor;
    state.run_kills = save.run_kills;
    state.run_gold_earned = save.run_gold_earned;

    let max_hp = state.hero_max_hp();
    state.hero_hp = if save.hero_hp == 0 {
        max_hp
    } else {
        save.hero_hp.min(max_hp)
    };
    state.combat_focus = save.combat_focus;
    state.hero_atk_cooldown = state.hero_atk_period();
    state.hero_regen_acc_x100 = 0;

    state.floor_kind = match save.floor_kind {
        1 => FloorKind::Treasure,
        2 => FloorKind::Elite,
        3 => FloorKind::Bonanza,
        _ => FloorKind::Normal,
    };
    state.auto_descend = save.auto_descend;
    state.tab = match save.tab {
        1 => Tab::Souls,
        2 => Tab::Stats,
        3 => Tab::Gacha,
        _ => Tab::Upgrades,
    };

    state.keys = save.keys;
    state.pulls_since_epic = save.pulls_since_epic;
    state.total_pulls = save.total_pulls;
    state.deepest_floor_ever = save.deepest_floor_ever.max(state.floor);
    state.total_kills = save.total_kills;
    state.deaths = save.deaths;
    state.total_ticks = save.total_ticks;
    state.rng_state = save.rng_state;

    // 敵を 0 化して次 tick で再スポーンさせる (Enemy 自体は保存しない方針)。
    state.current_enemy.hp = 0;
    state.current_enemy.max_hp = 0;
    state.last_enemy_damage = None;
    state.last_hero_damage = None;
    state.hero_hurt_flash = 0;
    state.enemy_hurt_flash = 0;
    state.descent_flash = 0;
    state.last_gacha = None;
    state.log.clear();
}

#[cfg(target_arch = "wasm32")]
fn get_storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok()?
}

#[cfg(target_arch = "wasm32")]
pub fn save_game(state: &AbyssState) {
    let save_data = extract_save(state);
    let json = match serde_json::to_string(&save_data) {
        Ok(j) => j,
        Err(e) => {
            web_sys::console::warn_1(
                &format!("Abyss Idle: セーブのシリアライズに失敗: {e}").into(),
            );
            return;
        }
    };
    if let Some(storage) = get_storage() {
        if let Err(e) = storage.set_item(STORAGE_KEY, &json) {
            web_sys::console::warn_1(
                &format!("Abyss Idle: localStorage への保存に失敗: {e:?}").into(),
            );
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub fn load_game(state: &mut AbyssState) -> bool {
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
                &format!("Abyss Idle: セーブデータのパースに失敗 (破棄します): {e}").into(),
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

#[cfg(target_arch = "wasm32")]
pub fn delete_save() {
    if let Some(storage) = get_storage() {
        let _ = storage.remove_item(STORAGE_KEY);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::games::abyss::state::{SoulPerk, UpgradeKind};

    #[test]
    fn extract_and_apply_roundtrip() {
        let mut original = AbyssState::new();
        original.upgrades[UpgradeKind::Sword.index()] = 5;
        original.upgrades[UpgradeKind::Vitality.index()] = 3;
        original.soul_perks[SoulPerk::Might.index()] = 2;
        original.souls = 42;
        original.gold = 12345;
        original.floor = 7;
        original.max_floor = 10;
        original.kills_on_floor = 3;
        original.run_kills = 50;
        original.run_gold_earned = 9999;
        original.combat_focus = 4;
        original.floor_kind = FloorKind::Elite;
        original.auto_descend = false;
        original.tab = Tab::Gacha;
        original.keys = 25;
        original.pulls_since_epic = 12;
        original.total_pulls = 80;
        original.deepest_floor_ever = 15;
        original.total_kills = 200;
        original.deaths = 3;
        original.total_ticks = 50000;
        original.rng_state = 0xABCD;
        original.hero_hp = original.hero_max_hp() / 2;

        let save = extract_save(&original);
        let json = serde_json::to_string(&save).unwrap();
        let loaded: SaveData = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.version, SAVE_VERSION);

        let mut restored = AbyssState::new();
        apply_save(&mut restored, &loaded.game);

        assert_eq!(restored.upgrades[UpgradeKind::Sword.index()], 5);
        assert_eq!(restored.upgrades[UpgradeKind::Vitality.index()], 3);
        assert_eq!(restored.soul_perks[SoulPerk::Might.index()], 2);
        assert_eq!(restored.souls, 42);
        assert_eq!(restored.gold, 12345);
        assert_eq!(restored.floor, 7);
        assert_eq!(restored.max_floor, 10);
        assert_eq!(restored.kills_on_floor, 3);
        assert_eq!(restored.run_kills, 50);
        assert_eq!(restored.run_gold_earned, 9999);
        assert_eq!(restored.combat_focus, 4);
        assert_eq!(restored.floor_kind, FloorKind::Elite);
        assert!(!restored.auto_descend);
        assert_eq!(restored.tab, Tab::Gacha);
        assert_eq!(restored.keys, 25);
        assert_eq!(restored.pulls_since_epic, 12);
        assert_eq!(restored.total_pulls, 80);
        assert_eq!(restored.deepest_floor_ever, 15);
        assert_eq!(restored.total_kills, 200);
        assert_eq!(restored.deaths, 3);
        assert_eq!(restored.total_ticks, 50000);
        assert_eq!(restored.rng_state, 0xABCD);
        assert!(restored.hero_hp > 0 && restored.hero_hp <= restored.hero_max_hp());
        // Enemy は再スポーン用に zero 化されている。
        assert_eq!(restored.current_enemy.hp, 0);
        assert_eq!(restored.current_enemy.max_hp, 0);
    }

    /// hero_hp == 0 のセーブは max_hp に補正される (死亡途中で保存されたケースを救う)。
    #[test]
    fn zero_hero_hp_restored_to_full() {
        let mut original = AbyssState::new();
        original.hero_hp = 0;
        let save = extract_save(&original);
        let json = serde_json::to_string(&save).unwrap();
        let loaded: SaveData = serde_json::from_str(&json).unwrap();

        let mut restored = AbyssState::new();
        apply_save(&mut restored, &loaded.game);
        assert_eq!(restored.hero_hp, restored.hero_max_hp());
    }

    /// hero_hp が max_hp を超えるケースは max_hp に clamp される。
    #[test]
    fn over_max_hp_clamped() {
        let mut original = AbyssState::new();
        original.hero_hp = 999_999;
        let save = extract_save(&original);
        let json = serde_json::to_string(&save).unwrap();
        let loaded: SaveData = serde_json::from_str(&json).unwrap();

        let mut restored = AbyssState::new();
        apply_save(&mut restored, &loaded.game);
        assert!(restored.hero_hp <= restored.hero_max_hp());
    }

    /// 未知の追加フィールドが含まれていても無視される。
    #[test]
    fn unknown_fields_in_json_are_ignored() {
        let json = r#"{
            "version": 1,
            "game": {
                "upgrades": [1,0,0,0,0,0,0],
                "soul_perks": [0,0,0,0],
                "souls": 0,
                "gold": 100,
                "floor": 2,
                "max_floor": 2,
                "kills_on_floor": 0,
                "run_kills": 0,
                "run_gold_earned": 0,
                "hero_hp": 50,
                "combat_focus": 0,
                "floor_kind": 0,
                "auto_descend": true,
                "tab": 0,
                "keys": 0,
                "pulls_since_epic": 0,
                "total_pulls": 0,
                "deepest_floor_ever": 2,
                "total_kills": 0,
                "deaths": 0,
                "total_ticks": 0,
                "rng_state": 1,
                "future_unknown_field": 999
            }
        }"#;
        let loaded: SaveData = serde_json::from_str(json).unwrap();
        assert_eq!(loaded.game.gold, 100);
        assert_eq!(loaded.game.floor, 2);
    }

    /// 部分的なフィールドのみ持つ JSON でも default で補完される。
    #[test]
    fn partial_json_uses_defaults() {
        let json = r#"{ "version": 1, "game": { "gold": 500 } }"#;
        let loaded: SaveData = serde_json::from_str(json).unwrap();
        assert_eq!(loaded.game.gold, 500);
        assert_eq!(loaded.game.floor, 0);

        let mut restored = AbyssState::new();
        apply_save(&mut restored, &loaded.game);
        assert_eq!(restored.gold, 500);
        // floor は最低 1 に補正される。
        assert_eq!(restored.floor, 1);
    }
}
