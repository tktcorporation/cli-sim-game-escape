//! Gacha state — owned cards, currencies, pity/spark tracking.

use serde::{Deserialize, Serialize};
use super::cards::{card_def, BonusAxis, Rarity};

/// A card owned by the player.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OwnedCard {
    pub card_id: u32,
    pub level: u32,
    pub rank_ups: u32,
    pub duplicates: u32,
}

impl OwnedCard {
    pub fn new(card_id: u32) -> Self {
        Self { card_id, level: 1, rank_ups: 0, duplicates: 0 }
    }

    /// Effective multiplier = base_rarity + rank_ups*0.2 + (level-1)*0.02
    pub fn multiplier(&self) -> f64 {
        let def = match card_def(self.card_id) {
            Some(d) => d,
            None => return 1.0,
        };
        let base = def.rarity.base_multiplier();
        let rank_bonus = self.rank_ups as f64 * 0.2;
        let level_bonus = (self.level.saturating_sub(1)) as f64 * 0.02;
        base + rank_bonus + level_bonus
    }

    #[allow(dead_code)] // Phase 2+: card enhancement UI
    pub fn max_level(&self) -> u32 {
        let def = match card_def(self.card_id) {
            Some(d) => d,
            None => return 10,
        };
        let base_max = match def.rarity {
            Rarity::Star1 => 10,
            Rarity::Star2 => 20,
            Rarity::Star3 => 30,
        };
        base_max + self.rank_ups * 10
    }

    #[allow(dead_code)] // Phase 2+: card enhancement UI
    pub fn level_up_cost(&self) -> u32 {
        self.level * 10 + 50
    }
}

/// Player's gacha/card collection state.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CardState {
    pub cards: Vec<OwnedCard>,
    /// Gems (premium currency for gacha).
    pub gems: u32,
    /// Coins (basic currency for leveling).
    pub coins: u32,
    /// Consecutive pulls without ★3 (pity).
    pub pity_counter: u32,
    /// Pulls on current banner (for spark).
    pub banner_pulls: u32,
    /// Current banner ID being tracked for spark.
    pub current_banner_id: u32,
    /// Daily draw used today.
    pub daily_draw_used: bool,
    /// JST day of last daily draw.
    pub daily_draw_day: u32,
    /// Currently equipped card index.
    pub equipped_card: Option<usize>,
    /// Lifetime paid gacha pulls (excludes daily draw). Drives the fortune
    /// tier — the more you pull, the higher the base ★3/★2 rate becomes.
    /// New field: defaults to 0 for existing saves via `#[serde(default)]`.
    #[serde(default)]
    pub lifetime_pulls: u32,
}

impl CardState {
    pub fn check_daily_reset(&mut self, jst_day: u32) {
        if jst_day != self.daily_draw_day {
            self.daily_draw_used = false;
            self.daily_draw_day = jst_day;
        }
    }

    pub fn equipped_multiplier(&self) -> f64 {
        self.equipped_card
            .and_then(|idx| self.cards.get(idx))
            .map(|c| c.multiplier())
            .unwrap_or(1.0)
    }

    pub fn equipped_bonus_axis(&self) -> Option<BonusAxis> {
        self.equipped_card
            .and_then(|idx| self.cards.get(idx))
            .and_then(|c| card_def(c.card_id))
            .map(|d| d.bonus_axis)
    }

    /// Add a card. Dupes give coins + character shards.
    /// The duplicate coin reward scales with the fortune tier (derived from
    /// `lifetime_pulls`) so high-tier players also see better content yield,
    /// not just better rates.
    /// Returns (card_def, is_new, shards_given_to_character).
    pub fn add_card(&mut self, card_id: u32) -> Option<&'static super::cards::CardDef> {
        let def = card_def(card_id)?;

        if let Some(owned) = self.cards.iter_mut().find(|c| c.card_id == card_id) {
            owned.duplicates += 1;
            let base = match def.rarity {
                Rarity::Star1 => 10u32,
                Rarity::Star2 => 30,
                Rarity::Star3 => 100,
            };
            let mult_x10 = super::fortune_dupe_multiplier_x10(super::fortune_tier(self.lifetime_pulls));
            self.coins = self.coins.saturating_add(base * mult_x10 / 10);
            // Note: character shards are handled by the caller based on card.character
        } else {
            self.cards.push(OwnedCard::new(card_id));
        }

        if def.rarity == Rarity::Star3 {
            self.pity_counter = 0;
        } else {
            self.pity_counter += 1;
        }

        Some(def)
    }

    /// Level up a card by index.
    #[allow(dead_code)] // Phase 2+: card enhancement UI
    pub fn level_up(&mut self, card_idx: usize) -> bool {
        let card = match self.cards.get(card_idx) {
            Some(c) => c,
            None => return false,
        };
        let cost = card.level_up_cost();
        let max = card.max_level();
        if card.level >= max || self.coins < cost {
            return false;
        }
        self.coins -= cost;
        self.cards[card_idx].level += 1;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn card_multiplier_base() {
        let card = OwnedCard::new(1);
        assert!((card.multiplier() - 1.0).abs() < 0.01);
    }

    #[test]
    fn card_multiplier_leveled() {
        let card = OwnedCard { card_id: 10, level: 5, rank_ups: 0, duplicates: 0 };
        assert!((card.multiplier() - 1.38).abs() < 0.01);
    }

    #[test]
    fn add_duplicate_gives_coins() {
        let mut state = CardState::default();
        state.add_card(1);
        let coins_before = state.coins;
        state.add_card(1);
        assert!(state.coins > coins_before);
    }

    #[test]
    fn level_up_costs_coins() {
        let mut state = CardState::default();
        state.add_card(1);
        state.coins = 1000;
        assert!(state.level_up(0));
        assert_eq!(state.cards[0].level, 2);
        assert!(state.coins < 1000);
    }
}
