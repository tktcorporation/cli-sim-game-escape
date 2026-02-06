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
            return_rate: 0.00002,
        },
        InvestKind::Stocks => InvestInfo {
            name: "株式投資",
            increment: 5_000.0,
            return_rate: 0.0001,
        },
        InvestKind::RealEstate => InvestInfo {
            name: "不動産",
            increment: 50_000.0,
            return_rate: 0.0003,
        },
    }
}

/// Active screen within the career game.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Screen {
    Main,
    JobMarket,
    Invest,
}

/// Maximum skill level.
pub const SKILL_CAP: f64 = 100.0;

/// Ticks per game day (10 ticks/sec × 10 sec = 1 day).
pub const TICKS_PER_DAY: u64 = 100;

/// Reputation gain per tick from working.
const BASE_REP_GAIN: f64 = 0.002;

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
}

impl CareerState {
    pub fn new() -> Self {
        Self {
            money: 0.0,
            total_earned: 0.0,
            total_ticks: 0,
            technical: 0.0,
            social: 0.0,
            management: 0.0,
            knowledge: 0.0,
            reputation: 0.0,
            job: JobKind::Freeter,
            savings: 0.0,
            stocks: 0.0,
            real_estate: 0.0,
            screen: Screen::Main,
            log: vec!["キャリアシミュレーターへようこそ！".into()],
        }
    }

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
}
