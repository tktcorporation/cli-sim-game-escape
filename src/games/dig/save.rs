//! 穴掘り長屋の localStorage 保存。

#[cfg(any(target_arch = "wasm32", test))]
use serde::{Deserialize, Serialize};

#[cfg(any(target_arch = "wasm32", test))]
use super::state::{
    DigState, ItemKind, COLLECTION_COUNT, MAX_SHOVEL_LEVEL, NEIGHBOR_COUNT, PIECE_COUNT, YARD_LEN,
};

#[cfg(any(target_arch = "wasm32", test))]
const SAVE_VERSION: u32 = 1;

#[cfg(target_arch = "wasm32")]
const MIN_COMPATIBLE_VERSION: u32 = 1;

#[cfg(target_arch = "wasm32")]
const STORAGE_KEY: &str = "dig_save";

/// オートセーブ間隔 (tick)。10 ticks/sec → 30 秒。
pub const AUTOSAVE_INTERVAL: u32 = 300;

/// 庭セルのシリアライズ表現。`0` = 未掘、`1..=12` = `ItemKind::to_save_id() + 1`。
#[cfg(any(target_arch = "wasm32", test))]
fn encode_yard_cell(c: Option<ItemKind>) -> u8 {
    match c {
        None => 0,
        Some(item) => item.to_save_id() + 1,
    }
}

#[cfg(any(target_arch = "wasm32", test))]
fn decode_yard_cell(v: u8) -> Option<ItemKind> {
    if v == 0 {
        None
    } else {
        ItemKind::from_save_id(v - 1)
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
struct NeighborSave {
    dug_today: bool,
    total_digs: u32,
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct GameSave {
    yard: Vec<u8>,
    actions_remaining: u8,
    last_reset_day: u64,
    coins: u64,
    total_coins_earned: u64,
    piece_counts: Vec<u32>,
    completed_sets: Vec<bool>,
    neighbors: Vec<NeighborSave>,
    shovel_level: u8,
    rng_state: u64,
}

#[cfg(any(target_arch = "wasm32", test))]
fn extract_save(state: &DigState) -> SaveData {
    let yard = state.yard.iter().map(|c| encode_yard_cell(*c)).collect();
    let neighbors = state
        .neighbors
        .iter()
        .map(|n| NeighborSave {
            dug_today: n.dug_today,
            total_digs: n.total_digs,
        })
        .collect();
    SaveData {
        version: SAVE_VERSION,
        game: GameSave {
            yard,
            actions_remaining: state.actions_remaining,
            last_reset_day: state.last_reset_day,
            coins: state.coins,
            total_coins_earned: state.total_coins_earned,
            piece_counts: state.piece_counts.to_vec(),
            completed_sets: state.completed_sets.to_vec(),
            neighbors,
            shovel_level: state.shovel_level,
            rng_state: state.rng_state,
        },
    }
}

#[cfg(any(target_arch = "wasm32", test))]
fn apply_save(state: &mut DigState, save: &GameSave) {
    if save.yard.len() == YARD_LEN {
        for (i, v) in save.yard.iter().enumerate() {
            state.yard[i] = decode_yard_cell(*v);
        }
    }
    state.actions_remaining = save.actions_remaining.min(super::state::MAX_ACTIONS_PER_DAY);
    state.last_reset_day = save.last_reset_day;
    state.coins = save.coins;
    state.total_coins_earned = save.total_coins_earned.max(save.coins);
    if save.piece_counts.len() == PIECE_COUNT {
        state.piece_counts.copy_from_slice(&save.piece_counts);
    }
    if save.completed_sets.len() == COLLECTION_COUNT {
        for (i, v) in save.completed_sets.iter().enumerate() {
            state.completed_sets[i] = *v;
        }
    }
    // ネイバー人数が一致する場合のみ反映する。名前・専門は固定定義由来なので
    // 保存対象から外し、`DigState::new()` が組み立てた既定値を保つ。
    if save.neighbors.len() == NEIGHBOR_COUNT {
        for (i, n) in save.neighbors.iter().enumerate() {
            state.neighbors[i].dug_today = n.dug_today;
            state.neighbors[i].total_digs = n.total_digs;
        }
    }
    state.shovel_level = save.shovel_level.min(MAX_SHOVEL_LEVEL);
    if save.rng_state != 0 {
        state.rng_state = save.rng_state;
    }
    // 演出 state はロード後にクリーンにする。
    state.collection_flash = None;
    state.log.clear();
}

#[cfg(target_arch = "wasm32")]
fn get_storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok()?
}

/// wall-clock (`Date.now()`, ms since epoch) を u64 で返す。
/// 非 finite / 負値は「未計測」扱いの 0 にフォールバックする。
#[cfg(target_arch = "wasm32")]
pub fn wall_clock_now_ms() -> u64 {
    let v = js_sys::Date::now();
    if v.is_finite() && v >= 0.0 {
        v as u64
    } else {
        0
    }
}

#[cfg(target_arch = "wasm32")]
pub fn save_game(state: &DigState) {
    let save_data = extract_save(state);
    let json = match serde_json::to_string(&save_data) {
        Ok(j) => j,
        Err(e) => {
            web_sys::console::warn_1(&format!("穴掘り長屋: シリアライズ失敗: {e}").into());
            return;
        }
    };
    if let Some(storage) = get_storage() {
        if let Err(e) = storage.set_item(STORAGE_KEY, &json) {
            web_sys::console::warn_1(&format!("穴掘り長屋: 保存失敗: {e:?}").into());
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub fn load_game(state: &mut DigState) -> bool {
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
            web_sys::console::warn_1(&format!("穴掘り長屋: パース失敗 (破棄): {e}").into());
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
    use super::super::state::CollectionSet;

    #[test]
    fn yard_cellのエンコードは往復可能() {
        assert_eq!(decode_yard_cell(encode_yard_cell(None)), None);
        for item in ItemKind::all() {
            assert_eq!(decode_yard_cell(encode_yard_cell(Some(item))), Some(item));
        }
    }

    #[test]
    fn save_roundtripは進行状況を保持する() {
        let mut original = DigState::new();
        logic::dig_yard(&mut original, 0);
        logic::dig_neighbor(&mut original, 1);
        original.coins = 999;
        original.total_coins_earned = 1500;
        original.shovel_level = 2;
        original.piece_counts[3] = 7;
        original.completed_sets[CollectionSet::Maneki.index()] = true;
        original.last_reset_day = 42;
        original.actions_remaining = 2;

        let saved = extract_save(&original);
        let mut restored = DigState::new();
        apply_save(&mut restored, &saved.game);

        assert_eq!(restored.coins, 999);
        assert_eq!(restored.total_coins_earned, 1500);
        assert_eq!(restored.shovel_level, 2);
        assert_eq!(restored.piece_counts[3], 7);
        assert!(restored.completed_sets[CollectionSet::Maneki.index()]);
        assert_eq!(restored.last_reset_day, 42);
        assert_eq!(restored.actions_remaining, 2);
        assert_eq!(restored.yard[0], original.yard[0]);
        assert!(restored.neighbors[1].dug_today);
        assert_eq!(restored.neighbors[1].total_digs, 1);
        // 固定定義由来のフィールドは保存対象外でも保持される。
        assert_eq!(restored.neighbors[1].name, original.neighbors[1].name);
    }

    #[test]
    fn apply_saveは不正な長さのvecを無視して既定値を保つ() {
        let save = GameSave {
            yard: vec![1, 2, 3], // YARD_LEN と不一致
            piece_counts: vec![1, 2],
            completed_sets: vec![true],
            neighbors: vec![NeighborSave::default()],
            ..Default::default()
        };
        let mut s = DigState::new();
        apply_save(&mut s, &save);
        assert!(s.yard.iter().all(|c| c.is_none()));
        assert_eq!(s.piece_counts, [0; PIECE_COUNT]);
        assert_eq!(s.completed_sets, [false; COLLECTION_COUNT]);
        assert!(!s.neighbors[0].dug_today);
    }

    #[test]
    fn apply_saveはshovel_levelを上限でclampする() {
        let save = GameSave {
            shovel_level: 99,
            ..Default::default()
        };
        let mut s = DigState::new();
        apply_save(&mut s, &save);
        assert_eq!(s.shovel_level, MAX_SHOVEL_LEVEL);
    }

    #[test]
    fn apply_saveはtotal_coins_earnedがcoins未満なら引き上げる() {
        let save = GameSave {
            coins: 500,
            total_coins_earned: 100, // 壊れたデータ (coins より少ない)
            ..Default::default()
        };
        let mut s = DigState::new();
        apply_save(&mut s, &save);
        assert_eq!(s.total_coins_earned, 500);
    }
}
