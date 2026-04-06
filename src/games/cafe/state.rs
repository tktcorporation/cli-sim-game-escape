//! Pure data structures for the Café game.
//!
//! Revamped with social game systems inspired by adv-game-candy:
//! - AP action system (5 AP/day)
//! - Multi-axis affinity per character
//! - Card collection & gacha
//! - Player rank
//! - Memory (思い出) equipment

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::affinity::{ActionType, CharacterAffinity, CharacterId};
use super::cards::CardState;
use super::social::{DailyMissionState, LoginBonusState, StaminaState};

// ── Game Phase ────────────────────────────────────────────

/// Which phase/screen the game is currently in.
#[derive(Debug, Clone, PartialEq)]
pub enum GamePhase {
    /// Displaying story text (novel ADV mode).
    Story,
    /// Main hub — choose actions, view status.
    Hub,
    /// Selecting a character to interact with.
    CharacterSelect,
    /// Choosing an action for a character.
    ActionSelect {
        target: CharacterId,
    },
    /// Showing action result (affinity gains, etc.).
    ActionResult {
        target: CharacterId,
        action: ActionType,
        trust_gain: u32,
        understanding_gain: u32,
        empathy_gain: u32,
    },
    /// Card collection & gacha screen.
    CardScreen,
    /// Gacha result display.
    GachaResult {
        card_ids: Vec<u32>,
    },
    /// Character detail (affinity, episodes).
    CharacterDetail {
        target: CharacterId,
    },
    /// Showing daily results after a business day.
    DayResult,
}

// ── Story Line ────────────────────────────────────────────

/// A line of story text with optional speaker name.
#[derive(Debug, Clone)]
pub struct StoryLine {
    /// Speaker name. None = narration / monologue.
    pub speaker: Option<&'static str>,
    /// The text content.
    pub text: &'static str,
    /// Whether this is a monologue (rendered in parentheses).
    pub is_monologue: bool,
}

/// A complete story scene (sequence of lines).
#[derive(Debug, Clone)]
pub struct StoryScene {
    pub lines: &'static [StoryLine],
}

// ── Menu Item ─────────────────────────────────────────────

/// A menu item that can be served to customers.
#[derive(Debug, Clone)]
pub struct MenuItem {
    pub name: &'static str,
    #[allow(dead_code)] // Phase 2+ profit calculation
    pub cost: u32,
    pub price: u32,
    #[allow(dead_code)] // Phase 2+ menu display
    pub description: &'static str,
}

/// A customer visit during the day.
#[derive(Debug, Clone)]
pub struct CustomerVisit {
    pub name: &'static str,
    pub order: &'static str,
    pub satisfied: bool,
    pub revenue: u32,
}

// ── Memory (思い出) Equipment ──────────────────────────────

/// A memory (思い出) — passive bonus earned by meeting conditions.
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

/// Player rank (commander level) — gates story chapters.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PlayerRank {
    pub level: u32,
    pub exp: u32,
}

impl PlayerRank {
    /// EXP needed for next level.
    pub fn exp_to_next(&self) -> u32 {
        30 + self.level * 20
    }

    /// Add EXP and handle level ups. Returns number of levels gained.
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

    /// Chapter unlocked at this rank.
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

// ── Complete Game State ───────────────────────────────────

/// The complete game state.
#[derive(Debug, Clone)]
pub struct CafeState {
    // ── Phase management ───────────────────────────────
    pub phase: GamePhase,

    // ── Story state ────────────────────────────────────
    /// Which chapter is currently being viewed.
    pub current_chapter: u32,
    /// Index of the current scene being displayed.
    pub current_scene_index: usize,
    /// Index of the current line within the scene.
    pub current_line_index: usize,
    /// Highest chapter completed (0 = prologue done).
    pub chapters_completed: u32,

    // ── Business / Economy ─────────────────────────────
    pub day: u32,
    pub money: i64,
    pub menu: Vec<MenuItem>,
    pub today_visits: Vec<CustomerVisit>,
    pub total_customers_served: u32,

    // ── AP Action System ───────────────────────────────
    /// Action Points remaining today (max 5).
    pub ap_current: u32,
    /// Total actions performed today.
    pub actions_today: u32,

    // ── Player Rank ────────────────────────────────────
    pub player_rank: PlayerRank,

    // ── Character Affinities ───────────────────────────
    pub affinities: HashMap<CharacterId, CharacterAffinity>,

    // ── Card System ────────────────────────────────────
    pub card_state: CardState,

    // ── Memory Equipment ───────────────────────────────
    pub memories: Vec<Memory>,
    /// Indices into `memories` for equipped slots (max 3).
    pub equipped_memories: Vec<usize>,

    // ── Social game systems ─────────────────────────────
    pub stamina: StaminaState,
    pub daily_missions: DailyMissionState,
    pub login_bonus: LoginBonusState,
    /// Pending login bonus popup (shown once on game start).
    pub pending_login_reward: Option<i64>,
    /// Pending recovery bonus (shown once after absence).
    pub pending_recovery_bonus: Option<i64>,
    /// Total business runs today (for mission tracking).
    pub today_business_runs: u32,

    // ── UI state ───────────────────────────────────────
    #[allow(dead_code)] // Phase 2+ menu selection UI
    pub selected_menu_item: usize,
    /// Current tab in hub view.
    pub hub_tab: HubTab,
    /// Scroll offset for lists.
    #[allow(dead_code)] // Phase 2+ scrollable lists
    pub scroll_offset: usize,
}

/// Tabs in the hub screen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HubTab {
    /// Main hub: status, actions, story
    Home,
    /// Character list & affinity
    Characters,
    /// Card collection & gacha
    Cards,
    /// Settings / missions
    Missions,
}

// ── AP Constants ───────────────────────────────────────────

pub const AP_MAX: u32 = 5;

impl CafeState {
    pub fn new() -> Self {
        // Initialize affinity for all characters (locked by default)
        let mut affinities = HashMap::new();
        for &ch in CharacterId::ALL {
            let mut aff = CharacterAffinity::default();
            // Sakura is unlocked from start (appears in Ch.0)
            if ch == CharacterId::Sakura {
                aff.unlocked = true;
            }
            affinities.insert(ch, aff);
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
                MenuItem {
                    name: "ブレンドコーヒー",
                    cost: 50,
                    price: 300,
                    description: "基本のドリップコーヒー",
                },
                MenuItem {
                    name: "カフェラテ",
                    cost: 80,
                    price: 400,
                    description: "エスプレッソ + ミルク",
                },
                MenuItem {
                    name: "ほうじ茶",
                    cost: 30,
                    price: 250,
                    description: "香ばしい和のお茶",
                },
            ],
            today_visits: Vec::new(),
            total_customers_served: 0,
            ap_current: AP_MAX,
            actions_today: 0,
            player_rank: PlayerRank::default(),
            affinities,
            card_state: CardState::default(),
            memories: Vec::new(),
            equipped_memories: Vec::new(),
            stamina: StaminaState::default(),
            daily_missions: DailyMissionState::default(),
            login_bonus: LoginBonusState::default(),
            pending_login_reward: None,
            pending_recovery_bonus: None,
            today_business_runs: 0,
            selected_menu_item: 0,
            hub_tab: HubTab::Home,
            scroll_offset: 0,
        }
    }

    /// Calculate today's total revenue.
    pub fn today_revenue(&self) -> u32 {
        self.today_visits.iter().map(|v| v.revenue).sum()
    }

    /// Calculate today's total cost.
    pub fn today_cost(&self) -> u32 {
        self.today_visits
            .iter()
            .filter(|v| v.satisfied)
            .count() as u32
            * 50
    }

    /// Total memory bonuses for equipped memories.
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
                self.affinities
                    .get(ch)
                    .is_some_and(|a| a.unlocked)
            })
            .copied()
            .collect()
    }
}
