//! Social game systems — stamina, missions, login bonus, achievements.

pub mod achievements;
pub mod login;
pub mod missions;
pub mod stamina;

pub use login::LoginBonusState;
pub use missions::{DailyMissionState, MissionType, WeeklyMissionState};
pub use stamina::{StaminaState, BUSINESS_DAY_COST, STAMINA_MAX};

// ── Time Constants ───────────────────────────────────────

/// Daily reset hour in JST (04:00).
const DAILY_RESET_HOUR_JST: u32 = 4;
/// JST offset from UTC in milliseconds (+9 hours).
const JST_OFFSET_MS: f64 = 9.0 * 60.0 * 60.0 * 1000.0;

// ── AP Reset ─────────────────────────────────────────────

use serde::{Deserialize, Serialize};

/// AP daily reset state.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ApResetState {
    pub last_reset_day: u32,
}

impl ApResetState {
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

// ── Time Utilities ───────────────────────────────────────

/// Get current JST day number.
pub fn current_jst_day(now_ms: f64) -> u32 {
    jst_day_number(now_ms)
}

/// Convert Unix timestamp (ms) to day number in JST (reset at 04:00).
pub fn jst_day_number(unix_ms: f64) -> u32 {
    let jst_ms = unix_ms + JST_OFFSET_MS;
    let reset_offset_ms = DAILY_RESET_HOUR_JST as f64 * 60.0 * 60.0 * 1000.0;
    let adjusted_ms = jst_ms - reset_offset_ms;
    let ms_per_day = 24.0 * 60.0 * 60.0 * 1000.0;
    (adjusted_ms / ms_per_day) as u32
}

/// JST week number (for weekly missions).
pub fn jst_week_number(unix_ms: f64) -> u32 {
    jst_day_number(unix_ms) / 7
}

/// Get current Unix timestamp in milliseconds.
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

#[cfg(test)]
mod tests {
    use super::*;

    const BASE_TIME: f64 = 1_775_044_800_000.0;

    #[test]
    fn jst_day_same_day() {
        let t1 = BASE_TIME;
        let t2 = BASE_TIME + 3_600_000.0;
        assert_eq!(jst_day_number(t1), jst_day_number(t2));
    }

    #[test]
    fn jst_day_different_day() {
        let t1 = BASE_TIME;
        let t2 = BASE_TIME + 25.0 * 3_600_000.0;
        assert_ne!(jst_day_number(t1), jst_day_number(t2));
    }

    #[test]
    fn week_number_changes() {
        let t1 = BASE_TIME;
        let t2 = BASE_TIME + 8.0 * 24.0 * 3_600_000.0;
        assert_ne!(jst_week_number(t1), jst_week_number(t2));
    }
}
