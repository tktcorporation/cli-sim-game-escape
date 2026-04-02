//! Pure data structures for the Café game.

use super::social::{DailyMissionState, LoginBonusState, StaminaState};

/// Which phase the game is currently in.
#[derive(Debug, Clone, PartialEq)]
pub enum GamePhase {
    /// Displaying story text (novel ADV mode).
    Story,
    /// Running the café (business/management mode).
    Business,
    /// Showing daily results after a business day.
    DayResult,
}

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

/// A menu item that can be served to customers.
#[derive(Debug, Clone)]
pub struct MenuItem {
    pub name: &'static str,
    #[allow(dead_code)] // Used in Phase 2 profit calculation
    pub cost: u32,
    pub price: u32,
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

/// The complete game state.
#[derive(Debug, Clone)]
pub struct CafeState {
    // ── Phase management ───────────────────────────────
    pub phase: GamePhase,

    // ── Story state ────────────────────────────────────
    /// Index of the current scene being displayed.
    pub current_scene_index: usize,
    /// Index of the current line within the scene.
    pub current_line_index: usize,
    /// Whether we've finished all queued scenes.
    pub story_complete: bool,

    // ── Business state ─────────────────────────────────
    pub day: u32,
    pub money: i64,
    pub menu: Vec<MenuItem>,
    pub today_visits: Vec<CustomerVisit>,
    pub total_customers_served: u32,

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
    pub selected_menu_item: usize,
}

impl CafeState {
    pub fn new() -> Self {
        Self {
            phase: GamePhase::Story,
            current_scene_index: 0,
            current_line_index: 0,
            story_complete: false,
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
            stamina: StaminaState::default(),
            daily_missions: DailyMissionState::default(),
            login_bonus: LoginBonusState::default(),
            pending_login_reward: None,
            pending_recovery_bonus: None,
            today_business_runs: 0,
            selected_menu_item: 0,
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
            * 50 // simplified: flat cost per served customer
    }
}
