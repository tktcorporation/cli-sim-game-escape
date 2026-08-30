//! 常夜灯 — 拠点の目標行と「夜番へ出る」CTA を評価する。

use crate::critique::frame::{capture_frame, ScreenSnapshot};
use crate::critique::probe::{ActionFact, ProbeFacts, Subject};
use crate::games::everlight::actions::{CAMP_START_VIGIL, CAMP_TAB_PREPARE, LANE_CLICK_BASE};
use crate::games::everlight::logic;
use crate::games::everlight::render;
use crate::games::everlight::state::{CampTab, EverlightState, Phase, COLUMNS, WORLD_W};

pub struct EverlightSubject {
    state: EverlightState,
}

impl EverlightSubject {
    pub fn new() -> Self {
        Self {
            state: EverlightState::new(),
        }
    }
}

impl Subject for EverlightSubject {
    fn name(&self) -> &'static str {
        "everlight"
    }

    fn probe(&self) -> ProbeFacts {
        match self.state.phase {
            Phase::Camp => {
                let rank = self.state.camp.effective_selected_rank();
                let milestone = logic::milestone_wave(rank);
                let boss = logic::boss_kind_for(milestone, rank).name();
                ProbeFacts {
                    phase: "camp".into(),
                    next_goal: Some(format!("第{milestone}波『{boss}』を討伐する")),
                    actions: vec![
                        ActionFact {
                            id: CAMP_START_VIGIL,
                            label: "夜番へ出る".into(),
                            hint: None,
                            primary: true,
                        },
                        ActionFact {
                            id: CAMP_TAB_PREPARE,
                            label: "出撃".into(),
                            hint: None,
                            primary: false,
                        },
                    ],
                    progress: vec![
                        ("ember".into(), self.state.ember as f64),
                        ("max_rank".into(), self.state.camp.max_unlocked_rank as f64),
                    ],
                    recent_feedback: self.state.log.iter().rev().take(3).cloned().collect(),
                }
            }
            Phase::Vigil => ProbeFacts {
                phase: "vigil".into(),
                next_goal: Some(format!("第{}波", self.state.wave)),
                actions: vec![ActionFact {
                    id: LANE_CLICK_BASE + self.state.lantern.target_lane as u16,
                    label: "灯".into(),
                    hint: None,
                    primary: true,
                }],
                progress: vec![
                    ("wave".into(), self.state.wave as f64),
                    ("light".into(), self.state.lantern.light as f64),
                    ("kills".into(), self.state.kill_count as f64),
                    // 主操作（レーン移動）そのものを進捗として見る。
                    // kills だけだと無意味な再タップが feedback を下げる。
                    (
                        "target_lane".into(),
                        self.state.lantern.target_lane as f64,
                    ),
                ],
                recent_feedback: self.state.log.iter().rev().take(3).cloned().collect(),
            },
        }
    }

    fn capture(&self, width: u16, height: u16) -> ScreenSnapshot {
        capture_frame(width, height, |f, cs| {
            render::render(&self.state, f, f.area(), cs);
        })
    }

    fn tick(&mut self, n: u32) {
        logic::tick_n(&mut self.state, n);
    }

    fn apply_action(&mut self, action_id: u16) -> bool {
        if action_id == CAMP_START_VIGIL {
            if self.state.phase == Phase::Camp {
                logic::start_vigil(&mut self.state);
                return true;
            }
            return false;
        }
        if action_id == CAMP_TAB_PREPARE {
            self.state.camp_tab = CampTab::Prepare;
            return true;
        }
        if (LANE_CLICK_BASE..LANE_CLICK_BASE + COLUMNS as u16).contains(&action_id) {
            let lane = (action_id - LANE_CLICK_BASE) as usize;
            logic::set_lantern_target_lane(&mut self.state, lane);
            return true;
        }
        false
    }

    fn suggest_action(&self, _facts: &ProbeFacts, _screen: &ScreenSnapshot) -> Option<u16> {
        match self.state.phase {
            Phase::Camp => Some(CAMP_START_VIGIL),
            Phase::Vigil => {
                // シミュレーターと同じく、敵が多いレーンへ灯を向ける。
                // 固定レーン再タップは no-op で feedback を落とすだけなので避ける。
                let mut counts = vec![0u32; COLUMNS];
                let lane_w = WORLD_W / COLUMNS as f64;
                for e in &self.state.enemies {
                    let lane = ((e.x / lane_w) as usize).min(COLUMNS - 1);
                    counts[lane] += 1;
                }
                let best = counts
                    .iter()
                    .enumerate()
                    .max_by_key(|&(_, c)| *c)
                    .map(|(lane, _)| lane)
                    .unwrap_or(self.state.lantern.target_lane);
                let lane = if best == self.state.lantern.target_lane {
                    (self.state.lantern.target_lane + 2) % COLUMNS
                } else {
                    best
                };
                Some(LANE_CLICK_BASE + lane as u16)
            }
        }
    }
}
