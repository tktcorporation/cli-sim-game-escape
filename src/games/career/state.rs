//! Career Simulator game state.

/// Available job types, ordered by progression tier.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum JobKind {
    Freeter,
    OfficeClerk,
    Programmer,
    Designer,
    Sales,
    Accountant,
    Manager,
    Consultant,
    Director,
    Entrepreneur,
}

/// All jobs in display order.
pub const ALL_JOBS: [JobKind; 10] = [
    JobKind::Freeter,
    JobKind::OfficeClerk,
    JobKind::Programmer,
    JobKind::Designer,
    JobKind::Sales,
    JobKind::Accountant,
    JobKind::Manager,
    JobKind::Consultant,
    JobKind::Director,
    JobKind::Entrepreneur,
];

/// Static info about a job: requirements, salary, and passive skill gains.
pub struct JobInfo {
    pub name: &'static str,
    pub salary: f64,
    pub req_technical: f64,
    pub req_social: f64,
    pub req_management: f64,
    pub req_knowledge: f64,
    pub req_reputation: f64,
    pub req_money: f64,
    pub gain_technical: f64,
    pub gain_social: f64,
    pub gain_management: f64,
    pub gain_knowledge: f64,
}

pub fn job_info(kind: JobKind) -> JobInfo {
    match kind {
        JobKind::Freeter => JobInfo {
            name: "フリーター",
            salary: 8.0,
            req_technical: 0.0,
            req_social: 0.0,
            req_management: 0.0,
            req_knowledge: 0.0,
            req_reputation: 0.0,
            req_money: 0.0,
            gain_technical: 0.0,
            gain_social: 0.0,
            gain_management: 0.0,
            gain_knowledge: 0.0,
        },
        JobKind::OfficeClerk => JobInfo {
            name: "事務員",
            salary: 20.0,
            req_technical: 0.0,
            req_social: 0.0,
            req_management: 0.0,
            req_knowledge: 5.0,
            req_reputation: 0.0,
            req_money: 0.0,
            gain_technical: 0.0,
            gain_social: 0.0,
            gain_management: 0.0,
            gain_knowledge: 0.005,
        },
        JobKind::Programmer => JobInfo {
            name: "プログラマー",
            salary: 50.0,
            req_technical: 15.0,
            req_social: 0.0,
            req_management: 0.0,
            req_knowledge: 0.0,
            req_reputation: 0.0,
            req_money: 0.0,
            gain_technical: 0.01,
            gain_social: 0.0,
            gain_management: 0.0,
            gain_knowledge: 0.0,
        },
        JobKind::Designer => JobInfo {
            name: "デザイナー",
            salary: 45.0,
            req_technical: 10.0,
            req_social: 8.0,
            req_management: 0.0,
            req_knowledge: 0.0,
            req_reputation: 0.0,
            req_money: 0.0,
            gain_technical: 0.005,
            gain_social: 0.005,
            gain_management: 0.0,
            gain_knowledge: 0.0,
        },
        JobKind::Sales => JobInfo {
            name: "営業",
            salary: 55.0,
            req_technical: 0.0,
            req_social: 15.0,
            req_management: 0.0,
            req_knowledge: 0.0,
            req_reputation: 0.0,
            req_money: 0.0,
            gain_technical: 0.0,
            gain_social: 0.01,
            gain_management: 0.0,
            gain_knowledge: 0.0,
        },
        JobKind::Accountant => JobInfo {
            name: "経理",
            salary: 40.0,
            req_technical: 0.0,
            req_social: 0.0,
            req_management: 5.0,
            req_knowledge: 15.0,
            req_reputation: 0.0,
            req_money: 0.0,
            gain_technical: 0.0,
            gain_social: 0.0,
            gain_management: 0.005,
            gain_knowledge: 0.005,
        },
        JobKind::Manager => JobInfo {
            name: "マネージャー",
            salary: 100.0,
            req_technical: 0.0,
            req_social: 10.0,
            req_management: 20.0,
            req_knowledge: 0.0,
            req_reputation: 0.0,
            req_money: 0.0,
            gain_technical: 0.0,
            gain_social: 0.0,
            gain_management: 0.01,
            gain_knowledge: 0.0,
        },
        JobKind::Consultant => JobInfo {
            name: "コンサルタント",
            salary: 130.0,
            req_technical: 0.0,
            req_social: 25.0,
            req_management: 0.0,
            req_knowledge: 20.0,
            req_reputation: 0.0,
            req_money: 0.0,
            gain_technical: 0.0,
            gain_social: 0.005,
            gain_management: 0.0,
            gain_knowledge: 0.005,
        },
        JobKind::Director => JobInfo {
            name: "部長",
            salary: 220.0,
            req_technical: 0.0,
            req_social: 20.0,
            req_management: 35.0,
            req_knowledge: 0.0,
            req_reputation: 30.0,
            req_money: 0.0,
            gain_technical: 0.0,
            gain_social: 0.005,
            gain_management: 0.005,
            gain_knowledge: 0.0,
        },
        JobKind::Entrepreneur => JobInfo {
            name: "起業家",
            salary: 400.0,
            req_technical: 30.0,
            req_social: 30.0,
            req_management: 30.0,
            req_knowledge: 30.0,
            req_reputation: 50.0,
            req_money: 100_000.0,
            gain_technical: 0.003,
            gain_social: 0.003,
            gain_management: 0.003,
            gain_knowledge: 0.003,
        },
    }
}

/// Training option info.
pub struct TrainingInfo {
    pub name: &'static str,
    pub cost: f64,
    pub technical: f64,
    pub social: f64,
    pub management: f64,
    pub knowledge: f64,
    pub reputation: f64,
}

pub const TRAININGS: [TrainingInfo; 5] = [
    TrainingInfo {
        name: "プログラミング講座",
        cost: 3_000.0,
        technical: 3.0,
        social: 0.0,
        management: 0.0,
        knowledge: 0.0,
        reputation: 0.0,
    },
    TrainingInfo {
        name: "ビジネスセミナー",
        cost: 3_000.0,
        technical: 0.0,
        social: 3.0,
        management: 0.0,
        knowledge: 0.0,
        reputation: 0.0,
    },
    TrainingInfo {
        name: "マネジメント研修",
        cost: 5_000.0,
        technical: 0.0,
        social: 0.0,
        management: 3.0,
        knowledge: 0.0,
        reputation: 0.0,
    },
    TrainingInfo {
        name: "独学する",
        cost: 0.0,
        technical: 0.0,
        social: 0.0,
        management: 0.0,
        knowledge: 1.0,
        reputation: 0.0,
    },
    TrainingInfo {
        name: "資格を取る",
        cost: 8_000.0,
        technical: 0.0,
        social: 0.0,
        management: 0.0,
        knowledge: 4.0,
        reputation: 5.0,
    },
];

/// Investment type.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum InvestKind {
    Savings,
    Stocks,
    RealEstate,
}

/// Lifestyle level — higher level gives skill bonuses but costs more.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LifestyleLevel {
    Frugal,  // Lv.1 質素
    Normal,  // Lv.2 普通
    Comfort, // Lv.3 快適
    Wealthy, // Lv.4 裕福
    Luxury,  // Lv.5 豪華
}

pub const ALL_LIFESTYLES: [LifestyleLevel; 5] = [
    LifestyleLevel::Frugal,
    LifestyleLevel::Normal,
    LifestyleLevel::Comfort,
    LifestyleLevel::Wealthy,
    LifestyleLevel::Luxury,
];

/// Static info about a lifestyle level.
pub struct LifestyleInfo {
    pub name: &'static str,
    pub level: u8,
    pub living_cost: f64,       // per month
    pub rent: f64,              // per month
    pub skill_efficiency: f64,  // multiplier bonus (0.0 = no bonus)
    pub rep_bonus: f64,         // per tick (additional)
}

pub fn lifestyle_info(level: LifestyleLevel) -> LifestyleInfo {
    match level {
        LifestyleLevel::Frugal => LifestyleInfo {
            name: "質素",
            level: 1,
            living_cost: 600.0,
            rent: 400.0,
            skill_efficiency: 0.0,
            rep_bonus: 0.0,
        },
        LifestyleLevel::Normal => LifestyleInfo {
            name: "普通",
            level: 2,
            living_cost: 1_200.0,
            rent: 800.0,
            skill_efficiency: 0.15,
            rep_bonus: 0.0,
        },
        LifestyleLevel::Comfort => LifestyleInfo {
            name: "快適",
            level: 3,
            living_cost: 2_500.0,
            rent: 1_500.0,
            skill_efficiency: 0.30,
            rep_bonus: 0.003,
        },
        LifestyleLevel::Wealthy => LifestyleInfo {
            name: "裕福",
            level: 4,
            living_cost: 5_000.0,
            rent: 3_000.0,
            skill_efficiency: 0.50,
            rep_bonus: 0.006,
        },
        LifestyleLevel::Luxury => LifestyleInfo {
            name: "豪華",
            level: 5,
            living_cost: 10_000.0,
            rent: 6_000.0,
            skill_efficiency: 0.70,
            rep_bonus: 0.010,
        },
    }
}

/// Investment option info.
pub struct InvestInfo {
    pub name: &'static str,
    pub increment: f64,
    pub return_rate: f64,
}

pub fn invest_info(kind: InvestKind) -> InvestInfo {
    match kind {
        InvestKind::Savings => InvestInfo {
            name: "貯金",
            increment: 1_000.0,
            // 0.05%/month = 0.0005/month / 300 ticks
            return_rate: 0.0005 / 300.0,
        },
        InvestKind::Stocks => InvestInfo {
            name: "株式投資",
            increment: 5_000.0,
            // 0.5%/month = 0.005/month / 300 ticks
            return_rate: 0.005 / 300.0,
        },
        InvestKind::RealEstate => InvestInfo {
            name: "不動産",
            increment: 80_000.0,
            // 1.5%/month = 0.015/month / 300 ticks
            return_rate: 0.015 / 300.0,
        },
    }
}

/// Active screen within the career game.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Screen {
    Main,
    Training,
    JobMarket,
    Invest,
    Budget,
    Lifestyle,
}

/// Maximum skill level.
pub const SKILL_CAP: f64 = 100.0;

/// Ticks per game day (10 ticks/sec × 10 sec = 1 day).
#[cfg(test)]
pub const TICKS_PER_DAY: u64 = 100;

/// Ticks per month (game pay period: 300 ticks = 30 seconds).
pub const TICKS_PER_MONTH: u32 = 300;

/// Reputation gain per tick from working.
const BASE_REP_GAIN: f64 = 0.002;

// ── Monthly Events ─────────────────────────────────────────────

/// Monthly event types that affect gameplay.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MonthEvent {
    TrainingSale,
    BullMarket,
    Recession,
    SkillBoom,
    WindfallBonus,
    MarketCrash,
    TaxRefund,
    ExpenseSpike,
}

pub fn event_name(event: MonthEvent) -> &'static str {
    match event {
        MonthEvent::TrainingSale => "研修セール",
        MonthEvent::BullMarket => "好景気",
        MonthEvent::Recession => "不景気",
        MonthEvent::SkillBoom => "学習ブースト",
        MonthEvent::WindfallBonus => "臨時ボーナス",
        MonthEvent::MarketCrash => "市場暴落",
        MonthEvent::TaxRefund => "税還付",
        MonthEvent::ExpenseSpike => "物価高騰",
    }
}

pub fn event_description(event: MonthEvent) -> &'static str {
    match event {
        MonthEvent::TrainingSale => "研修費用が50%オフ！",
        MonthEvent::BullMarket => "投資リターンが2倍！",
        MonthEvent::Recession => "今月の給与が20%減…",
        MonthEvent::SkillBoom => "スキル獲得量が2倍！",
        MonthEvent::WindfallBonus => "今月の給与が50%増！",
        MonthEvent::MarketCrash => "今月の投資リターンなし…",
        MonthEvent::TaxRefund => "今月の税率が半分！",
        MonthEvent::ExpenseSpike => "今月の生活費が50%増…",
    }
}

// ── Action Points ──────────────────────────────────────────────

/// Returns max AP for a given job kind.
pub fn ap_for_job(kind: JobKind) -> u8 {
    match kind {
        JobKind::Freeter | JobKind::OfficeClerk => 2,
        JobKind::Programmer | JobKind::Designer | JobKind::Sales | JobKind::Accountant => 3,
        JobKind::Manager | JobKind::Consultant | JobKind::Director | JobKind::Entrepreneur => 4,
    }
}

/// Monthly snapshot for the budget display.
#[derive(Clone, Debug)]
pub struct MonthlyReport {
    pub gross_salary: f64,
    pub tax: f64,
    pub insurance: f64,
    pub net_salary: f64,
    pub passive_income: f64,
    pub living_cost: f64,
    pub rent: f64,
    pub cashflow: f64,
}

impl MonthlyReport {
    pub fn empty() -> Self {
        Self {
            gross_salary: 0.0,
            tax: 0.0,
            insurance: 0.0,
            net_salary: 0.0,
            passive_income: 0.0,
            living_cost: 0.0,
            rent: 0.0,
            cashflow: 0.0,
        }
    }
}

/// Career simulator game state.
pub struct CareerState {
    pub money: f64,
    pub total_earned: f64,
    pub total_ticks: u64,

    pub technical: f64,
    pub social: f64,
    pub management: f64,
    pub knowledge: f64,
    pub reputation: f64,

    pub job: JobKind,

    pub savings: f64,
    pub stocks: f64,
    pub real_estate: f64,

    pub screen: Screen,
    pub log: Vec<String>,

    // Cash flow system
    pub lifestyle: LifestyleLevel,
    pub month_ticks: u32,
    pub months_elapsed: u32,
    pub month_gross_earned: f64,

    // Monthly report (updated each payday)
    pub last_report: MonthlyReport,

    // Economic freedom tracking
    pub won: bool,
    pub won_message: Option<String>,

    // Action Points system
    pub ap: u8,
    pub ap_max: u8,

    // Event system
    pub current_event: Option<MonthEvent>,
    pub event_seed: u64,
}

impl CareerState {
    pub fn new() -> Self {
        let initial_job = JobKind::Freeter;
        Self {
            money: 0.0,
            total_earned: 0.0,
            total_ticks: 0,
            technical: 0.0,
            social: 0.0,
            management: 0.0,
            knowledge: 0.0,
            reputation: 0.0,
            job: initial_job,
            savings: 0.0,
            stocks: 0.0,
            real_estate: 0.0,
            screen: Screen::Main,
            log: vec!["キャリアシミュレーターへようこそ！".into()],
            lifestyle: LifestyleLevel::Frugal,
            month_ticks: 0,
            months_elapsed: 0,
            month_gross_earned: 0.0,
            last_report: MonthlyReport::empty(),
            won: false,
            won_message: None,
            ap: ap_for_job(initial_job),
            ap_max: ap_for_job(initial_job),
            current_event: None,
            event_seed: 42,
        }
    }

    #[cfg(test)]
    pub fn day(&self) -> u64 {
        self.total_ticks / TICKS_PER_DAY + 1
    }

    pub fn base_rep_gain() -> f64 {
        BASE_REP_GAIN
    }

    pub fn add_log(&mut self, text: &str) {
        self.log.push(text.to_string());
        if self.log.len() > 30 {
            self.log.remove(0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_state() {
        let s = CareerState::new();
        assert_eq!(s.money, 0.0);
        assert_eq!(s.job, JobKind::Freeter);
        assert_eq!(s.technical, 0.0);
        assert_eq!(s.screen, Screen::Main);
        assert_eq!(s.day(), 1);
        assert_eq!(s.lifestyle, LifestyleLevel::Frugal);
        assert_eq!(s.months_elapsed, 0);
        assert!(!s.won);
        assert_eq!(s.ap, 2);
        assert_eq!(s.ap_max, 2);
        assert_eq!(s.current_event, None);
    }

    #[test]
    fn ap_for_job_tiers() {
        assert_eq!(ap_for_job(JobKind::Freeter), 2);
        assert_eq!(ap_for_job(JobKind::OfficeClerk), 2);
        assert_eq!(ap_for_job(JobKind::Programmer), 3);
        assert_eq!(ap_for_job(JobKind::Sales), 3);
        assert_eq!(ap_for_job(JobKind::Manager), 4);
        assert_eq!(ap_for_job(JobKind::Entrepreneur), 4);
    }

    #[test]
    fn event_info_valid() {
        let events = [
            MonthEvent::TrainingSale, MonthEvent::BullMarket, MonthEvent::Recession,
            MonthEvent::SkillBoom, MonthEvent::WindfallBonus, MonthEvent::MarketCrash,
            MonthEvent::TaxRefund, MonthEvent::ExpenseSpike,
        ];
        for e in events {
            assert!(!event_name(e).is_empty());
            assert!(!event_description(e).is_empty());
        }
    }

    #[test]
    fn day_calculation() {
        let mut s = CareerState::new();
        s.total_ticks = 0;
        assert_eq!(s.day(), 1);
        s.total_ticks = 99;
        assert_eq!(s.day(), 1);
        s.total_ticks = 100;
        assert_eq!(s.day(), 2);
        s.total_ticks = 250;
        assert_eq!(s.day(), 3);
    }

    #[test]
    fn log_truncation() {
        let mut s = CareerState::new();
        for i in 0..40 {
            s.add_log(&format!("msg {}", i));
        }
        assert!(s.log.len() <= 30);
    }

    #[test]
    fn all_jobs_have_info() {
        for &kind in &ALL_JOBS {
            let info = job_info(kind);
            assert!(info.salary > 0.0);
            assert!(!info.name.is_empty());
        }
    }

    #[test]
    fn skill_cap_is_100() {
        assert_eq!(SKILL_CAP, 100.0);
    }

    #[test]
    fn trainings_have_valid_data() {
        for t in &TRAININGS {
            assert!(!t.name.is_empty());
            assert!(t.cost >= 0.0);
            let total_gain = t.technical + t.social + t.management + t.knowledge + t.reputation;
            assert!(total_gain > 0.0, "training {} gives no benefit", t.name);
        }
    }

    #[test]
    fn invest_info_valid() {
        for kind in [InvestKind::Savings, InvestKind::Stocks, InvestKind::RealEstate] {
            let info = invest_info(kind);
            assert!(info.increment > 0.0);
            assert!(info.return_rate > 0.0);
        }
    }

    #[test]
    fn lifestyle_info_valid() {
        for &level in &ALL_LIFESTYLES {
            let info = lifestyle_info(level);
            assert!(!info.name.is_empty());
            assert!(info.living_cost >= 0.0);
            assert!(info.rent >= 0.0);
            assert!(info.skill_efficiency >= 0.0);
        }
    }

    #[test]
    fn lifestyle_costs_increase_with_level() {
        let frugal = lifestyle_info(LifestyleLevel::Frugal);
        let luxury = lifestyle_info(LifestyleLevel::Luxury);
        assert!(luxury.living_cost > frugal.living_cost);
        assert!(luxury.rent > frugal.rent);
        assert!(luxury.skill_efficiency > frugal.skill_efficiency);
    }

    #[test]
    fn monthly_report_empty_has_zero_fields() {
        let r = MonthlyReport::empty();
        assert_eq!(r.gross_salary, 0.0);
        assert_eq!(r.tax, 0.0);
        assert_eq!(r.cashflow, 0.0);
    }
}
