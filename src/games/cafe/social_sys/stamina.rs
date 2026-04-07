//! Stamina system — real-time recovery.

use serde::{Deserialize, Serialize};

const STAMINA_RECOVERY_MS: f64 = 6.0 * 60.0 * 1000.0;
pub const STAMINA_MAX: u32 = 100;
pub const BUSINESS_DAY_COST: u32 = 20;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct StaminaState {
    pub current: u32,
    pub last_update_ms: f64,
}

impl Default for StaminaState {
    fn default() -> Self {
        Self { current: STAMINA_MAX, last_update_ms: 0.0 }
    }
}

impl StaminaState {
    pub fn recover(&mut self, now_ms: f64) {
        if self.last_update_ms <= 0.0 {
            self.last_update_ms = now_ms;
            return;
        }
        let elapsed = now_ms - self.last_update_ms;
        if elapsed <= 0.0 { return; }
        let points = (elapsed / STAMINA_RECOVERY_MS) as u32;
        if points > 0 {
            self.current = (self.current + points).min(STAMINA_MAX);
            self.last_update_ms += points as f64 * STAMINA_RECOVERY_MS;
        }
    }

    pub fn consume(&mut self, amount: u32, now_ms: f64) -> bool {
        self.recover(now_ms);
        if self.current >= amount {
            self.current -= amount;
            true
        } else {
            false
        }
    }

    #[allow(dead_code)] // Phase 2+: stamina countdown display
    pub fn seconds_to_next(&self, now_ms: f64) -> u32 {
        if self.current >= STAMINA_MAX { return 0; }
        let elapsed = now_ms - self.last_update_ms;
        let remaining_ms = STAMINA_RECOVERY_MS - elapsed;
        (remaining_ms / 1000.0).max(0.0).ceil() as u32
    }

    pub fn minutes_to_full(&self, now_ms: f64) -> u32 {
        if self.current >= STAMINA_MAX { return 0; }
        let deficit = STAMINA_MAX - self.current;
        let elapsed = now_ms - self.last_update_ms;
        let remaining_for_next = STAMINA_RECOVERY_MS - elapsed;
        let total_ms = remaining_for_next + (deficit.saturating_sub(1) as f64 * STAMINA_RECOVERY_MS);
        (total_ms / 60_000.0).max(0.0).ceil() as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const BASE_TIME: f64 = 1_775_044_800_000.0;

    #[test]
    fn stamina_recovery_basic() {
        let mut s = StaminaState { current: 50, last_update_ms: BASE_TIME };
        s.recover(BASE_TIME + 12.0 * 60.0 * 1000.0);
        assert_eq!(s.current, 52);
    }

    #[test]
    fn stamina_capped_at_max() {
        let mut s = StaminaState { current: 99, last_update_ms: BASE_TIME };
        s.recover(BASE_TIME + 30.0 * 60.0 * 1000.0);
        assert_eq!(s.current, STAMINA_MAX);
    }

    #[test]
    fn stamina_consume_success() {
        let mut s = StaminaState { current: 30, last_update_ms: BASE_TIME };
        assert!(s.consume(20, BASE_TIME));
        assert_eq!(s.current, 10);
    }

    #[test]
    fn stamina_consume_fail() {
        let mut s = StaminaState { current: 10, last_update_ms: BASE_TIME };
        assert!(!s.consume(20, BASE_TIME));
        assert_eq!(s.current, 10);
    }
}
