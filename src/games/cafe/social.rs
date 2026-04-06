//! Social game systems: stamina, daily missions, login bonus.
//!
//! All systems are real-time based (wall clock) following the
//! Arknights/Blue Archive model described in DESIGN.md.

use serde::{Deserialize, Serialize};

// ── Constants ─────────────────────────────────────────────

/// Stamina recovery: 1 point per 6 minutes (360,000 ms).
const STAMINA_RECOVERY_MS: f64 = 6.0 * 60.0 * 1000.0;
/// Maximum stamina.
pub const STAMINA_MAX: u32 = 100;
/// Cost per business day (simplified flat cost for MVP).
pub const BUSINESS_DAY_COST: u32 = 20;

/// Daily reset hour in JST (04:00).
const DAILY_RESET_HOUR_JST: u32 = 4;
/// JST offset from UTC in milliseconds (+9 hours).
const JST_OFFSET_MS: f64 = 9.0 * 60.0 * 60.0 * 1000.0;

/// Days of absence before recovery bonus triggers.
const RECOVERY_BONUS_THRESHOLD_DAYS: u32 = 3;

// ── Stamina ───────────────────────────────────────────────

/// Stamina system state (仕入れ予算).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct StaminaState {
    /// Current stamina points.
    pub current: u32,
    /// Timestamp (ms since epoch) when stamina was last updated.
    pub last_update_ms: f64,
}

impl Default for StaminaState {
    fn default() -> Self {
        Self {
            current: STAMINA_MAX,
            last_update_ms: 0.0,
        }
    }
}

impl StaminaState {
    /// Update stamina based on elapsed real time.
    /// Call this on game load and periodically during play.
    pub fn recover(&mut self, now_ms: f64) {
        if self.last_update_ms <= 0.0 {
            self.last_update_ms = now_ms;
            return;
        }

        let elapsed = now_ms - self.last_update_ms;
        if elapsed <= 0.0 {
            return;
        }

        let points_recovered = (elapsed / STAMINA_RECOVERY_MS) as u32;
        if points_recovered > 0 {
            self.current = (self.current + points_recovered).min(STAMINA_MAX);
            // Advance last_update by exact recovery amount (preserve remainder)
            self.last_update_ms += points_recovered as f64 * STAMINA_RECOVERY_MS;
        }
    }

    /// Try to consume stamina. Returns true if successful.
    pub fn consume(&mut self, amount: u32, now_ms: f64) -> bool {
        self.recover(now_ms);
        if self.current >= amount {
            self.current -= amount;
            true
        } else {
            false
        }
    }

    /// Seconds until next stamina point recovery.
    #[allow(dead_code)] // Used in render for recovery timer display
    pub fn seconds_to_next(&self, now_ms: f64) -> u32 {
        if self.current >= STAMINA_MAX {
            return 0;
        }
        let elapsed = now_ms - self.last_update_ms;
        let remaining_ms = STAMINA_RECOVERY_MS - elapsed;
        (remaining_ms / 1000.0).max(0.0).ceil() as u32
    }

    /// Minutes until fully recovered.
    pub fn minutes_to_full(&self, now_ms: f64) -> u32 {
        if self.current >= STAMINA_MAX {
            return 0;
        }
        let deficit = STAMINA_MAX - self.current;
        let elapsed = now_ms - self.last_update_ms;
        let remaining_for_next = STAMINA_RECOVERY_MS - elapsed;
        let total_ms = remaining_for_next + (deficit.saturating_sub(1) as f64 * STAMINA_RECOVERY_MS);
        (total_ms / 60_000.0).max(0.0).ceil() as u32
    }
}

// ── Daily Missions ────────────────────────────────────────

/// Individual mission types (data-driven for easy expansion).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum MissionType {
    /// Run business N times.
    RunBusiness(u32),
    /// Serve N special customers.
    ServeSpecial(u32),
    /// Discover N new recipes (future).
    DiscoverRecipe(u32),
    /// Serve a regular's favorite menu.
    ServeFavorite,
}

/// A single daily mission.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mission {
    pub name: String,
    pub mission_type: MissionType,
    pub progress: u32,
    pub target: u32,
    pub reward_money: i64,
    pub completed: bool,
}

impl Mission {
    pub fn is_done(&self) -> bool {
        self.progress >= self.target
    }
}

/// Daily mission state.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DailyMissionState {
    /// The "day number" in JST when missions were last reset.
    pub last_reset_day: u32,
    /// Current missions.
    pub missions: Vec<Mission>,
    /// Whether the all-clear bonus was claimed.
    pub all_clear_claimed: bool,
}

impl DailyMissionState {
    /// Check if a daily reset is needed and reset if so.
    /// Returns true if a reset occurred.
    pub fn check_reset(&mut self, now_ms: f64) -> bool {
        let current_day = jst_day_number(now_ms);
        if current_day != self.last_reset_day {
            self.reset(current_day);
            true
        } else {
            false
        }
    }

    fn reset(&mut self, day_number: u32) {
        self.last_reset_day = day_number;
        self.all_clear_claimed = false;
        self.missions = generate_daily_missions();
    }

    /// Record progress for a mission type.
    pub fn record(&mut self, mission_type: MissionType) {
        for mission in &mut self.missions {
            if std::mem::discriminant(&mission.mission_type)
                == std::mem::discriminant(&mission_type)
                && !mission.completed
            {
                mission.progress += 1;
                if mission.is_done() {
                    mission.completed = true;
                }
            }
        }
    }

    /// Check if all missions are complete.
    pub fn all_complete(&self) -> bool {
        !self.missions.is_empty() && self.missions.iter().all(|m| m.completed)
    }

    /// Total reward money from completed missions.
    #[allow(dead_code)] // Will be used in Phase 2+ mission reward claiming
    pub fn claimable_rewards(&self) -> i64 {
        self.missions
            .iter()
            .filter(|m| m.completed)
            .map(|m| m.reward_money)
            .sum()
    }
}

/// Generate today's missions.
fn generate_daily_missions() -> Vec<Mission> {
    vec![
        Mission {
            name: "営業を1回行う".into(),
            mission_type: MissionType::RunBusiness(1),
            progress: 0,
            target: 1,
            reward_money: 100,
            completed: false,
        },
        Mission {
            name: "営業を3回行う".into(),
            mission_type: MissionType::RunBusiness(3),
            progress: 0,
            target: 3,
            reward_money: 300,
            completed: false,
        },
        Mission {
            name: "常連客にお気に入りを出す".into(),
            mission_type: MissionType::ServeFavorite,
            progress: 0,
            target: 1,
            reward_money: 200,
            completed: false,
        },
    ]
}

// ── Login Bonus ───────────────────────────────────────────

/// Login bonus reward for a specific day.
#[derive(Debug, Clone)]
pub struct LoginReward {
    pub day: u32,
    #[allow(dead_code)] // Used in Phase 2+ login calendar UI
    pub description: &'static str,
    pub money: i64,
}

/// Monthly login bonus calendar.
pub static LOGIN_REWARDS: &[LoginReward] = &[
    LoginReward { day: 1, description: "資金 ¥200", money: 200 },
    LoginReward { day: 2, description: "資金 ¥200", money: 200 },
    LoginReward { day: 3, description: "資金 ¥300", money: 300 },
    LoginReward { day: 4, description: "資金 ¥200", money: 200 },
    LoginReward { day: 5, description: "資金 ¥200", money: 200 },
    LoginReward { day: 6, description: "資金 ¥200", money: 200 },
    LoginReward { day: 7, description: "資金 ¥500", money: 500 },
    LoginReward { day: 14, description: "資金 ¥800", money: 800 },
    LoginReward { day: 21, description: "資金 ¥1000", money: 1000 },
    LoginReward { day: 28, description: "資金 ¥2000", money: 2000 },
];

/// Login bonus state.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct LoginBonusState {
    /// Cumulative login days (never resets — Blue Archive style).
    pub total_login_days: u32,
    /// The JST day number of the last login.
    pub last_login_day: u32,
    /// Whether today's bonus was claimed.
    pub today_claimed: bool,
    /// Days since last login (for recovery bonus).
    pub absence_days: u32,
    /// Whether recovery bonus was shown.
    pub recovery_shown: bool,
}

impl LoginBonusState {
    /// Process a login event. Returns the reward if a new day.
    pub fn process_login(&mut self, now_ms: f64) -> Option<&'static LoginReward> {
        let current_day = jst_day_number(now_ms);

        if current_day == self.last_login_day {
            // Same day, no new reward
            return None;
        }

        // Calculate absence
        if self.last_login_day > 0 {
            self.absence_days = current_day.saturating_sub(self.last_login_day);
            self.recovery_shown = false;
        }

        self.last_login_day = current_day;
        self.total_login_days += 1;
        self.today_claimed = false;

        // Find reward for this day
        self.current_reward()
    }

    /// Get the reward for the current login day (if any).
    pub fn current_reward(&self) -> Option<&'static LoginReward> {
        LOGIN_REWARDS
            .iter()
            .find(|r| r.day == self.total_login_days)
    }

    /// Whether the player qualifies for a recovery bonus.
    pub fn has_recovery_bonus(&self) -> bool {
        self.absence_days >= RECOVERY_BONUS_THRESHOLD_DAYS && !self.recovery_shown
    }

    /// Recovery bonus amount (scales with absence).
    pub fn recovery_bonus_money(&self) -> i64 {
        if self.has_recovery_bonus() {
            (self.absence_days as i64 * 100).min(1000)
        } else {
            0
        }
    }
}

// ── AP (Action Points) Daily Reset ───────────────────────

/// AP daily reset state.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ApResetState {
    /// JST day number of last AP reset.
    pub last_reset_day: u32,
}

impl ApResetState {
    /// Check if AP needs resetting for a new day. Returns true if reset.
    pub fn check_reset(&mut self, now_ms: f64) -> bool {
        let current_day = jst_day_number(now_ms);
        if current_day != self.last_reset_day {
            self.last_reset_day = current_day;
            true
        } else {
            false
        }
    }
}

/// Get current JST day number (public for card daily reset).
pub fn current_jst_day(now_ms: f64) -> u32 {
    jst_day_number(now_ms)
}

// ── Time Utilities ────────────────────────────────────────

/// Convert a Unix timestamp (ms) to a "day number" in JST.
/// Days start at 04:00 JST (the daily reset time).
fn jst_day_number(unix_ms: f64) -> u32 {
    // Convert to JST, then subtract reset hour offset
    let jst_ms = unix_ms + JST_OFFSET_MS;
    let reset_offset_ms = DAILY_RESET_HOUR_JST as f64 * 60.0 * 60.0 * 1000.0;
    let adjusted_ms = jst_ms - reset_offset_ms;
    // Day number = floor(adjusted_ms / ms_per_day)
    let ms_per_day = 24.0 * 60.0 * 60.0 * 1000.0;
    (adjusted_ms / ms_per_day) as u32
}

/// Get the current Unix timestamp in milliseconds.
/// Uses js_sys::Date::now() in WASM, returns 0.0 in non-WASM.
pub fn now_ms() -> f64 {
    #[cfg(target_arch = "wasm32")]
    {
        js_sys::Date::now()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        0.0
    }
}

// ═══════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    // 2026-04-02 10:00:00 UTC in ms
    const BASE_TIME: f64 = 1_775_044_800_000.0;

    #[test]
    fn stamina_recovery_basic() {
        let mut s = StaminaState {
            current: 50,
            last_update_ms: BASE_TIME,
        };
        // 12 minutes later = 2 points recovered
        s.recover(BASE_TIME + 12.0 * 60.0 * 1000.0);
        assert_eq!(s.current, 52);
    }

    #[test]
    fn stamina_capped_at_max() {
        let mut s = StaminaState {
            current: 99,
            last_update_ms: BASE_TIME,
        };
        // 30 minutes = 5 points, but capped at 100
        s.recover(BASE_TIME + 30.0 * 60.0 * 1000.0);
        assert_eq!(s.current, STAMINA_MAX);
    }

    #[test]
    fn stamina_consume_success() {
        let mut s = StaminaState {
            current: 30,
            last_update_ms: BASE_TIME,
        };
        assert!(s.consume(20, BASE_TIME));
        assert_eq!(s.current, 10);
    }

    #[test]
    fn stamina_consume_fail() {
        let mut s = StaminaState {
            current: 10,
            last_update_ms: BASE_TIME,
        };
        assert!(!s.consume(20, BASE_TIME));
        assert_eq!(s.current, 10);
    }

    #[test]
    fn jst_day_same_day() {
        // Two timestamps 1 hour apart on same JST day
        let t1 = BASE_TIME;
        let t2 = BASE_TIME + 3_600_000.0;
        assert_eq!(jst_day_number(t1), jst_day_number(t2));
    }

    #[test]
    fn jst_day_different_day() {
        // 25 hours apart should be different days
        let t1 = BASE_TIME;
        let t2 = BASE_TIME + 25.0 * 3_600_000.0;
        assert_ne!(jst_day_number(t1), jst_day_number(t2));
    }

    #[test]
    fn daily_missions_reset() {
        let mut dm = DailyMissionState::default();
        dm.check_reset(BASE_TIME);
        assert_eq!(dm.missions.len(), 3);
        assert!(!dm.all_complete());
    }

    #[test]
    fn daily_missions_progress() {
        let mut dm = DailyMissionState::default();
        dm.check_reset(BASE_TIME);
        dm.record(MissionType::RunBusiness(1));
        assert!(dm.missions[0].completed); // "営業を1回行う"
        assert!(!dm.missions[1].completed); // "営業を3回行う" needs 3
    }

    #[test]
    fn login_bonus_cumulative() {
        let mut lb = LoginBonusState::default();
        let reward = lb.process_login(BASE_TIME);
        assert!(reward.is_some());
        assert_eq!(lb.total_login_days, 1);

        // Same day = no reward
        let reward2 = lb.process_login(BASE_TIME + 1000.0);
        assert!(reward2.is_none());
        assert_eq!(lb.total_login_days, 1);
    }

    #[test]
    fn login_bonus_next_day() {
        let mut lb = LoginBonusState::default();
        lb.process_login(BASE_TIME);
        assert_eq!(lb.total_login_days, 1);

        // Next day (25 hours later)
        let reward = lb.process_login(BASE_TIME + 25.0 * 3_600_000.0);
        assert!(reward.is_some());
        assert_eq!(lb.total_login_days, 2);
    }

    #[test]
    fn recovery_bonus_after_absence() {
        let mut lb = LoginBonusState::default();
        lb.process_login(BASE_TIME);

        // 5 days later
        lb.process_login(BASE_TIME + 5.0 * 24.0 * 3_600_000.0);
        assert!(lb.has_recovery_bonus());
        assert_eq!(lb.recovery_bonus_money(), 500); // 5 days * 100
    }
}
