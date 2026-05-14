//! マージゲームの localStorage 保存。

#[cfg(any(target_arch = "wasm32", test))]
use serde::{Deserialize, Serialize};

#[cfg(any(target_arch = "wasm32", test))]
use super::state::{Cell, ItemType, MergeState, Quest, GENERATOR_POSITIONS, GRID_H, GRID_W};

#[cfg(any(target_arch = "wasm32", test))]
const SAVE_VERSION: u32 = 1;

#[cfg(target_arch = "wasm32")]
const MIN_COMPATIBLE_VERSION: u32 = 1;

#[cfg(target_arch = "wasm32")]
const STORAGE_KEY: &str = "merge_save";

/// オートセーブ間隔 (tick)。10 ticks/sec → 30 秒。
pub const AUTOSAVE_INTERVAL: u32 = 300;

/// セルのシリアライズ表現。
/// - `Empty` = 0
/// - `Generator(t)` = 100 + type_id  (固定位置から再構築するので保存不要だが
///   形式の対称性のため保存しておく)
/// - `Item(t, lv)` = t*10 + lv  (1..=15)
#[cfg(any(target_arch = "wasm32", test))]
fn encode_cell(c: Cell) -> u8 {
    match c {
        Cell::Empty => 0,
        Cell::Generator(t) => 100 + t.to_save_id(),
        Cell::Item(t, lv) => t.to_save_id() * 10 + lv.min(15),
    }
}

#[cfg(any(target_arch = "wasm32", test))]
fn decode_cell(v: u8) -> Cell {
    if v == 0 {
        Cell::Empty
    } else if v >= 100 {
        Cell::Generator(ItemType::from_save_id(v - 100))
    } else {
        let t = ItemType::from_save_id(v / 10);
        let lv = (v % 10).max(1);
        Cell::Item(t, lv)
    }
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Serialize, Deserialize)]
struct SaveData {
    version: u32,
    game: GameSave,
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct QuestSave {
    item_type: u8,
    level: u8,
    needed: u8,
    reward: u32,
    present: bool,
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct GameSave {
    grid: Vec<u8>,
    gen_cooldown: [u32; 3],
    gen_upgrade_level: u8,
    coins: u64,
    total_coins_earned: u64,
    quests: Vec<QuestSave>,
    rng_state: u64,
    best_level: u8,
}

#[cfg(any(target_arch = "wasm32", test))]
fn extract_save(state: &MergeState) -> SaveData {
    let grid = state.grid.iter().copied().map(encode_cell).collect();
    let quests = state
        .quests
        .iter()
        .map(|q| match q {
            Some(q) => QuestSave {
                item_type: q.item_type.to_save_id(),
                level: q.level,
                needed: q.needed,
                reward: q.reward,
                present: true,
            },
            None => QuestSave {
                present: false,
                ..Default::default()
            },
        })
        .collect();
    SaveData {
        version: SAVE_VERSION,
        game: GameSave {
            grid,
            gen_cooldown: state.gen_cooldown,
            gen_upgrade_level: state.gen_upgrade_level,
            coins: state.coins,
            total_coins_earned: state.total_coins_earned,
            quests,
            rng_state: state.rng_state,
            best_level: state.best_level,
        },
    }
}

#[cfg(any(target_arch = "wasm32", test))]
fn apply_save(state: &mut MergeState, save: &GameSave) {
    // grid サイズが壊れていたら新規 state に戻す (ジェネレーター位置を保証)。
    if save.grid.len() == GRID_W * GRID_H {
        for (i, v) in save.grid.iter().enumerate() {
            state.grid[i] = decode_cell(*v);
        }
        // ジェネレーター位置は不変条件。save 経由で壊れても再設定する。
        for (gx, gy, t) in GENERATOR_POSITIONS {
            state.set(gx, gy, Cell::Generator(t));
        }
    }
    state.gen_cooldown = save.gen_cooldown;
    state.gen_upgrade_level = save.gen_upgrade_level.min(super::state::MAX_UPGRADE);
    state.coins = save.coins;
    state.total_coins_earned = save.total_coins_earned.max(save.coins);
    state.best_level = save.best_level.min(super::state::MAX_LEVEL);
    if save.rng_state != 0 {
        state.rng_state = save.rng_state;
    }
    state.quests = [None; super::state::QUEST_SLOTS];
    for (i, q) in save.quests.iter().enumerate() {
        if i >= super::state::QUEST_SLOTS {
            break;
        }
        if q.present {
            state.quests[i] = Some(Quest {
                item_type: ItemType::from_save_id(q.item_type),
                level: q.level.clamp(1, super::state::MAX_LEVEL),
                needed: q.needed.max(1),
                reward: q.reward,
            });
        }
    }
    // 演出 state はロード後にクリーンに。
    state.selected = None;
    state.flash_cell = None;
    state.anim_frame = 0;
    state.log.clear();
}

#[cfg(target_arch = "wasm32")]
fn get_storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok()?
}

#[cfg(target_arch = "wasm32")]
pub fn save_game(state: &MergeState) {
    let save_data = extract_save(state);
    let json = match serde_json::to_string(&save_data) {
        Ok(j) => j,
        Err(e) => {
            web_sys::console::warn_1(&format!("merge: シリアライズ失敗: {e}").into());
            return;
        }
    };
    if let Some(storage) = get_storage() {
        if let Err(e) = storage.set_item(STORAGE_KEY, &json) {
            web_sys::console::warn_1(&format!("merge: 保存失敗: {e:?}").into());
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub fn load_game(state: &mut MergeState) -> bool {
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
            web_sys::console::warn_1(&format!("merge: パース失敗 (破棄): {e}").into());
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
    use super::super::logic;
    use super::super::state::ItemType;

    #[test]
    fn cell_encode_roundtrip() {
        for c in [
            Cell::Empty,
            Cell::Generator(ItemType::Flower),
            Cell::Generator(ItemType::Gem),
            Cell::Generator(ItemType::Tool),
            Cell::Item(ItemType::Flower, 1),
            Cell::Item(ItemType::Flower, 5),
            Cell::Item(ItemType::Gem, 3),
            Cell::Item(ItemType::Tool, 4),
        ] {
            assert_eq!(decode_cell(encode_cell(c)), c);
        }
    }

    #[test]
    fn save_roundtrip_preserves_progress() {
        let mut original = MergeState::new();
        // 何ターン進めて状態を作る
        logic::tap_cell(&mut original, 0, 0); // Flower 生成
        logic::tick(&mut original, 1); // クエストが埋まる
        original.coins = 1234;
        original.total_coins_earned = 5678;
        original.gen_upgrade_level = 2;
        original.best_level = 3;
        original.set(2, 2, Cell::Item(ItemType::Tool, 4));

        let saved = extract_save(&original);
        let mut restored = MergeState::new();
        apply_save(&mut restored, &saved.game);

        assert_eq!(restored.coins, 1234);
        assert_eq!(restored.total_coins_earned, 5678);
        assert_eq!(restored.gen_upgrade_level, 2);
        assert_eq!(restored.best_level, 3);
        assert_eq!(restored.get(2, 2), Cell::Item(ItemType::Tool, 4));
        assert_eq!(restored.quests, original.quests);
        // ジェネレーター位置は維持
        assert_eq!(restored.get(0, 0), Cell::Generator(ItemType::Flower));
    }

    #[test]
    fn apply_save_with_corrupt_grid_keeps_generators() {
        let save = GameSave {
            grid: vec![],
            ..Default::default()
        };
        let mut s = MergeState::new();
        apply_save(&mut s, &save);
        // 不正な grid サイズは無視、新規状態を保持
        assert_eq!(s.get(0, 0), Cell::Generator(ItemType::Flower));
    }

    #[test]
    fn apply_save_clamps_upgrade_and_level() {
        let mut save = GameSave {
            grid: (0..(GRID_W * GRID_H)).map(|_| 0).collect(),
            ..Default::default()
        };
        save.gen_upgrade_level = 99;
        save.best_level = 99;
        let mut s = MergeState::new();
        apply_save(&mut s, &save);
        assert_eq!(s.gen_upgrade_level, super::super::state::MAX_UPGRADE);
        assert_eq!(s.best_level, super::super::state::MAX_LEVEL);
    }
}
