//! Cookie Factory — 開口は「CLICK!」が主操作。

use crate::critique::frame::{capture_frame, ScreenSnapshot};
use crate::critique::probe::{ActionFact, ProbeFacts, Subject};
use crate::games::cookie::actions::{BUY_PRODUCER_BASE, CLICK_COOKIE, TAB_PRODUCERS};
use crate::games::cookie::logic;
use crate::games::cookie::render;
use crate::games::cookie::state::{CookieState, ProducerKind};

pub struct CookieSubject {
    state: CookieState,
}

impl CookieSubject {
    pub fn new() -> Self {
        Self {
            state: CookieState::new(),
        }
    }
}

impl Subject for CookieSubject {
    fn name(&self) -> &'static str {
        "cookie"
    }

    fn probe(&self) -> ProbeFacts {
        let mut actions = vec![ActionFact {
            id: CLICK_COOKIE,
            label: "CLICK!".into(),
            hint: None,
            primary: true,
        }];
        actions.push(ActionFact {
            id: TAB_PRODUCERS,
            label: "生産".into(),
            hint: None,
            primary: false,
        });
        if let Some(kind) = ProducerKind::all().iter().find(|k| {
            let p = &self.state.producers[k.index()];
            self.state.cookies >= p.cost()
        }) {
            actions.push(ActionFact {
                id: BUY_PRODUCER_BASE + kind.index() as u16,
                label: kind.name().to_string(),
                hint: None,
                primary: false,
            });
        }
        ProbeFacts {
            phase: "main".into(),
            // Cookie は明示目標 UI を持たない。未設計として GoalVisibility は
            // 減点しない（note で分かる）。
            next_goal: None,
            actions,
            progress: vec![
                ("cookies".into(), self.state.cookies),
                ("cps".into(), self.state.total_cps()),
                ("clicks".into(), self.state.total_clicks as f64),
            ],
            recent_feedback: self
                .state
                .log
                .iter()
                .rev()
                .take(3)
                .map(|l| l.text.clone())
                .collect(),
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
        if action_id == CLICK_COOKIE {
            logic::click(&mut self.state);
            return true;
        }
        if action_id == TAB_PRODUCERS {
            self.state.show_upgrades = false;
            self.state.show_research = false;
            self.state.show_milestones = false;
            self.state.show_prestige = false;
            return true;
        }
        if (BUY_PRODUCER_BASE..BUY_PRODUCER_BASE + 12).contains(&action_id) {
            let idx = (action_id - BUY_PRODUCER_BASE) as usize;
            if let Some(kind) = ProducerKind::from_index(idx) {
                return logic::buy_producer(&mut self.state, &kind);
            }
        }
        false
    }

    fn suggest_action(&self, _facts: &ProbeFacts, _screen: &ScreenSnapshot) -> Option<u16> {
        if let Some(kind) = ProducerKind::all().iter().find(|k| {
            let p = &self.state.producers[k.index()];
            self.state.cookies >= p.cost()
        }) {
            return Some(BUY_PRODUCER_BASE + kind.index() as u16);
        }
        Some(CLICK_COOKIE)
    }
}
