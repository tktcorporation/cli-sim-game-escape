//! Career Simulator — pure game logic (no rendering / IO).

use super::state::{
    invest_info, job_info, CareerState, InvestKind, JobKind, Screen, ALL_JOBS, SKILL_CAP,
    TRAININGS,
};

// ── Tick ───────────────────────────────────────────────────────────────

pub fn tick(state: &mut CareerState, delta_ticks: u32) {
    for _ in 0..delta_ticks {
        tick_once(state);
    }
}

fn tick_once(state: &mut CareerState) {
    state.total_ticks += 1;

    // Salary
    let info = job_info(state.job);
    state.money += info.salary;
    state.total_earned += info.salary;

    // Passive skill gains from working
    add_skill(&mut state.technical, info.gain_technical);
    add_skill(&mut state.social, info.gain_social);
    add_skill(&mut state.management, info.gain_management);
    add_skill(&mut state.knowledge, info.gain_knowledge);

    // Reputation from working
    add_skill(&mut state.reputation, CareerState::base_rep_gain());

    // Investment returns
    let inv_return = investment_return(state);
    state.money += inv_return;
    state.total_earned += inv_return;
}

fn add_skill(skill: &mut f64, amount: f64) {
    *skill = (*skill + amount).min(SKILL_CAP);
}

fn investment_return(state: &CareerState) -> f64 {
    let s = invest_info(InvestKind::Savings);
    let st = invest_info(InvestKind::Stocks);
    let r = invest_info(InvestKind::RealEstate);
    state.savings * s.return_rate + state.stocks * st.return_rate + state.real_estate * r.return_rate
}

// ── Training ───────────────────────────────────────────────────────────

pub fn buy_training(state: &mut CareerState, index: usize) -> bool {
    if index >= TRAININGS.len() {
        return false;
    }
    let t = &TRAININGS[index];
    if state.money < t.cost {
        state.add_log(&format!("お金が足りません (必要: ¥{})", format_money(t.cost)));
        return false;
    }
    state.money -= t.cost;
    add_skill(&mut state.technical, t.technical);
    add_skill(&mut state.social, t.social);
    add_skill(&mut state.management, t.management);
    add_skill(&mut state.knowledge, t.knowledge);
    add_skill(&mut state.reputation, t.reputation);

    let mut parts = Vec::new();
    if t.technical > 0.0 {
        parts.push(format!("技術+{}", t.technical as u32));
    }
    if t.social > 0.0 {
        parts.push(format!("営業+{}", t.social as u32));
    }
    if t.management > 0.0 {
        parts.push(format!("管理+{}", t.management as u32));
    }
    if t.knowledge > 0.0 {
        parts.push(format!("知識+{}", t.knowledge as u32));
    }
    if t.reputation > 0.0 {
        parts.push(format!("評判+{}", t.reputation as u32));
    }
    state.add_log(&format!("{} 完了 ({})", t.name, parts.join(", ")));
    true
}

// ── Job Change ─────────────────────────────────────────────────────────

pub fn can_apply(state: &CareerState, kind: JobKind) -> bool {
    let info = job_info(kind);
    state.technical >= info.req_technical
        && state.social >= info.req_social
        && state.management >= info.req_management
        && state.knowledge >= info.req_knowledge
        && state.reputation >= info.req_reputation
        && state.money >= info.req_money
}

pub fn apply_job(state: &mut CareerState, index: usize) -> bool {
    if index >= ALL_JOBS.len() {
        return false;
    }
    let kind = ALL_JOBS[index];
    if kind == state.job {
        state.add_log("既に同じ職業です");
        return false;
    }
    if !can_apply(state, kind) {
        state.add_log("条件を満たしていません");
        return false;
    }
    let info = job_info(kind);
    state.job = kind;
    state.screen = Screen::Main;
    state.add_log(&format!("{}に転職しました！ (給料 ¥{}/tick)", info.name, info.salary as u64));
    true
}

// ── Investment ─────────────────────────────────────────────────────────

pub fn invest(state: &mut CareerState, kind: InvestKind) -> bool {
    let info = invest_info(kind);
    if state.money < info.increment {
        state.add_log(&format!(
            "お金が足りません (必要: ¥{})",
            format_money(info.increment)
        ));
        return false;
    }
    state.money -= info.increment;
    match kind {
        InvestKind::Savings => state.savings += info.increment,
        InvestKind::Stocks => state.stocks += info.increment,
        InvestKind::RealEstate => state.real_estate += info.increment,
    }
    state.add_log(&format!(
        "{}に ¥{} 投資しました",
        info.name,
        format_money(info.increment)
    ));
    true
}

// ── Formatting ─────────────────────────────────────────────────────────

pub fn format_money(amount: f64) -> String {
    let n = amount as u64;
    if n >= 1_000_000 {
        format!("{}M", n / 1_000_000)
    } else if n >= 10_000 {
        format!("{}万", n / 10_000)
    } else {
        format_with_commas(n)
    }
}

pub fn format_money_exact(amount: f64) -> String {
    format_with_commas(amount as u64)
}

fn format_with_commas(n: u64) -> String {
    let s = n.to_string();
    let mut result = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }
    result.chars().rev().collect()
}

// ── Queries ────────────────────────────────────────────────────────────

pub fn income_per_tick(state: &CareerState) -> f64 {
    let salary = job_info(state.job).salary;
    let inv = investment_return(state);
    salary + inv
}

pub fn next_available_job(state: &CareerState) -> Option<(JobKind, &'static str)> {
    for &kind in &ALL_JOBS {
        if kind == state.job {
            continue;
        }
        let info = job_info(kind);
        if info.salary > job_info(state.job).salary && !can_apply(state, kind) {
            return Some((kind, info.name));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tick_earns_salary() {
        let mut s = CareerState::new();
        tick(&mut s, 10);
        assert_eq!(s.money, 80.0); // freeter: 8 × 10
        assert_eq!(s.total_ticks, 10);
    }

    #[test]
    fn tick_gains_reputation() {
        let mut s = CareerState::new();
        tick(&mut s, 100);
        assert!(s.reputation > 0.0);
        assert!((s.reputation - 0.2).abs() < 0.001);
    }

    #[test]
    fn tick_gains_work_skills() {
        let mut s = CareerState::new();
        s.job = JobKind::Programmer;
        tick(&mut s, 100);
        assert!((s.technical - 1.0).abs() < 0.001);
    }

    #[test]
    fn buy_training_success() {
        let mut s = CareerState::new();
        s.money = 10_000.0;
        assert!(buy_training(&mut s, 0)); // programming course ¥3,000
        assert_eq!(s.money, 7_000.0);
        assert_eq!(s.technical, 3.0);
    }

    #[test]
    fn buy_training_insufficient_funds() {
        let mut s = CareerState::new();
        s.money = 100.0;
        assert!(!buy_training(&mut s, 0)); // ¥3,000 needed
        assert_eq!(s.money, 100.0); // unchanged
    }

    #[test]
    fn buy_training_free() {
        let mut s = CareerState::new();
        s.money = 0.0;
        assert!(buy_training(&mut s, 3)); // 独学 is free
        assert_eq!(s.knowledge, 1.0);
    }

    #[test]
    fn buy_training_invalid_index() {
        let mut s = CareerState::new();
        assert!(!buy_training(&mut s, 99));
    }

    #[test]
    fn skill_capped_at_100() {
        let mut s = CareerState::new();
        s.knowledge = 99.5;
        buy_training(&mut s, 3); // knowledge +1
        assert_eq!(s.knowledge, SKILL_CAP);
    }

    #[test]
    fn apply_job_success() {
        let mut s = CareerState::new();
        s.knowledge = 10.0;
        assert!(apply_job(&mut s, 1)); // office clerk requires knowledge >= 5
        assert_eq!(s.job, JobKind::OfficeClerk);
        assert_eq!(s.screen, Screen::Main);
    }

    #[test]
    fn apply_job_insufficient_skills() {
        let mut s = CareerState::new();
        assert!(!apply_job(&mut s, 2)); // programmer requires technical >= 15
        assert_eq!(s.job, JobKind::Freeter);
    }

    #[test]
    fn apply_same_job_fails() {
        let mut s = CareerState::new();
        assert!(!apply_job(&mut s, 0)); // already freeter
    }

    #[test]
    fn apply_job_invalid_index() {
        let mut s = CareerState::new();
        assert!(!apply_job(&mut s, 99));
    }

    #[test]
    fn invest_success() {
        let mut s = CareerState::new();
        s.money = 10_000.0;
        assert!(invest(&mut s, InvestKind::Savings));
        assert_eq!(s.savings, 1_000.0);
        assert_eq!(s.money, 9_000.0);
    }

    #[test]
    fn invest_insufficient_funds() {
        let mut s = CareerState::new();
        s.money = 500.0;
        assert!(!invest(&mut s, InvestKind::Savings));
        assert_eq!(s.savings, 0.0);
    }

    #[test]
    fn investment_generates_returns() {
        let mut s = CareerState::new();
        s.savings = 10_000.0;
        s.stocks = 10_000.0;
        let ret = investment_return(&s);
        assert!(ret > 0.0);
    }

    #[test]
    fn format_money_small() {
        assert_eq!(format_money(999.0), "999");
        assert_eq!(format_money(1_234.0), "1,234");
    }

    #[test]
    fn format_money_large() {
        assert_eq!(format_money(50_000.0), "5万");
        assert_eq!(format_money(1_500_000.0), "1M");
    }

    #[test]
    fn format_with_commas_works() {
        assert_eq!(format_with_commas(0), "0");
        assert_eq!(format_with_commas(999), "999");
        assert_eq!(format_with_commas(1_000), "1,000");
        assert_eq!(format_with_commas(1_234_567), "1,234,567");
    }

    #[test]
    fn income_per_tick_includes_investments() {
        let mut s = CareerState::new();
        s.savings = 50_000.0;
        let income = income_per_tick(&s);
        assert!(income > job_info(JobKind::Freeter).salary);
    }

    #[test]
    fn can_apply_entrepreneur_needs_everything() {
        let mut s = CareerState::new();
        s.technical = 30.0;
        s.social = 30.0;
        s.management = 30.0;
        s.knowledge = 30.0;
        s.reputation = 50.0;
        s.money = 100_000.0;
        assert!(can_apply(&s, JobKind::Entrepreneur));

        s.money = 99_999.0;
        assert!(!can_apply(&s, JobKind::Entrepreneur));
    }

    #[test]
    fn next_available_job_finds_target() {
        let s = CareerState::new(); // freeter, all skills 0
        let next = next_available_job(&s);
        assert!(next.is_some());
    }
}
