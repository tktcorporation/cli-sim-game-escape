//! Login bonus system — BA-style cumulative login calendar.

use serde::{Deserialize, Serialize};
use super::jst_day_number;

const RECOVERY_BONUS_THRESHOLD_DAYS: u32 = 3;

#[derive(Debug, Clone)]
pub struct LoginReward {
    pub day: u32,
    #[allow(dead_code)] // Phase 2+: login calendar UI
    pub description: &'static str,
    pub money: i64,
    pub gems: u32,
}

pub static LOGIN_REWARDS: &[LoginReward] = &[
    LoginReward { day: 1, description: "資金 ¥200 + 💎50", money: 200, gems: 50 },
    LoginReward { day: 2, description: "資金 ¥200 + 💎50", money: 200, gems: 50 },
    LoginReward { day: 3, description: "資金 ¥300 + 💎80", money: 300, gems: 80 },
    LoginReward { day: 4, description: "資金 ¥200 + 💎50", money: 200, gems: 50 },
    LoginReward { day: 5, description: "資金 ¥200 + 💎50", money: 200, gems: 50 },
    LoginReward { day: 6, description: "資金 ¥200 + 💎50", money: 200, gems: 50 },
    LoginReward { day: 7, description: "資金 ¥500 + 💎120", money: 500, gems: 120 },
    LoginReward { day: 14, description: "資金 ¥800 + 💎200", money: 800, gems: 200 },
    LoginReward { day: 21, description: "資金 ¥1000 + 💎300", money: 1000, gems: 300 },
    LoginReward { day: 28, description: "資金 ¥2000 + 💎500", money: 2000, gems: 500 },
];

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct LoginBonusState {
    pub total_login_days: u32,
    pub last_login_day: u32,
    pub today_claimed: bool,
    pub absence_days: u32,
    pub recovery_shown: bool,
}

impl LoginBonusState {
    pub fn process_login(&mut self, now_ms: f64) -> Option<&'static LoginReward> {
        let current_day = jst_day_number(now_ms);
        if current_day == self.last_login_day {
            return None;
        }
        if self.last_login_day > 0 {
            self.absence_days = current_day.saturating_sub(self.last_login_day);
            self.recovery_shown = false;
        }
        self.last_login_day = current_day;
        self.total_login_days += 1;
        self.today_claimed = false;
        self.current_reward()
    }

    pub fn current_reward(&self) -> Option<&'static LoginReward> {
        LOGIN_REWARDS.iter().find(|r| r.day == self.total_login_days)
    }

    pub fn has_recovery_bonus(&self) -> bool {
        self.absence_days >= RECOVERY_BONUS_THRESHOLD_DAYS && !self.recovery_shown
    }

    pub fn recovery_bonus_money(&self) -> i64 {
        if self.has_recovery_bonus() {
            (self.absence_days as i64 * 100).min(1000)
        } else {
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const BASE_TIME: f64 = 1_775_044_800_000.0;

    #[test]
    fn login_bonus_cumulative() {
        let mut lb = LoginBonusState::default();
        let reward = lb.process_login(BASE_TIME);
        assert!(reward.is_some());
        assert_eq!(lb.total_login_days, 1);
        let reward2 = lb.process_login(BASE_TIME + 1000.0);
        assert!(reward2.is_none());
    }

    #[test]
    fn login_bonus_next_day() {
        let mut lb = LoginBonusState::default();
        lb.process_login(BASE_TIME);
        let reward = lb.process_login(BASE_TIME + 25.0 * 3_600_000.0);
        assert!(reward.is_some());
        assert_eq!(lb.total_login_days, 2);
    }

    #[test]
    fn recovery_bonus_after_absence() {
        let mut lb = LoginBonusState::default();
        lb.process_login(BASE_TIME);
        lb.process_login(BASE_TIME + 5.0 * 24.0 * 3_600_000.0);
        assert!(lb.has_recovery_bonus());
        assert_eq!(lb.recovery_bonus_money(), 500);
    }

    #[test]
    fn login_rewards_include_gems() {
        let reward = &LOGIN_REWARDS[0];
        assert!(reward.gems > 0);
        assert!(reward.money > 0);
    }
}
