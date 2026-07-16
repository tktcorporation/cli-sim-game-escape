//! 穴掘り長屋の localStorage 保存。
//!
//! 現場の宝配置は日付から決定的に再生成できるため保存しない — `day` と
//! 掘削状況だけ保存すれば現場を完全に復元できる。

#[cfg(any(target_arch = "wasm32", test))]
use serde::{Deserialize, Serialize};

#[cfg(any(target_arch = "wasm32", test))]
use super::logic;
#[cfg(any(target_arch = "wasm32", test))]
use super::state::{
    DigState, COLLECTION_COUNT, KIND_COUNT, RADAR_MAX_PER_DAY, SHOVELS_PER_DAY, SITE_LEN,
};

#[cfg(any(target_arch = "wasm32", test))]
const SAVE_VERSION: u32 = 2;

/// v2 でゲーム構造ごと作り直したため v1 セーブは互換なし (破棄)。
#[cfg(target_arch = "wasm32")]
const MIN_COMPATIBLE_VERSION: u32 = 2;

#[cfg(target_arch = "wasm32")]
const STORAGE_KEY: &str = "dig_save";

/// オートセーブ間隔 (tick)。10 ticks/sec → 30 秒。
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
    day: u64,
    /// 保存時点の現場配置のフィンガープリント。ロード時に day から
    /// 再生成した現場と一致しなければ、`dug` 等の当日進行は別配置への
    /// 誤適用になるため破棄する (生成ロジック変更時の安全弁)。
    site_fingerprint: u64,
    dug: Vec<bool>,
    scanned: Vec<bool>,
    shovels: u8,
    radar_uses: u8,
    perfect_bonus_given: bool,
    coins: u64,
    total_coins_earned: u64,
    museum_counts: Vec<u32>,
    completed_sets: Vec<bool>,
}

#[cfg(any(target_arch = "wasm32", test))]
fn extract_save(state: &DigState) -> SaveData {
    SaveData {
        version: SAVE_VERSION,
        game: GameSave {
            day: state.day,
            site_fingerprint: logic::site_fingerprint(&state.treasures),
            dug: state.dug.to_vec(),
            scanned: state.scanned.to_vec(),
            shovels: state.shovels,
            radar_uses: state.radar_uses,
            perfect_bonus_given: state.perfect_bonus_given,
            coins: state.coins,
            total_coins_earned: state.total_coins_earned,
            museum_counts: state.museum_counts.to_vec(),
            completed_sets: state.completed_sets.to_vec(),
        },
    }
}

#[cfg(any(target_arch = "wasm32", test))]
fn apply_save(state: &mut DigState, save: &GameSave) {
    // 現場は day から再生成する。dug/scanned はその現場に対する掘削状況。
    logic::setup_site(state, save.day);

    // 再生成した現場が保存時と一致する時だけ当日進行を復元する。
    // 一致しない (= 生成ロジックか宝の形が変わった) 場合、古い dug を
    // 新配置に重ねると「掘っていない★」や「◆のハズレ化」が起きるため、
    // その日はまっさらな現場からやり直しにする。図鑑・コインは影響なし。
    let same_site = save.site_fingerprint == logic::site_fingerprint(&state.treasures);
    if same_site {
        if save.dug.len() == SITE_LEN {
            for (i, v) in save.dug.iter().enumerate() {
                state.dug[i] = *v;
            }
        }
        if save.scanned.len() == SITE_LEN {
            for (i, v) in save.scanned.iter().enumerate() {
                state.scanned[i] = *v;
            }
        }
        // 正規プレイでは返却が消費の直後に起きるため5本を超えることはない。
        // 改ざんセーブ対策として上限で clamp する。
        state.shovels = save.shovels.min(SHOVELS_PER_DAY);
        state.radar_uses = save.radar_uses.min(RADAR_MAX_PER_DAY);
        state.perfect_bonus_given = save.perfect_bonus_given;
    }

    state.coins = save.coins;
    state.total_coins_earned = save.total_coins_earned.max(save.coins);
    // 図鑑は「新種は末尾追記」の前提でプレフィックスだけコピーする。
    // 厳密長一致にすると、種類を追加した瞬間に旧セーブの図鑑が全消去される。
    let n = save.museum_counts.len().min(KIND_COUNT);
    state.museum_counts[..n].copy_from_slice(&save.museum_counts[..n]);
    let n = save.completed_sets.len().min(COLLECTION_COUNT);
    for (i, v) in save.completed_sets.iter().take(n).enumerate() {
        state.completed_sets[i] = *v;
    }
    // 演出・UI state はロード後にクリーンにする。
    state.radar_armed = false;
    state.flash = None;
    state.museum_scroll.set(0);
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
    use super::super::state::{CollectionSet, SHOVELS_PER_DAY};

    #[test]
    fn save_roundtripは進行状況を保持し現場をdayから再生成する() {
        let mut original = DigState::new();
        logic::setup_site(&mut original, 42);
        logic::dig(&mut original, 0);
        original.scanned[5] = true;
        original.coins = 999;
        original.total_coins_earned = 1500;
        original.radar_uses = 2;
        original.perfect_bonus_given = true;
        original.museum_counts[3] = 7;
        original.completed_sets[CollectionSet::Fuku.index()] = true;

        let saved = extract_save(&original);
        let mut restored = DigState::new();
        apply_save(&mut restored, &saved.game);

        assert_eq!(restored.day, 42);
        assert_eq!(restored.treasures, logic::generate_site(42), "現場は再生成");
        assert_eq!(restored.dug, original.dug);
        assert_eq!(restored.scanned, original.scanned);
        assert_eq!(restored.shovels, original.shovels);
        assert_eq!(restored.radar_uses, 2);
        assert!(restored.perfect_bonus_given);
        assert_eq!(restored.coins, 999);
        assert_eq!(restored.total_coins_earned, 1500);
        assert_eq!(restored.museum_counts[3], 7);
        assert!(restored.completed_sets[CollectionSet::Fuku.index()]);
    }

    #[test]
    fn apply_saveは不正な長さのvecを無視して既定値を保つ() {
        let day = 3;
        let save = GameSave {
            day,
            site_fingerprint: logic::site_fingerprint(&logic::generate_site(day)),
            dug: vec![true; 3],
            scanned: vec![true],
            museum_counts: vec![],
            completed_sets: vec![],
            shovels: SHOVELS_PER_DAY,
            ..Default::default()
        };
        let mut s = DigState::new();
        apply_save(&mut s, &save);
        assert!(s.dug.iter().all(|d| !d));
        assert!(s.scanned.iter().all(|d| !d));
        assert_eq!(s.museum_counts, [0; KIND_COUNT]);
        assert_eq!(s.completed_sets, [false; COLLECTION_COUNT]);
    }

    #[test]
    fn apply_saveはシャベルと羅盤回数を上限でclampする() {
        let save = GameSave {
            day: 0,
            site_fingerprint: logic::site_fingerprint(&logic::generate_site(0)),
            shovels: 200,
            radar_uses: 99,
            ..Default::default()
        };
        let mut s = DigState::new();
        apply_save(&mut s, &save);
        assert_eq!(s.shovels, SHOVELS_PER_DAY);
        assert_eq!(s.radar_uses, RADAR_MAX_PER_DAY);
    }

    #[test]
    fn フィンガープリント不一致なら当日進行を破棄し永続進行は保持する() {
        // 生成ロジック変更後のロードを模す: fp が再生成結果と食い違う。
        let save = GameSave {
            day: 5,
            site_fingerprint: 0xDEAD_BEEF,
            dug: vec![true; SITE_LEN],
            scanned: vec![true; SITE_LEN],
            shovels: 1,
            radar_uses: 3,
            perfect_bonus_given: true,
            coins: 777,
            museum_counts: vec![9; KIND_COUNT],
            ..Default::default()
        };
        let mut s = DigState::new();
        apply_save(&mut s, &save);
        // 当日進行は破棄 → まっさらな現場
        assert!(s.dug.iter().all(|d| !d));
        assert!(s.scanned.iter().all(|d| !d));
        assert_eq!(s.shovels, SHOVELS_PER_DAY);
        assert_eq!(s.radar_uses, 0);
        assert!(!s.perfect_bonus_given);
        // 永続進行は保持
        assert_eq!(s.coins, 777);
        assert_eq!(s.museum_counts, [9; KIND_COUNT]);
    }

    #[test]
    fn 図鑑は種類が増えた将来セーブでもプレフィックスが保持される() {
        // 旧バージョン (種類が少ない) のセーブを模す: 長さ不一致でも
        // 先頭からのプレフィックスは復元されること。
        let save = GameSave {
            museum_counts: vec![5, 3],
            completed_sets: vec![true],
            ..Default::default()
        };
        let mut s = DigState::new();
        apply_save(&mut s, &save);
        assert_eq!(s.museum_counts[0], 5);
        assert_eq!(s.museum_counts[1], 3);
        assert_eq!(s.museum_counts[2], 0);
        assert!(s.completed_sets[0]);
        assert!(!s.completed_sets[1]);
    }

    #[test]
    fn apply_saveはtotal_coins_earnedがcoins未満なら引き上げる() {
        let save = GameSave {
            coins: 500,
            total_coins_earned: 100,
            ..Default::default()
        };
        let mut s = DigState::new();
        apply_save(&mut s, &save);
        assert_eq!(s.total_coins_earned, 500);
    }

    #[test]
    fn apply_saveは演出とui_stateをクリーンにする() {
        let save = GameSave::default();
        let mut s = DigState::new();
        s.radar_armed = true;
        s.flash = Some(super::super::state::Flash { cells: vec![1], ttl: 5 });
        apply_save(&mut s, &save);
        assert!(!s.radar_armed);
        assert!(s.flash.is_none());
        assert!(s.log.is_empty());
    }
}
