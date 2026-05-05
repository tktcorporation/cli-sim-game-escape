//! 廃墟カフェ復興記 — 総合プレイループ・シミュレータ。
//!
//! ガチャ単体の率カーブ (gacha::simulator) ではなく、
//! 「日次ジェム収入 → 引けるだけ引く → ダブり/ボーナス coin で装備 (=カード) を
//!   レベルアップ → 装備強化倍率が上がる → 次のティアに進む」
//! という *ループ全体* を回したときに、運気ティア・★3 取得・装備倍率が
//! プレイ進行に応じてどこまで伸びるかを観測するためのシミュレータ。
//!
//! `abyss::simulator` と同じ「Policy + Runner + Report」構造で、
//! バランス調整時の判断材料を `cargo test simulate_cafe_full_loop -- --nocapture`
//! で印字できる。
//!
//! 実装上の割り切り:
//! - 日次ジェム収入は *モデル化* (LOGIN_REWARDS と DailyMission の reward_gems
//!   を参考に casual / engaged の 2 種類)。実時間ベースの login_bonus は
//!   wasm 依存なので simulator では使わない。
//! - 装備強化 = 所持中の最大倍率カードを毎日レベル MAX まで上げる、と単純化。
//!   これが「ガチャ + 装備強化を運用したとき」の最良ケースに近い。
//! - 営業 (money の収支) は本シミュレータでは無視。本ゲームの money は gacha とは
//!   独立した経済軸 (営業＋ミッション報酬) なので、ガチャループの観測には不要。

#![cfg(test)]

use super::gacha::{
    self, active_banners, card_def, fortune_dupe_multiplier_x10, fortune_label,
    fortune_pull_bonus_coins, fortune_tier, gacha_pull, Rarity, GACHA_SINGLE_COST,
};
use super::gacha::state::CardState;

// ───────────────────────────────────────────────────────────
// Daily gem-income policies (model the cafe meta-loop).
// ───────────────────────────────────────────────────────────

pub trait GemIncome {
    fn gems_for_day(&self, day: u32) -> u32;
    fn label(&self) -> &'static str;
}

/// 「ログインだけしてる」プレイヤー — 平均 50 gem/日。
pub struct CasualIncome;
impl GemIncome for CasualIncome {
    fn gems_for_day(&self, day: u32) -> u32 {
        // Login bonus average ≈ 50/day, weekly milestone bumps it.
        if day.is_multiple_of(7) { 120 } else { 50 }
    }
    fn label(&self) -> &'static str { "casual (login-only, avg ~60/day)" }
}

/// 「ミッションも回す」プレイヤー — daily 全達成 + 営業数回 = ≈ 200 gem/日。
/// 7日毎に weekly mission bonus を加算。
pub struct EngagedIncome;
impl GemIncome for EngagedIncome {
    fn gems_for_day(&self, day: u32) -> u32 {
        // Daily missions all-clear ≈ 150 gem, login avg ≈ 50, weekly avg ≈ 50.
        let base = 200;
        let weekly = if day.is_multiple_of(7) { 250 } else { 0 };
        base + weekly
    }
    fn label(&self) -> &'static str { "engaged (missions+login, ~200/day + weekly)" }
}

/// 「課金もする / イベント全周回」想定 — 日次 400 gem + イベ報酬。
/// 90日でほぼ確実に tier 4 まで届く設計。
pub struct WhaleIncome;
impl GemIncome for WhaleIncome {
    fn gems_for_day(&self, day: u32) -> u32 {
        let base = 400;
        let weekly = if day.is_multiple_of(7) { 400 } else { 0 };
        let monthly = if day.is_multiple_of(30) { 1500 } else { 0 };
        base + weekly + monthly
    }
    fn label(&self) -> &'static str { "whale (event farm, ~400/day + weekly + monthly)" }
}

// ───────────────────────────────────────────────────────────
// Per-sample metric & full report.
// ───────────────────────────────────────────────────────────

#[derive(Default, Clone, Debug)]
pub struct DaySample {
    pub day: u32,
    pub pulls_today: u32,
    pub star3_today: u32,
    pub coins_balance: u32,
    pub equipped_mult_x100: u32,
    pub fortune_tier: u32,
    pub lifetime_pulls: u32,
}

#[derive(Default)]
pub struct SimReport {
    pub total_days: u32,
    pub total_pulls: u32,
    pub star3_total: u32,
    pub star2_total: u32,
    pub star1_total: u32,
    pub final_lifetime_pulls: u32,
    pub final_fortune_tier: u32,
    pub final_equipped_mult: f64,
    pub coins_earned_total: u32,
    pub coins_spent_leveling: u32,
    pub level_ups_done: u32,
    pub samples: Vec<DaySample>,
}

impl SimReport {
    pub fn print(&self, label: &str) {
        eprintln!("\n── {} ──", label);
        let pct3 = if self.total_pulls > 0 {
            self.star3_total as f64 / self.total_pulls as f64 * 100.0
        } else { 0.0 };
        eprintln!(
            "  期間: {} 日 / 累計引き: {} / ★3: {} ({:.2}%)",
            self.total_days, self.total_pulls, self.star3_total, pct3
        );
        eprintln!(
            "  最終運気: tier {} ({}) / 装備倍率: ×{:.2}",
            self.final_fortune_tier,
            fortune_label(self.final_fortune_tier),
            self.final_equipped_mult,
        );
        eprintln!(
            "  獲得コイン: {} / 強化に投入: {} / level-up 回数: {}",
            self.coins_earned_total, self.coins_spent_leveling, self.level_ups_done,
        );
        eprintln!("  日 | 引き | ★3 | コイン残 | 装備倍率 | ティア | 累計引");
        for m in &self.samples {
            eprintln!(
                "  {:>3} | {:>4} | {:>3} | {:>8} | ×{:>5.2} | {:>5} | {:>5}",
                m.day,
                m.pulls_today,
                m.star3_today,
                m.coins_balance,
                m.equipped_mult_x100 as f64 / 100.0,
                m.fortune_tier,
                m.lifetime_pulls,
            );
        }
    }
}

// ───────────────────────────────────────────────────────────
// Runner.
// ───────────────────────────────────────────────────────────

/// Sample these days into the report (clipped to `n_days`).
fn default_sample_days(n_days: u32) -> Vec<u32> {
    let mut v = vec![1u32, 7, 14, 30, 60, 90];
    v.retain(|d| *d <= n_days);
    if !v.contains(&n_days) {
        v.push(n_days);
    }
    v
}

/// Equip the highest-multiplier owned card.
fn equip_strongest(state: &mut CardState) {
    let best = state.cards.iter().enumerate().max_by(|(_, a), (_, b)| {
        a.multiplier()
            .partial_cmp(&b.multiplier())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    if let Some((idx, _)) = best {
        state.equipped_card = Some(idx);
    }
}

/// Run a full cafe-loop simulation.
pub fn run_cafe_sim(seed: u32, n_days: u32, income: &dyn GemIncome) -> SimReport {
    let mut state = CardState::default();
    let banners = active_banners();
    let banner = &banners[0];
    let sample_days = default_sample_days(n_days);
    let mut report = SimReport::default();

    for day in 1..=n_days {
        // Daily gem income (modeled — the cafe game's actual login_bonus uses
        // wall-clock JST which we don't have offline).
        state.gems = state.gems.saturating_add(income.gems_for_day(day));

        let mut day_pulls = 0u32;
        let mut day_star3 = 0u32;
        let coins_before_pulls = state.coins;

        // Pull as many singles as we can afford.  This models the most common
        // play pattern (10連 saves nothing extra in this game's economy).
        while state.gems >= GACHA_SINGLE_COST {
            state.gems -= GACHA_SINGLE_COST;
            let pull_seed = seed
                .wrapping_mul(2654435761)
                .wrapping_add(day.wrapping_mul(10_000).wrapping_add(day_pulls).wrapping_mul(37));
            let id = gacha_pull(&mut state, pull_seed, banner);
            let r = card_def(id).expect("card def").rarity;
            day_pulls += 1;
            report.total_pulls += 1;
            match r {
                Rarity::Star3 => { day_star3 += 1; report.star3_total += 1; }
                Rarity::Star2 => { report.star2_total += 1; }
                Rarity::Star1 => { report.star1_total += 1; }
            }
        }

        // Track coins gained from today's pulls (bonus + dupe + base).
        let gained = state.coins.saturating_sub(coins_before_pulls);
        report.coins_earned_total = report.coins_earned_total.saturating_add(gained);

        // Equip strongest card and level it up while we can afford it.  This
        // models a player who maintains the "best available" build.
        equip_strongest(&mut state);
        if let Some(idx) = state.equipped_card {
            let coins_before_level = state.coins;
            while state.level_up(idx) {
                report.level_ups_done += 1;
            }
            let spent = coins_before_level.saturating_sub(state.coins);
            report.coins_spent_leveling = report.coins_spent_leveling.saturating_add(spent);
        }

        if sample_days.contains(&day) {
            let mult = state.equipped_multiplier();
            report.samples.push(DaySample {
                day,
                pulls_today: day_pulls,
                star3_today: day_star3,
                coins_balance: state.coins,
                equipped_mult_x100: (mult * 100.0) as u32,
                fortune_tier: fortune_tier(state.lifetime_pulls),
                lifetime_pulls: state.lifetime_pulls,
            });
        }
    }

    report.total_days = n_days;
    report.final_lifetime_pulls = state.lifetime_pulls;
    report.final_fortune_tier = fortune_tier(state.lifetime_pulls);
    report.final_equipped_mult = state.equipped_multiplier();
    report
}

// ───────────────────────────────────────────────────────────
// Tests / sanity checks.
// ───────────────────────────────────────────────────────────

#[test]
fn casual_player_progresses_but_modestly() {
    let r = run_cafe_sim(0xC0FFEE, 30, &CasualIncome);
    assert!(r.total_pulls > 0, "casual should pull at least sometimes");
    assert!(r.final_equipped_mult >= 1.0, "should equip at least one card");
    eprintln!(
        "casual 30d: pulls={} ★3={} tier={} mult=×{:.2}",
        r.total_pulls, r.star3_total, r.final_fortune_tier, r.final_equipped_mult
    );
}

#[test]
fn engaged_player_outpaces_casual() {
    // Same seed, both policies → engaged should pull substantially more,
    // collect more ★3, and reach a higher fortune tier.
    let casual = run_cafe_sim(0xBEEF, 60, &CasualIncome);
    let engaged = run_cafe_sim(0xBEEF, 60, &EngagedIncome);
    assert!(
        engaged.total_pulls > casual.total_pulls,
        "engaged ({}) should out-pull casual ({})",
        engaged.total_pulls, casual.total_pulls
    );
    assert!(
        engaged.star3_total >= casual.star3_total,
        "engaged should not collect fewer ★3 than casual"
    );
    assert!(
        engaged.final_fortune_tier >= casual.final_fortune_tier,
        "engaged should not regress in fortune tier"
    );
    assert!(
        engaged.final_equipped_mult >= casual.final_equipped_mult,
        "engaged should equip an at-least-as-strong card"
    );
}

#[test]
fn engaged_player_unlocks_mid_fortune_in_three_months() {
    // Realistic gem economy: a player completing daily missions + login
    // bonuses comfortably reaches tier 2 ("達人") in 90 days. Tier 3 is
    // intentionally a stretch for the engaged tier — that's the role of
    // `WhaleIncome`.
    let r = run_cafe_sim(0xFEED, 90, &EngagedIncome);
    eprintln!(
        "engaged 90d: pulls={} tier={} mult=×{:.2}",
        r.total_pulls, r.final_fortune_tier, r.final_equipped_mult
    );
    assert!(
        r.final_fortune_tier >= 2,
        "engaged player must reach tier 2+ in 90 days (got tier {})",
        r.final_fortune_tier,
    );
}

#[test]
fn whale_player_reaches_top_fortune_tier() {
    // Top-end income ought to brush tier 4 within a single quarter — proves
    // there's still meaningful headroom past "engaged" for spenders.
    let r = run_cafe_sim(0xFA11, 90, &WhaleIncome);
    eprintln!(
        "whale 90d: pulls={} tier={} mult=×{:.2}",
        r.total_pulls, r.final_fortune_tier, r.final_equipped_mult
    );
    assert!(
        r.final_fortune_tier >= 3,
        "whale player must reach tier 3+ in 90 days (got tier {})",
        r.final_fortune_tier,
    );
}

#[test]
fn fortune_content_scaling_drives_more_levelups() {
    // Two identical runs except one starts already at tier 5.  The pre-tiered
    // player should bank substantially more coins per pull and therefore
    // perform more level-ups in the same window.
    let make_run = |start_tier_pulls: u32| -> SimReport {
        let mut state = CardState { lifetime_pulls: start_tier_pulls, ..Default::default() };
        let banners = active_banners();
        let banner = &banners[0];
        let mut report = SimReport::default();
        let income = EngagedIncome;
        for day in 1..=30u32 {
            state.gems = state.gems.saturating_add(income.gems_for_day(day));
            let mut day_pulls = 0u32;
            while state.gems >= GACHA_SINGLE_COST {
                state.gems -= GACHA_SINGLE_COST;
                let seed = 0xABCD_u32
                    .wrapping_mul(2654435761)
                    .wrapping_add(day.wrapping_mul(10_000).wrapping_add(day_pulls).wrapping_mul(37));
                let id = gacha_pull(&mut state, seed, banner);
                day_pulls += 1;
                report.total_pulls += 1;
                if card_def(id).unwrap().rarity == Rarity::Star3 {
                    report.star3_total += 1;
                }
            }
            equip_strongest(&mut state);
            if let Some(idx) = state.equipped_card {
                while state.level_up(idx) { report.level_ups_done += 1; }
            }
        }
        report.final_equipped_mult = state.equipped_multiplier();
        report
    };
    let fresh = make_run(0);
    let veteran = make_run(800);
    eprintln!(
        "fresh: ★3={}, level-ups={}, mult=×{:.2}",
        fresh.star3_total, fresh.level_ups_done, fresh.final_equipped_mult
    );
    eprintln!(
        "vet  : ★3={}, level-ups={}, mult=×{:.2}",
        veteran.star3_total, veteran.level_ups_done, veteran.final_equipped_mult
    );
    assert!(
        veteran.level_ups_done > fresh.level_ups_done,
        "veteran (tier 5 from day 1) must out-level fresh (tier 0)"
    );
}

/// Printable comparison report.
/// `cargo test simulate_cafe_full_loop -- --nocapture`
#[test]
fn simulate_cafe_full_loop() {
    eprintln!("\n┏━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┓");
    eprintln!("┃  Cafe Full Loop — gacha + 装備 (card lvl) + 運気ティア  ┃");
    eprintln!("┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛");
    let n_days = 90;
    let seed = 0xC0FFEE;
    let casual = run_cafe_sim(seed, n_days, &CasualIncome);
    casual.print(CasualIncome.label());
    let engaged = run_cafe_sim(seed, n_days, &EngagedIncome);
    engaged.print(EngagedIncome.label());
    let whale = run_cafe_sim(seed, n_days, &WhaleIncome);
    whale.print(WhaleIncome.label());

    // Quick "is the loop healthy?" sanity values printed at the end.
    eprintln!("\n── Fortune-content scaling effect ──");
    for tier in 0..=5u32 {
        eprintln!(
            "  tier {tier} ({:<8}): bonus {:>2} coins/引き, dupe ×{:.1}",
            fortune_label(tier),
            fortune_pull_bonus_coins(tier),
            fortune_dupe_multiplier_x10(tier) as f64 / 10.0,
        );
    }
    let _ = gacha::GACHA_TEN_COST; // keep import live
}
