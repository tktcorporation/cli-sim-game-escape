//! Career Simulator — pure game logic (no rendering / IO).

use super::state::{
    event_description, event_name, invest_info, job_info, lifestyle_info, CareerState,
    InvestKind, JobKind, MonthEvent, MonthlyReport, Screen, ALL_JOBS, ALL_LIFESTYLES,
    INFLATION_RATE, MAX_MONTHS, REP_DECAY_PER_MONTH, SKILL_CAP, TICKS_PER_MONTH, TRAININGS,
};

// ── Tick (no-op: command-based game) ──────────────────────────────────

pub fn tick(_state: &mut CareerState, _delta_ticks: u32) {
    // Command-based: time advances via advance_month(), not per-tick.
}

// ── RNG ───────────────────────────────────────────────────────────────

fn next_rng(seed: u64) -> u64 {
    seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407)
}

fn generate_event(state: &mut CareerState) -> Option<MonthEvent> {
    state.event_seed = next_rng(state.event_seed);
    let roll = (state.event_seed >> 33) % 11;
    match roll {
        0..=2 => None, // ~27% chance of quiet month
        3 => Some(MonthEvent::TrainingSale),
        4 => Some(MonthEvent::BullMarket),
        5 => Some(MonthEvent::Recession),
        6 => Some(MonthEvent::SkillBoom),
        7 => Some(MonthEvent::WindfallBonus),
        8 => Some(MonthEvent::MarketCrash),
        9 => Some(MonthEvent::TaxRefund),
        10 => Some(MonthEvent::ExpenseSpike),
        _ => None,
    }
}

// ── Event Modifiers ──────────────────────────────────────────────────

fn salary_multiplier(event: Option<MonthEvent>) -> f64 {
    match event {
        Some(MonthEvent::Recession) => 0.8,
        Some(MonthEvent::WindfallBonus) => 1.5,
        _ => 1.0,
    }
}

fn investment_multiplier(event: Option<MonthEvent>) -> f64 {
    match event {
        Some(MonthEvent::BullMarket) => 2.0,
        Some(MonthEvent::MarketCrash) => 0.0,
        _ => 1.0,
    }
}

fn tax_multiplier(event: Option<MonthEvent>) -> f64 {
    match event {
        Some(MonthEvent::TaxRefund) => 0.5,
        _ => 1.0,
    }
}

fn expense_multiplier(event: Option<MonthEvent>) -> f64 {
    match event {
        Some(MonthEvent::ExpenseSpike) => 1.5,
        _ => 1.0,
    }
}

fn skill_multiplier(event: Option<MonthEvent>) -> f64 {
    match event {
        Some(MonthEvent::SkillBoom) => 2.0,
        _ => 1.0,
    }
}

pub fn training_cost_multiplier(event: Option<MonthEvent>) -> f64 {
    match event {
        Some(MonthEvent::TrainingSale) => 0.5,
        _ => 1.0,
    }
}

// ── Inflation & Time Limit ────────────────────────────────────────────

/// Expense inflation multiplier based on months elapsed.
/// 0.3% per month compounding: (1 + 0.003)^months
pub fn expense_inflation(months: u32) -> f64 {
    (1.0 + INFLATION_RATE).powi(months as i32)
}

/// Check if the game has ended (won or lost).
pub fn is_game_over(state: &CareerState) -> bool {
    state.won || state.months_elapsed >= MAX_MONTHS
}

/// Remaining months before deadline.
pub fn months_remaining(state: &CareerState) -> u32 {
    MAX_MONTHS.saturating_sub(state.months_elapsed)
}

// ── Advance Month ────────────────────────────────────────────────────

/// Advance the game by one full month. This is the core turn action.
pub fn advance_month(state: &mut CareerState) {
    if is_game_over(state) {
        return;
    }

    let info = job_info(state.job);
    let ls = lifestyle_info(state.lifestyle);
    let eff = 1.0 + ls.skill_efficiency;
    let month_ticks = TICKS_PER_MONTH as f64;
    let event = state.current_event;

    // Salary for the month (with event modifier)
    let gross = info.salary * month_ticks * salary_multiplier(event);
    state.money += gross;
    state.total_earned += gross;
    state.month_gross_earned = gross;

    // Passive skill gains from working (with event modifier)
    let s_mult = skill_multiplier(event);
    add_skill(&mut state.technical, info.gain_technical * eff * month_ticks * s_mult);
    add_skill(&mut state.social, info.gain_social * eff * month_ticks * s_mult);
    add_skill(&mut state.management, info.gain_management * eff * month_ticks * s_mult);
    add_skill(&mut state.knowledge, info.gain_knowledge * eff * month_ticks * s_mult);

    // Reputation from working + lifestyle bonus (month total)
    add_skill(
        &mut state.reputation,
        (CareerState::base_rep_gain() + ls.rep_bonus) * month_ticks,
    );

    // Reputation decay (must actively network to maintain high reputation)
    state.reputation = (state.reputation - REP_DECAY_PER_MONTH).max(0.0);

    // Investment returns for the month (with event modifier)
    let inv_return = monthly_investment_return(state) * investment_multiplier(event);
    state.money += inv_return;
    state.total_earned += inv_return;

    // Update tick tracking
    state.total_ticks += TICKS_PER_MONTH as u64;

    // Process payday (deductions, report, freedom check) — uses current event
    process_payday(state);

    // Generate event for next month
    let new_event = generate_event(state);
    state.current_event = new_event;

    if let Some(evt) = new_event {
        state.add_log(&format!("【イベント】{}: {}", event_name(evt), event_description(evt)));
    }

    // Reset per-month action tracking for next month
    state.reset_monthly_actions();

    // Check time limit
    if state.months_elapsed >= MAX_MONTHS && !state.won {
        state.add_log("━━━ 60ヶ月が経過しました ━━━");
        state.add_log("経済的自由は達成できませんでした…");
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

    let event = state.current_event;
    let gross = state.month_gross_earned;
    state.month_gross_earned = 0.0;

    // Tax and insurance calculation (with event modifier)
    let (tax_rate, insurance_rate) = tax_rates(gross);
    let tax = gross * tax_rate * tax_multiplier(event);
    let insurance = gross * insurance_rate;
    let net_salary = gross - tax - insurance;

    // Investment passive income (monthly total, with event modifier)
    let monthly_passive = monthly_investment_return(state) * investment_multiplier(event);
    // Tax on investment income (20%)
    let inv_tax = monthly_passive * 0.20;
    let net_passive = monthly_passive - inv_tax;

    // Living expenses (with event modifier + inflation)
    let ls = lifestyle_info(state.lifestyle);
    let exp_mult = expense_multiplier(event);
    let inflation = expense_inflation(state.months_elapsed);
    let living_cost = ls.living_cost * exp_mult * inflation;
    let rent = ls.rent * inflation;
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

    // Check economic freedom using BASE passive (without event boosts)
    // This prevents winning from a lucky BullMarket month alone
    let base_passive = monthly_investment_return(state) * 0.80;
    let inflated_expenses = (ls.living_cost + ls.rent) * inflation;
    check_economic_freedom(state, base_passive, inflated_expenses);
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
    if state.training_done[index] {
        state.add_log("今月はこの研修を受講済みです");
        return false;
    }
    let t = &TRAININGS[index];
    let cost = t.cost * training_cost_multiplier(state.current_event);
    if state.money < cost {
        state.add_log(&format!(
            "お金が足りません (必要: ¥{})",
            format_money(cost)
        ));
        return false;
    }
    state.training_done[index] = true;
    state.money -= cost;

    // Apply skill gains with lifestyle efficiency bonus and event modifier
    let ls = lifestyle_info(state.lifestyle);
    let eff = 1.0 + ls.skill_efficiency;
    let s_mult = skill_multiplier(state.current_event);
    add_skill(&mut state.technical, t.technical * eff * s_mult);
    add_skill(&mut state.social, t.social * eff * s_mult);
    add_skill(&mut state.management, t.management * eff * s_mult);
    add_skill(&mut state.knowledge, t.knowledge * eff * s_mult);
    add_skill(&mut state.reputation, t.reputation);

    let mut parts = Vec::new();
    if t.technical > 0.0 {
        parts.push(format!("技術+{}", (t.technical * eff * s_mult) as u32));
    }
    if t.social > 0.0 {
        parts.push(format!("営業+{}", (t.social * eff * s_mult) as u32));
    }
    if t.management > 0.0 {
        parts.push(format!("管理+{}", (t.management * eff * s_mult) as u32));
    }
    if t.knowledge > 0.0 {
        parts.push(format!("知識+{}", (t.knowledge * eff * s_mult) as u32));
    }
    if t.reputation > 0.0 {
        parts.push(format!("評判+{}", t.reputation as u32));
    }

    let cost_str = if cost > 0.0 {
        format!(" ¥{}", format_money(cost))
    } else {
        String::new()
    };
    state.add_log(&format!("{} 完了{} ({})", t.name, cost_str, parts.join(", ")));
    true
}

// ── Networking ─────────────────────────────────────────────────────────

pub fn do_networking(state: &mut CareerState) -> bool {
    if state.networked {
        state.add_log("今月は既にネットワーキング済みです");
        return false;
    }
    state.networked = true;

    let s_mult = skill_multiplier(state.current_event);
    let social_gain = 2.0 * s_mult;
    let rep_gain = 3.0;
    add_skill(&mut state.social, social_gain);
    add_skill(&mut state.reputation, rep_gain);

    state.add_log(&format!(
        "ネットワーキング: 営業+{}, 評判+{}",
        social_gain as u32, rep_gain as u32
    ));
    true
}

// ── Side Job ──────────────────────────────────────────────────────────

pub fn do_side_job(state: &mut CareerState) -> bool {
    if state.side_job_done {
        state.add_log("今月は既に副業済みです");
        return false;
    }

    let best_skill = state.technical
        .max(state.social)
        .max(state.management)
        .max(state.knowledge);

    if best_skill < 5.0 {
        state.add_log("副業にはスキル5以上が必要です");
        return false;
    }

    state.side_job_done = true;
    let earnings = best_skill * 100.0;
    state.money += earnings;
    state.total_earned += earnings;

    state.add_log(&format!(
        "副業で ¥{} 稼ぎました",
        format_money(earnings)
    ));
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
        format_money(monthly),
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

// ── Monthly Action State ──────────────────────────────────────────────

/// Check if all per-month actions have been used or are currently unavailable.
pub fn monthly_actions_exhausted(state: &CareerState) -> bool {
    let cost_mult = training_cost_multiplier(state.current_event);
    let any_training = TRAININGS.iter().enumerate().any(|(i, t)| {
        !state.training_done[i] && state.money >= t.cost * cost_mult
    });

    let networking_available = !state.networked;

    let best_skill = state
        .technical
        .max(state.social)
        .max(state.management)
        .max(state.knowledge);
    let side_job_available = !state.side_job_done && best_skill >= 5.0;

    !any_training && !networking_available && !side_job_available
}

// ── Next Goal ─────────────────────────────────────────────────────────

/// Returns a short description of what the player should aim for next.
pub fn next_goal(state: &CareerState) -> &'static str {
    if state.won {
        return "経済的自由を達成済み！";
    }

    // Phase 1: Need to get first job upgrade
    if state.job == JobKind::Freeter {
        if state.knowledge >= 5.0 {
            return "事務員に転職しよう[6]";
        }
        if !state.training_done[0] {
            return "独学[1]で知識を5にしよう";
        }
        if !state.networked {
            return "人脈作り[2]もやってみよう";
        }
        return "次の月へ[0]→独学を繰り返そう";
    }

    // Phase 2: Early career - get to Tier 2
    if matches!(state.job, JobKind::OfficeClerk) {
        if can_apply(state, JobKind::Programmer) {
            return "プログラマーに転職しよう[6]";
        }
        if can_apply(state, JobKind::Sales) {
            return "営業に転職しよう[6]";
        }
        if !state.training_done[1] && state.money >= 2_000.0 {
            return "研修[1]でスキルを上げよう";
        }
        if !state.training_done[0] {
            return "独学[1]で知識を伸ばそう";
        }
        if !state.networked {
            return "人脈作り[2]もやろう";
        }
        return "次の月へ[0]→研修を繰り返そう";
    }

    // Phase 3: All monthly actions exhausted → guide to advance
    if monthly_actions_exhausted(state) {
        if state.money >= 1_000.0 {
            return "投資[7]してから次の月へ[0]";
        }
        return "次の月へ進もう[0]";
    }

    // Phase 4: Mid career - suggest investment or higher jobs
    let passive = monthly_passive(state);
    let expenses = monthly_expenses(state);
    if passive > 0.0 && passive >= expenses * 0.5 {
        return "もう少しで経済的自由！投資を続けよう";
    }

    // Suggest available training
    if !state.training_done[1] && state.money >= 2_000.0 {
        return "研修[1]でスキルを上げよう";
    }
    if !state.training_done[0] {
        return "独学[1]も活用しよう";
    }
    if !state.networked {
        return "人脈作り[2]で評判UP";
    }

    // Suggest investing if sitting on cash
    if state.money >= 30_000.0 && state.real_estate == 0.0 {
        return "余剰資金を投資[7]に回そう";
    }
    if state.money >= 5_000.0 && state.stocks == 0.0 {
        return "株式投資[7]を始めよう";
    }

    // Suggest job upgrade
    for &kind in &ALL_JOBS {
        if kind == state.job {
            continue;
        }
        let info = job_info(kind);
        if info.salary > job_info(state.job).salary && can_apply(state, kind) {
            return "より高給の職に転職しよう[6]";
        }
    }

    "研修と投資でスキルと資産を伸ばそう"
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

pub fn monthly_salary(state: &CareerState) -> f64 {
    job_info(state.job).salary * TICKS_PER_MONTH as f64
}

pub fn monthly_expenses(state: &CareerState) -> f64 {
    let ls = lifestyle_info(state.lifestyle);
    (ls.living_cost + ls.rent) * expense_inflation(state.months_elapsed)
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
    fn advance_month_earns_salary() {
        let mut s = CareerState::new();
        let initial_money = s.money; // 5,000
        s.current_event = None;
        advance_month(&mut s);
        // Freeter: 10/tick * 300 ticks = 3,000 gross
        let gross = 3_000.0;
        let inflation = expense_inflation(1);
        let deductions = gross * 0.10 + gross * 0.05 + 800.0 * inflation + 700.0 * inflation;
        let expected = initial_money + gross - deductions;
        assert!((s.money - expected).abs() < 0.01, "money: {}, expected: {}", s.money, expected);
        assert_eq!(s.months_elapsed, 1);
        assert_eq!(s.total_ticks, 300);
    }

    #[test]
    fn advance_month_resets_monthly_actions() {
        let mut s = CareerState::new();
        s.training_done[0] = true;
        s.networked = true;
        s.side_job_done = true;
        advance_month(&mut s);
        assert_eq!(s.training_done, [false; 5]);
        assert!(!s.networked);
        assert!(!s.side_job_done);
    }

    #[test]
    fn advance_month_generates_event() {
        let mut s = CareerState::new();
        advance_month(&mut s);
        // With seed 42, we get a deterministic event (or None)
        // Just verify the system works
        assert_eq!(s.months_elapsed, 1);
    }

    #[test]
    fn advance_month_gains_reputation_with_decay() {
        let mut s = CareerState::new();
        s.current_event = None;
        advance_month(&mut s);
        // 0.002/tick * 300 ticks = 0.6, minus decay 0.3 = 0.3
        assert!((s.reputation - 0.3).abs() < 0.001);
    }

    #[test]
    fn advance_month_gains_work_skills() {
        let mut s = CareerState::new();
        s.job = JobKind::Programmer;
        s.current_event = None; // no event for clean test
        advance_month(&mut s);
        // 0.012/tick * 300 = 3.6
        assert!((s.technical - 3.6).abs() < 0.001);
    }

    #[test]
    fn lifestyle_boosts_skill_gain() {
        let mut s1 = CareerState::new();
        s1.job = JobKind::Programmer;
        s1.lifestyle = LifestyleLevel::Frugal;
        s1.current_event = None;
        advance_month(&mut s1);

        let mut s2 = CareerState::new();
        s2.job = JobKind::Programmer;
        s2.lifestyle = LifestyleLevel::Normal; // +20%
        s2.current_event = None;
        advance_month(&mut s2);

        assert!(s2.technical > s1.technical);
    }

    #[test]
    fn lifestyle_boosts_training() {
        let mut s1 = CareerState::new();
        s1.money = 10_000.0;
        s1.lifestyle = LifestyleLevel::Frugal;
        buy_training(&mut s1, 1); // programming: tech +4

        let mut s2 = CareerState::new();
        s2.money = 10_000.0;
        s2.lifestyle = LifestyleLevel::Normal; // +20%
        buy_training(&mut s2, 1); // programming: tech +4 * 1.20

        assert!(s2.technical > s1.technical);
    }

    #[test]
    fn buy_training_success() {
        let mut s = CareerState::new();
        s.money = 10_000.0;
        assert!(buy_training(&mut s, 1)); // programming course ¥2,000
        assert_eq!(s.money, 8_000.0);
        assert_eq!(s.technical, 4.0);
        assert!(s.training_done[1]); // marked as done
    }

    #[test]
    fn buy_training_already_done_this_month() {
        let mut s = CareerState::new();
        s.money = 10_000.0;
        assert!(buy_training(&mut s, 1));
        assert!(!buy_training(&mut s, 1)); // same training again
        assert_eq!(s.money, 8_000.0); // unchanged from first purchase
    }

    #[test]
    fn buy_training_insufficient_funds() {
        let mut s = CareerState::new();
        s.money = 100.0;
        assert!(!buy_training(&mut s, 1)); // ¥2,000 needed
        assert_eq!(s.money, 100.0); // unchanged
        assert!(!s.training_done[1]); // not marked done
    }

    #[test]
    fn buy_training_free() {
        let mut s = CareerState::new();
        s.money = 0.0;
        assert!(buy_training(&mut s, 0)); // 独学 is free
        assert_eq!(s.knowledge, 2.0);
        assert!(s.training_done[0]);
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
        buy_training(&mut s, 0); // knowledge +2
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
        assert!(!apply_job(&mut s, 2)); // programmer requires technical >= 12
        assert_eq!(s.job, JobKind::Freeter);
    }

    #[test]
    fn apply_job_no_ap_restriction_removed() {
        // Job changes no longer cost AP in v3
        let mut s = CareerState::new();
        s.knowledge = 10.0;
        assert!(apply_job(&mut s, 1));
        assert_eq!(s.job, JobKind::OfficeClerk);
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
        let ret = monthly_investment_return(&s);
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
    fn can_apply_entrepreneur_needs_everything() {
        let mut s = CareerState::new();
        s.technical = 30.0;
        s.social = 30.0;
        s.management = 30.0;
        s.knowledge = 30.0;
        s.reputation = 50.0;
        s.money = 100_000.0;
        assert!(can_apply(&s, JobKind::Entrepreneur));

        s.money = 79_999.0;
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
    fn advance_month_processes_payday() {
        let mut s = CareerState::new();
        advance_month(&mut s);
        assert_eq!(s.months_elapsed, 1);
        assert_eq!(s.month_ticks, 0);
    }

    #[test]
    fn payday_deducts_expenses_with_inflation() {
        let mut s = CareerState::new();
        let initial_money = s.money; // 5,000
        // Frugal lifestyle: base living ¥800 + rent ¥700 = ¥1,500/month
        // After 1 month inflation (1.003^1)
        // Freeter: 10/tick * 300 = ¥3,000 gross
        // Tax 10% = ¥300, Insurance 5% = ¥150
        s.current_event = None;
        advance_month(&mut s);
        assert_eq!(s.months_elapsed, 1);
        let inflation = expense_inflation(1);
        let expected = initial_money + 3_000.0 - (3_000.0 * 0.10) - (3_000.0 * 0.05) - 800.0 * inflation - 700.0 * inflation;
        assert!((s.money - expected).abs() < 0.01, "money: {}, expected: {}", s.money, expected);
    }

    #[test]
    fn payday_report_is_updated() {
        let mut s = CareerState::new();
        advance_month(&mut s);
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
        s.lifestyle = LifestyleLevel::Frugal; // expenses = ¥1,500/month
        s.current_event = None;
        // Need base_passive (after 20% tax) >= inflated expenses
        // expenses = ¥1,500 * 1.003 = ¥1,504.5 (at month 1)
        // base_passive = stocks * (0.005/300) * 300 * 0.80 = stocks * 0.004
        // ¥1,504.5 / 0.004 = ~376,125 → use 400,000
        s.stocks = 400_000.0;
        advance_month(&mut s);
        assert!(s.won);
        assert!(s.won_message.is_some());
    }

    #[test]
    fn game_stops_after_win() {
        let mut s = CareerState::new();
        s.won = true;
        let money_before = s.money;
        advance_month(&mut s);
        assert_eq!(s.money, money_before);
    }

    #[test]
    fn game_stops_at_max_months() {
        let mut s = CareerState::new();
        s.months_elapsed = MAX_MONTHS;
        let money_before = s.money;
        advance_month(&mut s);
        assert_eq!(s.money, money_before);
        assert!(is_game_over(&s));
    }

    #[test]
    fn is_game_over_conditions() {
        let mut s = CareerState::new();
        assert!(!is_game_over(&s));

        s.won = true;
        assert!(is_game_over(&s));

        let mut s2 = CareerState::new();
        s2.months_elapsed = MAX_MONTHS;
        assert!(is_game_over(&s2));
    }

    #[test]
    fn months_remaining_decreases() {
        let mut s = CareerState::new();
        assert_eq!(months_remaining(&s), 60);
        s.months_elapsed = 30;
        assert_eq!(months_remaining(&s), 30);
        s.months_elapsed = 60;
        assert_eq!(months_remaining(&s), 0);
    }

    #[test]
    fn reputation_decays_each_month() {
        let mut s = CareerState::new();
        s.reputation = 10.0;
        s.current_event = None;
        advance_month(&mut s);
        // Working gains: 0.002 * 300 = 0.6, decay: -0.3, net: +0.3
        assert!((s.reputation - 10.3).abs() < 0.001);
    }

    #[test]
    fn reputation_does_not_go_negative() {
        let mut s = CareerState::new();
        s.reputation = 0.0;
        s.current_event = None;
        advance_month(&mut s);
        assert!(s.reputation >= 0.0);
    }

    #[test]
    fn inflation_compounds_over_time() {
        let m0 = expense_inflation(0);
        let m30 = expense_inflation(30);
        let m60 = expense_inflation(60);
        assert!((m0 - 1.0).abs() < 0.001);
        assert!(m30 > 1.09); // ~1.094
        assert!(m60 > 1.19); // ~1.197
    }

    #[test]
    fn tick_is_noop() {
        let mut s = CareerState::new();
        let money_before = s.money;
        tick(&mut s, 1000);
        assert_eq!(s.money, money_before);
    }

    #[test]
    fn monthly_expenses_calculation() {
        let mut s = CareerState::new();
        s.lifestyle = LifestyleLevel::Normal;
        // At month 0, inflation = 1.0
        // Normal: living_cost=1500 + rent=1000 = 2500
        assert_eq!(monthly_expenses(&s), 2_500.0);
    }

    #[test]
    fn monthly_expenses_increase_with_inflation() {
        let mut s = CareerState::new();
        let initial = monthly_expenses(&s);
        s.months_elapsed = 60;
        let after_60 = monthly_expenses(&s);
        assert!(after_60 > initial);
        // 1.003^60 ≈ 1.197
        let ratio = after_60 / initial;
        assert!((ratio - 1.197).abs() < 0.01);
    }

    #[test]
    fn multiple_months_accumulate() {
        let mut s = CareerState::new();
        s.current_event = None;
        advance_month(&mut s);
        s.current_event = None;
        advance_month(&mut s);
        s.current_event = None;
        advance_month(&mut s);
        assert_eq!(s.months_elapsed, 3);
        assert_eq!(s.total_ticks, 900);
        assert!(s.money > 0.0);
    }

    // ── Networking tests ──────────────────────────────────────

    #[test]
    fn networking_success() {
        let mut s = CareerState::new();
        assert!(do_networking(&mut s));
        assert_eq!(s.social, 2.0);
        assert_eq!(s.reputation, 3.0);
        assert!(s.networked);
    }

    #[test]
    fn networking_already_done() {
        let mut s = CareerState::new();
        assert!(do_networking(&mut s));
        assert!(!do_networking(&mut s)); // second time fails
        assert_eq!(s.social, 2.0); // unchanged from first
    }

    // ── Side job tests ──────────────────────────────────────

    #[test]
    fn side_job_success() {
        let mut s = CareerState::new();
        s.technical = 10.0;
        let money_before = s.money;
        assert!(do_side_job(&mut s));
        assert_eq!(s.money, money_before + 1_000.0); // 10 * 100
        assert!(s.side_job_done);
    }

    #[test]
    fn side_job_requires_skill() {
        let mut s = CareerState::new();
        assert!(!do_side_job(&mut s)); // all skills 0
        assert!(!s.side_job_done); // not marked done
    }

    #[test]
    fn side_job_already_done() {
        let mut s = CareerState::new();
        s.technical = 10.0;
        assert!(do_side_job(&mut s));
        assert!(!do_side_job(&mut s)); // second time fails
    }

    #[test]
    fn side_job_uses_best_skill() {
        let mut s = CareerState::new();
        s.technical = 5.0;
        s.social = 20.0;
        s.management = 10.0;
        let money_before = s.money;
        do_side_job(&mut s);
        assert_eq!(s.money, money_before + 2_000.0); // 20 * 100
    }

    // ── Event modifier tests ──────────────────────────────────

    #[test]
    fn training_sale_halves_cost() {
        let mut s = CareerState::new();
        s.money = 10_000.0;
        s.current_event = Some(MonthEvent::TrainingSale);
        assert!(buy_training(&mut s, 1)); // programming ¥2,000 → ¥1,000
        assert_eq!(s.money, 9_000.0);
    }

    #[test]
    fn skill_boom_doubles_training_gain() {
        let mut s1 = CareerState::new();
        s1.money = 10_000.0;
        s1.current_event = None;
        buy_training(&mut s1, 1); // tech +4

        let mut s2 = CareerState::new();
        s2.money = 10_000.0;
        s2.current_event = Some(MonthEvent::SkillBoom);
        buy_training(&mut s2, 1); // tech +8

        assert!((s2.technical - s1.technical * 2.0).abs() < 0.001);
    }

    #[test]
    fn recession_reduces_salary() {
        let mut s = CareerState::new();
        s.current_event = Some(MonthEvent::Recession);
        advance_month(&mut s);
        // Gross should be 3000 * 0.8 = 2400
        assert!((s.last_report.gross_salary - 2_400.0).abs() < 0.01);
    }

    #[test]
    fn windfall_bonus_increases_salary() {
        let mut s = CareerState::new();
        s.current_event = Some(MonthEvent::WindfallBonus);
        advance_month(&mut s);
        // Gross should be 3000 * 1.5 = 4500
        assert!((s.last_report.gross_salary - 4_500.0).abs() < 0.01);
    }

    #[test]
    fn rng_is_deterministic() {
        let seed = 42u64;
        let a = next_rng(seed);
        let b = next_rng(seed);
        assert_eq!(a, b);
        assert_ne!(a, seed);
    }

    // ── Next Goal tests ──────────────────────────────────────

    #[test]
    fn next_goal_for_new_player() {
        let s = CareerState::new();
        let goal = next_goal(&s);
        assert!(goal.contains("独学"), "goal was: {}", goal);
    }

    #[test]
    fn next_goal_after_training_done() {
        let mut s = CareerState::new();
        s.training_done[0] = true; // did self-study
        let goal = next_goal(&s);
        // Should suggest networking or advancing, NOT re-suggest self-study
        assert!(!goal.contains("独学"), "should not suggest done training, got: {}", goal);
    }

    #[test]
    fn next_goal_all_actions_done_suggests_advance() {
        let mut s = CareerState::new();
        s.training_done = [true; 5]; // all trainings done
        s.networked = true;
        s.side_job_done = true;
        s.money = 500.0; // can't invest
        let goal = next_goal(&s);
        assert!(goal.contains("次の月"), "should suggest advance, got: {}", goal);
    }

    #[test]
    fn next_goal_actions_done_with_money_suggests_invest() {
        let mut s = CareerState::new();
        s.job = JobKind::Programmer;
        s.technical = 20.0;
        s.training_done = [true; 5];
        s.networked = true;
        s.side_job_done = true;
        s.money = 10_000.0; // can invest
        let goal = next_goal(&s);
        assert!(goal.contains("投資"), "should suggest investing, got: {}", goal);
    }

    #[test]
    fn next_goal_after_win() {
        let mut s = CareerState::new();
        s.won = true;
        assert!(next_goal(&s).contains("達成"));
    }

    #[test]
    fn monthly_actions_exhausted_initial() {
        let mut s = CareerState::new();
        // New player: can do self-study (free) and networking
        assert!(!monthly_actions_exhausted(&s));
        // After doing both
        s.training_done[0] = true; // self-study done
        s.networked = true;
        // Remaining trainings cost ¥2000+ but we have ¥5000 → can still train
        assert!(!monthly_actions_exhausted(&s));
        // Do programming training too
        s.training_done[1] = true;
        s.money = 1_500.0; // can't afford remaining trainings (¥2000+)
        // Still have side job available if skill >= 5
        assert!(monthly_actions_exhausted(&s)); // skill < 5, so exhausted
    }

    #[test]
    fn monthly_actions_exhausted_with_side_job() {
        let mut s = CareerState::new();
        s.training_done = [true; 5];
        s.networked = true;
        s.technical = 10.0; // skill >= 5
        assert!(!monthly_actions_exhausted(&s)); // side job available
        s.side_job_done = true;
        assert!(monthly_actions_exhausted(&s)); // now exhausted
    }

    // ── Balance Simulation ──────────────────────────────────────

    fn simulate(strategy: fn(&mut CareerState)) -> (bool, u32, f64, f64, f64) {
        let mut state = CareerState::new();
        while !is_game_over(&state) {
            strategy(&mut state);
            state.current_event = None; // deterministic simulation
            advance_month(&mut state);
        }
        (state.won, state.months_elapsed, state.money,
         monthly_passive(&state), monthly_expenses(&state))
    }

    /// Common investment: stocks first, switch to RE after threshold
    fn invest_mixed(state: &mut CareerState, stock_target: f64) {
        let re_cost = invest_info(InvestKind::RealEstate).increment;
        // Buy RE if affordable
        while state.money >= re_cost {
            invest(state, InvestKind::RealEstate);
        }
        // Build stocks up to target first, then save for RE
        if state.stocks < stock_target {
            while state.money >= 5_000.0 {
                invest(state, InvestKind::Stocks);
            }
        }
        // Otherwise save cash for next RE purchase
    }

    fn strat_idle(_state: &mut CareerState) {}

    fn strat_tech_rush(state: &mut CareerState) {
        // Train: each training once per month
        if state.knowledge < 5.0 && !state.training_done[0] { buy_training(state, 0); }
        if state.job == JobKind::Freeter && can_apply(state, JobKind::OfficeClerk) {
            apply_job(state, 1);
        }
        if state.technical < 12.0 && !state.training_done[1] {
            if state.money >= 2000.0 { buy_training(state, 1); }
            else if !state.training_done[0] { buy_training(state, 0); }
        }
        if state.job != JobKind::Programmer && can_apply(state, JobKind::Programmer) {
            apply_job(state, 2);
        }
        if !state.networked { do_networking(state); }
        invest_mixed(state, 50_000.0);
    }

    fn strat_social_rush(state: &mut CareerState) {
        if state.social < 12.0 && !state.training_done[2] {
            if state.money >= 2000.0 { buy_training(state, 2); }
        }
        if !state.networked { do_networking(state); }
        if state.job != JobKind::Sales && can_apply(state, JobKind::Sales) {
            apply_job(state, 4);
        }
        invest_mixed(state, 50_000.0);
    }

    fn strat_balanced(state: &mut CareerState) {
        if state.knowledge < 5.0 && !state.training_done[0] { buy_training(state, 0); }
        if state.job == JobKind::Freeter && can_apply(state, JobKind::OfficeClerk) {
            apply_job(state, 1);
        }
        if state.technical < 12.0 && !state.training_done[1] {
            if state.money >= 2000.0 { buy_training(state, 1); }
        }
        if state.job != JobKind::Programmer && can_apply(state, JobKind::Programmer) {
            apply_job(state, 2);
        }
        // Manager: management 18, social 10
        if state.management < 18.0 && !state.training_done[3] {
            if state.money >= 3000.0 { buy_training(state, 3); }
        }
        if !state.networked { do_networking(state); }
        if state.job != JobKind::Manager && can_apply(state, JobKind::Manager) {
            apply_job(state, 6);
        }
        invest_mixed(state, 50_000.0);
    }

    fn strat_optimal(state: &mut CareerState) {
        if state.knowledge < 5.0 && !state.training_done[0] { buy_training(state, 0); }
        if state.job == JobKind::Freeter && can_apply(state, JobKind::OfficeClerk) {
            apply_job(state, 1);
        }
        if state.technical < 12.0 && !state.training_done[1] {
            if state.money >= 2000.0 { buy_training(state, 1); }
        }
        if state.job != JobKind::Programmer && can_apply(state, JobKind::Programmer) {
            apply_job(state, 2);
        }
        // Manager: management 18, social 10
        if state.management < 18.0 && !state.training_done[3] {
            if state.money >= 3000.0 { buy_training(state, 3); }
        }
        if !state.networked { do_networking(state); }
        if state.job != JobKind::Manager && can_apply(state, JobKind::Manager) {
            apply_job(state, 6);
        }
        // Director: management 30, social 18, reputation 25
        if state.management < 30.0 && !state.training_done[3] {
            if state.money >= 3000.0 { buy_training(state, 3); }
        }
        if !state.side_job_done {
            let best = state.technical.max(state.social).max(state.management).max(state.knowledge);
            if best >= 5.0 { do_side_job(state); }
        }
        if state.job != JobKind::Director && can_apply(state, JobKind::Director) {
            apply_job(state, 8);
        }
        invest_mixed(state, 50_000.0);
    }

    #[test]
    fn balance_simulation() {
        let strategies: &[(&str, fn(&mut CareerState))] = &[
            ("Idle", strat_idle),
            ("Tech Rush", strat_tech_rush),
            ("Social Rush", strat_social_rush),
            ("Balanced", strat_balanced),
            ("Optimal", strat_optimal),
        ];

        eprintln!("\n=== Balance Simulation Results (v3) ===");
        eprintln!("{:<15} {:>5} {:>6} {:>10} {:>10} {:>10}", "Strategy", "Won?", "Month", "Money", "Passive", "Expenses");
        eprintln!("{}", "-".repeat(60));

        for (name, strategy) in strategies {
            let (won, months, money, passive, expenses) = simulate(*strategy);
            eprintln!("{:<15} {:>5} {:>6} {:>10.0} {:>10.0} {:>10.0}",
                name, if won { "YES" } else { "NO" }, months, money, passive, expenses);
        }
        eprintln!();

        // Balance targets for 60-month game:
        // Idle: LOSE
        // Tech/Social Rush (single path): may or may not win
        // Balanced (multi-path): should win
        // Optimal (director path): should win fastest
        let (idle_won, _, _, _, _) = simulate(strat_idle);
        assert!(!idle_won, "Idle should NOT win");

        let (balanced_won, _, _, _, _) = simulate(strat_balanced);
        assert!(balanced_won, "Balanced should win");

        let (optimal_won, optimal_months, _, _, _) = simulate(strat_optimal);
        assert!(optimal_won, "Optimal should win");
        assert!(optimal_months <= 60, "Optimal should win within time limit: {} months", optimal_months);
    }
}
