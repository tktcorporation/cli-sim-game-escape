//! Gacha system — BA-style with spark, banners, pity, and lifetime fortune.
//!
//! - Standard banner + Pickup banner
//! - ★3 base rate: 2.5%, ★2: 18.5%, ★1: 79%
//! - Soft pity from 70 pulls (★3 rate +2% per pull)
//! - Hard pity at 200 pulls (guaranteed ★3)
//! - Spark: 200 pulls on same banner → choose any featured
//! - **Fortune system**: lifetime paid pulls grant a 0–5 tier bonus that
//!   permanently increases the base ★3 / ★2 rate. The more you pull, the
//!   higher the floor — at tier 5 (800+ pulls) ★3 is doubled to 5%.

pub mod cards;
pub mod state;

pub use cards::{all_cards, card_def, BonusAxis, CardDef, Rarity};
pub use state::CardState;

// ── Constants ─────────────────────────────────────────────

pub const GACHA_SINGLE_COST: u32 = 120;
pub const GACHA_TEN_COST: u32 = 1200;
pub const DAILY_DRAW_COUNT: u32 = 4;
#[allow(dead_code)] // Phase 2+: spark UI
pub const SPARK_THRESHOLD: u32 = 200;

const SOFT_PITY_THRESHOLD: u32 = 70;
const HARD_PITY_THRESHOLD: u32 = 200;

// ── Reveal animation timing (used by render layer) ───────

/// Frames spent in the "anticipation" phase before the first card reveals.
/// At 10 ticks/sec this is 300ms of build-up.
pub const GACHA_ANIM_ANTICIPATION_FRAMES: u32 = 3;

/// How many cards have been revealed at the given animation frame.
pub fn gacha_anim_revealed(frame: u32, total: usize) -> usize {
    if frame < GACHA_ANIM_ANTICIPATION_FRAMES {
        0
    } else {
        ((frame - GACHA_ANIM_ANTICIPATION_FRAMES + 1) as usize).min(total)
    }
}

/// Frame at which all reveals are complete and OK becomes the dismiss button.
pub fn gacha_anim_complete_frame(total: usize) -> u32 {
    GACHA_ANIM_ANTICIPATION_FRAMES + total as u32
}

pub fn gacha_anim_is_complete(frame: u32, total: usize) -> bool {
    frame >= gacha_anim_complete_frame(total)
}

// ── Fortune (lifetime quality boost) ──────────────────────

/// Tiers 0..=5 derived from lifetime paid pulls. Higher tier = better base rates.
pub fn fortune_tier(lifetime_pulls: u32) -> u32 {
    match lifetime_pulls {
        0..=49 => 0,
        50..=149 => 1,
        150..=299 => 2,
        300..=499 => 3,
        500..=799 => 4,
        _ => 5,
    }
}

pub fn fortune_label(tier: u32) -> &'static str {
    match tier {
        0 => "新人",
        1 => "見習い",
        2 => "達人",
        3 => "幸運使い",
        4 => "宿命",
        _ => "超越",
    }
}

/// Lifetime pulls needed to reach the next tier, or `None` if maxed out.
pub fn next_fortune_threshold(tier: u32) -> Option<u32> {
    match tier {
        0 => Some(50),
        1 => Some(150),
        2 => Some(300),
        3 => Some(500),
        4 => Some(800),
        _ => None,
    }
}

/// Bonus to ★3 base rate, in per-mille (1/1000). 0..=25 → +0% .. +2.5%.
fn fortune_star3_bonus_per_mille(tier: u32) -> u32 {
    tier * 5
}

/// Bonus to ★2 base rate, in per-mille. 0..=50 → +0% .. +5%.
fn fortune_star2_bonus_per_mille(tier: u32) -> u32 {
    tier * 10
}

/// Free coins handed out **on every paid pull** based on fortune tier.
/// Scales the felt value of pulling at higher tiers — higher tier players
/// don't just see better rates, they also bank more upgrade currency per pull.
pub fn fortune_pull_bonus_coins(tier: u32) -> u32 {
    [0u32, 5, 12, 20, 30, 50][tier.min(5) as usize]
}

/// Multiplier (×10 for integer math) applied to the duplicate-coin reward.
/// Tier 0: ×1.0, tier 5: ×2.0 — collecting dupes is dramatically more
/// valuable for veteran players.
pub fn fortune_dupe_multiplier_x10(tier: u32) -> u32 {
    [10u32, 11, 12, 14, 16, 20][tier.min(5) as usize]
}

// ── Banner System ─────────────────────────────────────────

/// A gacha banner.
#[derive(Debug, Clone)]
#[allow(dead_code)] // Phase 2+: banner selection UI
pub struct Banner {
    pub id: u32,
    pub name: &'static str,
    /// Featured ★3 card IDs (rate-up).
    pub featured_ids: &'static [u32],
    /// Whether this is the permanent standard banner.
    pub is_standard: bool,
}

/// Current available banners.
pub fn active_banners() -> Vec<Banner> {
    vec![
        Banner {
            id: 0,
            name: "通常募集",
            featured_ids: &[],
            is_standard: true,
        },
        Banner {
            id: 1,
            name: "月灯りピックアップ",
            featured_ids: &[20, 22], // 月灯りの記憶, レシピノート
            is_standard: false,
        },
    ]
}

// ── Gacha Logic ───────────────────────────────────────────

/// Determine rarity for a single pull, factoring in pity and the lifetime
/// fortune tier.  `lifetime_pulls` is the player's total paid pull count
/// **before** this pull — soft/hard pity still trump everything.
pub fn determine_rarity(pity: u32, lifetime_pulls: u32, seed: u32) -> Rarity {
    if pity >= HARD_PITY_THRESHOLD {
        return Rarity::Star3;
    }

    let tier = fortune_tier(lifetime_pulls);
    let star3_base = 25 + fortune_star3_bonus_per_mille(tier);
    let star2_base = 185 + fortune_star2_bonus_per_mille(tier);

    let star3_rate = if pity >= SOFT_PITY_THRESHOLD {
        star3_base + (pity - SOFT_PITY_THRESHOLD) * 20
    } else {
        star3_base
    };
    let star3_rate = star3_rate.min(1000);
    // ★2 sits on top of ★3; clamp so the cumulative window never exceeds 100%.
    let star2_rate = star2_base.min(1000 - star3_rate);

    let roll = seed % 1000;
    if roll < star3_rate {
        Rarity::Star3
    } else if roll < star3_rate + star2_rate {
        Rarity::Star2
    } else {
        Rarity::Star1
    }
}

/// Select a card of given rarity, with rate-up for featured cards.
pub fn select_card(rarity: Rarity, seed: u32, featured_ids: &[u32]) -> u32 {
    let candidates: Vec<&CardDef> = all_cards()
        .iter()
        .filter(|c| c.rarity == rarity)
        .collect();
    if candidates.is_empty() {
        return 1;
    }

    // Rate-up: 50% chance to get featured card if any match this rarity
    let featured_of_rarity: Vec<&&CardDef> = candidates
        .iter()
        .filter(|c| featured_ids.contains(&c.id))
        .collect();

    if !featured_of_rarity.is_empty() && (seed / 3).is_multiple_of(2) {
        // Rate-up hit: pick from featured
        let idx = (seed as usize / 7) % featured_of_rarity.len();
        return featured_of_rarity[idx].id;
    }

    // Normal selection
    let idx = (seed as usize) % candidates.len();
    candidates[idx].id
}

/// Perform a single gacha pull on a banner. Returns card ID.
///
/// Side effects beyond returning the card:
/// - `add_card` records the pull (with fortune-scaled dupe coins).
/// - The "fortune pull bonus" coin is added on top of any dupe payout.
/// - `banner_pulls` (spark) and `lifetime_pulls` (fortune) both advance.
pub fn gacha_pull(state: &mut CardState, seed: u32, banner: &Banner) -> u32 {
    // Tier is captured *before* the pull so the bonus matches what the UI
    // showed before the player tapped.
    let tier_for_pull = fortune_tier(state.lifetime_pulls);
    let rarity = determine_rarity(state.pity_counter, state.lifetime_pulls, seed);
    let card_id = select_card(rarity, seed / 7 + 13, banner.featured_ids);
    state.add_card(card_id);
    let bonus = fortune_pull_bonus_coins(tier_for_pull);
    if bonus > 0 {
        state.coins = state.coins.saturating_add(bonus);
    }
    state.banner_pulls += 1; // Track for spark
    state.lifetime_pulls = state.lifetime_pulls.saturating_add(1); // Fortune
    card_id
}

/// Perform daily draw. Returns list of card IDs.
pub fn daily_draw(state: &mut CardState, base_seed: u32) -> Vec<u32> {
    let mut results = Vec::new();
    for i in 0..DAILY_DRAW_COUNT {
        let seed = base_seed.wrapping_mul(2654435761).wrapping_add(i * 37);
        let rarity = {
            let roll = seed % 6;
            match roll {
                0 => Rarity::Star3,
                1 | 2 => Rarity::Star2,
                _ => Rarity::Star1,
            }
        };
        let card_id = select_card(rarity, seed / 11 + 7, &[]);
        state.add_card(card_id);
        results.push(card_id);
    }
    state.daily_draw_used = true;
    results
}

/// Check if player can spark (choose a featured card).
#[allow(dead_code)] // Phase 2+: spark selection UI
pub fn can_spark(state: &CardState) -> bool {
    state.banner_pulls >= SPARK_THRESHOLD
}

/// Execute spark: pick a card from featured list.
#[allow(dead_code)] // Phase 2+: spark selection UI
pub fn execute_spark(state: &mut CardState, card_id: u32) {
    state.add_card(card_id);
    state.banner_pulls = 0; // Reset spark counter
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gacha_pity_guarantees_star3() {
        let rarity = determine_rarity(HARD_PITY_THRESHOLD, 0, 999);
        assert_eq!(rarity, Rarity::Star3);
    }

    #[test]
    fn fortune_tier_thresholds_are_monotonic() {
        let samples = [(0, 0), (49, 0), (50, 1), (149, 1), (150, 2), (299, 2),
                       (300, 3), (499, 3), (500, 4), (799, 4), (800, 5),
                       (5000, 5)];
        for (pulls, expected) in samples {
            assert_eq!(
                fortune_tier(pulls),
                expected,
                "fortune_tier({pulls}) should be {expected}"
            );
        }
    }

    #[test]
    fn fortune_thresholds_match_tiers() {
        // next_fortune_threshold should match the boundary that bumps the tier.
        for tier in 0..5 {
            let next = next_fortune_threshold(tier).expect("tier < 5 has next");
            assert_eq!(fortune_tier(next - 1), tier, "tier {tier} edge");
            assert_eq!(fortune_tier(next), tier + 1, "tier {tier} bump");
        }
        assert_eq!(next_fortune_threshold(5), None);
    }

    #[test]
    fn gacha_pull_increments_lifetime_pulls() {
        let mut state = CardState::default();
        let banner = Banner { id: 0, name: "test", featured_ids: &[], is_standard: true };
        gacha_pull(&mut state, 42, &banner);
        gacha_pull(&mut state, 43, &banner);
        gacha_pull(&mut state, 44, &banner);
        assert_eq!(state.lifetime_pulls, 3);
    }

    #[test]
    fn fortune_bonus_curves_are_monotonic() {
        let mut prev_coins = 0u32;
        let mut prev_dupe = 0u32;
        for tier in 0..=5 {
            let coins = fortune_pull_bonus_coins(tier);
            let dupe = fortune_dupe_multiplier_x10(tier);
            assert!(coins >= prev_coins, "pull bonus must not regress at tier {tier}");
            assert!(dupe >= prev_dupe, "dupe mult must not regress at tier {tier}");
            prev_coins = coins;
            prev_dupe = dupe;
        }
        // Floor and ceiling sanity.
        assert_eq!(fortune_pull_bonus_coins(0), 0);
        assert_eq!(fortune_dupe_multiplier_x10(0), 10);
        assert_eq!(fortune_pull_bonus_coins(5), 50);
        assert_eq!(fortune_dupe_multiplier_x10(5), 20);
    }

    #[test]
    fn pull_bonus_coins_are_credited_per_pull() {
        // Jump straight to tier 5 so the bonus is non-zero and observable.
        let mut state = CardState { lifetime_pulls: 800, ..Default::default() };
        let coins_before = state.coins;
        let banner = Banner { id: 0, name: "test", featured_ids: &[], is_standard: true };
        gacha_pull(&mut state, 1, &banner);
        // At least the pull bonus should have landed (dupe may add more on top).
        assert!(
            state.coins >= coins_before + fortune_pull_bonus_coins(5),
            "tier 5 pull should add ≥{} bonus coins",
            fortune_pull_bonus_coins(5),
        );
    }

    #[test]
    fn dupe_coins_scale_with_fortune() {
        // Pull the same ★1 card twice at tier 0 vs tier 5 and compare the
        // dupe coin reward — tier 5 should give noticeably more.
        let banner = Banner { id: 0, name: "test", featured_ids: &[], is_standard: true };

        let mut tier0 = CardState::default();
        // Force-add a known card so the next pull is guaranteed a dupe.
        tier0.cards.push(state::OwnedCard::new(1));
        tier0.add_card(1);
        let dupe0 = tier0.coins;

        let mut tier5 = CardState { lifetime_pulls: 800, ..Default::default() };
        tier5.cards.push(state::OwnedCard::new(1));
        tier5.add_card(1);
        let dupe5 = tier5.coins;

        let _ = banner; // keeps the doc-comment example consistent
        assert!(
            dupe5 > dupe0,
            "tier 5 dupe coins ({dupe5}) should exceed tier 0 ({dupe0})"
        );
    }

    #[test]
    fn anim_revealed_progresses() {
        // Anticipation phase: nothing revealed.
        for f in 0..GACHA_ANIM_ANTICIPATION_FRAMES {
            assert_eq!(gacha_anim_revealed(f, 10), 0, "frame {f} should be in anticipation");
        }
        // First card reveals exactly at the anticipation boundary.
        assert_eq!(gacha_anim_revealed(GACHA_ANIM_ANTICIPATION_FRAMES, 10), 1);
        // After enough frames everything is revealed.
        let complete = gacha_anim_complete_frame(10);
        assert_eq!(gacha_anim_revealed(complete, 10), 10);
        assert!(gacha_anim_is_complete(complete, 10));
        assert!(!gacha_anim_is_complete(complete - 1, 10));
    }

    #[test]
    fn daily_draw_returns_four() {
        let mut state = CardState::default();
        let results = daily_draw(&mut state, 42);
        assert_eq!(results.len(), 4);
        assert!(state.daily_draw_used);
    }

    #[test]
    fn spark_after_200_pulls() {
        let mut state = CardState::default();
        state.banner_pulls = 200;
        assert!(can_spark(&state));
    }

    #[test]
    fn spark_resets_counter() {
        let mut state = CardState::default();
        state.banner_pulls = 200;
        execute_spark(&mut state, 20);
        assert_eq!(state.banner_pulls, 0);
    }

    #[test]
    fn featured_rate_up() {
        // With featured IDs, at least some pulls should hit featured
        let featured = &[20u32]; // Star3 card
        let mut featured_count = 0;
        for seed in 0..100u32 {
            let id = select_card(Rarity::Star3, seed, featured);
            if id == 20 {
                featured_count += 1;
            }
        }
        assert!(featured_count > 30); // Should be roughly 50%
    }
}

// ───────────────────────────────────────────────────────────
// Simulator — gacha balance verification.
// ───────────────────────────────────────────────────────────
//
// The fortune system claims that "more pulls → better quality".  The unit
// tests below verify that claim numerically: simulate thousands of pulls
// across many seeds, and assert the ★3 rate climbs as the lifetime counter
// grows.  Run the printable balance report with:
//
//     cargo test simulate_gacha_balance_curve -- --nocapture
//
// This pattern mirrors `abyss::simulator` so the maintenance story is uniform
// across games.

#[cfg(test)]
mod simulator {
    use super::*;

    /// Per-tier rarity counts collected during a run.
    #[derive(Default)]
    struct TierStats {
        pulls: u64,
        star3: u64,
        star2: u64,
        star1: u64,
    }

    impl TierStats {
        fn record(&mut self, r: Rarity) {
            self.pulls += 1;
            match r {
                Rarity::Star3 => self.star3 += 1,
                Rarity::Star2 => self.star2 += 1,
                Rarity::Star1 => self.star1 += 1,
            }
        }
        fn star3_pct(&self) -> f64 {
            if self.pulls == 0 { 0.0 } else { self.star3 as f64 / self.pulls as f64 * 100.0 }
        }
        fn star2_pct(&self) -> f64 {
            if self.pulls == 0 { 0.0 } else { self.star2 as f64 / self.pulls as f64 * 100.0 }
        }
        fn star1_pct(&self) -> f64 {
            if self.pulls == 0 { 0.0 } else { self.star1 as f64 / self.pulls as f64 * 100.0 }
        }
    }

    /// Run `pulls` paid pulls with `seed`, returning the rarity buckets per
    /// fortune tier the player passed through during the run. The pity
    /// counter is shared across the whole run (matches in-game behavior).
    fn simulate_run(seed: u32, pulls: u32) -> [TierStats; 6] {
        let mut state = CardState::default();
        let banners = active_banners();
        let banner = &banners[0]; // Standard
        let mut buckets: [TierStats; 6] = Default::default();
        for i in 0..pulls {
            let pull_seed = seed
                .wrapping_mul(2654435761)
                .wrapping_add(i.wrapping_mul(37));
            // Tier *before* the pull is what determines the rate, matching
            // `gacha_pull` semantics.
            let tier = fortune_tier(state.lifetime_pulls) as usize;
            let id = gacha_pull(&mut state, pull_seed, banner);
            let r = card_def(id).expect("card def exists").rarity;
            buckets[tier].record(r);
        }
        buckets
    }

    #[test]
    fn higher_fortune_tier_yields_more_star3() {
        // Simulate the whole 0..=tier5 trajectory for 50 seeds × 1000 pulls.
        // Expectation: the per-tier ★3 rate rises monotonically across tiers
        // (within sampling noise).
        let mut agg: [TierStats; 6] = Default::default();
        for seed in 1..=50u32 {
            let buckets = simulate_run(seed, 1000);
            for (i, b) in buckets.iter().enumerate() {
                agg[i].pulls += b.pulls;
                agg[i].star3 += b.star3;
                agg[i].star2 += b.star2;
                agg[i].star1 += b.star1;
            }
        }

        // Tier 5 should pull ★3 noticeably more often than tier 0.  We
        // include pity-driven ★3 rolls in both buckets, so the absolute
        // observed rate is higher than 2.5%/5%; what matters is the
        // *direction*.  Assert at least +30% relative improvement.
        let t0 = agg[0].star3_pct();
        let t5 = agg[5].star3_pct();
        eprintln!("tier 0 pulls={} ★3={:.2}%  tier 5 pulls={} ★3={:.2}%",
            agg[0].pulls, t0, agg[5].pulls, t5);
        assert!(agg[0].pulls > 1000, "tier 0 should have plenty of samples");
        assert!(agg[5].pulls > 1000, "tier 5 should have plenty of samples");
        assert!(
            t5 > t0 * 1.3,
            "fortune tier 5 ★3 rate ({:.2}%) should be ≥30% higher than tier 0 ({:.2}%)",
            t5, t0
        );
    }

    #[test]
    fn ten_pull_value_increases_with_fortune() {
        // Ensure that a player who already has 800+ lifetime pulls gets a
        // visibly better-than-average 10-pull on average than a fresh player.
        let trials = 200;
        let mut fresh_star3 = 0u32;
        let mut veteran_star3 = 0u32;
        let banners = active_banners();
        let banner = &banners[0];
        for s in 0..trials {
            let mut fresh = CardState::default();
            let mut veteran = CardState { lifetime_pulls: 1_000, ..Default::default() };
            let base_seed = (s as u32).wrapping_mul(2654435761);
            for i in 0..10u32 {
                let seed = base_seed.wrapping_add(i * 37);
                let id_f = gacha_pull(&mut fresh, seed, banner);
                let id_v = gacha_pull(&mut veteran, seed, banner);
                if card_def(id_f).unwrap().rarity == Rarity::Star3 { fresh_star3 += 1; }
                if card_def(id_v).unwrap().rarity == Rarity::Star3 { veteran_star3 += 1; }
            }
        }
        eprintln!("fresh ★3/2000 = {} ({:.2}%) | veteran ★3/2000 = {} ({:.2}%)",
            fresh_star3, fresh_star3 as f64 / 20.0,
            veteran_star3, veteran_star3 as f64 / 20.0);
        assert!(
            veteran_star3 > fresh_star3,
            "veteran (lifetime=1000) should pull more ★3 than fresh (lifetime=0)"
        );
    }

    /// Printable report. `cargo test simulate_gacha_balance_curve -- --nocapture`
    #[test]
    fn simulate_gacha_balance_curve() {
        let seeds = 50u32;
        let pulls_per_seed = 1500u32;
        eprintln!("\n┏━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┓");
        eprintln!("┃  Cafe Gacha Balance — fortune tier × {seeds} seeds × {pulls_per_seed} pulls  ┃");
        eprintln!("┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛");

        let mut agg: [TierStats; 6] = Default::default();
        for seed in 1..=seeds {
            let buckets = simulate_run(seed, pulls_per_seed);
            for (i, b) in buckets.iter().enumerate() {
                agg[i].pulls += b.pulls;
                agg[i].star3 += b.star3;
                agg[i].star2 += b.star2;
                agg[i].star1 += b.star1;
            }
        }

        eprintln!(
            "{:<6}{:<10}{:>8}{:>10}{:>10}{:>10}",
            "tier", "label", "pulls", "★3%", "★2%", "★1%"
        );
        eprintln!("{}", "─".repeat(56));
        for tier in 0..6u32 {
            let s = &agg[tier as usize];
            eprintln!(
                "{:<6}{:<10}{:>8}{:>9.2}%{:>9.2}%{:>9.2}%",
                tier,
                fortune_label(tier),
                s.pulls,
                s.star3_pct(),
                s.star2_pct(),
                s.star1_pct(),
            );
        }
        eprintln!("\n(★3% includes pity-forced ★3, so all tiers exceed their\n base rate. The relative improvement across tiers is what matters.)");
    }

    /// Verify the pure base-rate behavior (pity disabled) — sanity check that
    /// the configured per-mille bonuses are actually applied.
    #[test]
    fn base_rate_curve_no_pity() {
        let trials = 20_000u32;
        eprintln!("\n── Base-rate curve (pity disabled) ──");
        let mut prev_rate = 0.0f64;
        for tier in 0..6u32 {
            let lifetime_for_tier = match tier {
                0 => 0, 1 => 50, 2 => 150, 3 => 300, 4 => 500, _ => 800,
            };
            let mut s3 = 0u32;
            let mut s2 = 0u32;
            for s in 0..trials {
                match determine_rarity(0, lifetime_for_tier, s) {
                    Rarity::Star3 => s3 += 1,
                    Rarity::Star2 => s2 += 1,
                    Rarity::Star1 => {}
                }
            }
            let r3 = s3 as f64 / trials as f64 * 100.0;
            let r2 = s2 as f64 / trials as f64 * 100.0;
            eprintln!(
                "  tier {tier} ({:>6} pulls, {:<8}): ★3={:.2}%  ★2={:.2}%",
                lifetime_for_tier, fortune_label(tier), r3, r2
            );
            assert!(
                r3 + 0.05 >= prev_rate,
                "★3 rate must not regress between tiers (tier {tier}: {r3:.2}% vs prev {prev_rate:.2}%)"
            );
            prev_rate = r3;
        }
    }
}
