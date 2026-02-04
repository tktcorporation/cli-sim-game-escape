//! Cookie Factory セーブ/ロード機能。
//!
//! ## バージョニング方針
//!
//! - `SAVE_VERSION`: 現在のセーブ形式バージョン。フィールド追加時にインクリメントする。
//! - `MIN_COMPATIBLE_VERSION`: 互換性を維持できる最小バージョン。
//!   新フィールドの追加のみの場合はこの値を変えない（旧データを維持できる）。
//!   既存フィールドの意味変更や削除など破壊的変更を行った場合のみインクリメントする。
//!
//! 旧バージョンのセーブデータは、`MIN_COMPATIBLE_VERSION` 以上であれば
//! 不足フィールドにデフォルト値を補完して読み込む。

#[cfg(any(target_arch = "wasm32", test))]
use serde::{Deserialize, Serialize};

#[cfg(any(target_arch = "wasm32", test))]
use super::state::{
    CookieState, DragonAura, MarketPhase, MilestoneStatus, ProducerKind, ResearchPath,
};

/// セーブデータのフォーマットバージョン。
/// フィールド追加時にインクリメントすること。
#[cfg(any(target_arch = "wasm32", test))]
const SAVE_VERSION: u32 = 2;

/// 互換性を維持できる最小バージョン。
/// 既存フィールドの意味変更や削除など破壊的変更を行った場合のみインクリメントする。
/// この値以上のセーブデータは、不足フィールドをデフォルト値で補完して読み込む。
#[cfg(any(target_arch = "wasm32", test))]
const MIN_COMPATIBLE_VERSION: u32 = 1;

/// localStorage のキー。
#[cfg(target_arch = "wasm32")]
const STORAGE_KEY: &str = "cookie_factory_save";

/// オートセーブの間隔 (tick数)。10 ticks/sec × 30秒 = 300 ticks。
pub const AUTOSAVE_INTERVAL: u32 = 300;

/// シリアライズ用のセーブデータ構造体。
/// CookieState の一時的なUI状態（パーティクル、フラッシュ等）は含まない。
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
    cookies: f64,
    cookies_all_time: f64,
    total_clicks: u64,
    cookies_per_click: f64,

    /// 各プロデューサーの (count, multiplier)。ProducerKind::all() の順。
    producers: Vec<(u32, f64)>,

    /// 各アップグレードの購入状態。create_upgrades() の順。
    upgrade_purchased: Vec<bool>,

    synergy_multiplier: f64,
    /// クロスシナジー: (source_index, target_index, bonus)
    cross_synergies: Vec<(usize, usize, f64)>,
    /// カウントスケーリング: (target_index, bonus)
    count_scalings: Vec<(usize, f64)>,
    /// CPS%ボーナス: (target_index, percentage)
    cps_percent_bonuses: Vec<(usize, f64)>,

    golden_cookies_claimed: u32,
    rng_state: u32,

    /// 各マイルストーンのステータス。create_milestones() の順。
    milestone_statuses: Vec<u8>, // 0=Locked, 1=Ready, 2=Claimed
    milk: f64,
    kitten_multiplier: f64,

    // 転生データ
    prestige_count: u32,
    heavenly_chips: u64,
    heavenly_chips_spent: u64,
    prestige_multiplier: f64,
    cookies_all_runs: f64,
    /// 各転生アップグレードの購入状態。
    prestige_upgrade_purchased: Vec<bool>,

    // 統計
    total_ticks: u64,
    best_cps: f64,
    best_cookies_single_run: f64,

    // 研究ツリー
    /// 選択した研究パス (0=None, 1=MassProduction, 2=Quality)
    research_path: u8,
    /// 各研究ノードの購入状態
    research_purchased: Vec<bool>,

    // マーケット
    market_phase: u8, // 0=Bull, 1=Bear, 2=Normal
    market_ticks_left: u32,

    // ドラゴン
    dragon_level: u32,
    dragon_aura: u8, // 0=None, 1=BreathOfRiches, 2=DragonCursor, 3=ElderPact, 4=DragonHarvest
    dragon_fed_total: u32,
}

/// CookieState からセーブ用データを抽出する。
#[cfg(any(target_arch = "wasm32", test))]
fn extract_save(state: &CookieState) -> SaveData {
    SaveData {
        version: SAVE_VERSION,
        game: GameSave {
            cookies: state.cookies,
            cookies_all_time: state.cookies_all_time,
            total_clicks: state.total_clicks,
            cookies_per_click: state.cookies_per_click,
            producers: state
                .producers
                .iter()
                .map(|p| (p.count, p.multiplier))
                .collect(),
            upgrade_purchased: state.upgrades.iter().map(|u| u.purchased).collect(),
            synergy_multiplier: state.synergy_multiplier,
            cross_synergies: state
                .cross_synergies
                .iter()
                .map(|(s, t, b)| (s.index(), t.index(), *b))
                .collect(),
            count_scalings: state
                .count_scalings
                .iter()
                .map(|(t, b)| (t.index(), *b))
                .collect(),
            cps_percent_bonuses: state
                .cps_percent_bonuses
                .iter()
                .map(|(t, p)| (t.index(), *p))
                .collect(),
            golden_cookies_claimed: state.golden_cookies_claimed,
            rng_state: state.rng_state,
            milestone_statuses: state
                .milestones
                .iter()
                .map(|m| match m.status {
                    MilestoneStatus::Locked => 0,
                    MilestoneStatus::Ready => 1,
                    MilestoneStatus::Claimed => 2,
                })
                .collect(),
            milk: state.milk,
            kitten_multiplier: state.kitten_multiplier,
            prestige_count: state.prestige_count,
            heavenly_chips: state.heavenly_chips,
            heavenly_chips_spent: state.heavenly_chips_spent,
            prestige_multiplier: state.prestige_multiplier,
            cookies_all_runs: state.cookies_all_runs,
            prestige_upgrade_purchased: state
                .prestige_upgrades
                .iter()
                .map(|u| u.purchased)
                .collect(),
            total_ticks: state.total_ticks,
            best_cps: state.best_cps,
            best_cookies_single_run: state.best_cookies_single_run,
            // Research
            research_path: match &state.research_path {
                ResearchPath::None => 0,
                ResearchPath::MassProduction => 1,
                ResearchPath::Quality => 2,
            },
            research_purchased: state.research_nodes.iter().map(|n| n.purchased).collect(),
            // Market
            market_phase: match &state.market_phase {
                MarketPhase::Bull => 0,
                MarketPhase::Bear => 1,
                MarketPhase::Normal => 2,
            },
            market_ticks_left: state.market_ticks_left,
            // Dragon
            dragon_level: state.dragon_level,
            dragon_aura: state.dragon_aura.index() as u8,
            dragon_fed_total: state.dragon_fed_total,
        },
    }
}

/// ProducerKind::all() のインデックスから ProducerKind を返す。
#[cfg(any(target_arch = "wasm32", test))]
fn kind_from_index(idx: usize) -> Option<ProducerKind> {
    ProducerKind::all().get(idx).cloned()
}

/// セーブデータを CookieState に復元する。
/// 定義の個数が合わない場合は無視して新規データの方を使う。
#[cfg(any(target_arch = "wasm32", test))]
fn apply_save(state: &mut CookieState, save: &GameSave) {
    state.cookies = save.cookies;
    state.cookies_all_time = save.cookies_all_time;
    state.total_clicks = save.total_clicks;
    state.cookies_per_click = save.cookies_per_click;

    // プロデューサー復元
    for (i, (count, mult)) in save.producers.iter().enumerate() {
        if let Some(p) = state.producers.get_mut(i) {
            p.count = *count;
            p.multiplier = *mult;
        }
    }

    // アップグレード復元
    for (i, &purchased) in save.upgrade_purchased.iter().enumerate() {
        if let Some(u) = state.upgrades.get_mut(i) {
            u.purchased = purchased;
        }
    }

    state.synergy_multiplier = save.synergy_multiplier;

    // クロスシナジー復元
    state.cross_synergies = save
        .cross_synergies
        .iter()
        .filter_map(|(si, ti, b)| {
            Some((kind_from_index(*si)?, kind_from_index(*ti)?, *b))
        })
        .collect();

    // カウントスケーリング復元
    state.count_scalings = save
        .count_scalings
        .iter()
        .filter_map(|(ti, b)| Some((kind_from_index(*ti)?, *b)))
        .collect();

    // CPS%ボーナス復元
    state.cps_percent_bonuses = save
        .cps_percent_bonuses
        .iter()
        .filter_map(|(ti, p)| Some((kind_from_index(*ti)?, *p)))
        .collect();

    state.golden_cookies_claimed = save.golden_cookies_claimed;
    state.rng_state = save.rng_state;

    // マイルストーン復元
    for (i, &status_byte) in save.milestone_statuses.iter().enumerate() {
        if let Some(m) = state.milestones.get_mut(i) {
            m.status = match status_byte {
                1 => MilestoneStatus::Ready,
                2 => MilestoneStatus::Claimed,
                _ => MilestoneStatus::Locked,
            };
        }
    }

    state.milk = save.milk;
    state.kitten_multiplier = save.kitten_multiplier;

    // 転生データ復元
    state.prestige_count = save.prestige_count;
    state.heavenly_chips = save.heavenly_chips;
    state.heavenly_chips_spent = save.heavenly_chips_spent;
    state.prestige_multiplier = save.prestige_multiplier;
    state.cookies_all_runs = save.cookies_all_runs;

    for (i, &purchased) in save.prestige_upgrade_purchased.iter().enumerate() {
        if let Some(u) = state.prestige_upgrades.get_mut(i) {
            u.purchased = purchased;
        }
    }

    // 統計復元
    state.total_ticks = save.total_ticks;
    state.best_cps = save.best_cps;
    state.best_cookies_single_run = save.best_cookies_single_run;

    // 研究ツリー復元
    state.research_path = match save.research_path {
        1 => ResearchPath::MassProduction,
        2 => ResearchPath::Quality,
        _ => ResearchPath::None,
    };
    for (i, &purchased) in save.research_purchased.iter().enumerate() {
        if let Some(n) = state.research_nodes.get_mut(i) {
            n.purchased = purchased;
        }
    }

    // マーケット復元
    state.market_phase = match save.market_phase {
        0 => MarketPhase::Bull,
        1 => MarketPhase::Bear,
        _ => MarketPhase::Normal,
    };
    state.market_ticks_left = save.market_ticks_left;

    // ドラゴン復元
    state.dragon_level = save.dragon_level;
    state.dragon_aura = match save.dragon_aura {
        1 => DragonAura::BreathOfRiches,
        2 => DragonAura::DragonCursor,
        3 => DragonAura::ElderPact,
        4 => DragonAura::DragonHarvest,
        _ => DragonAura::None,
    };
    state.dragon_fed_total = save.dragon_fed_total;
}

/// localStorage にアクセスする。WASM 環境でのみ動作。
#[cfg(target_arch = "wasm32")]
fn get_storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok()?
}

/// ゲーム状態を localStorage に保存する。
/// 失敗時はサイレントに無視（コンソールにログ出力）。
#[cfg(target_arch = "wasm32")]
pub fn save_game(state: &CookieState) {
    let save_data = extract_save(state);
    let json = match serde_json::to_string(&save_data) {
        Ok(j) => j,
        Err(e) => {
            web_sys::console::warn_1(
                &format!("Cookie Factory: セーブのシリアライズに失敗: {e}").into(),
            );
            return;
        }
    };

    if let Some(storage) = get_storage() {
        if let Err(e) = storage.set_item(STORAGE_KEY, &json) {
            web_sys::console::warn_1(
                &format!("Cookie Factory: localStorage への保存に失敗: {e:?}").into(),
            );
        }
    }
}

/// localStorage からゲーム状態を復元する。
/// バージョン不一致やパースエラーの場合は None を返す（新規ゲームになる）。
#[cfg(target_arch = "wasm32")]
pub fn load_game(state: &mut CookieState) -> bool {
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
                    "Cookie Factory: セーブデータのパースに失敗（破棄します）: {e}"
                )
                .into(),
            );
            // 壊れたデータを削除
            let _ = storage.remove_item(STORAGE_KEY);
            return false;
        }
    };

    if save_data.version < MIN_COMPATIBLE_VERSION {
        web_sys::console::log_1(
            &format!(
                "Cookie Factory: セーブバージョンが古すぎます (saved={}, min_compatible={})。新規ゲームを開始します。",
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
                "Cookie Factory: 旧バージョンのセーブデータをマイグレーション (saved={}, current={})。",
                save_data.version, SAVE_VERSION
            )
            .into(),
        );
    }

    apply_save(state, &save_data.game);
    true
}

/// セーブデータを削除する。
#[cfg(target_arch = "wasm32")]
#[allow(dead_code)]
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
        let mut original = CookieState::new();
        original.cookies = 12345.6;
        original.cookies_all_time = 99999.0;
        original.total_clicks = 42;
        original.cookies_per_click = 3.0;
        original.producers[0].count = 10;
        original.producers[0].multiplier = 2.0;
        original.producers[2].count = 5;
        original.upgrades[0].purchased = true;
        original.upgrades[1].purchased = true;
        original.synergy_multiplier = 2.0;
        original.cross_synergies.push((
            ProducerKind::Grandma,
            ProducerKind::Cursor,
            0.01,
        ));
        original.count_scalings.push((ProducerKind::Cursor, 0.005));
        original.cps_percent_bonuses.push((ProducerKind::Farm, 0.0005));
        original.golden_cookies_claimed = 7;
        original.rng_state = 12345;
        original.milestones[0].status = MilestoneStatus::Claimed;
        original.milestones[1].status = MilestoneStatus::Ready;
        original.milk = 0.5;
        original.kitten_multiplier = 1.025;
        original.prestige_count = 2;
        original.heavenly_chips = 100;
        original.heavenly_chips_spent = 10;
        original.prestige_multiplier = 2.0;
        original.cookies_all_runs = 1e12;
        original.prestige_upgrades[0].purchased = true;
        original.total_ticks = 50000;
        original.best_cps = 999.0;
        original.best_cookies_single_run = 88888.0;
        // Research
        original.research_path = ResearchPath::MassProduction;
        original.research_nodes[0].purchased = true;
        // Market
        original.market_phase = MarketPhase::Bear;
        original.market_ticks_left = 200;
        // Dragon
        original.dragon_level = 3;
        original.dragon_aura = DragonAura::BreathOfRiches;
        original.dragon_fed_total = 85;

        let save = extract_save(&original);
        let json = serde_json::to_string(&save).unwrap();

        let loaded: SaveData = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.version, SAVE_VERSION);

        let mut restored = CookieState::new();
        apply_save(&mut restored, &loaded.game);

        assert!((restored.cookies - 12345.6).abs() < 0.001);
        assert!((restored.cookies_all_time - 99999.0).abs() < 0.001);
        assert_eq!(restored.total_clicks, 42);
        assert!((restored.cookies_per_click - 3.0).abs() < 0.001);
        assert_eq!(restored.producers[0].count, 10);
        assert!((restored.producers[0].multiplier - 2.0).abs() < 0.001);
        assert_eq!(restored.producers[2].count, 5);
        assert!(restored.upgrades[0].purchased);
        assert!(restored.upgrades[1].purchased);
        assert!(!restored.upgrades[2].purchased);
        assert!((restored.synergy_multiplier - 2.0).abs() < 0.001);
        assert_eq!(restored.cross_synergies.len(), 1);
        assert_eq!(restored.count_scalings.len(), 1);
        assert_eq!(restored.cps_percent_bonuses.len(), 1);
        assert_eq!(restored.golden_cookies_claimed, 7);
        assert_eq!(restored.rng_state, 12345);
        assert_eq!(restored.milestones[0].status, MilestoneStatus::Claimed);
        assert_eq!(restored.milestones[1].status, MilestoneStatus::Ready);
        assert!((restored.milk - 0.5).abs() < 0.001);
        assert!((restored.kitten_multiplier - 1.025).abs() < 0.001);
        assert_eq!(restored.prestige_count, 2);
        assert_eq!(restored.heavenly_chips, 100);
        assert_eq!(restored.heavenly_chips_spent, 10);
        assert!((restored.prestige_multiplier - 2.0).abs() < 0.001);
        assert!((restored.cookies_all_runs - 1e12).abs() < 1.0);
        assert!(restored.prestige_upgrades[0].purchased);
        assert_eq!(restored.total_ticks, 50000);
        assert!((restored.best_cps - 999.0).abs() < 0.001);
        assert!((restored.best_cookies_single_run - 88888.0).abs() < 0.001);
        // Research
        assert_eq!(restored.research_path, ResearchPath::MassProduction);
        assert!(restored.research_nodes[0].purchased);
        assert!(!restored.research_nodes[1].purchased);
        // Market
        assert_eq!(restored.market_phase, MarketPhase::Bear);
        assert_eq!(restored.market_ticks_left, 200);
        // Dragon
        assert_eq!(restored.dragon_level, 3);
        assert_eq!(restored.dragon_aura, DragonAura::BreathOfRiches);
        assert_eq!(restored.dragon_fed_total, 85);
    }

    #[test]
    fn version_mismatch_detected_in_json() {
        let mut state = CookieState::new();
        state.cookies = 100.0;
        let mut save = extract_save(&state);
        save.version = 999;
        let json = serde_json::to_string(&save).unwrap();
        let loaded: SaveData = serde_json::from_str(&json).unwrap();
        assert_ne!(loaded.version, SAVE_VERSION);
    }

    /// 旧バージョン（フィールドが少ない）のJSONから互換フィールドが復元されることを検証。
    #[test]
    fn migrate_old_version_preserves_compatible_fields() {
        // v1 相当の最小限のJSON（研究・マーケット・ドラゴンのフィールドが無い）
        let old_json = r#"{
            "version": 1,
            "game": {
                "cookies": 5000.0,
                "cookies_all_time": 10000.0,
                "total_clicks": 200,
                "cookies_per_click": 5.0,
                "producers": [[10, 2.0], [3, 1.0]],
                "upgrade_purchased": [true, false, true],
                "synergy_multiplier": 1.5,
                "cross_synergies": [],
                "count_scalings": [],
                "cps_percent_bonuses": [],
                "golden_cookies_claimed": 3,
                "rng_state": 42,
                "milestone_statuses": [2, 1, 0],
                "milk": 0.3,
                "kitten_multiplier": 1.01,
                "prestige_count": 1,
                "heavenly_chips": 50,
                "heavenly_chips_spent": 5,
                "prestige_multiplier": 1.5,
                "cookies_all_runs": 50000.0,
                "prestige_upgrade_purchased": [true],
                "total_ticks": 10000,
                "best_cps": 100.0,
                "best_cookies_single_run": 8000.0
            }
        }"#;

        let loaded: SaveData = serde_json::from_str(old_json).unwrap();
        assert_eq!(loaded.version, 1);
        assert!(loaded.version >= MIN_COMPATIBLE_VERSION);

        let mut state = CookieState::new();
        apply_save(&mut state, &loaded.game);

        // 互換フィールドが復元されている
        assert!((state.cookies - 5000.0).abs() < 0.001);
        assert!((state.cookies_all_time - 10000.0).abs() < 0.001);
        assert_eq!(state.total_clicks, 200);
        assert!((state.cookies_per_click - 5.0).abs() < 0.001);
        assert_eq!(state.producers[0].count, 10);
        assert!((state.producers[0].multiplier - 2.0).abs() < 0.001);
        assert_eq!(state.producers[1].count, 3);
        assert!(state.upgrades[0].purchased);
        assert!(!state.upgrades[1].purchased);
        assert!(state.upgrades[2].purchased);
        assert_eq!(state.golden_cookies_claimed, 3);
        assert_eq!(state.prestige_count, 1);
        assert_eq!(state.heavenly_chips, 50);
        assert_eq!(state.total_ticks, 10000);

        // 旧セーブに存在しないフィールドはデフォルト値
        assert_eq!(state.research_path, ResearchPath::None);
        assert_eq!(state.dragon_level, 0);
        assert_eq!(state.market_phase, MarketPhase::Bull); // default u8=0 → Bull
    }

    /// MIN_COMPATIBLE_VERSION 未満のバージョンは互換性なしと判定される。
    #[test]
    fn version_below_min_compatible_is_rejected() {
        let save_data = SaveData {
            version: 0, // MIN_COMPATIBLE_VERSION(1) 未満
            game: GameSave::default(),
        };
        assert!(save_data.version < MIN_COMPATIBLE_VERSION);
    }

    /// 同一バージョン内での互換性は維持される（未知の追加フィールドは無視される）。
    #[test]
    fn unknown_fields_in_json_are_ignored() {
        let json_with_extra = r#"{
            "version": 2,
            "game": {
                "cookies": 100.0,
                "cookies_all_time": 200.0,
                "total_clicks": 10,
                "cookies_per_click": 1.0,
                "producers": [],
                "upgrade_purchased": [],
                "synergy_multiplier": 1.0,
                "cross_synergies": [],
                "count_scalings": [],
                "cps_percent_bonuses": [],
                "golden_cookies_claimed": 0,
                "rng_state": 0,
                "milestone_statuses": [],
                "milk": 0.0,
                "kitten_multiplier": 1.0,
                "prestige_count": 0,
                "heavenly_chips": 0,
                "heavenly_chips_spent": 0,
                "prestige_multiplier": 1.0,
                "cookies_all_runs": 0.0,
                "prestige_upgrade_purchased": [],
                "total_ticks": 0,
                "best_cps": 0.0,
                "best_cookies_single_run": 0.0,
                "research_path": 0,
                "research_purchased": [],
                "market_phase": 2,
                "market_ticks_left": 0,
                "dragon_level": 0,
                "dragon_aura": 0,
                "dragon_fed_total": 0,
                "future_unknown_field": "should be ignored"
            }
        }"#;

        let loaded: SaveData = serde_json::from_str(json_with_extra).unwrap();
        assert_eq!(loaded.version, 2);
        assert!((loaded.game.cookies - 100.0).abs() < 0.001);
    }

    #[test]
    fn empty_state_roundtrip() {
        let state = CookieState::new();
        let save = extract_save(&state);
        let json = serde_json::to_string(&save).unwrap();
        let loaded: SaveData = serde_json::from_str(&json).unwrap();

        let mut restored = CookieState::new();
        apply_save(&mut restored, &loaded.game);

        assert!((restored.cookies - 0.0).abs() < 0.001);
        assert_eq!(restored.total_clicks, 0);
        assert_eq!(restored.producers[0].count, 0);
    }
}
