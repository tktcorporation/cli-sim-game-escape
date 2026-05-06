//! Idle Metropolis セーブ/ロード機能。
//!
//! ## バージョニング方針 (cookie/save.rs と同じ)
//!
//! - `SAVE_VERSION`: 現在のセーブ形式バージョン。フィールド追加時に +1。
//! - `MIN_COMPATIBLE_VERSION`: 互換性を維持できる最小バージョン。
//!   破壊的変更時のみ +1。新フィールド追加だけならこの値は変えない
//!   (旧データは欠損フィールドをデフォルト値で補完してロード)。
//!
//! ## metropolis 固有の事情
//!
//! - 32x16 のグリッドと terrain レイヤを持つため、Tile/Building/Terrain を
//!   ディスクリミネータ付きの小さな数値構造に変換して保存する。
//!   ゲーム側の enum に `Serialize` を強制せず、保存フォーマットは
//!   独立に進化させられる。
//! - 完成フラッシュ等の UI 一時状態は保存しない (再ロード時はクリア)。
//! - terrain は `world_seed` から再生成可能だが、整地で永続的に Plain に
//!   書き換わるため、フル保存する (ロード時に整地済みエリアを復元)。

#[cfg(any(target_arch = "wasm32", test))]
use serde::{Deserialize, Serialize};

#[cfg(any(target_arch = "wasm32", test))]
use super::state::{
    AiTier, Building, City, CityTier, GRID_H, GRID_W, MAX_EVENTS, PanelTab, Strategy, Tile,
};

#[cfg(any(target_arch = "wasm32", test))]
use super::terrain::Terrain;

/// セーブデータのフォーマットバージョン。フィールド追加で +1。
///
/// バージョン履歴:
///   v1: 初期 (cookie/save と同じ schema 形式)
///   v2: `last_outpost_dispatch_tick` / `last_auto_demolish_tick` を追加
///       (Phase A 撤去・開拓の自動化クールダウン)
#[cfg(any(target_arch = "wasm32", test))]
const SAVE_VERSION: u32 = 2;

/// 互換性を維持できる最小バージョン。破壊的変更で +1。
/// v1 → v2 はフィールド追加だけなので `serde(default)` で透過的にロード可能。
#[cfg(any(target_arch = "wasm32", test))]
const MIN_COMPATIBLE_VERSION: u32 = 1;

/// localStorage のキー。
#[cfg(target_arch = "wasm32")]
const STORAGE_KEY: &str = "metropolis_save";

/// オートセーブ間隔 (tick数)。10 ticks/sec × 30秒 = 300 ticks。
pub const AUTOSAVE_INTERVAL: u32 = 300;

// ── タイル / 建物 / 地形 のシリアライズ用エンコーディング ────
//
// 数値定数で持つことで、ゲーム enum の variant 追加が直接セーブフォーマットを
// 壊さない (新 variant が出たら新しい数値を割り当てるだけ)。
// `cfg(any(...))` ガードは関数群と揃える — 非 WASM / 非テストビルドでは
// セーブ機能自体が無効化されているため、定数も dead_code になる。

#[cfg(any(target_arch = "wasm32", test))]
mod codes {
    pub const TILE_EMPTY: u8 = 0;
    pub const TILE_CLEARING: u8 = 1;
    pub const TILE_CONSTRUCTION: u8 = 2;
    pub const TILE_BUILT: u8 = 3;

    pub const BUILDING_ROAD: u8 = 0;
    pub const BUILDING_HOUSE: u8 = 1;
    pub const BUILDING_WORKSHOP: u8 = 2;
    pub const BUILDING_SHOP: u8 = 3;
    pub const BUILDING_PARK: u8 = 4;
    pub const BUILDING_OUTPOST: u8 = 5;

    pub const TERRAIN_PLAIN: u8 = 0;
    pub const TERRAIN_FOREST: u8 = 1;
    pub const TERRAIN_WATER: u8 = 2;
    pub const TERRAIN_WASTELAND: u8 = 3;
    pub const TERRAIN_ROCK: u8 = 4;

    pub const STRATEGY_GROWTH: u8 = 0;
    pub const STRATEGY_INCOME: u8 = 1;
    pub const STRATEGY_TECH: u8 = 2;
    pub const STRATEGY_ECO: u8 = 3;

    pub const AI_TIER_RANDOM: u8 = 1;
    pub const AI_TIER_GREEDY: u8 = 2;
    pub const AI_TIER_ROAD_PLANNER: u8 = 3;
    pub const AI_TIER_DEMAND_AWARE: u8 = 4;

    pub const PANEL_STATUS: u8 = 0;
    pub const PANEL_MANAGER: u8 = 1;
    pub const PANEL_EVENTS: u8 = 2;
    pub const PANEL_WORLD: u8 = 3;

    pub const CITY_TIER_VILLAGE: u8 = 0;
    pub const CITY_TIER_TOWN: u8 = 1;
    pub const CITY_TIER_CITY: u8 = 2;
    pub const CITY_TIER_METROPOLIS: u8 = 3;
}

#[cfg(any(target_arch = "wasm32", test))]
use codes::*;

#[cfg(any(target_arch = "wasm32", test))]
fn building_to_u8(b: Building) -> u8 {
    match b {
        Building::Road => BUILDING_ROAD,
        Building::House => BUILDING_HOUSE,
        Building::Workshop => BUILDING_WORKSHOP,
        Building::Shop => BUILDING_SHOP,
        Building::Park => BUILDING_PARK,
        Building::Outpost => BUILDING_OUTPOST,
    }
}

#[cfg(any(target_arch = "wasm32", test))]
fn building_from_u8(v: u8) -> Option<Building> {
    match v {
        BUILDING_ROAD => Some(Building::Road),
        BUILDING_HOUSE => Some(Building::House),
        BUILDING_WORKSHOP => Some(Building::Workshop),
        BUILDING_SHOP => Some(Building::Shop),
        BUILDING_PARK => Some(Building::Park),
        BUILDING_OUTPOST => Some(Building::Outpost),
        _ => None,
    }
}

#[cfg(any(target_arch = "wasm32", test))]
fn terrain_to_u8(t: Terrain) -> u8 {
    match t {
        Terrain::Plain => TERRAIN_PLAIN,
        Terrain::Forest => TERRAIN_FOREST,
        Terrain::Water => TERRAIN_WATER,
        Terrain::Wasteland => TERRAIN_WASTELAND,
        Terrain::Rock => TERRAIN_ROCK,
    }
}

#[cfg(any(target_arch = "wasm32", test))]
fn terrain_from_u8(v: u8) -> Terrain {
    match v {
        TERRAIN_FOREST => Terrain::Forest,
        TERRAIN_WATER => Terrain::Water,
        TERRAIN_WASTELAND => Terrain::Wasteland,
        TERRAIN_ROCK => Terrain::Rock,
        // 不正値は安全な Plain にフォールバック (ロード破損対策)。
        _ => Terrain::Plain,
    }
}

/// シリアライズ用のタイル表現。
///
/// `kind` でディスパッチ。各 variant の付帯情報を `building` と `ticks` に
/// 詰める (省略時 0)。Empty なら全てゼロでフォーマットコストが極小。
#[cfg(any(target_arch = "wasm32", test))]
#[derive(Serialize, Deserialize, Default, Clone, Copy)]
#[serde(default)]
struct TileSave {
    kind: u8,
    building: u8, // Construction.target / Built.building
    ticks: u32,   // Clearing.ticks_remaining / Construction.ticks_remaining
}

#[cfg(any(target_arch = "wasm32", test))]
fn tile_to_save(t: &Tile) -> TileSave {
    match t {
        Tile::Empty => TileSave {
            kind: TILE_EMPTY,
            ..Default::default()
        },
        Tile::Clearing { ticks_remaining } => TileSave {
            kind: TILE_CLEARING,
            building: 0,
            ticks: *ticks_remaining,
        },
        Tile::Construction {
            target,
            ticks_remaining,
        } => TileSave {
            kind: TILE_CONSTRUCTION,
            building: building_to_u8(*target),
            ticks: *ticks_remaining,
        },
        Tile::Built(b) => TileSave {
            kind: TILE_BUILT,
            building: building_to_u8(*b),
            ticks: 0,
        },
    }
}

#[cfg(any(target_arch = "wasm32", test))]
fn tile_from_save(s: TileSave) -> Tile {
    match s.kind {
        TILE_CLEARING => Tile::Clearing {
            ticks_remaining: s.ticks,
        },
        TILE_CONSTRUCTION => match building_from_u8(s.building) {
            Some(b) => Tile::Construction {
                target: b,
                ticks_remaining: s.ticks,
            },
            // 不正値: 安全のため Empty に倒す。整地済みの土地が再露出する
            // 程度の影響なのでセーブを破棄するよりマシ。
            None => Tile::Empty,
        },
        TILE_BUILT => match building_from_u8(s.building) {
            Some(b) => Tile::Built(b),
            None => Tile::Empty,
        },
        _ => Tile::Empty,
    }
}

#[cfg(any(target_arch = "wasm32", test))]
fn strategy_to_u8(s: Strategy) -> u8 {
    match s {
        Strategy::Growth => STRATEGY_GROWTH,
        Strategy::Income => STRATEGY_INCOME,
        Strategy::Tech => STRATEGY_TECH,
        Strategy::Eco => STRATEGY_ECO,
    }
}

#[cfg(any(target_arch = "wasm32", test))]
fn strategy_from_u8(v: u8) -> Strategy {
    match v {
        STRATEGY_INCOME => Strategy::Income,
        STRATEGY_TECH => Strategy::Tech,
        STRATEGY_ECO => Strategy::Eco,
        _ => Strategy::Growth,
    }
}

#[cfg(any(target_arch = "wasm32", test))]
fn ai_tier_to_u8(t: AiTier) -> u8 {
    match t {
        AiTier::Random => AI_TIER_RANDOM,
        AiTier::Greedy => AI_TIER_GREEDY,
        AiTier::RoadPlanner => AI_TIER_ROAD_PLANNER,
        AiTier::DemandAware => AI_TIER_DEMAND_AWARE,
    }
}

#[cfg(any(target_arch = "wasm32", test))]
fn ai_tier_from_u8(v: u8) -> AiTier {
    match v {
        AI_TIER_GREEDY => AiTier::Greedy,
        AI_TIER_ROAD_PLANNER => AiTier::RoadPlanner,
        AI_TIER_DEMAND_AWARE => AiTier::DemandAware,
        _ => AiTier::Random,
    }
}

#[cfg(any(target_arch = "wasm32", test))]
fn panel_to_u8(p: PanelTab) -> u8 {
    match p {
        PanelTab::Status => PANEL_STATUS,
        PanelTab::Manager => PANEL_MANAGER,
        PanelTab::Events => PANEL_EVENTS,
        PanelTab::World => PANEL_WORLD,
    }
}

#[cfg(any(target_arch = "wasm32", test))]
fn panel_from_u8(v: u8) -> PanelTab {
    match v {
        PANEL_STATUS => PanelTab::Status,
        PANEL_EVENTS => PanelTab::Events,
        PANEL_WORLD => PanelTab::World,
        _ => PanelTab::Manager,
    }
}

#[cfg(any(target_arch = "wasm32", test))]
fn city_tier_to_u8(t: CityTier) -> u8 {
    match t {
        CityTier::Village => CITY_TIER_VILLAGE,
        CityTier::Town => CITY_TIER_TOWN,
        CityTier::City => CITY_TIER_CITY,
        CityTier::Metropolis => CITY_TIER_METROPOLIS,
    }
}

#[cfg(any(target_arch = "wasm32", test))]
fn city_tier_from_u8(v: u8) -> CityTier {
    match v {
        CITY_TIER_TOWN => CityTier::Town,
        CITY_TIER_CITY => CityTier::City,
        CITY_TIER_METROPOLIS => CityTier::Metropolis,
        _ => CityTier::Village,
    }
}

/// シリアライズ用のセーブデータ全体。
///
/// `#[serde(default)]` を付けることで、新フィールドが追加された旧データを
/// ロードする時にも欠損フィールドをデフォルト値で補完してくれる。
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
    world_seed: u64,
    cash: i64,
    tick: u64,

    /// グリッド全タイル。長さ = GRID_W * GRID_H、行優先 (y * W + x)。
    tiles: Vec<TileSave>,
    /// 地形レイヤ。長さ = GRID_W * GRID_H。整地で書き換わるためフル保存。
    terrain: Vec<u8>,

    ai_tier: u8,
    strategy: u8,
    panel_tab: u8,
    last_observed_tier: u8,
    workers: u32,
    rng_state: u64,

    buildings_started: u64,
    buildings_finished: u64,
    cash_earned_total: i64,
    cash_spent_total: i64,

    /// イベントログ (新→旧)。`MAX_EVENTS` で切られている。
    events: Vec<String>,

    /// v2 以降: 自動運用クールダウン用 tick。
    /// 旧データには無いので `serde(default)` の 0 を使う (= 「未着手」扱い、
    /// ロード後最初の tick で発火可能になる)。
    last_outpost_dispatch_tick: u64,
    last_auto_demolish_tick: u64,
    /// v2 以降: 累計 Outpost 派遣数 (戦略の挙動を測る統計)。
    /// 旧データは 0 始まり (未計測扱い)。
    outposts_dispatched_total: u64,
}

/// `City` の全フィールドを「永続化対象 / 一時状態 (transient)」の 2 群に
/// 明示的に振り分けて取り出す。**`..` を使わず destructure** することで、
/// `City` にフィールドが追加された時に compile error で気付ける = 管理漏れ
/// 防止の中核。新フィールドを足したら必ずどちらかの群に分類すること。
#[cfg(any(target_arch = "wasm32", test))]
fn extract_save(state: &City) -> SaveData {
    // ── 重要: ここで `..` を使わない ───────────────────────────────
    // `City` に新しいフィールドが追加されると compile error になる。
    // 永続化するなら `GameSave` への書き込みを足す。一時状態 (フラッシュ
    // タイマ等) なら `_` バインドして無視する旨をコメントで残す。
    let City {
        // ── 永続化対象 ─────────────────────────────────────────
        ref grid,
        ref terrain,
        world_seed,
        cash,
        tick,
        ai_tier,
        strategy,
        panel_tab,
        last_observed_tier,
        workers,
        rng_state,
        buildings_started,
        buildings_finished,
        cash_earned_total,
        cash_spent_total,
        ref events,
        last_outpost_dispatch_tick,
        last_auto_demolish_tick,
        outposts_dispatched_total,
        // ── 一時状態 (再ロード後はリセットでよい UI / フラッシュタイマ) ──
        // ティア進化バナーフラッシュ。
        tier_flash_until: _,
        // 完成セルフラッシュ (per-cell)。
        completion_flash_until: _,
        // 給料セルフラッシュ (per-cell)。
        payout_flash_until: _,
        // 直近の収入額 (status の "+$X" 演出用)。
        last_payout_amount: _,
        // 直近の収入が発生した tick。
        last_payout_tick: _,
    } = state;

    let mut tiles = Vec::with_capacity(GRID_W * GRID_H);
    let mut terrain_buf = Vec::with_capacity(GRID_W * GRID_H);
    for y in 0..GRID_H {
        for x in 0..GRID_W {
            tiles.push(tile_to_save(&grid[y][x]));
            terrain_buf.push(terrain_to_u8(terrain[y][x]));
        }
    }

    SaveData {
        version: SAVE_VERSION,
        game: GameSave {
            world_seed: *world_seed,
            cash: *cash,
            tick: *tick,
            tiles,
            terrain: terrain_buf,
            ai_tier: ai_tier_to_u8(*ai_tier),
            strategy: strategy_to_u8(*strategy),
            panel_tab: panel_to_u8(*panel_tab),
            last_observed_tier: city_tier_to_u8(*last_observed_tier),
            workers: *workers,
            rng_state: *rng_state,
            buildings_started: *buildings_started,
            buildings_finished: *buildings_finished,
            cash_earned_total: *cash_earned_total,
            cash_spent_total: *cash_spent_total,
            events: events.clone(),
            last_outpost_dispatch_tick: *last_outpost_dispatch_tick,
            last_auto_demolish_tick: *last_auto_demolish_tick,
            outposts_dispatched_total: *outposts_dispatched_total,
        },
    }
}

/// `GameSave` を `City` に書き戻す。
///
/// `extract_save` と対称に「永続化対象」と「一時状態」を明示し、新フィールド
/// 追加時は両方の関数を更新するルールを徹底する。`GameSave` の destructure
/// も `..` 無しで行うため、`GameSave` にフィールドを足したらここで未バインド
/// になり compile error で気付ける。
#[cfg(any(target_arch = "wasm32", test))]
fn apply_save(state: &mut City, save: &GameSave) {
    let GameSave {
        world_seed,
        cash,
        tick,
        tiles,
        terrain,
        ai_tier,
        strategy,
        panel_tab,
        last_observed_tier,
        workers,
        rng_state,
        buildings_started,
        buildings_finished,
        cash_earned_total,
        cash_spent_total,
        events,
        last_outpost_dispatch_tick,
        last_auto_demolish_tick,
        outposts_dispatched_total,
    } = save;

    state.world_seed = *world_seed;
    state.cash = *cash;
    state.tick = *tick;

    // タイルと地形は (GRID_W * GRID_H) 長を期待する。長さが足りなければ
    // 残りはデフォルト (Empty / Plain) のままにする — 破損データへの安全策。
    let expected = GRID_W * GRID_H;
    for i in 0..expected {
        let y = i / GRID_W;
        let x = i % GRID_W;
        if let Some(t) = tiles.get(i) {
            state.grid[y][x] = tile_from_save(*t);
        }
        if let Some(tr) = terrain.get(i) {
            state.terrain[y][x] = terrain_from_u8(*tr);
        }
    }

    state.ai_tier = ai_tier_from_u8(*ai_tier);
    state.strategy = strategy_from_u8(*strategy);
    state.panel_tab = panel_from_u8(*panel_tab);
    state.last_observed_tier = city_tier_from_u8(*last_observed_tier);
    // workers は 1..=MAX_WORKERS にクランプ (0 や巨大値はゲームを壊す)。
    state.workers = (*workers).clamp(1, super::state::MAX_WORKERS);
    state.rng_state = *rng_state;

    state.buildings_started = *buildings_started;
    state.buildings_finished = *buildings_finished;
    state.cash_earned_total = *cash_earned_total;
    state.cash_spent_total = *cash_spent_total;
    state.last_outpost_dispatch_tick = *last_outpost_dispatch_tick;
    state.last_auto_demolish_tick = *last_auto_demolish_tick;
    state.outposts_dispatched_total = *outposts_dispatched_total;

    // イベントログは長さ上限を切る。
    let mut ev = events.clone();
    if ev.len() > MAX_EVENTS {
        ev.truncate(MAX_EVENTS);
    }
    state.events = ev;

    // 一時状態は再ロード時にリセット (フラッシュタイマ等)。
    state.tier_flash_until = 0;
    state.last_payout_amount = 0;
    state.last_payout_tick = 0;
    for row in state.completion_flash_until.iter_mut() {
        for v in row.iter_mut() {
            *v = 0;
        }
    }
    for row in state.payout_flash_until.iter_mut() {
        for v in row.iter_mut() {
            *v = 0;
        }
    }
}

/// localStorage を取得する。WASM 環境のみ。
#[cfg(target_arch = "wasm32")]
fn get_storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok()?
}

/// City 状態を localStorage に保存する。失敗時はサイレントに無視 (console 出力)。
#[cfg(target_arch = "wasm32")]
pub fn save_game(state: &City) {
    let storage = match get_storage() {
        Some(s) => s,
        None => return,
    };
    let save = extract_save(state);
    let json = match serde_json::to_string(&save) {
        Ok(j) => j,
        Err(e) => {
            web_sys::console::warn_1(
                &format!("Idle Metropolis: セーブのシリアライズに失敗: {e}").into(),
            );
            return;
        }
    };
    if let Err(e) = storage.set_item(STORAGE_KEY, &json) {
        web_sys::console::warn_1(
            &format!("Idle Metropolis: localStorage への書き込みに失敗: {e:?}").into(),
        );
    }
}

/// localStorage からロードして state に反映。成功時 true。
/// データ破損や非互換バージョンの時はキーごと削除して false を返す。
#[cfg(target_arch = "wasm32")]
pub fn load_game(state: &mut City) -> bool {
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
                &format!(
                    "Idle Metropolis: セーブのパースに失敗 (破棄します): {e}"
                )
                .into(),
            );
            let _ = storage.remove_item(STORAGE_KEY);
            return false;
        }
    };
    if save_data.version < MIN_COMPATIBLE_VERSION {
        web_sys::console::log_1(
            &format!(
                "Idle Metropolis: セーブが古すぎます (saved={}, min={})。新規開始。",
                save_data.version, MIN_COMPATIBLE_VERSION
            )
            .into(),
        );
        let _ = storage.remove_item(STORAGE_KEY);
        return false;
    }
    if save_data.version < SAVE_VERSION {
        web_sys::console::log_1(
            &format!(
                "Idle Metropolis: 旧バージョンをマイグレーション (saved={}, current={})",
                save_data.version, SAVE_VERSION
            )
            .into(),
        );
    }
    apply_save(state, &save_data.game);
    true
}

/// セーブデータを削除する。設定画面の「データをリセット」から呼ばれる。
#[cfg(target_arch = "wasm32")]
pub fn delete_save() {
    if let Some(storage) = get_storage() {
        let _ = storage.remove_item(STORAGE_KEY);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 状態を一通りいじって、save → load で完全復元できることを確認する。
    #[test]
    fn extract_apply_roundtrip() {
        let mut original = City::with_seed(0xDEADBEEF);
        original.cash = 12345;
        original.tick = 1000;
        original.ai_tier = AiTier::DemandAware;
        original.strategy = Strategy::Eco;
        original.panel_tab = PanelTab::Events;
        original.last_observed_tier = CityTier::City;
        original.workers = 4;
        original.rng_state = 0xABCD_1234;
        original.buildings_started = 50;
        original.buildings_finished = 47;
        original.cash_earned_total = 99999;
        original.cash_spent_total = 87654;
        // 自動運用クールダウン (v2) と Outpost 派遣カウンタも復元される。
        original.last_outpost_dispatch_tick = 800;
        original.last_auto_demolish_tick = 600;
        original.outposts_dispatched_total = 7;
        original.set_tile(0, 0, Tile::Built(Building::House));
        original.set_tile(1, 0, Tile::Built(Building::Workshop));
        original.set_tile(2, 0, Tile::Built(Building::Shop));
        original.set_tile(0, 1, Tile::Built(Building::Road));
        original.grid[1][1] = Tile::Construction {
            target: Building::House,
            ticks_remaining: 42,
        };
        original.grid[1][2] = Tile::Clearing {
            ticks_remaining: 30,
        };
        // 整地で書き換わった想定の Plain (元 Forest だったセル)。
        original.terrain[5][5] = Terrain::Plain;
        original.events = vec!["a".into(), "b".into(), "c".into()];

        let save = extract_save(&original);
        let json = serde_json::to_string(&save).unwrap();
        let loaded: SaveData = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.version, SAVE_VERSION);

        let mut restored = City::new();
        apply_save(&mut restored, &loaded.game);

        assert_eq!(restored.cash, 12345);
        assert_eq!(restored.tick, 1000);
        assert_eq!(restored.ai_tier, AiTier::DemandAware);
        assert_eq!(restored.strategy, Strategy::Eco);
        assert_eq!(restored.panel_tab, PanelTab::Events);
        assert_eq!(restored.last_observed_tier, CityTier::City);
        assert_eq!(restored.workers, 4);
        assert_eq!(restored.rng_state, 0xABCD_1234);
        assert_eq!(restored.buildings_started, 50);
        assert_eq!(restored.buildings_finished, 47);
        assert_eq!(restored.cash_earned_total, 99999);
        assert_eq!(restored.cash_spent_total, 87654);
        assert_eq!(restored.world_seed, 0xDEADBEEF);
        assert_eq!(restored.last_outpost_dispatch_tick, 800);
        assert_eq!(restored.last_auto_demolish_tick, 600);
        assert_eq!(restored.outposts_dispatched_total, 7);

        assert!(matches!(restored.tile(0, 0), Tile::Built(Building::House)));
        assert!(matches!(
            restored.tile(1, 0),
            Tile::Built(Building::Workshop)
        ));
        assert!(matches!(restored.tile(2, 0), Tile::Built(Building::Shop)));
        assert!(matches!(restored.tile(0, 1), Tile::Built(Building::Road)));
        match restored.tile(1, 1) {
            Tile::Construction {
                target,
                ticks_remaining,
            } => {
                assert_eq!(*target, Building::House);
                assert_eq!(*ticks_remaining, 42);
            }
            _ => panic!("expected Construction at (1,1)"),
        }
        match restored.tile(2, 1) {
            Tile::Clearing { ticks_remaining } => {
                assert_eq!(*ticks_remaining, 30);
            }
            _ => panic!("expected Clearing at (2,1)"),
        }
        assert_eq!(restored.terrain[5][5], Terrain::Plain);
        assert_eq!(restored.events, vec!["a", "b", "c"]);
    }

    /// 不正な workers 値はクランプされる。
    #[test]
    fn workers_clamped_on_load() {
        let mut city = City::new();
        let mut save = extract_save(&city);
        save.game.workers = 99; // MAX_WORKERS = 8 を超える
        apply_save(&mut city, &save.game);
        assert!(city.workers <= super::super::state::MAX_WORKERS);
        assert!(city.workers >= 1);

        save.game.workers = 0; // 0 ワーカーはゲームを止める
        apply_save(&mut city, &save.game);
        assert_eq!(city.workers, 1);
    }

    /// 不正な enum 数値は安全なフォールバックで読み込まれる (壊さない)。
    #[test]
    fn unknown_enum_values_fallback_safely() {
        let mut city = City::new();
        let mut save = extract_save(&city);
        save.game.strategy = 99;
        save.game.ai_tier = 99;
        save.game.panel_tab = 99;
        save.game.last_observed_tier = 99;
        // タイル内の building 数値も汚染。
        if let Some(t) = save.game.tiles.first_mut() {
            t.kind = TILE_BUILT;
            t.building = 99;
        }
        if let Some(tr) = save.game.terrain.first_mut() {
            *tr = 99;
        }
        apply_save(&mut city, &save.game);
        // フォールバックは Growth / Random / Manager / Village / Empty / Plain。
        assert_eq!(city.strategy, Strategy::Growth);
        assert_eq!(city.ai_tier, AiTier::Random);
        assert_eq!(city.panel_tab, PanelTab::Manager);
        assert_eq!(city.last_observed_tier, CityTier::Village);
        assert!(matches!(city.tile(0, 0), Tile::Empty));
        assert_eq!(city.terrain[0][0], Terrain::Plain);
    }

    /// 旧バージョンのセーブ (フィールド欠損) は default 値で補完される。
    /// `serde(default)` を信頼する回帰テスト。
    #[test]
    fn missing_fields_default_to_zero() {
        // `{ "version": 1, "game": {} }` を書く。
        let json = r#"{"version":1,"game":{}}"#;
        let save: SaveData = serde_json::from_str(json).unwrap();
        let mut city = City::new();
        apply_save(&mut city, &save.game);
        // 全フィールドが default になり、ゲームは新規状態とほぼ同じ。
        assert_eq!(city.cash, 0); // GameSave::default
        assert_eq!(city.tick, 0);
        assert_eq!(city.workers, 1); // クランプ後
    }
}
