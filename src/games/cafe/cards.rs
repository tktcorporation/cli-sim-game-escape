//! Card system with gacha mechanics.
//!
//! Inspired by adv-game-candy's dual-axis card progression:
//! - ★ Rank (1-5): base multiplier 1.0x - 2.0x
//! - Level (1-N): adds +2% per level
//! - Daily draw: 4 cards from weighted pool
//! - Gacha: 120 gems single, 1200 gems 10-pull
//! - Pity system: soft at 70, hard at 200

use serde::{Deserialize, Serialize};

use super::affinity::CharacterId;

// ── Constants ─────────────────────────────────────────────

pub const GACHA_SINGLE_COST: u32 = 120;
pub const GACHA_TEN_COST: u32 = 1200;
pub const DAILY_DRAW_COUNT: u32 = 4;

/// Pity: after this many pulls without ★3, rate increases 2% per pull.
const SOFT_PITY_THRESHOLD: u32 = 70;
/// Hard pity: guaranteed ★3 selection.
const HARD_PITY_THRESHOLD: u32 = 200;

// ── Card Definitions ──────────────────────────────────────

/// Card rarity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Rarity {
    Star1,
    Star2,
    Star3,
}

impl Rarity {
    pub fn label(self) -> &'static str {
        match self {
            Self::Star1 => "★",
            Self::Star2 => "★★",
            Self::Star3 => "★★★",
        }
    }

    /// Base multiplier for affinity gains.
    pub fn base_multiplier(self) -> f64 {
        match self {
            Self::Star1 => 1.0,
            Self::Star2 => 1.3,
            Self::Star3 => 1.8,
        }
    }
}

/// A card definition (static template).
#[derive(Debug, Clone)]
pub struct CardDef {
    pub id: u32,
    pub name: &'static str,
    #[allow(dead_code)] // Used in Phase 3+ character-specific filtering
    pub character: Option<CharacterId>,
    pub rarity: Rarity,
    pub description: &'static str,
    /// Bonus axis: which affinity axis gets extra boost
    pub bonus_axis: BonusAxis,
}

/// Which affinity axis a card boosts.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum BonusAxis {
    Trust,
    Understanding,
    Empathy,
    Balanced,
}

/// All available cards in the game.
pub fn all_cards() -> &'static [CardDef] {
    static CARDS: &[CardDef] = &[
        // ── ★1 Common cards ──
        CardDef {
            id: 1,
            name: "朝のブレンド",
            character: None,
            rarity: Rarity::Star1,
            description: "基本のドリップコーヒー",
            bonus_axis: BonusAxis::Trust,
        },
        CardDef {
            id: 2,
            name: "窓辺の読書",
            character: None,
            rarity: Rarity::Star1,
            description: "静かなひととき",
            bonus_axis: BonusAxis::Understanding,
        },
        CardDef {
            id: 3,
            name: "雨の日の来客",
            character: None,
            rarity: Rarity::Star1,
            description: "傘を忘れた客との会話",
            bonus_axis: BonusAxis::Empathy,
        },
        CardDef {
            id: 4,
            name: "エスプレッソ練習",
            character: None,
            rarity: Rarity::Star1,
            description: "マシンの使い方を覚える",
            bonus_axis: BonusAxis::Balanced,
        },
        CardDef {
            id: 5,
            name: "仕入れの朝",
            character: None,
            rarity: Rarity::Star1,
            description: "市場で食材を選ぶ",
            bonus_axis: BonusAxis::Trust,
        },
        CardDef {
            id: 6,
            name: "掃除の時間",
            character: None,
            rarity: Rarity::Star1,
            description: "店を綺麗に保つ",
            bonus_axis: BonusAxis::Understanding,
        },
        // ── ★2 Uncommon cards ──
        CardDef {
            id: 10,
            name: "佐倉の定位置",
            character: Some(CharacterId::Sakura),
            rarity: Rarity::Star2,
            description: "カウンター端の指定席",
            bonus_axis: BonusAxis::Understanding,
        },
        CardDef {
            id: 11,
            name: "蓮のバイト申請",
            character: Some(CharacterId::Amano),
            rarity: Rarity::Star2,
            description: "元気な助っ人登場",
            bonus_axis: BonusAxis::Empathy,
        },
        CardDef {
            id: 12,
            name: "宮内の昔話",
            character: Some(CharacterId::Miyauchi),
            rarity: Rarity::Star2,
            description: "この店の昔のこと",
            bonus_axis: BonusAxis::Trust,
        },
        CardDef {
            id: 13,
            name: "凛の取材ノート",
            character: Some(CharacterId::Kanzaki),
            rarity: Rarity::Star2,
            description: "小さな記事が繋ぐ縁",
            bonus_axis: BonusAxis::Understanding,
        },
        CardDef {
            id: 14,
            name: "楓の視察",
            character: Some(CharacterId::Kiritani),
            rarity: Rarity::Star2,
            description: "チェーン店マネージャーの目",
            bonus_axis: BonusAxis::Balanced,
        },
        CardDef {
            id: 15,
            name: "手作りスコーン",
            character: None,
            rarity: Rarity::Star2,
            description: "新メニュー開発の第一歩",
            bonus_axis: BonusAxis::Trust,
        },
        // ── ★3 Rare cards ──
        CardDef {
            id: 20,
            name: "月灯りの記憶",
            character: Some(CharacterId::Sakura),
            rarity: Rarity::Star3,
            description: "佐倉が語る、あの日の味",
            bonus_axis: BonusAxis::Empathy,
        },
        CardDef {
            id: 21,
            name: "商店街の絆",
            character: Some(CharacterId::Amano),
            rarity: Rarity::Star3,
            description: "蓮と商店街を歩く午後",
            bonus_axis: BonusAxis::Trust,
        },
        CardDef {
            id: 22,
            name: "レシピノート",
            character: Some(CharacterId::Miyauchi),
            rarity: Rarity::Star3,
            description: "前の店主が残したもの",
            bonus_axis: BonusAxis::Understanding,
        },
        CardDef {
            id: 23,
            name: "書きたい記事",
            character: Some(CharacterId::Kanzaki),
            rarity: Rarity::Star3,
            description: "凛が本当に伝えたいこと",
            bonus_axis: BonusAxis::Empathy,
        },
        CardDef {
            id: 24,
            name: "二つのカフェ",
            character: Some(CharacterId::Kiritani),
            rarity: Rarity::Star3,
            description: "効率じゃない何か",
            bonus_axis: BonusAxis::Balanced,
        },
    ];
    CARDS
}

/// Look up a card definition by ID.
pub fn card_def(id: u32) -> Option<&'static CardDef> {
    all_cards().iter().find(|c| c.id == id)
}

// ── Owned Card Instance ───────────────────────────────────

/// A card owned by the player.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OwnedCard {
    pub card_id: u32,
    /// Current level (1-based).
    pub level: u32,
    /// ★ rank upgrades (0 = base rarity, can be promoted up to +2).
    pub rank_ups: u32,
    /// Duplicate count (for shards).
    pub duplicates: u32,
}

impl OwnedCard {
    pub fn new(card_id: u32) -> Self {
        Self {
            card_id,
            level: 1,
            rank_ups: 0,
            duplicates: 0,
        }
    }

    /// Effective multiplier = base_rarity_mult + rank_ups * 0.2 + (level-1) * 0.02
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

    /// Max level based on rank: base_rarity_max + rank_ups * 10.
    #[allow(dead_code)] // Phase 2+ card enhancement UI
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

    /// Coin cost to level up.
    #[allow(dead_code)] // Phase 2+ card enhancement UI
    pub fn level_up_cost(&self) -> u32 {
        self.level * 10 + 50
    }
}

// ── Gacha State ───────────────────────────────────────────

/// Player's gacha/card collection state.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CardState {
    /// Owned cards.
    pub cards: Vec<OwnedCard>,
    /// Gems (premium currency).
    pub gems: u32,
    /// Coins (basic currency for leveling).
    pub coins: u32,
    /// Consecutive pulls without ★3 (for pity).
    pub pity_counter: u32,
    /// Whether daily draw has been used today.
    pub daily_draw_used: bool,
    /// JST day number of last daily draw.
    pub daily_draw_day: u32,
    /// Currently equipped card index (in self.cards).
    pub equipped_card: Option<usize>,
}

impl CardState {
    /// Check and reset daily draw if new day.
    pub fn check_daily_reset(&mut self, jst_day: u32) {
        if jst_day != self.daily_draw_day {
            self.daily_draw_used = false;
            self.daily_draw_day = jst_day;
        }
    }

    /// Get the equipped card's multiplier.
    pub fn equipped_multiplier(&self) -> f64 {
        self.equipped_card
            .and_then(|idx| self.cards.get(idx))
            .map(|c| c.multiplier())
            .unwrap_or(1.0)
    }

    /// Get the equipped card's bonus axis.
    pub fn equipped_bonus_axis(&self) -> Option<BonusAxis> {
        self.equipped_card
            .and_then(|idx| self.cards.get(idx))
            .and_then(|c| card_def(c.card_id))
            .map(|d| d.bonus_axis)
    }

    /// Add a card from gacha. Returns the card def for display.
    pub fn add_card(&mut self, card_id: u32) -> Option<&'static CardDef> {
        let def = card_def(card_id)?;

        // Check if already owned → duplicate
        if let Some(owned) = self.cards.iter_mut().find(|c| c.card_id == card_id) {
            owned.duplicates += 1;
            // Convert to shards: give coins
            self.coins += match def.rarity {
                Rarity::Star1 => 10,
                Rarity::Star2 => 30,
                Rarity::Star3 => 100,
            };
        } else {
            self.cards.push(OwnedCard::new(card_id));
        }

        // Update pity
        if def.rarity == Rarity::Star3 {
            self.pity_counter = 0;
        } else {
            self.pity_counter += 1;
        }

        Some(def)
    }

    /// Level up a card by index.
    #[allow(dead_code)] // Phase 2+ card enhancement UI
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

// ── Gacha Logic ───────────────────────────────────────────

/// Determine rarity for a single pull based on pity counter.
/// Uses a simple deterministic-ish approach based on seed.
pub fn determine_rarity(pity: u32, seed: u32) -> Rarity {
    if pity >= HARD_PITY_THRESHOLD {
        return Rarity::Star3;
    }

    // Base rates: ★3=2.5%, ★2=18.5%, ★1=79%
    // Soft pity: after threshold, ★3 rate increases 2% per pull
    let star3_rate = if pity >= SOFT_PITY_THRESHOLD {
        25 + (pity - SOFT_PITY_THRESHOLD) * 20 // per-mille
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

/// Select a random card of the given rarity using seed.
pub fn select_card(rarity: Rarity, seed: u32) -> u32 {
    let candidates: Vec<&CardDef> = all_cards()
        .iter()
        .filter(|c| c.rarity == rarity)
        .collect();
    if candidates.is_empty() {
        return 1; // fallback
    }
    let idx = (seed as usize) % candidates.len();
    candidates[idx].id
}

/// Perform a single gacha pull. Returns card ID.
pub fn gacha_pull(state: &mut CardState, seed: u32) -> u32 {
    let rarity = determine_rarity(state.pity_counter, seed);
    let card_id = select_card(rarity, seed / 7 + 13);
    state.add_card(card_id);
    card_id
}

/// Perform daily draw. Returns list of card IDs.
pub fn daily_draw(state: &mut CardState, base_seed: u32) -> Vec<u32> {
    let mut results = Vec::new();
    for i in 0..DAILY_DRAW_COUNT {
        let seed = base_seed.wrapping_mul(2654435761).wrapping_add(i * 37);
        // Daily draw: weighted toward ★1 (Normal 3x, Uncommon 2x, Rare 1x)
        let rarity = {
            let roll = seed % 6;
            match roll {
                0 => Rarity::Star3,       // 1/6
                1 | 2 => Rarity::Star2,   // 2/6
                _ => Rarity::Star1,        // 3/6
            }
        };
        let card_id = select_card(rarity, seed / 11 + 7);
        state.add_card(card_id);
        results.push(card_id);
    }
    state.daily_draw_used = true;
    results
}

// ═══════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn card_multiplier_base() {
        let card = OwnedCard::new(1); // ★1
        assert!((card.multiplier() - 1.0).abs() < 0.01);
    }

    #[test]
    fn card_multiplier_leveled() {
        let card = OwnedCard {
            card_id: 10, // ★2
            level: 5,
            rank_ups: 0,
            duplicates: 0,
        };
        // 1.3 + 0 + 4*0.02 = 1.38
        assert!((card.multiplier() - 1.38).abs() < 0.01);
    }

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
    fn add_duplicate_gives_coins() {
        let mut state = CardState::default();
        state.add_card(1); // first copy
        let coins_before = state.coins;
        state.add_card(1); // duplicate
        assert!(state.coins > coins_before);
    }

    #[test]
    fn level_up_costs_coins() {
        let mut state = CardState::default();
        state.add_card(1);
        state.coins = 1000;
        let success = state.level_up(0);
        assert!(success);
        assert_eq!(state.cards[0].level, 2);
        assert!(state.coins < 1000);
    }
}
