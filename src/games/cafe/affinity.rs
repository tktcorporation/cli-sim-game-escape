//! Character affinity (親密度) system.
//!
//! Inspired by adv-game-candy's dual-axis model:
//! - Three axes: Trust (信頼), Understanding (理解), Empathy (共感)
//! - Combined value = Affection Level (好感度)
//! - Quadratic growth: N² × 5 points per level
//! - Unlocks bond stories and cards at milestones

use serde::{Deserialize, Serialize};

// ── Characters ───────────────────────────────────────────

/// Identifiers for regular customers (常連客).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CharacterId {
    /// 佐倉 美月 — freelance proofreader, former pastry shop owner
    Sakura,
    /// 天野 蓮 — university student, cheerful
    Amano,
    /// 宮内 孝之 — old bookstore owner, former regular
    Miyauchi,
    /// 神崎 凛 — local newspaper reporter
    Kanzaki,
    /// 桐谷 楓 — chain café manager
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

    #[allow(dead_code)] // Used in compact UI layouts
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
            Self::Sakura => 0,  // appears in Ch.0
            Self::Amano => 1,
            Self::Miyauchi => 1,
            Self::Kanzaki => 2,
            Self::Kiritani => 3,
        }
    }
}

// ── Affinity Axes ────────────────────────────────────────

/// Three-axis affinity values for a single character.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AffinityAxes {
    /// 信頼 — built through consistent service and reliability
    pub trust: u32,
    /// 理解 — built through observing and learning about the character
    pub understanding: u32,
    /// 共感 — built through emotional conversations and shared experiences
    pub empathy: u32,
}

impl AffinityAxes {
    /// Combined affection points (total of all axes).
    pub fn total(&self) -> u32 {
        self.trust + self.understanding + self.empathy
    }

    /// Affection level derived from total points.
    /// Uses quadratic growth: level N requires N² × 5 cumulative points.
    pub fn level(&self) -> u32 {
        let total = self.total();
        // Solve: N² × 5 ≤ total → N ≤ sqrt(total / 5)
        ((total as f64 / 5.0).sqrt()) as u32
    }

    /// Points needed for the next level.
    pub fn points_to_next_level(&self) -> u32 {
        let next = self.level() + 1;
        let required = next * next * 5;
        required.saturating_sub(self.total())
    }

    /// Star rank (★1-5) based on affection level.
    pub fn star_rank(&self) -> u32 {
        match self.level() {
            0..=2 => 1,
            3..=5 => 2,
            6..=9 => 3,
            10..=14 => 4,
            _ => 5,
        }
    }
}

/// Affinity state for a single character.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CharacterAffinity {
    pub axes: AffinityAxes,
    /// Whether this character is unlocked.
    pub unlocked: bool,
    /// Episodes (bond stories) that have been viewed.
    pub viewed_episodes: Vec<u32>,
}

/// Gains from an action, before card multiplier.
#[derive(Debug, Clone, Copy)]
pub struct AffinityGain {
    pub trust: u32,
    pub understanding: u32,
    pub empathy: u32,
}

impl AffinityGain {
    /// Apply a multiplier (from card rank).
    pub fn multiply(self, mult: f64) -> Self {
        Self {
            trust: (self.trust as f64 * mult) as u32,
            understanding: (self.understanding as f64 * mult) as u32,
            empathy: (self.empathy as f64 * mult) as u32,
        }
    }
}

/// Action types that can be performed on characters.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum ActionType {
    /// 食事 — serve food/drink; primary: trust
    Eat,
    /// 観察 — observe behavior; primary: understanding
    Observe,
    /// 会話 — have a conversation; primary: empathy
    Talk,
    /// 特別 — special action (unlocked at higher affinity); balanced
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

    /// Base affinity gains for this action type.
    pub fn base_gains(self) -> AffinityGain {
        match self {
            Self::Eat => AffinityGain {
                trust: 15,
                understanding: 5,
                empathy: 5,
            },
            Self::Observe => AffinityGain {
                trust: 5,
                understanding: 15,
                empathy: 5,
            },
            Self::Talk => AffinityGain {
                trust: 5,
                understanding: 5,
                empathy: 15,
            },
            Self::Special => AffinityGain {
                trust: 10,
                understanding: 10,
                empathy: 10,
            },
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

// ── Episode Unlock Conditions ─────────────────────────────

/// Bond story episodes available per character.
#[allow(dead_code)] // Phase 3+ episode system
pub fn available_episodes(character: CharacterId, affinity: &CharacterAffinity) -> Vec<Episode> {
    let level = affinity.axes.level();
    let mut eps = Vec::new();

    let defs = episode_definitions(character);
    for ep in defs {
        if level >= ep.required_level && !affinity.viewed_episodes.contains(&ep.id) {
            eps.push(ep);
        }
    }
    eps
}

/// An episode (bond story) definition.
#[allow(dead_code)] // Phase 3+ episode system
#[derive(Debug, Clone)]
pub struct Episode {
    pub id: u32,
    pub title: &'static str,
    pub required_level: u32,
}

#[allow(dead_code)] // Phase 3+ episode system
fn episode_definitions(character: CharacterId) -> Vec<Episode> {
    match character {
        CharacterId::Sakura => vec![
            Episode { id: 1, title: "静かな読書", required_level: 2 },
            Episode { id: 2, title: "昔のお店", required_level: 5 },
            Episode { id: 3, title: "もう一度、菓子を", required_level: 8 },
            Episode { id: 4, title: "佐倉の選択", required_level: 12 },
        ],
        CharacterId::Amano => vec![
            Episode { id: 1, title: "バイト志願", required_level: 2 },
            Episode { id: 2, title: "実家の金物屋", required_level: 5 },
            Episode { id: 3, title: "商店街の未来", required_level: 8 },
            Episode { id: 4, title: "蓮の答え", required_level: 12 },
        ],
        CharacterId::Miyauchi => vec![
            Episode { id: 1, title: "古い常連", required_level: 2 },
            Episode { id: 2, title: "前の店主のこと", required_level: 5 },
            Episode { id: 3, title: "秘密の手紙", required_level: 8 },
            Episode { id: 4, title: "宮内の後悔", required_level: 12 },
        ],
        CharacterId::Kanzaki => vec![
            Episode { id: 1, title: "取材申し込み", required_level: 2 },
            Episode { id: 2, title: "バズった記事", required_level: 5 },
            Episode { id: 3, title: "書けない記事", required_level: 8 },
            Episode { id: 4, title: "凛の矜持", required_level: 12 },
        ],
        CharacterId::Kiritani => vec![
            Episode { id: 1, title: "偵察", required_level: 2 },
            Episode { id: 2, title: "効率と非効率", required_level: 5 },
            Episode { id: 3, title: "本部の圧力", required_level: 8 },
            Episode { id: 4, title: "楓の本音", required_level: 12 },
        ],
    }
}

// ═══════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn affinity_level_calculation() {
        let axes = AffinityAxes {
            trust: 10,
            understanding: 10,
            empathy: 5,
        };
        // total = 25, level = sqrt(25/5) = sqrt(5) ≈ 2
        assert_eq!(axes.level(), 2);
    }

    #[test]
    fn affinity_level_zero() {
        let axes = AffinityAxes::default();
        assert_eq!(axes.level(), 0);
        assert_eq!(axes.star_rank(), 1);
    }

    #[test]
    fn affinity_star_ranks() {
        // Level 3 → ★2
        let axes = AffinityAxes {
            trust: 30,
            understanding: 10,
            empathy: 5,
        };
        assert_eq!(axes.level(), 3);
        assert_eq!(axes.star_rank(), 2);

        // Level 10 → ★4
        let axes2 = AffinityAxes {
            trust: 200,
            understanding: 150,
            empathy: 150,
        };
        assert_eq!(axes2.level(), 10);
        assert_eq!(axes2.star_rank(), 4);
    }

    #[test]
    fn action_gains_multiply() {
        let gain = AffinityGain {
            trust: 10,
            understanding: 5,
            empathy: 5,
        };
        let multiplied = gain.multiply(1.5);
        assert_eq!(multiplied.trust, 15);
        assert_eq!(multiplied.understanding, 7);
        assert_eq!(multiplied.empathy, 7);
    }

    #[test]
    fn episode_availability() {
        let affinity = CharacterAffinity {
            axes: AffinityAxes {
                trust: 30,
                understanding: 10,
                empathy: 5,
            },
            unlocked: true,
            viewed_episodes: vec![1],
        };
        let eps = available_episodes(CharacterId::Sakura, &affinity);
        // Level 3, episode 1 (req 2) already viewed, episode 2 (req 5) not yet
        assert!(eps.is_empty()); // level 3, ep2 needs level 5
    }
}
