//! 周回討伐 — 拠点の「遠征に出発する」CTA。

use crate::critique::frame::{capture_frame, ScreenSnapshot};
use crate::critique::probe::{ActionFact, ProbeFacts, Subject};
use crate::games::loopmarch::actions::{
    CAMP_START_OR_RESUME, HAND_CLICK_BASE, PATH_CLICK_BASE, REFILL_HAND,
};
use crate::games::loopmarch::logic;
use crate::games::loopmarch::render;
use crate::games::loopmarch::state::{
    LoopMarchState, Phase, REFILL_STONE_COST, REFILL_WOOD_COST,
};

pub struct LoopMarchSubject {
    state: LoopMarchState,
}

impl LoopMarchSubject {
    pub fn new() -> Self {
        Self {
            state: LoopMarchState::new(),
        }
    }
}

impl Subject for LoopMarchSubject {
    fn name(&self) -> &'static str {
        "loopmarch"
    }

    fn probe(&self) -> ProbeFacts {
        match self.state.phase {
            Phase::Camp => ProbeFacts {
                phase: "camp".into(),
                next_goal: Some("遠征に出発する".into()),
                actions: vec![ActionFact {
                    id: CAMP_START_OR_RESUME,
                    label: "遠征に出発する".into(),
                    hint: None,
                    primary: true,
                }],
                progress: vec![
                    ("soul".into(), self.state.soul as f64),
                    ("best_lap".into(), self.state.best_lap as f64),
                ],
                recent_feedback: self.state.log.iter().rev().take(3).cloned().collect(),
            },
            Phase::Expedition => {
                let hand_cards = self.state.hand.iter().filter(|c| c.is_some()).count();
                let tiles = self
                    .state
                    .path
                    .iter()
                    .filter(|s| s.terrain.is_some())
                    .count();
                let mut actions = Vec::new();
                if let Some(idx) = self.state.selected_hand {
                    actions.push(ActionFact {
                        id: HAND_CLICK_BASE + idx as u16,
                        label: "手札".into(),
                        hint: None,
                        primary: false,
                    });
                    actions.push(ActionFact {
                        id: PATH_CLICK_BASE,
                        label: "道".into(),
                        hint: None,
                        primary: true,
                    });
                } else if let Some((idx, _)) = self
                    .state
                    .hand
                    .iter()
                    .enumerate()
                    .find(|(_, c)| c.is_some())
                {
                    actions.push(ActionFact {
                        id: HAND_CLICK_BASE + idx as u16,
                        label: "手札".into(),
                        hint: None,
                        primary: true,
                    });
                }
                ProbeFacts {
                    phase: "expedition".into(),
                    next_goal: Some("カードを選んで→道をタップで配置".into()),
                    actions,
                    progress: vec![
                        ("lap".into(), self.state.lap as f64),
                        ("hp".into(), self.state.hero.hp as f64),
                        ("wood".into(), self.state.wood as f64),
                        ("stone".into(), self.state.stone as f64),
                        ("soul".into(), self.state.soul as f64),
                        ("hand_cards".into(), hand_cards as f64),
                        ("tiles".into(), tiles as f64),
                        (
                            "selected".into(),
                            self.state
                                .selected_hand
                                .map(|i| i as f64 + 1.0)
                                .unwrap_or(0.0),
                        ),
                    ],
                    recent_feedback: self.state.log.iter().rev().take(3).cloned().collect(),
                }
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
        if action_id == CAMP_START_OR_RESUME {
            logic::start_or_resume_expedition(&mut self.state);
            return true;
        }
        if action_id == REFILL_HAND {
            return logic::refill_hand(&mut self.state);
        }
        if (HAND_CLICK_BASE..HAND_CLICK_BASE + 4).contains(&action_id) {
            let idx = (action_id - HAND_CLICK_BASE) as usize;
            if self.state.hand.get(idx).is_some_and(|c| c.is_some()) {
                logic::select_hand(&mut self.state, idx);
                return true;
            }
            return false;
        }
        if action_id >= PATH_CLICK_BASE {
            let path_index = (action_id - PATH_CLICK_BASE) as usize;
            return logic::place_selected(&mut self.state, path_index);
        }
        false
    }

    fn suggest_action(&self, _facts: &ProbeFacts, _screen: &ScreenSnapshot) -> Option<u16> {
        match self.state.phase {
            Phase::Camp => Some(CAMP_START_OR_RESUME),
            Phase::Expedition => {
                if self.state.selected_hand.is_none() {
                    if let Some((i, _)) = self
                        .state
                        .hand
                        .iter()
                        .enumerate()
                        .find(|(_, c)| c.is_some())
                    {
                        return Some(HAND_CLICK_BASE + i as u16);
                    }
                    // 手札が空なら補充を試す（資源が足りなければセッション終了）
                    if self.state.wood >= REFILL_WOOD_COST && self.state.stone >= REFILL_STONE_COST {
                        return Some(REFILL_HAND);
                    }
                    return None;
                }
                // 空きマスへ置く。無ければ先頭。
                let slot = self
                    .state
                    .path
                    .iter()
                    .position(|s| s.terrain.is_none())
                    .unwrap_or(0);
                Some(PATH_CLICK_BASE + slot as u16)
            }
        }
    }
}
