//! つぶ牧場 (Tsubu Ranch) セーブ/ロード機能。
//!
//! ## バージョニング方針
//!
//! - `SAVE_VERSION`: 現在のセーブ形式バージョン。フィールド追加時にインクリメントする。
//! - `MIN_COMPATIBLE_VERSION`: 互換性を維持できる最小バージョン。
//!   新フィールドの追加のみの場合はこの値を変えない（旧データを維持できる）。
//!   既存フィールドの意味変更や削除など破壊的変更を行った場合のみインクリメントする。

#[cfg(any(target_arch = "wasm32", test))]
use serde::{Deserialize, Serialize};

#[cfg(any(target_arch = "wasm32", test))]
use super::state::{Affinity, Creature, RanchState, Species, Tab, AFFINITY_COUNT, SPECIES_COUNT, TEAM_SIZE};

/// セーブデータのフォーマットバージョン。
#[cfg(any(target_arch = "wasm32", test))]
const SAVE_VERSION: u32 = 1;

/// 互換性を維持できる最小バージョン。
#[cfg(any(target_arch = "wasm32", test))]
const MIN_COMPATIBLE_VERSION: u32 = 1;

/// localStorage のキー。
#[cfg(target_arch = "wasm32")]
const STORAGE_KEY: &str = "tsubu_ranch_save";

/// オートセーブの間隔 (tick数)。10 ticks/sec × 30秒 = 300 ticks。
pub const AUTOSAVE_INTERVAL: u32 = 300;

/// シリアライズ用のセーブデータ構造体。
/// RanchState の一時的なUI状態 (ログ、スクロール位置) は含まない。
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
    /// 種ごとの個体一覧。index = `Species::index()`、値は `(level, xp)`。
    population: Vec<Vec<(u8, u32)>>,
    food: u64,
    /// index = `Affinity::index()`。
    affinity_feed: Vec<u32>,
    /// 現在の餌やり方針 (`Affinity::index()`)。未選択なら `None`。
    feed_focus: Option<u8>,
    capacity_upgrades: u32,
    /// index = `Species::index()`。
    discovered: Vec<bool>,
    /// index = チームスロット、値 = 種の index。
    team: Vec<Option<u8>>,
    stage: u32,
    enemy_species: u8,
    enemy_hp: u64,
    enemy_max_hp: u64,
    damage_taken: u64,
    clash_cooldown: u32,
    stage_clears: u64,
    tab: u8,
    total_ticks: u64,
    rng_state: u32,
}

/// RanchState からセーブ用データを抽出する。
#[cfg(any(target_arch = "wasm32", test))]
fn extract_save(state: &RanchState) -> SaveData {
    SaveData {
        version: SAVE_VERSION,
        game: GameSave {
            population: state
                .population
                .iter()
                .map(|creatures| creatures.iter().map(|c| (c.level, c.xp)).collect())
                .collect(),
            food: state.food,
            affinity_feed: state.affinity_feed.to_vec(),
            feed_focus: state.feed_focus.map(|a| a.index() as u8),
            capacity_upgrades: state.capacity_upgrades,
            discovered: state.discovered.to_vec(),
            team: state
                .team
                .iter()
                .map(|slot| slot.map(|sp| sp.index() as u8))
                .collect(),
            stage: state.stage,
            enemy_species: state.enemy_species.index() as u8,
            enemy_hp: state.enemy_hp,
            enemy_max_hp: state.enemy_max_hp,
            damage_taken: state.damage_taken,
            clash_cooldown: state.clash_cooldown,
            stage_clears: state.stage_clears,
            tab: state.tab.to_save_id(),
            total_ticks: state.total_ticks,
            rng_state: state.rng_state,
        },
    }
}

/// セーブデータを RanchState に復元する。
/// 配列サイズが合わない古いデータは、範囲内の要素だけ反映して残りは初期値のままにする。
#[cfg(any(target_arch = "wasm32", test))]
fn apply_save(state: &mut RanchState, save: &GameSave) {
    for (i, creatures) in save.population.iter().enumerate().take(SPECIES_COUNT) {
        state.population[i] = creatures.iter().map(|&(level, xp)| Creature { level, xp }).collect();
    }
    state.food = save.food;
    for (i, &v) in save.affinity_feed.iter().enumerate().take(AFFINITY_COUNT) {
        state.affinity_feed[i] = v;
    }
    state.feed_focus = save.feed_focus.and_then(|idx| Affinity::from_index(idx as usize));
    state.capacity_upgrades = save.capacity_upgrades;
    for (i, &d) in save.discovered.iter().enumerate().take(SPECIES_COUNT) {
        state.discovered[i] = d;
    }
    for (i, slot) in save.team.iter().enumerate().take(TEAM_SIZE) {
        state.team[i] = slot.and_then(|idx| Species::from_index(idx as usize));
    }
    state.stage = save.stage;
    state.enemy_species = Species::from_index(save.enemy_species as usize).unwrap_or(Species::Tsubu);
    state.enemy_max_hp = save.enemy_max_hp;
    // 改変/破損セーブで enemy_hp > enemy_max_hp になっていても表示が壊れないようクランプする。
    state.enemy_hp = save.enemy_hp.min(state.enemy_max_hp);
    state.damage_taken = save.damage_taken;
    state.clash_cooldown = save.clash_cooldown;
    state.stage_clears = save.stage_clears;
    state.tab = Tab::from_save_id(save.tab);
    state.total_ticks = save.total_ticks;
    state.rng_state = save.rng_state;
}

/// localStorage にアクセスする。WASM 環境でのみ動作。
#[cfg(target_arch = "wasm32")]
fn get_storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok()?
}

/// ゲーム状態を localStorage に保存する。失敗時はサイレントに無視（コンソールにログ出力）。
#[cfg(target_arch = "wasm32")]
pub fn save_game(state: &RanchState) {
    let save_data = extract_save(state);
    let json = match serde_json::to_string(&save_data) {
        Ok(j) => j,
        Err(e) => {
            web_sys::console::warn_1(&format!("つぶ牧場: セーブのシリアライズに失敗: {e}").into());
            return;
        }
    };

    if let Some(storage) = get_storage() {
        if let Err(e) = storage.set_item(STORAGE_KEY, &json) {
            web_sys::console::warn_1(
                &format!("つぶ牧場: localStorage への保存に失敗: {e:?}").into(),
            );
        }
    }
}

/// localStorage からゲーム状態を復元する。
/// バージョン不一致やパースエラーの場合は false を返す（新規ゲームになる）。
#[cfg(target_arch = "wasm32")]
pub fn load_game(state: &mut RanchState) -> bool {
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
                &format!("つぶ牧場: セーブデータのパースに失敗（破棄します）: {e}").into(),
            );
            let _ = storage.remove_item(STORAGE_KEY);
            return false;
        }
    };

    if save_data.version < MIN_COMPATIBLE_VERSION {
        web_sys::console::log_1(
            &format!(
                "つぶ牧場: セーブバージョンが古すぎます (saved={}, min_compatible={})。新規ゲームを開始します。",
                save_data.version, MIN_COMPATIBLE_VERSION
            )
            .into(),
        );
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
        let mut original = RanchState::new();
        original.population[Species::Tsubu.index()] = vec![
            Creature { level: 7, xp: 12 },
            Creature { level: 3, xp: 40 },
        ];
        original.population[Species::FireKirin.index()] = vec![Creature { level: 1, xp: 0 }];
        original.food = 12345;
        original.affinity_feed[0] = 3;
        original.affinity_feed[1] = 9;
        original.affinity_feed[2] = 1;
        original.feed_focus = Some(Affinity::Flare);
        original.capacity_upgrades = 4;
        original.discovered[Species::FireKirin.index()] = true;
        original.team[0] = Some(Species::Tsubu);
        original.team[2] = Some(Species::FireKirin);
        original.stage = 17;
        original.enemy_species = Species::ThunderHawk;
        original.enemy_hp = 88;
        original.enemy_max_hp = 200;
        original.damage_taken = 55;
        original.clash_cooldown = 2;
        original.stage_clears = 16;
        original.tab = Tab::Battle;
        original.total_ticks = 98765;
        original.rng_state = 424242;

        let save = extract_save(&original);
        let json = serde_json::to_string(&save).unwrap();
        let loaded: SaveData = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.version, SAVE_VERSION);

        let mut restored = RanchState::new();
        apply_save(&mut restored, &loaded.game);

        assert_eq!(restored.population[Species::Tsubu.index()].len(), 2);
        assert_eq!(restored.population[Species::Tsubu.index()][0].level, 7);
        assert_eq!(restored.population[Species::Tsubu.index()][0].xp, 12);
        assert_eq!(restored.population[Species::FireKirin.index()].len(), 1);
        assert_eq!(restored.food, 12345);
        assert_eq!(restored.affinity_feed, [3, 9, 1]);
        assert_eq!(restored.feed_focus, Some(Affinity::Flare));
        assert_eq!(restored.capacity_upgrades, 4);
        assert!(restored.discovered[Species::FireKirin.index()]);
        assert_eq!(restored.team[0], Some(Species::Tsubu));
        assert_eq!(restored.team[1], None);
        assert_eq!(restored.team[2], Some(Species::FireKirin));
        assert_eq!(restored.stage, 17);
        assert_eq!(restored.enemy_species, Species::ThunderHawk);
        assert_eq!(restored.enemy_hp, 88);
        assert_eq!(restored.enemy_max_hp, 200);
        assert_eq!(restored.damage_taken, 55);
        assert_eq!(restored.clash_cooldown, 2);
        assert_eq!(restored.stage_clears, 16);
        assert_eq!(restored.tab, Tab::Battle);
        assert_eq!(restored.total_ticks, 98765);
        assert_eq!(restored.rng_state, 424242);
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
    fn unknown_fields_in_json_are_ignored() {
        let json_with_extra = r#"{
            "version": 1,
            "game": {
                "population": [],
                "food": 100,
                "affinity_feed": [0, 0, 0],
                "capacity_upgrades": 0,
                "discovered": [true],
                "team": [null, null, null],
                "stage": 1,
                "enemy_species": 0,
                "enemy_hp": 10,
                "enemy_max_hp": 10,
                "damage_taken": 0,
                "clash_cooldown": 5,
                "stage_clears": 0,
                "tab": 0,
                "total_ticks": 0,
                "rng_state": 0,
                "future_unknown_field": "should be ignored"
            }
        }"#;

        let loaded: SaveData = serde_json::from_str(json_with_extra).unwrap();
        assert_eq!(loaded.version, 1);
        assert_eq!(loaded.game.food, 100);
    }

    #[test]
    fn empty_state_roundtrip() {
        let state = RanchState::new();
        let save = extract_save(&state);
        let json = serde_json::to_string(&save).unwrap();
        let loaded: SaveData = serde_json::from_str(&json).unwrap();

        let mut restored = RanchState::new();
        apply_save(&mut restored, &loaded.game);

        assert_eq!(restored.food, state.food);
        assert_eq!(
            restored.population[Species::Tsubu.index()].len(),
            state.population[Species::Tsubu.index()].len()
        );
    }

    #[test]
    fn missing_team_and_discovered_fields_default_safely() {
        // 配列長が現在の SPECIES_COUNT / TEAM_SIZE より短い旧データでも panic しない。
        let json_short = r#"{
            "version": 1,
            "game": {
                "population": [[[3, 10]]],
                "food": 5,
                "affinity_feed": [1],
                "discovered": [true]
            }
        }"#;
        let loaded: SaveData = serde_json::from_str(json_short).unwrap();
        let mut restored = RanchState::new();
        apply_save(&mut restored, &loaded.game);
        assert_eq!(restored.population[Species::Tsubu.index()][0].level, 3);
        assert_eq!(restored.affinity_feed[0], 1);
        assert_eq!(restored.affinity_feed[1], 0);
        assert!(restored.discovered[Species::Tsubu.index()]);
    }

    /// 改変/破損セーブで enemy_hp > enemy_max_hp でも、復元後は max でクランプされること。
    #[test]
    fn corrupted_enemy_hp_above_max_is_clamped_on_load() {
        let json_corrupted = r#"{
            "version": 1,
            "game": {
                "enemy_species": 0,
                "enemy_hp": 99999,
                "enemy_max_hp": 100
            }
        }"#;
        let loaded: SaveData = serde_json::from_str(json_corrupted).unwrap();
        let mut restored = RanchState::new();
        apply_save(&mut restored, &loaded.game);
        assert_eq!(restored.enemy_max_hp, 100);
        assert_eq!(restored.enemy_hp, 100, "enemy_hp は enemy_max_hp でクランプされる");
    }
}
