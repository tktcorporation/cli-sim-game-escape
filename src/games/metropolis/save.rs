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
///   v3: `built_at_tick` グリッドを追加 (Phase D 老朽化 / Tier 昇格 dwell time)
///   v4: `cam_x` / `cam_y` 追加 + マップサイズ 32×16 → 64×32 に拡張
///       (Phase 3 ビューポートスクロール)。
///       **破壊的変更**: フラット配列 (tile / terrain / built_at_tick) の
///       インデックスは `y * GRID_W + x` で計算するため、GRID_W が変わると
///       既存配列の再解釈ができない (旧 index 32 = 旧 (0,1) が、新 GRID_W=64
///       では (32,0) に化ける)。v3 以前のセーブは安全に再マップできない
///       ため `MIN_COMPATIBLE_VERSION = 4` でロード拒否し、新規開始を促す。
///   v5: `AiTier::DeepPlanner` (= 数値 5) 追加。
///   v6: `last_save_wall_ms` 追加 (オフライン進行ボーナス用 wall-clock 計測)。
///       旧データは default 0 → 「初回計測」扱いでボーナス未発動になる。
#[cfg(any(target_arch = "wasm32", test))]
const SAVE_VERSION: u32 = 6;

/// 互換性を維持できる最小バージョン。破壊的変更で +1。
/// v1-v3 はフィールド追加だけだったが、v4 でマップ寸法が変わったので
/// セーブの座標系自体が破壊された。Codex review #103 P1 (r3203124082)
/// の指摘で 4 に引き上げ。v ≤ 3 を読み込もうとするとサイレントに座標が
/// 化けるバグがあったため、明示的に拒否する。
#[cfg(any(target_arch = "wasm32", test))]
#[allow(dead_code)] // 非 WASM テストビルドでは load_game が呼ばれないため未使用扱い
const MIN_COMPATIBLE_VERSION: u32 = 4;

/// localStorage のキー。
#[cfg(target_arch = "wasm32")]
const STORAGE_KEY: &str = "metropolis_save";

/// オートセーブ間隔 (tick数)。10 ticks/sec × 30秒 = 300 ticks。
pub const AUTOSAVE_INTERVAL: u32 = 300;

// ── オフライン進行ボーナス ──────────────────────────────────────
//
// プレイヤーが離れている間も街が動いている「ふり」をして、戻ってきた時に
// 推定収入を一括加算する idle 系の定番機構。実シミュレーションは走らせず、
// セーブ時の収入見積もり × 経過秒 × 効率係数で十分な体験が出る。
//
// **設計判断 (簡易見積もり方式)**:
//   - 街並みは離れている間に育たない (= 戻ってきた時の見た目は変わらない)
//   - cash だけが「お留守番中の家賃」として加算される
//   - リアルタイム再生の「街が育つのを眺める」コア体験を温存しつつ、
//     離れがちな idle プレイヤーに小さな達成感を返す

/// オフライン報酬の対象とする経過時間の上限 (秒)。
/// 4 時間 = 14400 秒。1日1〜2回戻ってくるサイクルを想定し、
/// 「離れすぎても全部回収できる」状況を避けてオンライン誘導を残す。
#[cfg(any(target_arch = "wasm32", test))]
pub const MAX_OFFLINE_SECS: u64 = 4 * 60 * 60;

/// ボーナス発動の最低経過時間 (秒)。ページリロードや短時間のタブ切替で
/// 「30秒オフラインでした」のような無意味なメッセージが出るのを防ぐ。
#[cfg(any(target_arch = "wasm32", test))]
pub const OFFLINE_MIN_SECS: u64 = 60;

/// オフライン中の効率係数 (% 単位)。100 = オンライン同等、< 100 でオンライン優遇。
/// 70% は idle 系のいわゆる「セーフバイアス」値。
#[cfg(any(target_arch = "wasm32", test))]
pub const OFFLINE_EFFICIENCY_PCT: u32 = 70;

/// オフラインボーナスの計算結果。`offline_bonus` の戻り値。
#[cfg(any(target_arch = "wasm32", test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OfflineBonus {
    /// 実際の経過秒 (上限カット前)。表示メッセージで使う。
    pub elapsed_secs: u64,
    /// 報酬計算に使った秒数 = `min(elapsed_secs, MAX_OFFLINE_SECS)`。
    pub credited_secs: u64,
    /// 加算するキャッシュ。
    pub bonus_cash: i64,
    /// 上限で打ち切られたか (= elapsed > credited)。メッセージ分岐用。
    pub capped: bool,
}

/// オフライン進行ボーナスを算出する純関数。
///
/// 戻り値が `None` のケース (= ボーナス無し / 表示しない):
/// - `last_save_ms == 0`: 旧セーブまたは初回 (計測開始前)
/// - `now_ms <= last_save_ms`: 時計逆行 (TZ変更等)
/// - `elapsed_secs < OFFLINE_MIN_SECS`: ページリロード等の短時間
/// - `income_per_sec <= 0`: まだ街が収入を出していない
/// - 計算上のボーナスが 0 円
#[cfg(any(target_arch = "wasm32", test))]
pub fn offline_bonus(last_save_ms: u64, now_ms: u64, income_per_sec: i64) -> Option<OfflineBonus> {
    if last_save_ms == 0 || now_ms <= last_save_ms {
        return None;
    }
    let elapsed_secs = (now_ms - last_save_ms) / 1000;
    if elapsed_secs < OFFLINE_MIN_SECS {
        return None;
    }
    if income_per_sec <= 0 {
        return None;
    }
    let credited_secs = elapsed_secs.min(MAX_OFFLINE_SECS);
    let bonus_cash = (credited_secs as i64)
        .saturating_mul(income_per_sec)
        .saturating_mul(i64::from(OFFLINE_EFFICIENCY_PCT))
        / 100;
    if bonus_cash <= 0 {
        return None;
    }
    Some(OfflineBonus {
        elapsed_secs,
        credited_secs,
        bonus_cash,
        capped: elapsed_secs > credited_secs,
    })
}

/// 経過秒を「2時間13分」「45分」のような日本語表現にする。
/// `secs >= OFFLINE_MIN_SECS (60)` を前提に、0分表記は出さない。
///
/// **既知の表記限界**: capped メッセージは elapsed と上限を両方この関数で
/// フォーマットする。elapsed が cap より 0〜59 秒だけ大きい場合 (= 分単位の
/// 切り捨てで両者が同じ "X時間" に化ける) は「オフライン 4時間 (上限4時間まで回収)」
/// のような同表記が並ぶ。実害はほぼ無い (ボーナスは正しく支払われる) ので
/// 表記の修正は行わない。
#[cfg(any(target_arch = "wasm32", test))]
pub fn format_offline_duration(secs: u64) -> String {
    debug_assert!(
        secs >= OFFLINE_MIN_SECS,
        "format_offline_duration is only meaningful for secs >= OFFLINE_MIN_SECS (60); got {}",
        secs
    );
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    if h == 0 {
        format!("{}分", m.max(1))
    } else if m == 0 {
        format!("{}時間", h)
    } else {
        format!("{}時間{}分", h, m)
    }
}

/// イベントログ 1 行を生成する。`load_game` 側でフォーマットを散らかさない。
#[cfg(any(target_arch = "wasm32", test))]
fn make_offline_event_message(bonus: &OfflineBonus) -> String {
    if bonus.capped {
        format!(
            "🌙 オフライン {} (上限{}まで回収) — +${} ({}%効率)",
            format_offline_duration(bonus.elapsed_secs),
            format_offline_duration(MAX_OFFLINE_SECS),
            bonus.bonus_cash,
            OFFLINE_EFFICIENCY_PCT,
        )
    } else {
        format!(
            "🌙 オフライン {} — +${} ({}%効率)",
            format_offline_duration(bonus.elapsed_secs),
            bonus.bonus_cash,
            OFFLINE_EFFICIENCY_PCT,
        )
    }
}

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
    pub const AI_TIER_DEEP_PLANNER: u8 = 5;

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
        AiTier::DeepPlanner => AI_TIER_DEEP_PLANNER,
    }
}

#[cfg(any(target_arch = "wasm32", test))]
fn ai_tier_from_u8(v: u8) -> AiTier {
    match v {
        AI_TIER_GREEDY => AiTier::Greedy,
        AI_TIER_ROAD_PLANNER => AiTier::RoadPlanner,
        AI_TIER_DEMAND_AWARE => AiTier::DemandAware,
        AI_TIER_DEEP_PLANNER => AiTier::DeepPlanner,
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

    /// v3 以降: 各 Built タイルの完成 tick。長さ = GRID_W * GRID_H、行優先。
    /// 旧データには無いので `serde(default)` で空 Vec になる → `apply_save`
    /// 側で「現在の tick で全建物を新築扱い」にマイグレートする。
    built_at_tick: Vec<u64>,

    /// v4 以降: ビューポート左上座標 (Phase 3 マップ拡張)。
    /// 旧データ (32×16 マップ前提) は cam_x=cam_y=0 でロード — マップ中央が
    /// ずれるが、`scroll_camera` で補正可能。
    cam_x: u32,
    cam_y: u32,

    /// v6 以降: 前回セーブ時の wall-clock (Date.now(), ms since epoch)。
    /// 次回ロード時にオフライン経過秒の算出に使う。
    /// 0 は「未計測」(旧データまたは非 WASM 環境) で、`offline_bonus` が None を返す。
    last_save_wall_ms: u64,
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
        ref built_at_tick,
        cam_x,
        cam_y,
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
        // 選択中セル (UI 状態、再ロード後はリセット)。
        selected_cell: _,
        // 右パネル縦スクロール (UI 状態、再ロード後はリセット)。
        panel_scroll: _,
    } = state;

    let mut tiles = Vec::with_capacity(GRID_W * GRID_H);
    let mut terrain_buf = Vec::with_capacity(GRID_W * GRID_H);
    let mut built_at_buf = Vec::with_capacity(GRID_W * GRID_H);
    for y in 0..GRID_H {
        for x in 0..GRID_W {
            tiles.push(tile_to_save(&grid[y][x]));
            terrain_buf.push(terrain_to_u8(terrain[y][x]));
            built_at_buf.push(built_at_tick[y][x]);
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
            built_at_tick: built_at_buf,
            cam_x: *cam_x as u32,
            cam_y: *cam_y as u32,
            // 実際の wall-clock 注入は `save_game` が行う (WASM 専用 IO のため
            // `extract_save` は純粋に保つ)。テスト経由の roundtrip では呼び側で
            // 必要に応じて上書きする。
            last_save_wall_ms: 0,
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
        built_at_tick,
        cam_x,
        cam_y,
        // City state には保持せず、`load_game` 側で `save_data.game.last_save_wall_ms`
        // を直接読み出してオフラインボーナス算出に使う。ここでは束縛だけして
        // 「フィールド追加に気付ける」運用を維持する。
        last_save_wall_ms: _,
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
    // v4: cam_x / cam_y を反映 (旧データは default 0 で復元 → 中央自動補正は
    // しない。プレイヤーが hjkl で動かす)。安全クランプ: マップ範囲内に必ず収める。
    let max_cam_x = super::state::GRID_W.saturating_sub(super::state::VIEW_W);
    let max_cam_y = super::state::GRID_H.saturating_sub(super::state::VIEW_H);
    state.cam_x = (*cam_x as usize).min(max_cam_x);
    state.cam_y = (*cam_y as usize).min(max_cam_y);

    // 築年数: v3 で追加。旧データ (空 Vec) では「現在の tick で全建物を新築扱い」
    // にマイグレートする。これでロード直後に既存の街が突然全部老朽化扱いに
    // ならない (ロード時点を起点に再カウント開始)。
    let now = state.tick;
    let v3_provided = built_at_tick.len() == GRID_W * GRID_H;
    for y in 0..GRID_H {
        for x in 0..GRID_W {
            let idx = y * GRID_W + x;
            let is_built = matches!(state.grid[y][x], Tile::Built(_));
            if !is_built {
                state.built_at_tick[y][x] = 0;
                continue;
            }
            if v3_provided {
                state.built_at_tick[y][x] = built_at_tick[idx];
            } else {
                // v2 以前: 既存建物を「ロード時点で完成」扱いにマイグレート。
                state.built_at_tick[y][x] = now;
            }
        }
    }

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

/// f64 の wall-clock ms を u64 に安全にキャストする。
/// 非 finite (NaN / inf) や負値は「未計測」のセマンティクスに合流させる 0 を返す。
#[cfg(target_arch = "wasm32")]
fn wall_clock_ms_to_u64(v: f64) -> u64 {
    if v.is_finite() && v >= 0.0 {
        v as u64
    } else {
        0
    }
}

/// City 状態を localStorage に保存する。成功時 true、失敗時 false (warn 出力済)。
/// 戻り値は `load_game` 内のオフラインボーナスロールバック判定で使う。
#[cfg(target_arch = "wasm32")]
pub fn save_game(state: &City) -> bool {
    let storage = match get_storage() {
        Some(s) => s,
        None => return false,
    };
    let mut save = extract_save(state);
    // セーブ瞬間の wall-clock を記録。次回ロード時の経過秒算出に使う。
    save.game.last_save_wall_ms = wall_clock_ms_to_u64(js_sys::Date::now());
    let json = match serde_json::to_string(&save) {
        Ok(j) => j,
        Err(e) => {
            web_sys::console::warn_1(
                &format!("Idle Metropolis: セーブのシリアライズに失敗: {e}").into(),
            );
            return false;
        }
    };
    if let Err(e) = storage.set_item(STORAGE_KEY, &json) {
        web_sys::console::warn_1(
            &format!("Idle Metropolis: localStorage への書き込みに失敗: {e:?}").into(),
        );
        return false;
    }
    true
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

    // オフライン進行ボーナス: 前回セーブから現在までの経過時間に応じて
    // 推定収入を一括加算。`compute_income_per_sec` は state ロード後に評価して
    // 「戻ってきた時の街の実力」を反映する。
    let now_ms = wall_clock_ms_to_u64(js_sys::Date::now());
    let income_per_sec = super::logic::compute_income_per_sec(state);
    if let Some(bonus) = offline_bonus(save_data.game.last_save_wall_ms, now_ms, income_per_sec) {
        state.cash = state.cash.saturating_add(bonus.bonus_cash);
        state.cash_earned_total = state.cash_earned_total.saturating_add(bonus.bonus_cash);
        state.push_event(make_offline_event_message(&bonus));
        // ボーナス適用後は即時セーブして wall-clock を更新する。直後にタブを
        // 閉じて autosave (30秒間隔) が走らないと、次回ロードで同じオフライン
        // 期間が再算定され二重支給になるため。
        //
        // 保存に失敗 (localStorage 容量超過 / disable 等) した場合は state 側の
        // 加算をロールバックする — 二重支給を防ぐため次回ロードでもう一度
        // 算定し直す方が安全 (失敗ケースは元々 cash 永続化も壊れているので、
        // ボーナスを引っ込めても体感的な損失は最小)。
        if !save_game(state) {
            state.cash = state.cash.saturating_sub(bonus.bonus_cash);
            state.cash_earned_total = state.cash_earned_total.saturating_sub(bonus.bonus_cash);
            // push_event は先頭挿入なので index 0 を取り除けば直前のメッセージが消える。
            if !state.events.is_empty() {
                state.events.remove(0);
            }
            web_sys::console::warn_1(
                &"Idle Metropolis: オフラインボーナスを保存失敗のためロールバック".into(),
            );
        }
    }
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
        // v3: built_at_tick も保存対象。各セルに異なる築 tick を入れて roundtrip を確認。
        original.built_at_tick[0][0] = 100;
        original.built_at_tick[0][1] = 200;
        original.built_at_tick[0][2] = 300;
        original.built_at_tick[1][0] = 400;
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
        // v3: built_at_tick も完全復元される。
        assert_eq!(restored.built_at_tick[0][0], 100);
        assert_eq!(restored.built_at_tick[0][1], 200);
        assert_eq!(restored.built_at_tick[0][2], 300);
        assert_eq!(restored.built_at_tick[1][0], 400);
    }

    /// `apply_save` の field-level マイグレーション (built_at_tick が空配列の
    /// 場合は「現在の tick で新築」扱い) が機能することを確認する単体テスト。
    ///
    /// **注意**: 現状の `load_game` は `MIN_COMPATIBLE_VERSION = 4` で v ≤ 3
    /// セーブを丸ごと拒否するため、このマイグレーションが production で
    /// 走ることはない。`apply_save` は将来の v5+ で類似の field 追加が
    /// 起きた時にも同じパターンで使える保険として残し、本テストはその
    /// 動作を documentation する。
    #[test]
    fn v2_save_migrates_built_at_to_current_tick() {
        let mut city = City::new();
        let mut save = extract_save(&city);
        // v3 フィールドを空にして v2 相当のデータを作る。
        save.game.built_at_tick.clear();
        // ロード時の tick と、Built タイルを準備。
        save.game.tick = 5000;
        // (0,0) を House にする。
        if let Some(t) = save.game.tiles.first_mut() {
            t.kind = TILE_BUILT;
            t.building = BUILDING_HOUSE;
        }
        apply_save(&mut city, &save.game);
        // 既存 House は now=5000 で built_at_tick が埋まっている (= 新築扱い)。
        assert_eq!(city.built_at_tick[0][0], 5000);
        // Built でないセルは 0 のまま。
        assert_eq!(city.built_at_tick[0][1], 0);
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

    // ── オフライン進行ボーナス ───────────────────────────────

    /// 上限内の経過時間: 全額 70% 効率で支給。
    #[test]
    fn offline_bonus_under_cap() {
        let last = 1_700_000_000_000u64;
        let elapsed_secs = 2 * 3600 + 30 * 60; // 2h30m
        let now = last + elapsed_secs * 1000;
        let bonus = offline_bonus(last, now, 5).expect("bonus expected");
        assert_eq!(bonus.elapsed_secs, elapsed_secs);
        assert_eq!(bonus.credited_secs, elapsed_secs);
        assert!(!bonus.capped);
        // 9000 sec * $5 * 70% = $31500
        assert_eq!(bonus.bonus_cash, 31_500);
    }

    /// 上限超え: credited_secs が MAX_OFFLINE_SECS でクランプされ capped=true。
    #[test]
    fn offline_bonus_capped_at_max() {
        let last = 1_700_000_000_000u64;
        let elapsed_secs = 12 * 3600; // 12h (cap=4h)
        let now = last + elapsed_secs * 1000;
        let bonus = offline_bonus(last, now, 5).expect("bonus expected");
        assert_eq!(bonus.elapsed_secs, elapsed_secs);
        assert_eq!(bonus.credited_secs, MAX_OFFLINE_SECS);
        assert!(bonus.capped);
        // 14400 sec * $5 * 70% = $50400
        assert_eq!(bonus.bonus_cash, 50_400);
    }

    /// 上限ぴったり (elapsed == MAX): credited == elapsed で capped=false。
    /// 境界が `>` か `>=` かを担保する回帰テスト。
    #[test]
    fn offline_bonus_exact_max_is_not_capped() {
        let last = 1_700_000_000_000u64;
        let now = last + MAX_OFFLINE_SECS * 1000;
        let bonus = offline_bonus(last, now, 5).expect("bonus expected");
        assert_eq!(bonus.elapsed_secs, MAX_OFFLINE_SECS);
        assert_eq!(bonus.credited_secs, MAX_OFFLINE_SECS);
        assert!(!bonus.capped);
    }

    /// 上限+1 秒: credited は MAX に丸まり capped=true。
    #[test]
    fn offline_bonus_one_second_over_max_is_capped() {
        let last = 1_700_000_000_000u64;
        let now = last + (MAX_OFFLINE_SECS + 1) * 1000;
        let bonus = offline_bonus(last, now, 5).expect("bonus expected");
        assert_eq!(bonus.elapsed_secs, MAX_OFFLINE_SECS + 1);
        assert_eq!(bonus.credited_secs, MAX_OFFLINE_SECS);
        assert!(bonus.capped);
    }

    /// 巨大な income と elapsed でも `saturating_mul` が i64 範囲を保ち panic しない。
    /// 50年×i64::MAX/2 のような非現実的な値で `saturating_mul` が i64::MAX に
    /// 飽和し、最終的に `/100` した結果も正であることを確認する。
    #[test]
    fn offline_bonus_does_not_overflow_on_extreme_values() {
        let last = 1u64;
        let now = last + 50 * 365 * 24 * 3600 * 1000; // ~50年
        let bonus = offline_bonus(last, now, i64::MAX / 2).expect("bonus expected");
        assert_eq!(bonus.credited_secs, MAX_OFFLINE_SECS);
        assert!(bonus.capped);
        // 14400 * (i64::MAX/2) で saturating → i64::MAX。さらに * 70 saturate → i64::MAX。
        // 最後に /100 → i64::MAX / 100 ≈ 9.22e16。
        assert_eq!(bonus.bonus_cash, i64::MAX / 100);
    }

    /// 短時間 (60秒未満) はリロード扱いでボーナス無し。
    #[test]
    fn offline_bonus_below_threshold_returns_none() {
        let last = 1_700_000_000_000u64;
        // 30秒 — OFFLINE_MIN_SECS=60 の下。
        assert!(offline_bonus(last, last + 30_000, 5).is_none());
        // 59秒も発動しない (境界)。
        assert!(offline_bonus(last, last + 59_000, 5).is_none());
        // 60秒ちょうどで発動する。
        assert!(offline_bonus(last, last + 60_000, 5).is_some());
    }

    /// 時計が逆行している (TZ 変更等) 場合はボーナスなし。
    #[test]
    fn offline_bonus_clock_skew_returns_none() {
        let last = 1_700_000_000_000u64;
        assert!(offline_bonus(last, last - 1, 5).is_none());
        assert!(offline_bonus(last, last, 5).is_none());
    }

    /// 旧データ (last_save_ms=0) ではボーナスなし — 計測開始扱い。
    #[test]
    fn offline_bonus_uninitialized_returns_none() {
        assert!(offline_bonus(0, 1_700_000_000_000u64, 5).is_none());
    }

    /// 街がまだ収入を出していない場合はボーナスなし (= 0円メッセージを抑止)。
    #[test]
    fn offline_bonus_zero_or_negative_income_returns_none() {
        let last = 1_700_000_000_000u64;
        let now = last + 2 * 3600 * 1000;
        assert!(offline_bonus(last, now, 0).is_none());
        assert!(offline_bonus(last, now, -5).is_none());
    }

    /// 最小ケース: 60秒・1ドル/sec の境界で 70% 効率が ($1 * 60 * 70 / 100 = 42) で
    /// 整数除算の loss を含めて期待値どおり返ってくることを確認する。
    /// (income > 0 + elapsed >= MIN_SECS なら必ず正の bonus が出るという不変)。
    #[test]
    fn offline_bonus_smallest_valid_case_returns_42() {
        let last = 1_700_000_000_000u64;
        let now = last + 60_000;
        let bonus = offline_bonus(last, now, 1).expect("bonus expected");
        assert_eq!(bonus.bonus_cash, 42);
    }

    /// 経過時間表示の整形: 分のみ / 時間ちょうど / 時間+分。
    #[test]
    fn format_offline_duration_cases() {
        assert_eq!(format_offline_duration(60), "1分");
        assert_eq!(format_offline_duration(125), "2分");
        assert_eq!(format_offline_duration(3600), "1時間");
        assert_eq!(format_offline_duration(3660), "1時間1分");
        assert_eq!(format_offline_duration(7200), "2時間");
        assert_eq!(format_offline_duration(7320), "2時間2分");
        assert_eq!(format_offline_duration(MAX_OFFLINE_SECS), "4時間");
    }

    /// メッセージは上限到達の有無で文言が分かれる。
    #[test]
    fn offline_event_message_capped_vs_uncapped() {
        let under = OfflineBonus {
            elapsed_secs: 3600,
            credited_secs: 3600,
            bonus_cash: 1234,
            capped: false,
        };
        let msg = make_offline_event_message(&under);
        assert!(msg.contains("オフライン 1時間"));
        assert!(msg.contains("$1234"));
        assert!(!msg.contains("上限"));

        let over = OfflineBonus {
            elapsed_secs: 12 * 3600,
            credited_secs: MAX_OFFLINE_SECS,
            bonus_cash: 5678,
            capped: true,
        };
        let msg = make_offline_event_message(&over);
        assert!(msg.contains("12時間"));
        assert!(msg.contains("上限4時間"));
        assert!(msg.contains("$5678"));
    }

    /// `last_save_wall_ms` がシリアライズを通って読み戻せる。
    #[test]
    fn last_save_wall_ms_roundtrips_through_json() {
        let original = City::new();
        let mut save = extract_save(&original);
        // extract_save は 0 を入れる (wall-clock 注入は save_game の責務)。
        assert_eq!(save.game.last_save_wall_ms, 0);
        save.game.last_save_wall_ms = 1_700_000_000_000;
        let json = serde_json::to_string(&save).unwrap();
        let loaded: SaveData = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.game.last_save_wall_ms, 1_700_000_000_000);
    }
}
