//! Gacha system — BA-style with spark, banners, pity.
//!
//! - Standard banner + Pickup banner
//! - ★3 rate: 2.5% (pickup 0.7%), ★2: 18.5%, ★1: 79%
//! - Soft pity from 70 pulls (★3 rate +2% per pull)
//! - Hard pity at 200 pulls (guaranteed ★3)
//! - Spark: 200 pulls on same banner → choose any featured

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

/// Determine rarity for a single pull based on pity counter.
pub fn determine_rarity(pity: u32, seed: u32) -> Rarity {
    if pity >= HARD_PITY_THRESHOLD {
        return Rarity::Star3;
    }

    // Base rates: ★3=2.5%, ★2=18.5%, ★1=79% (per-mille)
    let star3_rate = if pity >= SOFT_PITY_THRESHOLD {
        25 + (pity - SOFT_PITY_THRESHOLD) * 20
    } else {
        25
    };
    let star3_rate = star3_rate.min(1000);

    let roll = seed % 1000;
    if roll < star3_rate {
        Rarity::Star3
    } else if roll < star3_rate + 185 {
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
pub fn gacha_pull(state: &mut CardState, seed: u32, banner: &Banner) -> u32 {
    let rarity = determine_rarity(state.pity_counter, seed);
    let card_id = select_card(rarity, seed / 7 + 13, banner.featured_ids);
    state.add_card(card_id);
    state.banner_pulls += 1; // Track for spark
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
        let rarity = determine_rarity(HARD_PITY_THRESHOLD, 999);
        assert_eq!(rarity, Rarity::Star3);
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
