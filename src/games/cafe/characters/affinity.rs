//! 3-axis affinity (親密度) system.
//!
//! - Trust (信頼), Understanding (理解), Empathy (共感)
//! - Quadratic growth: N² × 5 points per level
//! - Star rank derived from affinity level

use serde::{Deserialize, Serialize};

use super::ActionType;

/// Three-axis affinity values.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AffinityAxes {
    pub trust: u32,
    pub understanding: u32,
    pub empathy: u32,
}

impl AffinityAxes {
    pub fn total(&self) -> u32 {
        self.trust + self.understanding + self.empathy
    }

    /// Affection level: N where N² × 5 ≤ total.
    pub fn level(&self) -> u32 {
        ((self.total() as f64 / 5.0).sqrt()) as u32
    }

    pub fn points_to_next_level(&self) -> u32 {
        let next = self.level() + 1;
        let required = next * next * 5;
        required.saturating_sub(self.total())
    }

    /// Affinity star rank (★1-5).
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

/// Affinity state for a character (saved alongside CharacterData).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CharacterAffinity {
    pub axes: AffinityAxes,
    pub viewed_episodes: Vec<u32>,
}

/// Gains from an action, before multiplier.
#[derive(Debug, Clone, Copy)]
pub struct AffinityGain {
    pub trust: u32,
    pub understanding: u32,
    pub empathy: u32,
}

impl AffinityGain {
    pub fn multiply(self, mult: f64) -> Self {
        Self {
            trust: (self.trust as f64 * mult) as u32,
            understanding: (self.understanding as f64 * mult) as u32,
            empathy: (self.empathy as f64 * mult) as u32,
        }
    }
}

/// Base affinity gains per action type.
pub fn base_gains(action: ActionType) -> AffinityGain {
    match action {
        ActionType::Eat => AffinityGain { trust: 15, understanding: 5, empathy: 5 },
        ActionType::Observe => AffinityGain { trust: 5, understanding: 15, empathy: 5 },
        ActionType::Talk => AffinityGain { trust: 5, understanding: 5, empathy: 15 },
        ActionType::Special => AffinityGain { trust: 10, understanding: 10, empathy: 10 },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn affinity_level_calculation() {
        let axes = AffinityAxes { trust: 10, understanding: 10, empathy: 5 };
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
        let axes = AffinityAxes { trust: 30, understanding: 10, empathy: 5 };
        assert_eq!(axes.level(), 3);
        assert_eq!(axes.star_rank(), 2);

        let axes2 = AffinityAxes { trust: 200, understanding: 150, empathy: 150 };
        assert_eq!(axes2.level(), 10);
        assert_eq!(axes2.star_rank(), 4);
    }

    #[test]
    fn action_gains_multiply() {
        let gain = AffinityGain { trust: 10, understanding: 5, empathy: 5 };
        let m = gain.multiply(1.5);
        assert_eq!(m.trust, 15);
        assert_eq!(m.understanding, 7);
        assert_eq!(m.empathy, 7);
    }
}
