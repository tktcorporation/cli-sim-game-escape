//! 遠征団 — 開口は「次: 遠征に出る」と主 CTA。

use crate::critique::frame::{capture_frame, ScreenSnapshot};
use crate::critique::probe::{ActionFact, ProbeFacts, Subject};
use crate::games::expedition::actions::{
    ACK_RESULT, CANCEL_FORMING, LAUNCH, LAUNCH_WITH_SCOUT, START_FORMING, USE_AID,
};
use crate::games::expedition::logic;
use crate::games::expedition::render::{self, next_goal_line};
use crate::games::expedition::state::{ExpeditionState, Screen};

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
            Screen::Camp => (
                vec![ActionFact {
                    id: START_FORMING,
                    label: if self.state.rations == 0 {
                        "糧が貯まるまで待つ".into()
                    } else {
                        "遠征に出る".into()
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
                        label: "出発する".into(),
                        hint: None,
                        primary: true,
                    },
                    ActionFact {
                        id: LAUNCH_WITH_SCOUT,
                        label: "下調べ出発".into(),
                        hint: None,
                        primary: false,
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
            Screen::Running => {
                let mut actions = vec![];
                if self.state
                    .sortie
                    .as_ref()
                    .map(|s| s.aid_ready && s.enemy.is_some())
                    .unwrap_or(false)
                {
                    actions.push(ActionFact {
                        id: USE_AID,
                        label: "援護する".into(),
                        hint: Some('A'),
                        primary: true,
                    });
                }
                (actions, "running")
            }
            Screen::Result => (
                vec![ActionFact {
                    id: ACK_RESULT,
                    label: "拠点に戻る".into(),
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
                ("best_depth".into(), self.state.best_depth as f64),
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
        match action_id {
            START_FORMING => logic::primary_depart(&mut self.state),
            CANCEL_FORMING => logic::cancel_forming(&mut self.state),
            LAUNCH => logic::launch_sortie(&mut self.state, false),
            LAUNCH_WITH_SCOUT => logic::launch_sortie(&mut self.state, true),
            USE_AID => logic::use_aid(&mut self.state),
            ACK_RESULT => logic::acknowledge_result(&mut self.state),
            _ => false,
        }
    }

    fn suggest_action(&self, _facts: &ProbeFacts, _screen: &ScreenSnapshot) -> Option<u16> {
        match self.state.screen {
            Screen::Camp if self.state.rations > 0 => Some(START_FORMING),
            Screen::Camp => None,
            Screen::Forming => Some(LAUNCH),
            Screen::Running => None, // オート完走が基本。援護は任意
            Screen::Result => Some(ACK_RESULT),
        }
    }
}
