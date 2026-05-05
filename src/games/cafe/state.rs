//! Pure data structures for the Café game.
//!
//! BA/学マス style:
//! - Character data with levels, stars, shards, skills
//! - 3-axis affinity per character
//! - Card collection & gacha with spark
//! - Player rank
//! - Produce mode state
//! - Memory equipment

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::characters::affinity::CharacterAffinity;
use super::characters::{ActionType, CharacterData, CharacterId};
use super::gacha::CardState;
use super::produce::{ProduceState, TrainingType};
use super::social_sys::{DailyMissionState, LoginBonusState, StaminaState, WeeklyMissionState};

// ── Game Phase ────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum GamePhase {
    /// Story text display (novel ADV).
    Story,
    /// Main hub — tabs, actions.
    Hub,
    /// Selecting a character to interact with.
    CharacterSelect,
    /// Choosing an action for a character.
    ActionSelect { target: CharacterId },
    /// Showing action result.
    ActionResult {
        target: CharacterId,
        action: ActionType,
        trust_gain: u32,
        understanding_gain: u32,
        empathy_gain: u32,
    },
    /// Character detail (level up, star promote, skills, affinity).
    CharacterDetail { target: CharacterId },
    /// Card collection & gacha screen.
    CardScreen,
    /// Gacha result display.
    GachaResult { card_ids: Vec<u32> },
    /// Produce: character select.
    ProduceCharSelect,
    /// Produce: training turn (choose training type).
    ProduceTraining,
    /// Produce: turn result (show what happened).
    ProduceTurnResult { training: TrainingType },
    /// Produce: final evaluation.
    ProduceResult,
    /// Day result after business.
    DayResult,
}

// ── Story Line ────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct StoryLine {
    pub speaker: Option<&'static str>,
    pub text: &'static str,
    pub is_monologue: bool,
}

#[derive(Debug, Clone)]
pub struct StoryScene {
    pub lines: &'static [StoryLine],
}

// ── Menu Item ─────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct MenuItem {
    pub name: &'static str,
    #[allow(dead_code)] // Phase 2+: menu management UI
    pub cost: u32,
    pub price: u32,
    #[allow(dead_code)] // Phase 2+: menu detail UI
    pub description: &'static str,
}

#[derive(Debug, Clone)]
pub struct CustomerVisit {
    pub name: &'static str,
    pub order: &'static str,
    pub satisfied: bool,
    pub revenue: u32,
}

// ── Memory Equipment ──────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Memory {
    pub id: u32,
    pub name: String,
    pub description: String,
    pub trust_bonus: u32,
    pub understanding_bonus: u32,
    pub empathy_bonus: u32,
}

// ── Player Rank ───────────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PlayerRank {
    pub level: u32,
    pub exp: u32,
}

impl PlayerRank {
    pub fn exp_to_next(&self) -> u32 {
        30 + self.level * 20
    }

    pub fn add_exp(&mut self, amount: u32) -> u32 {
        self.exp += amount;
        let mut levels_gained = 0;
        loop {
            let needed = self.exp_to_next();
            if self.exp >= needed {
                self.exp -= needed;
                self.level += 1;
                levels_gained += 1;
            } else {
                break;
            }
        }
        levels_gained
    }

    pub fn max_chapter(&self) -> u32 {
        match self.level {
            0 => 0,
            1..=2 => 1,
            3..=5 => 2,
            6..=9 => 3,
            10..=14 => 4,
            15..=19 => 5,
            20..=24 => 6,
            25..=29 => 7,
            _ => 8,
        }
    }
}

// ── Hub Tabs ──────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HubTab {
    Home,
    Characters,
    Cards,
    Produce,
    Missions,
}

// ── AP Constants ──────────────────────────────────────────

pub const AP_MAX: u32 = 5;

// ── Complete Game State ───────────────────────────────────

#[derive(Debug, Clone)]
pub struct CafeState {
    // ── Phase ─────────────────────────────────────────
    pub phase: GamePhase,

    // ── Story ─────────────────────────────────────────
    pub current_chapter: u32,
    pub current_scene_index: usize,
    pub current_line_index: usize,
    pub chapters_completed: u32,

    // ── Economy ───────────────────────────────────────
    pub day: u32,
    pub money: i64,
    pub menu: Vec<MenuItem>,
    pub today_visits: Vec<CustomerVisit>,
    pub total_customers_served: u32,

    // ── AP ────────────────────────────────────────────
    pub ap_current: u32,
    pub actions_today: u32,

    // ── Player Rank ───────────────────────────────────
    pub player_rank: PlayerRank,

    // ── Characters (BA-style) ─────────────────────────
    /// Per-character progression (level, stars, shards, skills).
    pub character_data: HashMap<CharacterId, CharacterData>,
    /// Per-character affinity (3-axis bond).
    pub affinities: HashMap<CharacterId, CharacterAffinity>,

    // ── Cards / Gacha ─────────────────────────────────
    pub card_state: CardState,

    // ── Memory Equipment ──────────────────────────────
    pub memories: Vec<Memory>,
    pub equipped_memories: Vec<usize>,

    // ── Produce ───────────────────────────────────────
    /// Active produce run (None if not in produce).
    pub produce: Option<ProduceState>,
    /// Total produce completions (for missions).
    pub total_produce_completions: u32,

    // ── Social Systems ────────────────────────────────
    pub stamina: StaminaState,
    pub daily_missions: DailyMissionState,
    pub weekly_missions: WeeklyMissionState,
    pub login_bonus: LoginBonusState,
    pub pending_login_reward: Option<i64>,
    pub pending_login_gems: Option<u32>,
    pub pending_recovery_bonus: Option<i64>,
    pub today_business_runs: u32,

    // ── UI ────────────────────────────────────────────
    pub hub_tab: HubTab,
    /// Animation frame counter for the gacha reveal screen. Reset to 0 when
    /// entering `GachaResult`, advanced once per `tick`. Drives the staged
    /// reveal in `render/gacha.rs`.
    pub gacha_anim_frame: u32,
}

impl CafeState {
    pub fn new() -> Self {
        let mut character_data = HashMap::new();
        let mut affinities = HashMap::new();
        for &ch in CharacterId::ALL {
            let mut cd = CharacterData::with_stars(ch.base_stars());
            if ch == CharacterId::Sakura {
                cd.unlocked = true;
            }
            character_data.insert(ch, cd);
            affinities.insert(ch, CharacterAffinity::default());
        }

        Self {
            phase: GamePhase::Story,
            current_chapter: 0,
            current_scene_index: 0,
            current_line_index: 0,
            chapters_completed: 0,
            day: 1,
            money: 1000,
            menu: vec![
                MenuItem { name: "ブレンドコーヒー", cost: 50, price: 300, description: "基本のドリップコーヒー" },
                MenuItem { name: "カフェラテ", cost: 80, price: 400, description: "エスプレッソ + ミルク" },
                MenuItem { name: "ほうじ茶", cost: 30, price: 250, description: "香ばしい和のお茶" },
            ],
            today_visits: Vec::new(),
            total_customers_served: 0,
            ap_current: AP_MAX,
            actions_today: 0,
            player_rank: PlayerRank::default(),
            character_data,
            affinities,
            card_state: CardState::default(),
            memories: Vec::new(),
            equipped_memories: Vec::new(),
            produce: None,
            total_produce_completions: 0,
            stamina: StaminaState::default(),
            daily_missions: DailyMissionState::default(),
            weekly_missions: WeeklyMissionState::default(),
            login_bonus: LoginBonusState::default(),
            pending_login_reward: None,
            pending_login_gems: None,
            pending_recovery_bonus: None,
            today_business_runs: 0,
            hub_tab: HubTab::Home,
            gacha_anim_frame: 0,
        }
    }

    pub fn today_revenue(&self) -> u32 {
        self.today_visits.iter().map(|v| v.revenue).sum()
    }

    pub fn today_cost(&self) -> u32 {
        self.today_visits.iter().filter(|v| v.satisfied).count() as u32 * 50
    }

    pub fn memory_bonuses(&self) -> (u32, u32, u32) {
        let mut trust = 0u32;
        let mut understanding = 0u32;
        let mut empathy = 0u32;
        for &idx in &self.equipped_memories {
            if let Some(mem) = self.memories.get(idx) {
                trust += mem.trust_bonus;
                understanding += mem.understanding_bonus;
                empathy += mem.empathy_bonus;
            }
        }
        (trust, understanding, empathy)
    }

    /// Characters that are currently unlocked.
    pub fn unlocked_characters(&self) -> Vec<CharacterId> {
        CharacterId::ALL
            .iter()
            .filter(|ch| {
                self.character_data
                    .get(ch)
                    .is_some_and(|d| d.unlocked)
            })
            .copied()
            .collect()
    }
}
