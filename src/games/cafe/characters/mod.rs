//! Character system — levels, stars, shards, skills, affinity.
//!
//! Blue Archive-inspired character progression:
//! - Character level (1-80) via EXP
//! - Star rank (★1-★5) via character shards
//! - 3 skills per character
//! - 3-axis affinity (Trust/Understanding/Empathy)

pub mod affinity;
pub mod episodes;
pub mod skills;

use serde::{Deserialize, Serialize};

// ── Character IDs ────────────────────────────────────────

/// Identifiers for regular customers (常連客).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CharacterId {
    Sakura,
    Amano,
    Miyauchi,
    Kanzaki,
    Kiritani,
}

impl CharacterId {
    pub const ALL: &[CharacterId] = &[
        CharacterId::Sakura,
        CharacterId::Amano,
        CharacterId::Miyauchi,
        CharacterId::Kanzaki,
        CharacterId::Kiritani,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Sakura => "佐倉 美月",
            Self::Amano => "天野 蓮",
            Self::Miyauchi => "宮内 孝之",
            Self::Kanzaki => "神崎 凛",
            Self::Kiritani => "桐谷 楓",
        }
    }

    pub fn short_name(self) -> &'static str {
        match self {
            Self::Sakura => "佐倉",
            Self::Amano => "天野",
            Self::Miyauchi => "宮内",
            Self::Kanzaki => "神崎",
            Self::Kiritani => "桐谷",
        }
    }

    /// Chapter at which this character is unlocked.
    pub fn unlock_chapter(self) -> u32 {
        match self {
            Self::Sakura => 0,
            Self::Amano => 1,
            Self::Miyauchi => 1,
            Self::Kanzaki => 2,
            Self::Kiritani => 3,
        }
    }

    /// Base star rarity (gacha rarity when first obtained).
    pub fn base_stars(self) -> u32 {
        match self {
            Self::Sakura => 1,   // free starter
            Self::Amano => 1,    // story unlock
            Self::Miyauchi => 1, // story unlock
            Self::Kanzaki => 2,  // later unlock, higher base
            Self::Kiritani => 2, // later unlock, higher base
        }
    }
}

// ── Character Data (per-character persistent state) ──────

/// Per-character progression state (saved).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CharacterData {
    /// Character level (1-80).
    pub level: u32,
    /// Current EXP toward next level.
    pub exp: u32,
    /// Star rank (1-5). Starts at base_stars.
    pub stars: u32,
    /// Character shards accumulated (for star promotion).
    pub shards: u32,
    /// Whether unlocked (can interact).
    pub unlocked: bool,
    /// Skill levels [skill1, skill2, skill3].
    pub skill_levels: [u32; 3],
}

impl Default for CharacterData {
    fn default() -> Self {
        Self {
            level: 1,
            exp: 0,
            stars: 1,
            shards: 0,
            unlocked: false,
            skill_levels: [1, 0, 0], // Skill 1 starts at 1, others locked
        }
    }
}

impl CharacterData {
    /// Create with specific base stars (for character unlock).
    pub fn with_stars(stars: u32) -> Self {
        Self {
            stars,
            ..Default::default()
        }
    }

    /// EXP required for next level.
    pub fn exp_to_next_level(&self) -> u32 {
        // BA-style: 10 + level * 8 + (level/10) * 20
        10 + self.level * 8 + (self.level / 10) * 20
    }

    /// Add EXP and handle level ups. Returns levels gained.
    pub fn add_exp(&mut self, amount: u32) -> u32 {
        let cap = self.level_cap();
        self.exp += amount;
        let mut levels = 0;
        while self.level < cap {
            let needed = self.exp_to_next_level();
            if self.exp >= needed {
                self.exp -= needed;
                self.level += 1;
                levels += 1;
            } else {
                break;
            }
        }
        if self.level >= cap {
            self.exp = 0;
        }
        levels
    }

    /// Shards needed to promote to next star.
    pub fn shards_to_promote(&self) -> Option<u32> {
        match self.stars {
            1 => Some(10),
            2 => Some(30),
            3 => Some(80),
            4 => Some(120),
            _ => None, // already max or invalid
        }
    }

    /// Try to promote star rank. Returns true if successful.
    pub fn try_promote(&mut self) -> bool {
        if let Some(cost) = self.shards_to_promote() {
            if self.shards >= cost && self.stars < 5 {
                self.shards -= cost;
                self.stars += 1;
                // Unlock next skill on star promotion
                match self.stars {
                    3 => self.skill_levels[1] = 1, // Unlock skill 2 at ★3
                    5 => self.skill_levels[2] = 1, // Unlock skill 3 at ★5
                    _ => {}
                }
                return true;
            }
        }
        false
    }

    /// Level cap based on star rank.
    pub fn level_cap(&self) -> u32 {
        match self.stars {
            1 => 20,
            2 => 40,
            3 => 60,
            4 => 70,
            5 => 80,
            _ => 80,
        }
    }

    /// Total stat bonus from level.
    pub fn level_bonus(&self) -> u32 {
        self.level / 5
    }
}

// ── Action Types ─────────────────────────────────────────

/// Action types that can be performed on characters.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum ActionType {
    Eat,
    Observe,
    Talk,
    Special,
}

impl ActionType {
    pub fn name(self) -> &'static str {
        match self {
            Self::Eat => "食事",
            Self::Observe => "観察",
            Self::Talk => "会話",
            Self::Special => "特別",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Eat => "メニューを提供する",
            Self::Observe => "さりげなく様子を見る",
            Self::Talk => "言葉を交わす",
            Self::Special => "特別なサービスをする",
        }
    }

    /// AP cost for this action.
    pub fn ap_cost(self) -> u32 {
        match self {
            Self::Special => 2,
            _ => 1,
        }
    }
}

// ═══════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn character_level_up() {
        let mut ch = CharacterData::default();
        ch.stars = 2; // cap = 40
        let levels = ch.add_exp(500);
        assert!(levels > 0);
        assert!(ch.level > 1);
    }

    #[test]
    fn character_level_cap() {
        let mut ch = CharacterData::default();
        ch.stars = 1; // cap = 20
        ch.add_exp(100_000);
        assert_eq!(ch.level, ch.level_cap());
    }

    #[test]
    fn star_promotion() {
        let mut ch = CharacterData::default();
        ch.shards = 15;
        assert!(ch.try_promote()); // 1→2, costs 10
        assert_eq!(ch.stars, 2);
        assert_eq!(ch.shards, 5);
    }

    #[test]
    fn star_promotion_insufficient_shards() {
        let mut ch = CharacterData::default();
        ch.shards = 5;
        assert!(!ch.try_promote()); // needs 10
        assert_eq!(ch.stars, 1);
    }

    #[test]
    fn skill_unlock_on_promotion() {
        let mut ch = CharacterData::default();
        ch.shards = 200; // enough for multiple promotions
        ch.try_promote(); // 1→2
        assert_eq!(ch.skill_levels[1], 0); // not yet
        ch.try_promote(); // 2→3
        assert_eq!(ch.skill_levels[1], 1); // unlocked!
    }
}
