//! 玉響 — ホールで台を選ぶ目標と、着席後の打ち出し CTA。

use crate::critique::frame::{capture_frame, ScreenSnapshot};
use crate::critique::probe::{ActionFact, ProbeFacts, Subject};
use crate::games::pachinko::actions::{MACHINE_SELECT_BASE, TOGGLE_FIRE};
use crate::games::pachinko::logic;
use crate::games::pachinko::render;
use crate::games::pachinko::state::PachinkoState;

pub struct PachinkoSubject {
    state: PachinkoState,
}

impl PachinkoSubject {
    pub fn new() -> Self {
        let mut state = PachinkoState::new();
        logic::generate_hall(&mut state);
        Self { state }
    }
}

impl Subject for PachinkoSubject {
    fn name(&self) -> &'static str {
        "pachinko"
    }

    fn probe(&self) -> ProbeFacts {
        if !self.state.has_seated {
            ProbeFacts {
                phase: "hall".into(),
                next_goal: Some("台を選ぶ".into()),
                actions: vec![ActionFact {
                    id: MACHINE_SELECT_BASE,
                    label: "台を選ぶ".into(),
                    hint: None,
                    primary: true,
                }],
                progress: vec![
                    ("cash".into(), self.state.cash as f64),
                    ("machines".into(), self.state.machines.len() as f64),
                ],
                recent_feedback: self.state.log.iter().rev().take(3).cloned().collect(),
            }
        } else {
            let spins = self
                .state
                .seated_machine()
                .map(|m| m.spins_seen)
                .unwrap_or(0);
            ProbeFacts {
                phase: "playing".into(),
                next_goal: Some("打ち出し".into()),
                actions: vec![ActionFact {
                    id: TOGGLE_FIRE,
                    label: "打ち出し".into(),
                    hint: Some('A'),
                    primary: true,
                }],
                progress: vec![
                    ("balls_held".into(), self.state.balls_held as f64),
                    ("balls_on_board".into(), self.state.balls.len() as f64),
                    ("spins".into(), spins as f64),
                ],
                recent_feedback: self.state.log.iter().rev().take(3).cloned().collect(),
            }
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
        if !self.state.has_seated {
            if action_id >= MACHINE_SELECT_BASE {
                let index = (action_id - MACHINE_SELECT_BASE) as usize;
                return logic::sit_at(&mut self.state, index);
            }
            return false;
        }
        if action_id == TOGGLE_FIRE {
            return logic::toggle_fire(&mut self.state);
        }
        false
    }

    fn suggest_action(&self, _facts: &ProbeFacts, _screen: &ScreenSnapshot) -> Option<u16> {
        if !self.state.has_seated {
            Some(MACHINE_SELECT_BASE)
        } else if !self.state.firing {
            Some(TOGGLE_FIRE)
        } else {
            None
        }
    }
}
