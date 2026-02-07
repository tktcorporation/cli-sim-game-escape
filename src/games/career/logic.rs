//! Career Simulator — pure game logic (no rendering / IO).

use super::state::{
    invest_info, job_info, lifestyle_info, CareerState, InvestKind, JobKind, MonthlyReport, Screen,
    ALL_JOBS, ALL_LIFESTYLES, SKILL_CAP, TICKS_PER_MONTH, TRAININGS,
};

// ── Tick ───────────────────────────────────────────────────────────────

pub fn tick(state: &mut CareerState, delta_ticks: u32) {
    for _ in 0..delta_ticks {
        tick_once(state);
    }
}

fn tick_once(state: &mut CareerState) {
    if state.won {
        return;
    }

    state.total_ticks += 1;
    state.month_ticks += 1;

    // Salary (accumulates per tick)
    let info = job_info(state.job);
    state.money += info.salary;
    state.total_earned += info.salary;
    state.month_gross_earned += info.salary;

    // Passive skill gains from working (with lifestyle efficiency bonus)
    let ls = lifestyle_info(state.lifestyle);
    let eff = 1.0 + ls.skill_efficiency;
    add_skill(&mut state.technical, info.gain_technical * eff);
    add_skill(&mut state.social, info.gain_social * eff);
    add_skill(&mut state.management, info.gain_management * eff);
    add_skill(&mut state.knowledge, info.gain_knowledge * eff);

    // Reputation from working + lifestyle bonus
    add_skill(
        &mut state.reputation,
        CareerState::base_rep_gain() + ls.rep_bonus,
    );

    // Investment returns (per tick)
    let inv_return = investment_return(state);
    state.money += inv_return;
    state.total_earned += inv_return;

    // Monthly cycle: payday processing
    if state.month_ticks >= TICKS_PER_MONTH {
        process_payday(state);
    }
}

fn add_skill(skill: &mut f64, amount: f64) {
    *skill = (*skill + amount).min(SKILL_CAP);
}

fn investment_return(state: &CareerState) -> f64 {
    let s = invest_info(InvestKind::Savings);
    let st = invest_info(InvestKind::Stocks);
    let r = invest_info(InvestKind::RealEstate);
    state.savings * s.return_rate
        + state.stocks * st.return_rate
        + state.real_estate * r.return_rate
}

// ── Monthly Payday ────────────────────────────────────────────────────

fn process_payday(state: &mut CareerState) {
    state.months_elapsed += 1;
    state.month_ticks = 0;

    let gross = state.month_gross_earned;
    state.month_gross_earned = 0.0;

    // Tax and insurance calculation
    let (tax_rate, insurance_rate) = tax_rates(gross);
    let tax = gross * tax_rate;
    let insurance = gross * insurance_rate;
    let net_salary = gross - tax - insurance;

    // Investment passive income (monthly total)
    let monthly_passive = monthly_investment_return(state);
    // Tax on investment income (20%)
    let inv_tax = monthly_passive * 0.20;
    let net_passive = monthly_passive - inv_tax;

    // Living expenses
    let ls = lifestyle_info(state.lifestyle);
    let living_cost = ls.living_cost;
    let rent = ls.rent;
    let total_expenses = living_cost + rent;

    // Deduct monthly expenses from money
    let deductions = tax + insurance + living_cost + rent + inv_tax;
    state.money -= deductions;

    // Monthly cashflow
    let cashflow = net_salary + net_passive - total_expenses;

    // Update monthly report
    state.last_report = MonthlyReport {
        gross_salary: gross,
        tax: tax + inv_tax,
        insurance,
        net_salary,
        passive_income: net_passive,
        living_cost,
        rent,
        cashflow,
    };

    // Log the payday summary
    state.add_log(&format!(
        "【{}ヶ月目】給与¥{} 税¥{} 生活費¥{} → 手残¥{}",
        state.months_elapsed,
        format_money(gross),
        format_money(tax + insurance + inv_tax),
        format_money(total_expenses),
        format_money_signed(cashflow),
    ));

    // Check economic freedom
    check_economic_freedom(state, net_passive, total_expenses);
}

/// Calculate tax and insurance rates based on monthly gross income.
pub fn tax_rates(monthly_gross: f64) -> (f64, f64) {
    if monthly_gross <= 6_000.0 {
        (0.10, 0.05)
    } else if monthly_gross <= 20_000.0 {
        (0.15, 0.08)
    } else if monthly_gross <= 40_000.0 {
        (0.20, 0.10)
    } else if monthly_gross <= 80_000.0 {
        (0.25, 0.10)
    } else {
        (0.30, 0.10)
    }
}

/// Calculate monthly investment return.
pub fn monthly_investment_return(state: &CareerState) -> f64 {
    investment_return(state) * TICKS_PER_MONTH as f64
}

fn check_economic_freedom(state: &mut CareerState, passive: f64, expenses: f64) {
    if passive >= expenses && expenses > 0.0 && !state.won {
        state.won = true;
        state.won_message = Some(format!(
            "経済的自由を達成！不労所得¥{}/月 ≥ 支出¥{}/月 ({}ヶ月目)",
            format_money(passive),
            format_money(expenses),
            state.months_elapsed
        ));
        state.add_log("★★★ 経済的自由を達成しました！ ★★★");
        state.add_log("不労所得が支出を上回り、ラットレースを脱出！");
    }
}

// ── Lifestyle ─────────────────────────────────────────────────────────

pub fn change_lifestyle(state: &mut CareerState, index: usize) -> bool {
    if index >= ALL_LIFESTYLES.len() {
        return false;
    }
    let level = ALL_LIFESTYLES[index];
    if level == state.lifestyle {
        state.add_log("既に同じ生活水準です");
        return false;
    }
    let info = lifestyle_info(level);
    state.lifestyle = level;
    state.screen = Screen::Main;
    state.add_log(&format!(
        "生活水準をLv.{} {}に変更 (月額¥{})",
        info.level,
        info.name,
        format_money(info.living_cost + info.rent),
    ));
    true
}

// ── Training ───────────────────────────────────────────────────────────

pub fn buy_training(state: &mut CareerState, index: usize) -> bool {
    if index >= TRAININGS.len() {
        return false;
    }
    let t = &TRAININGS[index];
    if state.money < t.cost {
        state.add_log(&format!(
            "お金が足りません (必要: ¥{})",
            format_money(t.cost)
        ));
        return false;
    }
    state.money -= t.cost;

    // Apply skill gains with lifestyle efficiency bonus
    let ls = lifestyle_info(state.lifestyle);
    let eff = 1.0 + ls.skill_efficiency;
    add_skill(&mut state.technical, t.technical * eff);
    add_skill(&mut state.social, t.social * eff);
    add_skill(&mut state.management, t.management * eff);
    add_skill(&mut state.knowledge, t.knowledge * eff);
    add_skill(&mut state.reputation, t.reputation);

    let mut parts = Vec::new();
    if t.technical > 0.0 {
        parts.push(format!("技術+{}", (t.technical * eff) as u32));
    }
    if t.social > 0.0 {
        parts.push(format!("営業+{}", (t.social * eff) as u32));
    }
    if t.management > 0.0 {
        parts.push(format!("管理+{}", (t.management * eff) as u32));
    }
    if t.knowledge > 0.0 {
        parts.push(format!("知識+{}", (t.knowledge * eff) as u32));
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

    let monthly = info.salary * TICKS_PER_MONTH as f64;
    state.add_log(&format!(
        "{}に転職しました！ (月給 ¥{})",
        info.name,
        format_money(monthly)
    ));
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
    let n = amount.abs() as u64;
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

fn format_money_signed(amount: f64) -> String {
    if amount >= 0.0 {
        format!("+{}", format_money(amount))
    } else {
        format!("-{}", format_money(-amount))
    }
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

#[cfg(test)]
pub fn income_per_tick(state: &CareerState) -> f64 {
    let salary = job_info(state.job).salary;
    let inv = investment_return(state);
    salary + inv
}

pub fn monthly_salary(state: &CareerState) -> f64 {
    job_info(state.job).salary * TICKS_PER_MONTH as f64
}

pub fn monthly_expenses(state: &CareerState) -> f64 {
    let ls = lifestyle_info(state.lifestyle);
    ls.living_cost + ls.rent
}

pub fn monthly_passive(state: &CareerState) -> f64 {
    let gross = monthly_investment_return(state);
    gross * 0.80 // after 20% tax
}

pub fn freedom_progress(state: &CareerState) -> f64 {
    let expenses = monthly_expenses(state);
    if expenses <= 0.0 {
        return 0.0;
    }
    let passive = monthly_passive(state);
    (passive / expenses).min(1.0)
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
    use super::super::state::LifestyleLevel;

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
    fn lifestyle_boosts_skill_gain() {
        let mut s1 = CareerState::new();
        s1.job = JobKind::Programmer;
        s1.lifestyle = LifestyleLevel::Frugal;
        tick(&mut s1, 100);

        let mut s2 = CareerState::new();
        s2.job = JobKind::Programmer;
        s2.lifestyle = LifestyleLevel::Normal; // +15%
        tick(&mut s2, 100);

        assert!(s2.technical > s1.technical);
    }

    #[test]
    fn lifestyle_boosts_training() {
        let mut s1 = CareerState::new();
        s1.money = 10_000.0;
        s1.lifestyle = LifestyleLevel::Frugal;
        buy_training(&mut s1, 0); // tech +3

        let mut s2 = CareerState::new();
        s2.money = 10_000.0;
        s2.lifestyle = LifestyleLevel::Normal; // +15%
        buy_training(&mut s2, 0); // tech +3 * 1.15

        assert!(s2.technical > s1.technical);
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

    // ── Monthly cycle tests ──────────────────────────────────────

    #[test]
    fn monthly_cycle_triggers_at_300_ticks() {
        let mut s = CareerState::new();
        tick(&mut s, 299);
        assert_eq!(s.months_elapsed, 0);
        tick(&mut s, 1);
        assert_eq!(s.months_elapsed, 1);
        assert_eq!(s.month_ticks, 0);
    }

    #[test]
    fn payday_deducts_expenses() {
        let mut s = CareerState::new();
        // Frugal lifestyle: living ¥600 + rent ¥400 = ¥1,000/month
        // Freeter: 8/tick * 300 = ¥2,400 gross
        // Tax 10% = ¥240, Insurance 5% = ¥120
        // Total deductions = ¥240 + ¥120 + ¥1,000 = ¥1,360
        // Money after month = ¥2,400 - ¥1,360 = ¥1,040
        tick(&mut s, 300);
        assert_eq!(s.months_elapsed, 1);
        let expected = 2_400.0 - (2_400.0 * 0.10) - (2_400.0 * 0.05) - 600.0 - 400.0;
        assert!((s.money - expected).abs() < 0.01, "money: {}, expected: {}", s.money, expected);
    }

    #[test]
    fn payday_report_is_updated() {
        let mut s = CareerState::new();
        tick(&mut s, 300);
        assert!(s.last_report.gross_salary > 0.0);
        assert!(s.last_report.tax > 0.0);
        assert!(s.last_report.living_cost > 0.0);
    }

    #[test]
    fn tax_rate_tiers() {
        assert_eq!(tax_rates(2_000.0), (0.10, 0.05));
        assert_eq!(tax_rates(10_000.0), (0.15, 0.08));
        assert_eq!(tax_rates(30_000.0), (0.20, 0.10));
        assert_eq!(tax_rates(50_000.0), (0.25, 0.10));
        assert_eq!(tax_rates(100_000.0), (0.30, 0.10));
    }

    #[test]
    fn change_lifestyle_success() {
        let mut s = CareerState::new();
        assert_eq!(s.lifestyle, LifestyleLevel::Frugal);
        assert!(change_lifestyle(&mut s, 1)); // Normal
        assert_eq!(s.lifestyle, LifestyleLevel::Normal);
        assert_eq!(s.screen, Screen::Main);
    }

    #[test]
    fn change_lifestyle_same_fails() {
        let mut s = CareerState::new();
        assert!(!change_lifestyle(&mut s, 0)); // already Frugal
    }

    #[test]
    fn change_lifestyle_invalid_index() {
        let mut s = CareerState::new();
        assert!(!change_lifestyle(&mut s, 99));
    }

    #[test]
    fn freedom_progress_zero_with_no_investments() {
        let s = CareerState::new();
        assert_eq!(freedom_progress(&s), 0.0);
    }

    #[test]
    fn freedom_progress_increases_with_investments() {
        let mut s = CareerState::new();
        s.stocks = 100_000.0;
        let progress = freedom_progress(&s);
        assert!(progress > 0.0);
        assert!(progress <= 1.0);
    }

    #[test]
    fn economic_freedom_is_detected() {
        let mut s = CareerState::new();
        s.lifestyle = LifestyleLevel::Frugal; // expenses = ¥1,000/month
        // Need passive >= ¥1,000/month (after 20% tax)
        // ¥1,000 / 0.80 = ¥1,250 gross monthly passive needed
        // At 0.5%/month from stocks: need ¥250,000 in stocks
        s.stocks = 300_000.0;
        tick(&mut s, 300); // trigger payday
        assert!(s.won);
        assert!(s.won_message.is_some());
    }

    #[test]
    fn game_stops_ticking_after_win() {
        let mut s = CareerState::new();
        s.won = true;
        let money_before = s.money;
        tick(&mut s, 100);
        assert_eq!(s.money, money_before);
    }

    #[test]
    fn monthly_expenses_calculation() {
        let mut s = CareerState::new();
        s.lifestyle = LifestyleLevel::Normal;
        assert_eq!(monthly_expenses(&s), 2_000.0); // 1200 + 800
    }
}
