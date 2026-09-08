//! 遠征団 — 開口は「次: 防衛する」と主 CTA。配置とプッシャーが本編／育成。

use crate::critique::frame::{capture_frame, ScreenSnapshot};
use crate::critique::probe::{ActionFact, ProbeFacts, Subject};
use crate::games::expedition::actions::{
    drop_lane_id, ACK_RESULT, CANCEL_FORMING, CONFIRM_PLACEMENT, LAUNCH, START_FORMING,
};
use crate::games::expedition::logic;
use crate::games::expedition::render::{self, next_goal_line};
use crate::games::expedition::state::{ExpeditionState, HubTab, Screen};

pub struct ExpeditionSubject {
    state: ExpeditionState,
}

impl ExpeditionSubject {
    pub fn new() -> Self {
        Self {
            state: ExpeditionState::new(),
        }
    }
}

impl Subject for ExpeditionSubject {
    fn name(&self) -> &'static str {
        "expedition"
    }

    fn probe(&self) -> ProbeFacts {
        let goal = next_goal_line(&self.state);
        let (actions, phase) = match self.state.screen {
            Screen::Camp if self.state.hub_tab == HubTab::Arcade => (
                vec![ActionFact {
                    id: drop_lane_id(2),
                    label: if self.state.pending_level_pick {
                        "団員を選ぶ".into()
                    } else {
                        "メダルを落とす".into()
                    },
                    hint: Some('2'),
                    primary: true,
                }],
                "arcade",
            ),
            Screen::Camp => (
                vec![ActionFact {
                    id: START_FORMING,
                    label: if self.state.rations == 0 {
                        "糧が貯まるまで待つ".into()
                    } else {
                        "戦役に出る".into()
                    },
                    hint: None,
                    primary: true,
                }],
                "camp",
            ),
            Screen::Forming => (
                vec![
                    ActionFact {
                        id: LAUNCH,
                        label: "配置へ".into(),
                        hint: None,
                        primary: true,
                    },
                    ActionFact {
                        id: CANCEL_FORMING,
                        label: "やめる".into(),
                        hint: None,
                        primary: false,
                    },
                ],
                "forming",
            ),
            Screen::Placing => (
                vec![
                    ActionFact {
                        id: CONFIRM_PLACEMENT,
                        label: "防衛開始".into(),
                        hint: None,
                        primary: true,
                    },
                    ActionFact {
                        id: CANCEL_FORMING,
                        label: "撤退".into(),
                        hint: None,
                        primary: false,
                    },
                ],
                "placing",
            ),
            Screen::Running => (vec![], "running"),
            Screen::Result => (
                vec![ActionFact {
                    id: ACK_RESULT,
                    label: if self.state.last_failed {
                        "遊技場へ".into()
                    } else {
                        "拠点に戻る".into()
                    },
                    hint: None,
                    primary: true,
                }],
                "result",
            ),
        };

        ProbeFacts {
            phase: phase.into(),
            next_goal: Some(goal),
            actions,
            progress: vec![
                ("rations".into(), self.state.rations as f64),
                ("level".into(), self.state.total_level() as f64),
                ("chapter".into(), self.state.chapter as f64),
                ("medals".into(), self.state.medals as f64),
                ("orb_gauge".into(), self.state.orb_gauge as f64),
            ],
            recent_feedback: self.state.log.iter().rev().take(3).cloned().collect(),
        }
    }

    fn capture(&self, width: u16, height: u16) -> ScreenSnapshot {
        capture_frame(width, height, |f, cs| {
            render::render(&self.state, f, f.area(), cs);
        })
    }

    fn tick(&mut self, n: u32) {
        logic::tick(&mut self.state, n);
    }

    fn apply_action(&mut self, action_id: u16) -> bool {
        if let Some(lane) = crate::games::expedition::actions::lane_from_drop(action_id) {
            return logic::drop_medal(&mut self.state, lane as usize);
        }
        match action_id {
            START_FORMING => logic::primary_depart(&mut self.state),
            CANCEL_FORMING => logic::cancel_forming(&mut self.state),
            LAUNCH => logic::launch_sortie(&mut self.state),
            CONFIRM_PLACEMENT => logic::confirm_placement(&mut self.state),
            ACK_RESULT => logic::acknowledge_result(&mut self.state),
            _ => false,
        }
    }

    fn suggest_action(&self, _facts: &ProbeFacts, _screen: &ScreenSnapshot) -> Option<u16> {
        match self.state.screen {
            Screen::Camp if self.state.hub_tab == HubTab::Arcade && !self.state.pending_level_pick => {
                Some(drop_lane_id(2))
            }
            Screen::Camp if self.state.rations > 0 => Some(START_FORMING),
            Screen::Camp => None,
            Screen::Forming => Some(LAUNCH),
            Screen::Placing => Some(CONFIRM_PLACEMENT),
            Screen::Running => None,
            Screen::Result => Some(ACK_RESULT),
        }
    }
}
