//! Daily and weekly mission systems.

use serde::{Deserialize, Serialize};
use super::jst_day_number;

// ── Mission Types ────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum MissionType {
    RunBusiness(u32),
    ServeSpecial(u32),
    DiscoverRecipe(u32),
    ServeFavorite,
    /// Perform N character interactions.
    Interact(u32),
    /// Pull gacha N times.
    GachaPull(u32),
    /// Complete produce N times.
    ProduceComplete(u32),
    /// Reach score S in produce.
    ProduceScore(u32),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mission {
    pub name: String,
    pub mission_type: MissionType,
    pub progress: u32,
    pub target: u32,
    pub reward_money: i64,
    pub reward_gems: u32,
    pub completed: bool,
}

impl Mission {
    pub fn is_done(&self) -> bool {
        self.progress >= self.target
    }
}

// ── Daily Missions ───────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DailyMissionState {
    pub last_reset_day: u32,
    pub missions: Vec<Mission>,
    pub all_clear_claimed: bool,
}

impl DailyMissionState {
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

    pub fn record(&mut self, mission_type: MissionType) {
        for mission in &mut self.missions {
            if std::mem::discriminant(&mission.mission_type) == std::mem::discriminant(&mission_type)
                && !mission.completed
            {
                mission.progress += 1;
                if mission.is_done() {
                    mission.completed = true;
                }
            }
        }
    }

    pub fn all_complete(&self) -> bool {
        !self.missions.is_empty() && self.missions.iter().all(|m| m.completed)
    }

    #[allow(dead_code)] // Phase 2+: mission reward claim UI
    pub fn claimable_rewards(&self) -> (i64, u32) {
        let money: i64 = self.missions.iter().filter(|m| m.completed).map(|m| m.reward_money).sum();
        let gems: u32 = self.missions.iter().filter(|m| m.completed).map(|m| m.reward_gems).sum();
        (money, gems)
    }
}

fn generate_daily_missions() -> Vec<Mission> {
    vec![
        Mission {
            name: "営業を1回行う".into(),
            mission_type: MissionType::RunBusiness(1),
            progress: 0, target: 1,
            reward_money: 100, reward_gems: 20,
            completed: false,
        },
        Mission {
            name: "営業を3回行う".into(),
            mission_type: MissionType::RunBusiness(3),
            progress: 0, target: 3,
            reward_money: 300, reward_gems: 30,
            completed: false,
        },
        Mission {
            name: "常連客にお気に入りを出す".into(),
            mission_type: MissionType::ServeFavorite,
            progress: 0, target: 1,
            reward_money: 200, reward_gems: 20,
            completed: false,
        },
        Mission {
            name: "常連客と交流する".into(),
            mission_type: MissionType::Interact(1),
            progress: 0, target: 3,
            reward_money: 200, reward_gems: 30,
            completed: false,
        },
        Mission {
            name: "プロデュースを1回完了する".into(),
            mission_type: MissionType::ProduceComplete(1),
            progress: 0, target: 1,
            reward_money: 300, reward_gems: 50,
            completed: false,
        },
    ]
}

// ── Weekly Missions ──────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct WeeklyMissionState {
    pub last_reset_week: u32,
    pub missions: Vec<Mission>,
    pub all_clear_claimed: bool,
}

impl WeeklyMissionState {
    pub fn check_reset(&mut self, now_ms: f64) -> bool {
        let current_week = super::jst_week_number(now_ms);
        if current_week != self.last_reset_week {
            self.reset(current_week);
            true
        } else {
            false
        }
    }

    fn reset(&mut self, week_number: u32) {
        self.last_reset_week = week_number;
        self.all_clear_claimed = false;
        self.missions = generate_weekly_missions();
    }

    pub fn record(&mut self, mission_type: MissionType) {
        for mission in &mut self.missions {
            if std::mem::discriminant(&mission.mission_type) == std::mem::discriminant(&mission_type)
                && !mission.completed
            {
                mission.progress += 1;
                if mission.is_done() {
                    mission.completed = true;
                }
            }
        }
    }

    #[allow(dead_code)] // Phase 2+: weekly all-clear bonus UI
    pub fn all_complete(&self) -> bool {
        !self.missions.is_empty() && self.missions.iter().all(|m| m.completed)
    }
}

fn generate_weekly_missions() -> Vec<Mission> {
    vec![
        Mission {
            name: "営業を10回行う".into(),
            mission_type: MissionType::RunBusiness(10),
            progress: 0, target: 10,
            reward_money: 1000, reward_gems: 100,
            completed: false,
        },
        Mission {
            name: "ガチャを10回引く".into(),
            mission_type: MissionType::GachaPull(10),
            progress: 0, target: 10,
            reward_money: 500, reward_gems: 150,
            completed: false,
        },
        Mission {
            name: "プロデュースを5回完了する".into(),
            mission_type: MissionType::ProduceComplete(5),
            progress: 0, target: 5,
            reward_money: 2000, reward_gems: 200,
            completed: false,
        },
        Mission {
            name: "プロデュースでSランク以上を取る".into(),
            mission_type: MissionType::ProduceScore(1),
            progress: 0, target: 1,
            reward_money: 1000, reward_gems: 300,
            completed: false,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    const BASE_TIME: f64 = 1_775_044_800_000.0;

    #[test]
    fn daily_missions_reset() {
        let mut dm = DailyMissionState::default();
        dm.check_reset(BASE_TIME);
        assert_eq!(dm.missions.len(), 5);
        assert!(!dm.all_complete());
    }

    #[test]
    fn daily_missions_progress() {
        let mut dm = DailyMissionState::default();
        dm.check_reset(BASE_TIME);
        dm.record(MissionType::RunBusiness(1));
        assert!(dm.missions[0].completed);
        assert!(!dm.missions[1].completed);
    }

    #[test]
    fn weekly_missions_reset() {
        let mut wm = WeeklyMissionState::default();
        wm.check_reset(BASE_TIME);
        assert_eq!(wm.missions.len(), 4);
    }

    #[test]
    fn mission_rewards_include_gems() {
        let mut dm = DailyMissionState::default();
        dm.check_reset(BASE_TIME);
        dm.record(MissionType::RunBusiness(1));
        let (money, gems) = dm.claimable_rewards();
        assert!(money > 0);
        assert!(gems > 0);
    }
}
