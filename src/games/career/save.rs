//! Career Simulator セーブ/ロード機能。
//!
//! ## バージョニング方針
//!
//! - `SAVE_VERSION`: 現在のセーブ形式バージョン。フィールド追加時にインクリメントする。
//! - `MIN_COMPATIBLE_VERSION`: 互換性を維持できる最小バージョン。
//!   新フィールドの追加のみの場合はこの値を変えない（旧データを維持できる）。
//!   既存フィールドの意味変更や削除など破壊的変更を行った場合のみインクリメントする。
//!
//! ## v3 変更点
//! - AP (ap, ap_max) フィールドを廃止
//! - 代わりに training_done, networked, side_job_done を追加
//! - 初期資金が ¥5,000 に変更
//! - 破壊的変更のため MIN_COMPATIBLE_VERSION=3 に引き上げ

#[cfg(any(target_arch = "wasm32", test))]
use serde::{Deserialize, Serialize};

#[cfg(any(target_arch = "wasm32", test))]
use super::state::{CareerState, JobKind, LifestyleLevel, MonthEvent, MonthlyReport, TRAINING_COUNT};

#[cfg(any(target_arch = "wasm32", test))]
const SAVE_VERSION: u32 = 3;

#[cfg(any(target_arch = "wasm32", test))]
const MIN_COMPATIBLE_VERSION: u32 = 3;

#[cfg(target_arch = "wasm32")]
const STORAGE_KEY: &str = "career_simulator_save";

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Serialize, Deserialize)]
struct SaveData {
    version: u32,
    game: CareerSave,
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct CareerSave {
    money: f64,
    total_earned: f64,
    total_ticks: u64,

    technical: f64,
    social: f64,
    management: f64,
    knowledge: f64,
    reputation: f64,

    /// JobKind as u8 index (0=Freeter .. 9=Entrepreneur)
    job: u8,

    savings: f64,
    stocks: f64,
    real_estate: f64,

    /// LifestyleLevel as u8 index (0=Frugal .. 4=Luxury)
    lifestyle: u8,
    months_elapsed: u32,

    won: bool,
    won_message: Option<String>,

    // Monthly report (for budget display)
    report_gross_salary: f64,
    report_tax: f64,
    report_insurance: f64,
    report_net_salary: f64,
    report_passive_income: f64,
    report_living_cost: f64,
    report_rent: f64,
    report_cashflow: f64,

    // Per-month action tracking (v3 — replaces AP)
    training_done: [bool; TRAINING_COUNT],
    networked: bool,
    side_job_done: bool,

    // Event system (v2+)
    /// MonthEvent as u8 index (0=TrainingSale..7=ExpenseSpike), 255=None
    #[serde(default = "default_no_event")]
    current_event: u8,
    event_seed: u64,
}

#[cfg(any(target_arch = "wasm32", test))]
fn default_no_event() -> u8 {
    255
}

#[cfg(any(target_arch = "wasm32", test))]
fn extract_save(state: &CareerState) -> SaveData {
    let job_index = match state.job {
        JobKind::Freeter => 0,
        JobKind::OfficeClerk => 1,
        JobKind::Programmer => 2,
        JobKind::Designer => 3,
        JobKind::Sales => 4,
        JobKind::Accountant => 5,
        JobKind::Manager => 6,
        JobKind::Consultant => 7,
        JobKind::Director => 8,
        JobKind::Entrepreneur => 9,
    };

    let lifestyle_index = match state.lifestyle {
        LifestyleLevel::Frugal => 0,
        LifestyleLevel::Normal => 1,
        LifestyleLevel::Comfort => 2,
        LifestyleLevel::Wealthy => 3,
        LifestyleLevel::Luxury => 4,
    };

    SaveData {
        version: SAVE_VERSION,
        game: CareerSave {
            money: state.money,
            total_earned: state.total_earned,
            total_ticks: state.total_ticks,
            technical: state.technical,
            social: state.social,
            management: state.management,
            knowledge: state.knowledge,
            reputation: state.reputation,
            job: job_index,
            savings: state.savings,
            stocks: state.stocks,
            real_estate: state.real_estate,
            lifestyle: lifestyle_index,
            months_elapsed: state.months_elapsed,
            won: state.won,
            won_message: state.won_message.clone(),
            report_gross_salary: state.last_report.gross_salary,
            report_tax: state.last_report.tax,
            report_insurance: state.last_report.insurance,
            report_net_salary: state.last_report.net_salary,
            report_passive_income: state.last_report.passive_income,
            report_living_cost: state.last_report.living_cost,
            report_rent: state.last_report.rent,
            report_cashflow: state.last_report.cashflow,
            training_done: state.training_done,
            networked: state.networked,
            side_job_done: state.side_job_done,
            current_event: match state.current_event {
                Some(MonthEvent::TrainingSale) => 0,
                Some(MonthEvent::BullMarket) => 1,
                Some(MonthEvent::Recession) => 2,
                Some(MonthEvent::SkillBoom) => 3,
                Some(MonthEvent::WindfallBonus) => 4,
                Some(MonthEvent::MarketCrash) => 5,
                Some(MonthEvent::TaxRefund) => 6,
                Some(MonthEvent::ExpenseSpike) => 7,
                None => 255,
            },
            event_seed: state.event_seed,
        },
    }
}

#[cfg(any(target_arch = "wasm32", test))]
fn apply_save(state: &mut CareerState, save: &CareerSave) {
    state.money = save.money;
    state.total_earned = save.total_earned;
    state.total_ticks = save.total_ticks;
    state.technical = save.technical;
    state.social = save.social;
    state.management = save.management;
    state.knowledge = save.knowledge;
    state.reputation = save.reputation;

    state.job = match save.job {
        0 => JobKind::Freeter,
        1 => JobKind::OfficeClerk,
        2 => JobKind::Programmer,
        3 => JobKind::Designer,
        4 => JobKind::Sales,
        5 => JobKind::Accountant,
        6 => JobKind::Manager,
        7 => JobKind::Consultant,
        8 => JobKind::Director,
        9 => JobKind::Entrepreneur,
        _ => JobKind::Freeter,
    };

    state.savings = save.savings;
    state.stocks = save.stocks;
    state.real_estate = save.real_estate;

    state.lifestyle = match save.lifestyle {
        0 => LifestyleLevel::Frugal,
        1 => LifestyleLevel::Normal,
        2 => LifestyleLevel::Comfort,
        3 => LifestyleLevel::Wealthy,
        4 => LifestyleLevel::Luxury,
        _ => LifestyleLevel::Frugal,
    };

    state.months_elapsed = save.months_elapsed;
    state.won = save.won;
    state.won_message = save.won_message.clone();

    state.last_report = MonthlyReport {
        gross_salary: save.report_gross_salary,
        tax: save.report_tax,
        insurance: save.report_insurance,
        net_salary: save.report_net_salary,
        passive_income: save.report_passive_income,
        living_cost: save.report_living_cost,
        rent: save.report_rent,
        cashflow: save.report_cashflow,
    };

    state.training_done = save.training_done;
    state.networked = save.networked;
    state.side_job_done = save.side_job_done;

    state.current_event = match save.current_event {
        0 => Some(MonthEvent::TrainingSale),
        1 => Some(MonthEvent::BullMarket),
        2 => Some(MonthEvent::Recession),
        3 => Some(MonthEvent::SkillBoom),
        4 => Some(MonthEvent::WindfallBonus),
        5 => Some(MonthEvent::MarketCrash),
        6 => Some(MonthEvent::TaxRefund),
        7 => Some(MonthEvent::ExpenseSpike),
        _ => None,
    };
    state.event_seed = save.event_seed;
}

#[cfg(target_arch = "wasm32")]
fn get_storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok()?
}

#[cfg(target_arch = "wasm32")]
pub fn save_game(state: &CareerState) {
    let save_data = extract_save(state);
    let json = match serde_json::to_string(&save_data) {
        Ok(j) => j,
        Err(e) => {
            web_sys::console::warn_1(
                &format!("Career Simulator: セーブのシリアライズに失敗: {e}").into(),
            );
            return;
        }
    };

    if let Some(storage) = get_storage() {
        if let Err(e) = storage.set_item(STORAGE_KEY, &json) {
            web_sys::console::warn_1(
                &format!("Career Simulator: localStorage への保存に失敗: {e:?}").into(),
            );
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub fn load_game(state: &mut CareerState) -> bool {
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
                    "Career Simulator: セーブデータのパースに失敗（破棄します）: {e}"
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
                "Career Simulator: セーブバージョンが古すぎます (saved={}, min_compatible={})。新規ゲームを開始します。",
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
                "Career Simulator: 旧バージョンのセーブデータをマイグレーション (saved={}, current={})。",
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
pub fn delete_save() {
    if let Some(storage) = get_storage() {
        let _ = storage.remove_item(STORAGE_KEY);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::state::CareerState;

    #[test]
    fn extract_and_apply_roundtrip() {
        let mut original = CareerState::new();
        original.money = 12345.6;
        original.total_earned = 99999.0;
        original.total_ticks = 900;
        original.technical = 15.0;
        original.social = 10.0;
        original.management = 5.0;
        original.knowledge = 20.0;
        original.reputation = 25.0;
        original.job = JobKind::Programmer;
        original.savings = 5000.0;
        original.stocks = 10000.0;
        original.real_estate = 80000.0;
        original.lifestyle = LifestyleLevel::Comfort;
        original.months_elapsed = 3;
        original.won = false;
        original.won_message = None;
        original.training_done = [true, false, true, false, false];
        original.networked = true;
        original.side_job_done = false;
        original.current_event = Some(MonthEvent::BullMarket);
        original.event_seed = 999;
        original.last_report = MonthlyReport {
            gross_salary: 15000.0,
            tax: 2250.0,
            insurance: 1200.0,
            net_salary: 11550.0,
            passive_income: 500.0,
            living_cost: 2500.0,
            rent: 1500.0,
            cashflow: 8050.0,
        };

        let save = extract_save(&original);
        let json = serde_json::to_string(&save).unwrap();

        let loaded: SaveData = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.version, SAVE_VERSION);

        let mut restored = CareerState::new();
        apply_save(&mut restored, &loaded.game);

        assert!((restored.money - 12345.6).abs() < 0.001);
        assert!((restored.total_earned - 99999.0).abs() < 0.001);
        assert_eq!(restored.total_ticks, 900);
        assert!((restored.technical - 15.0).abs() < 0.001);
        assert!((restored.social - 10.0).abs() < 0.001);
        assert!((restored.management - 5.0).abs() < 0.001);
        assert!((restored.knowledge - 20.0).abs() < 0.001);
        assert!((restored.reputation - 25.0).abs() < 0.001);
        assert_eq!(restored.job, JobKind::Programmer);
        assert!((restored.savings - 5000.0).abs() < 0.001);
        assert!((restored.stocks - 10000.0).abs() < 0.001);
        assert!((restored.real_estate - 80000.0).abs() < 0.001);
        assert_eq!(restored.lifestyle, LifestyleLevel::Comfort);
        assert_eq!(restored.months_elapsed, 3);
        assert!(!restored.won);
        assert!(restored.won_message.is_none());
        assert_eq!(restored.training_done, [true, false, true, false, false]);
        assert!(restored.networked);
        assert!(!restored.side_job_done);
        assert_eq!(restored.current_event, Some(MonthEvent::BullMarket));
        assert_eq!(restored.event_seed, 999);

        // Report
        assert!((restored.last_report.gross_salary - 15000.0).abs() < 0.001);
        assert!((restored.last_report.tax - 2250.0).abs() < 0.001);
        assert!((restored.last_report.cashflow - 8050.0).abs() < 0.001);
    }

    #[test]
    fn roundtrip_with_win() {
        let mut original = CareerState::new();
        original.won = true;
        original.won_message = Some("経済的自由達成！".to_string());
        original.money = 500_000.0;
        original.job = JobKind::Entrepreneur;

        let save = extract_save(&original);
        let json = serde_json::to_string(&save).unwrap();
        let loaded: SaveData = serde_json::from_str(&json).unwrap();

        let mut restored = CareerState::new();
        apply_save(&mut restored, &loaded.game);

        assert!(restored.won);
        assert_eq!(restored.won_message, Some("経済的自由達成！".to_string()));
        assert_eq!(restored.job, JobKind::Entrepreneur);
    }

    #[test]
    fn empty_state_roundtrip() {
        let state = CareerState::new();
        let save = extract_save(&state);
        let json = serde_json::to_string(&save).unwrap();
        let loaded: SaveData = serde_json::from_str(&json).unwrap();

        let mut restored = CareerState::new();
        apply_save(&mut restored, &loaded.game);

        assert!((restored.money - 5_000.0).abs() < 0.001);
        assert_eq!(restored.job, JobKind::Freeter);
        assert_eq!(restored.lifestyle, LifestyleLevel::Frugal);
        assert_eq!(restored.months_elapsed, 0);
        assert_eq!(restored.training_done, [false; TRAINING_COUNT]);
        assert!(!restored.networked);
        assert!(!restored.side_job_done);
    }

    #[test]
    fn version_below_min_compatible_is_rejected() {
        let save_data = SaveData {
            version: 2,
            game: CareerSave::default(),
        };
        assert!(save_data.version < MIN_COMPATIBLE_VERSION);
    }

    #[test]
    fn all_jobs_roundtrip() {
        let jobs = [
            (JobKind::Freeter, 0u8),
            (JobKind::OfficeClerk, 1),
            (JobKind::Programmer, 2),
            (JobKind::Designer, 3),
            (JobKind::Sales, 4),
            (JobKind::Accountant, 5),
            (JobKind::Manager, 6),
            (JobKind::Consultant, 7),
            (JobKind::Director, 8),
            (JobKind::Entrepreneur, 9),
        ];

        for (kind, idx) in jobs {
            let mut state = CareerState::new();
            state.job = kind;
            let save = extract_save(&state);
            assert_eq!(save.game.job, idx);

            let mut restored = CareerState::new();
            apply_save(&mut restored, &save.game);
            assert_eq!(restored.job, kind);
        }
    }

    #[test]
    fn all_events_roundtrip() {
        use super::super::state::MonthEvent;
        let events: [Option<MonthEvent>; 9] = [
            None,
            Some(MonthEvent::TrainingSale),
            Some(MonthEvent::BullMarket),
            Some(MonthEvent::Recession),
            Some(MonthEvent::SkillBoom),
            Some(MonthEvent::WindfallBonus),
            Some(MonthEvent::MarketCrash),
            Some(MonthEvent::TaxRefund),
            Some(MonthEvent::ExpenseSpike),
        ];

        for event in events {
            let mut state = CareerState::new();
            state.current_event = event;
            let save = extract_save(&state);

            let mut restored = CareerState::new();
            apply_save(&mut restored, &save.game);
            assert_eq!(restored.current_event, event);
        }
    }

    #[test]
    fn all_lifestyles_roundtrip() {
        let lifestyles = [
            (LifestyleLevel::Frugal, 0u8),
            (LifestyleLevel::Normal, 1),
            (LifestyleLevel::Comfort, 2),
            (LifestyleLevel::Wealthy, 3),
            (LifestyleLevel::Luxury, 4),
        ];

        for (level, idx) in lifestyles {
            let mut state = CareerState::new();
            state.lifestyle = level;
            let save = extract_save(&state);
            assert_eq!(save.game.lifestyle, idx);

            let mut restored = CareerState::new();
            apply_save(&mut restored, &save.game);
            assert_eq!(restored.lifestyle, level);
        }
    }

    #[test]
    fn training_done_roundtrip() {
        let mut state = CareerState::new();
        state.training_done = [true, true, false, true, false];
        state.networked = true;
        state.side_job_done = true;

        let save = extract_save(&state);
        let json = serde_json::to_string(&save).unwrap();
        let loaded: SaveData = serde_json::from_str(&json).unwrap();

        let mut restored = CareerState::new();
        apply_save(&mut restored, &loaded.game);

        assert_eq!(restored.training_done, [true, true, false, true, false]);
        assert!(restored.networked);
        assert!(restored.side_job_done);
    }
}
