//! 深淵潜行 (Abyss Idle) セーブ/ロード機能。
//!
//! ## バージョニング方針
//!
//! - `SAVE_VERSION`: 現在のセーブ形式バージョン。フィールド追加時にインクリメントする。
//! - `MIN_COMPATIBLE_VERSION`: 互換性を維持できる最小バージョン。
//!
//! v3 で「装備中心の進行軸」へ刷新したため、旧 save (v1/v2) は破棄。
//! `MIN_COMPATIBLE_VERSION = 3` にして旧データは load_game で弾かれる。

#[cfg(any(target_arch = "wasm32", test))]
use serde::{Deserialize, Serialize};

#[cfg(any(target_arch = "wasm32", test))]
use super::state::{AbyssState, EquipmentId, FloorKind, Tab, EQUIPMENT_COUNT, LANE_COUNT};

#[cfg(any(target_arch = "wasm32", test))]
const SAVE_VERSION: u32 = 3;

#[cfg(target_arch = "wasm32")]
const MIN_COMPATIBLE_VERSION: u32 = 3;

#[cfg(target_arch = "wasm32")]
const STORAGE_KEY: &str = "abyss_idle_save";

/// イベントベース保存の保険として走らせる定期セーブ間隔 (tick 数)。
pub const AUTOSAVE_INTERVAL: u32 = 300;

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Serialize, Deserialize)]
struct SaveData {
    version: u32,
    game: GameSave,
}

/// 永続化対象フィールド。current_enemy / フラッシュ等の演出 state は保存しない。
#[cfg(any(target_arch = "wasm32", test))]
#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct GameSave {
    /// 解放済み装備フラグ。
    owned_equipment: Vec<bool>,
    /// 各装備の強化レベル。`EquipmentId::index()` 順。
    equipment_levels: Vec<u32>,
    /// 各 lane の装着 EquipmentId (`None` は -1 で表現)。
    /// `EquipmentLane::index()` 順 (Weapon=0, Armor=1, Accessory=2)。
    equipped: Vec<i16>,

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

    floor_kind: u8,
    auto_descend: bool,
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
    // 装着スロット (Option<EquipmentId>) を i16 列に詰める。-1 が None、
    // 0..EQUIPMENT_COUNT が EquipmentId::index()。range 検査は load 側で行う。
    let equipped: Vec<i16> = state
        .equipped
        .iter()
        .map(|slot| match slot {
            Some(id) => id.index() as i16,
            None => -1,
        })
        .collect();

    SaveData {
        version: SAVE_VERSION,
        game: GameSave {
            owned_equipment: state.owned_equipment.to_vec(),
            equipment_levels: state.equipment_levels.to_vec(),
            equipped,
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
            floor_kind: state.floor_kind.to_save_id(),
            auto_descend: state.auto_descend,
            tab: state.tab.to_save_id(),
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
    state.soul_perks = save.soul_perks;
    state.souls = save.souls;
    state.gold = save.gold;
    state.floor = save.floor.max(1);
    state.max_floor = save.max_floor.max(state.floor);
    state.kills_on_floor = save.kills_on_floor;
    state.run_kills = save.run_kills;
    state.run_gold_earned = save.run_gold_earned;

    // 装備フラグ・強化 Lv・装着スロットを **HP clamp より前に** 復元する。
    // 装備込みの max HP を確定させてから hero_hp を clamp しないと、
    // 装備で底上げされた HP が「装備抜き max」で切られて読み込まれてしまう。
    for (i, slot) in state.owned_equipment.iter_mut().enumerate() {
        *slot = save.owned_equipment.get(i).copied().unwrap_or(false);
    }
    for (i, slot) in state.equipment_levels.iter_mut().enumerate() {
        *slot = save.equipment_levels.get(i).copied().unwrap_or(0);
    }
    let _ = EQUIPMENT_COUNT;
    for (lane_i, slot) in state.equipped.iter_mut().enumerate() {
        *slot = match save.equipped.get(lane_i).copied() {
            Some(idx) if idx >= 0 => {
                let id = EquipmentId::from_index(idx as usize);
                // 装備が存在し、かつ所持済みのときだけ装着を復元する
                // (途中でテーブルが縮んだ・所持を失った等の異常からのフェイルセーフ)。
                id.filter(|id| state.owned_equipment[id.index()])
            }
            _ => None,
        };
    }
    let _ = LANE_COUNT;

    let max_hp = state.hero_max_hp();
    state.hero_hp = if save.hero_hp == 0 {
        max_hp
    } else {
        save.hero_hp.min(max_hp)
    };
    state.combat_focus = save.combat_focus;
    state.hero_atk_cooldown = state.hero_atk_period();
    state.hero_regen_acc_x100 = 0;

    state.floor_kind = FloorKind::from_save_id(save.floor_kind);
    state.auto_descend = save.auto_descend;
    state.tab = Tab::from_save_id(save.tab);

    state.keys = save.keys;
    state.pulls_since_epic = save.pulls_since_epic;
    state.total_pulls = save.total_pulls;
    state.deepest_floor_ever = save.deepest_floor_ever.max(state.floor);
    state.total_kills = save.total_kills;
    state.deaths = save.deaths;
    state.total_ticks = save.total_ticks;
    state.rng_state = save.rng_state;

    // 敵を 0 化して次 tick で再スポーンさせる。
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
        // v3 以前 (旧 UpgradeKind 体系) は破棄: 進行軸が根本的に変わったため、
        // 機械的なマイグレーションでは整合が取れない。完全新規スタートさせる。
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
    use crate::games::abyss::state::{EquipmentId, EquipmentLane, SoulPerk};

    #[test]
    fn extract_and_apply_roundtrip() {
        let mut original = AbyssState::new();
        original.gold = 1_000_000_000;
        crate::games::abyss::logic::buy_equipment(&mut original, EquipmentId::BronzeSword);
        crate::games::abyss::logic::buy_equipment(&mut original, EquipmentId::LeatherArmor);
        for _ in 0..5 {
            crate::games::abyss::logic::enhance_equipment(&mut original, EquipmentId::BronzeSword);
        }
        original.soul_perks[SoulPerk::Might.index()] = 2;
        original.souls = 42;
        original.gold = 12345;
        original.floor = 7;
        original.max_floor = 10;
        original.kills_on_floor = 3;
        original.run_kills = 50;
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

        assert!(restored.owned_equipment[EquipmentId::BronzeSword.index()]);
        assert!(restored.owned_equipment[EquipmentId::LeatherArmor.index()]);
        assert_eq!(restored.equipment_levels[EquipmentId::BronzeSword.index()], 5);
        assert_eq!(
            restored.equipped[EquipmentLane::Weapon.index()],
            Some(EquipmentId::BronzeSword)
        );
        assert_eq!(
            restored.equipped[EquipmentLane::Armor.index()],
            Some(EquipmentId::LeatherArmor)
        );
        assert_eq!(restored.soul_perks[SoulPerk::Might.index()], 2);
        assert_eq!(restored.souls, 42);
        assert_eq!(restored.gold, 12345);
        assert_eq!(restored.floor, 7);
        assert_eq!(restored.floor_kind, FloorKind::Elite);
        assert_eq!(restored.tab, Tab::Gacha);
        assert!(restored.hero_hp > 0 && restored.hero_hp <= restored.hero_max_hp());
        assert_eq!(restored.current_enemy.hp, 0);
    }

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

    /// 装備で max が大きく上がったセーブを、装備復元前に hp clamp しないこと。
    /// 装備フラグ → max_hp 計算 → hero_hp clamp の順を守れているかの不変条件。
    #[test]
    fn equipment_restored_before_hp_clamp() {
        let mut original = AbyssState::new();
        original.gold = 1_000_000_000;
        // 防具系を全部解放して HP 大盛りにする。
        for id in [
            EquipmentId::LeatherArmor,
            EquipmentId::SteelArmor,
            EquipmentId::MithrilArmor,
            EquipmentId::GodArmor,
        ] {
            crate::games::abyss::logic::buy_equipment(&mut original, id);
        }
        // GodArmor が装着中になっているはず (購入で自動装着)。
        for _ in 0..30 {
            crate::games::abyss::logic::enhance_equipment(&mut original, EquipmentId::GodArmor);
        }
        let full_max = original.hero_max_hp();
        original.hero_hp = full_max;

        let save = extract_save(&original);
        let json = serde_json::to_string(&save).unwrap();
        let loaded: SaveData = serde_json::from_str(&json).unwrap();
        let mut restored = AbyssState::new();
        apply_save(&mut restored, &loaded.game);

        assert_eq!(restored.hero_hp, full_max);
        assert_eq!(restored.hero_max_hp(), full_max);
    }

    /// 装着スロットに「所持していない」装備の id が入った変な save データは
    /// 装着 None にフォールバックされる。
    #[test]
    fn unequipped_if_not_owned() {
        let json = r#"{
            "version": 3,
            "game": {
                "owned_equipment": [false, false, false, false, false, false, false, false, false, false, false, false],
                "equipment_levels": [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
                "equipped": [0, -1, -1],
                "soul_perks": [0, 0, 0, 0],
                "souls": 0,
                "gold": 0,
                "floor": 1,
                "max_floor": 1,
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
                "deepest_floor_ever": 1,
                "total_kills": 0,
                "deaths": 0,
                "total_ticks": 0,
                "rng_state": 1
            }
        }"#;
        let loaded: SaveData = serde_json::from_str(json).unwrap();
        let mut restored = AbyssState::new();
        apply_save(&mut restored, &loaded.game);
        // 「BronzeSword 装着中」のフラグだったが所持していないので装着なしになる。
        assert!(restored.equipped.iter().all(|s| s.is_none()));
    }

    /// 部分的なフィールドのみ持つ JSON でも default で補完される。
    #[test]
    fn partial_json_uses_defaults() {
        let json = r#"{ "version": 3, "game": { "gold": 500 } }"#;
        let loaded: SaveData = serde_json::from_str(json).unwrap();
        assert_eq!(loaded.game.gold, 500);
        assert_eq!(loaded.game.floor, 0);

        let mut restored = AbyssState::new();
        apply_save(&mut restored, &loaded.game);
        assert_eq!(restored.gold, 500);
        assert_eq!(restored.floor, 1); // 最低 1 に補正
    }
}
