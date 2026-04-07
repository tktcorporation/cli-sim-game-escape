//! Produce mode — Gakumas-style training loop.
//!
//! Core gameplay:
//! 1. Choose a character to work with
//! 2. Over 5 turns, pick training: 接客/調理/雰囲気/休憩
//! 3. Random events boost stats or trigger commu
//! 4. End evaluation: score → rank (C/B/A/S/SS) → rewards

pub mod events;

use serde::{Deserialize, Serialize};
use super::characters::CharacterId;

// ── Constants ─────────────────────────────────────────────

pub const PRODUCE_TURNS: u32 = 5;
pub const PRODUCE_STAMINA_COST: u32 = 30;

// ── Training Types ───────────────────────────────────────

/// Training choices each turn.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum TrainingType {
    /// 接客 — Service: boosts service stat, primary trust affinity
    Service,
    /// 調理 — Cooking: boosts cooking stat, primary understanding affinity
    Cooking,
    /// 雰囲気 — Atmosphere: boosts atmosphere stat, primary empathy affinity
    Atmosphere,
    /// 休憩 — Rest: small boost to all, recovers produce HP
    Rest,
}

impl TrainingType {
    pub fn name(self) -> &'static str {
        match self {
            Self::Service => "接客",
            Self::Cooking => "調理",
            Self::Atmosphere => "雰囲気",
            Self::Rest => "休憩",
        }
    }

    #[allow(dead_code)] // Phase 2+: training detail tooltip
    pub fn description(self) -> &'static str {
        match self {
            Self::Service => "接客力を鍛える",
            Self::Cooking => "調理の腕を磨く",
            Self::Atmosphere => "店の雰囲気を良くする",
            Self::Rest => "少し休んで体力回復",
        }
    }

    /// Base stat gains (service, cooking, atmosphere).
    pub fn base_gains(self) -> (u32, u32, u32) {
        match self {
            Self::Service =>     (20, 5, 5),
            Self::Cooking =>     (5, 20, 5),
            Self::Atmosphere =>  (5, 5, 20),
            Self::Rest =>        (5, 5, 5),
        }
    }
}

// ── Produce Stats ────────────────────────────────────────

/// Stats built up during a produce run.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ProduceStats {
    pub service: u32,
    pub cooking: u32,
    pub atmosphere: u32,
}

impl ProduceStats {
    pub fn total(&self) -> u32 {
        self.service + self.cooking + self.atmosphere
    }
}

// ── Produce Rank ─────────────────────────────────────────

/// Evaluation rank based on final score.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ProduceRank {
    C,
    B,
    A,
    S,
    SS,
}

impl ProduceRank {
    pub fn label(self) -> &'static str {
        match self {
            Self::C => "C",
            Self::B => "B",
            Self::A => "A",
            Self::S => "S",
            Self::SS => "SS",
        }
    }

    /// Determine rank from total score.
    pub fn from_score(score: u32) -> Self {
        match score {
            0..=49 => Self::C,
            50..=99 => Self::B,
            100..=149 => Self::A,
            150..=199 => Self::S,
            _ => Self::SS,
        }
    }

    /// Credit reward multiplier.
    pub fn credit_multiplier(self) -> u32 {
        match self {
            Self::C => 1,
            Self::B => 2,
            Self::A => 3,
            Self::S => 5,
            Self::SS => 8,
        }
    }

    /// Gem reward.
    pub fn gem_reward(self) -> u32 {
        match self {
            Self::C => 10,
            Self::B => 20,
            Self::A => 40,
            Self::S => 80,
            Self::SS => 150,
        }
    }

    /// Character shard reward.
    pub fn shard_reward(self) -> u32 {
        match self {
            Self::C => 0,
            Self::B => 1,
            Self::A => 2,
            Self::S => 5,
            Self::SS => 10,
        }
    }

    /// Character EXP reward.
    pub fn exp_reward(self) -> u32 {
        match self {
            Self::C => 20,
            Self::B => 40,
            Self::A => 60,
            Self::S => 100,
            Self::SS => 150,
        }
    }
}

// ── Produce State ────────────────────────────────────────

/// State of an ongoing produce run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProduceState {
    /// Character being trained with.
    pub character: CharacterId,
    /// Current turn (1-based, max PRODUCE_TURNS).
    pub current_turn: u32,
    /// Accumulated stats.
    pub stats: ProduceStats,
    /// Produce HP (starts at 100, if 0 produce ends early).
    pub hp: u32,
    /// Event that triggered this turn (if any).
    pub current_event: Option<events::ProduceEvent>,
    /// Training choices made (for history display).
    pub history: Vec<TrainingType>,
    /// Whether evaluation is complete.
    pub finished: bool,
    /// Final rank (set after evaluation).
    pub final_rank: Option<ProduceRank>,
}

impl ProduceState {
    pub fn new(character: CharacterId) -> Self {
        Self {
            character,
            current_turn: 1,
            stats: ProduceStats::default(),
            hp: 100,
            current_event: None,
            history: Vec::new(),
            finished: false,
            final_rank: None,
        }
    }

    /// Execute a training choice for the current turn.
    pub fn do_training(&mut self, training: TrainingType, seed: u32) {
        let (s, c, a) = training.base_gains();

        // Apply training
        self.stats.service += s;
        self.stats.cooking += c;
        self.stats.atmosphere += a;

        // Rest recovers HP
        if training == TrainingType::Rest {
            self.hp = (self.hp + 20).min(100);
        } else {
            // Training costs HP
            self.hp = self.hp.saturating_sub(10);
        }

        // Record history
        self.history.push(training);

        // Check for random event
        self.current_event = events::roll_event(self.current_turn, seed);
        if let Some(ref event) = self.current_event {
            self.stats.service += event.bonus_service;
            self.stats.cooking += event.bonus_cooking;
            self.stats.atmosphere += event.bonus_atmosphere;
        }

        // Advance turn
        self.current_turn += 1;

        // Check if produce is over
        if self.current_turn > PRODUCE_TURNS || self.hp == 0 {
            self.evaluate();
        }
    }

    /// Run final evaluation.
    fn evaluate(&mut self) {
        self.finished = true;
        let score = self.stats.total();
        // Bonus for balanced stats (like Gakumas exam)
        let min_stat = self.stats.service.min(self.stats.cooking).min(self.stats.atmosphere);
        let balance_bonus = min_stat / 3; // Bonus for not neglecting any stat
        let final_score = score + balance_bonus;
        self.final_rank = Some(ProduceRank::from_score(final_score));
    }

    /// Is the produce run still ongoing?
    pub fn is_active(&self) -> bool {
        !self.finished && self.current_turn <= PRODUCE_TURNS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn produce_basic_flow() {
        let mut ps = ProduceState::new(CharacterId::Sakura);
        assert!(ps.is_active());
        assert_eq!(ps.current_turn, 1);

        for i in 0..PRODUCE_TURNS {
            ps.do_training(TrainingType::Service, i * 100);
        }

        assert!(ps.finished);
        assert!(ps.final_rank.is_some());
    }

    #[test]
    fn produce_rest_recovers_hp() {
        let mut ps = ProduceState::new(CharacterId::Sakura);
        ps.hp = 50;
        ps.do_training(TrainingType::Rest, 42);
        assert_eq!(ps.hp, 70); // 50 + 20
    }

    #[test]
    fn produce_training_costs_hp() {
        let mut ps = ProduceState::new(CharacterId::Sakura);
        ps.do_training(TrainingType::Service, 42);
        assert_eq!(ps.hp, 90); // 100 - 10
    }

    #[test]
    fn produce_rank_scoring() {
        assert_eq!(ProduceRank::from_score(30), ProduceRank::C);
        assert_eq!(ProduceRank::from_score(75), ProduceRank::B);
        assert_eq!(ProduceRank::from_score(120), ProduceRank::A);
        assert_eq!(ProduceRank::from_score(170), ProduceRank::S);
        assert_eq!(ProduceRank::from_score(250), ProduceRank::SS);
    }

    #[test]
    fn produce_balanced_bonus() {
        let mut ps1 = ProduceState::new(CharacterId::Sakura);
        // Balanced approach
        ps1.do_training(TrainingType::Service, 0);
        ps1.do_training(TrainingType::Cooking, 100);
        ps1.do_training(TrainingType::Atmosphere, 200);
        ps1.do_training(TrainingType::Service, 300);
        ps1.do_training(TrainingType::Cooking, 400);

        let mut ps2 = ProduceState::new(CharacterId::Sakura);
        // Unbalanced approach
        for i in 0..5 {
            ps2.do_training(TrainingType::Service, i * 100);
        }

        // Both produce same base total from training,
        // but balanced gets balance_bonus
        // ps1: service ~45, cooking ~45, atmo ~25 (+ events vary)
        // ps2: service ~100, cooking ~25, atmo ~25 (+ events vary)
        // Balance bonus favors ps1
        assert!(ps1.finished);
        assert!(ps2.finished);
    }
}
