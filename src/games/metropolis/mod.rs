//! Idle Metropolis — AI-driven city builder.
//!
//! The player buys upgrades and sets strategy; an automated CPU does the
//! actual placement.  Because the lowest AI tier is intentionally dumb,
//! `simulator.rs` is provided up-front to verify that even a bad CPU keeps
//! the game progressing (cash & population trending up over time).
//!
//! Architecture follows the project's "pure logic" pattern:
//!   • `state.rs`  — all data, no behavior.
//!   • `logic.rs`  — pure functions (tick, income, construction).
//!   • `ai.rs`     — strategy brains, one function per tier.
//!   • `simulator.rs` — balance tests (cargo test, no rendering).

pub mod ai;
pub mod logic;
pub mod render;
pub mod simulator;
pub mod state;
pub mod terrain;

use std::cell::RefCell;
use std::rc::Rc;

use ratzilla::ratatui::layout::Rect;
use ratzilla::ratatui::Frame;

use crate::games::{Game, GameChoice};
use crate::input::{ClickState, InputEvent};

use state::{City, PanelTab, Strategy};

// ── Action IDs scoped to MetropolisGame ─────────────────────────
//
// These are click/key actions on the manager panel.  Keep them stable —
// they're persisted through Click events keyed by `ClickScope::Game(...)`.
pub const ACT_STRATEGY_GROWTH: u16 = 1;
pub const ACT_STRATEGY_INCOME: u16 = 2;
/// 旧 `ACT_STRATEGY_BALANCED` の枠を流用。Tech 戦略 (建設速度 +20% / 収入 -20%)。
/// 数値 ID は永続クリックスコープのため変更しない。
pub const ACT_STRATEGY_TECH: u16 = 3;
pub const ACT_HIRE_WORKER: u16 = 4;
pub const ACT_UPGRADE_AI: u16 = 5;

// タブ切替アクション (10-13 を予約; 戦略の隣だが衝突しない)。
pub const ACT_TAB_STATUS: u16 = 10;
pub const ACT_TAB_MANAGER: u16 = 11;
pub const ACT_TAB_EVENTS: u16 = 12;
pub const ACT_TAB_WORLD: u16 = 13;

pub struct MetropolisGame {
    pub state: City,
}

impl MetropolisGame {
    pub fn new() -> Self {
        let mut state = City::new();
        state.push_event("🏙 都市建設を開始しました".to_string());
        Self { state }
    }
}

impl Default for MetropolisGame {
    fn default() -> Self {
        Self::new()
    }
}

impl Game for MetropolisGame {
    fn choice(&self) -> GameChoice {
        GameChoice::Metropolis
    }

    fn handle_input(&mut self, event: &InputEvent) -> bool {
        let action_id = match event {
            InputEvent::Click(_, id) => *id,
            InputEvent::Key(c) => match c {
                'g' | 'G' => ACT_STRATEGY_GROWTH,
                'i' | 'I' => ACT_STRATEGY_INCOME,
                't' | 'T' => ACT_STRATEGY_TECH,
                'w' | 'W' => ACT_HIRE_WORKER,
                'u' | 'U' => ACT_UPGRADE_AI,
                '1' => ACT_TAB_STATUS,
                '2' => ACT_TAB_MANAGER,
                '3' => ACT_TAB_EVENTS,
                '4' => ACT_TAB_WORLD,
                _ => return false,
            },
        };

        match action_id {
            ACT_STRATEGY_GROWTH => {
                set_strategy(&mut self.state, Strategy::Growth, "📈");
                true
            }
            ACT_STRATEGY_INCOME => {
                set_strategy(&mut self.state, Strategy::Income, "💰");
                true
            }
            ACT_STRATEGY_TECH => {
                set_strategy(&mut self.state, Strategy::Tech, "⚙");
                true
            }
            ACT_HIRE_WORKER => logic::hire_worker(&mut self.state),
            ACT_UPGRADE_AI => logic::upgrade_ai(&mut self.state),
            ACT_TAB_STATUS => {
                self.state.panel_tab = PanelTab::Status;
                true
            }
            ACT_TAB_MANAGER => {
                self.state.panel_tab = PanelTab::Manager;
                true
            }
            ACT_TAB_EVENTS => {
                self.state.panel_tab = PanelTab::Events;
                true
            }
            ACT_TAB_WORLD => {
                self.state.panel_tab = PanelTab::World;
                true
            }
            _ => false,
        }
    }

    fn tick(&mut self, delta_ticks: u32) {
        logic::tick(&mut self.state, delta_ticks);
    }

    fn render(&self, f: &mut Frame, area: Rect, click_state: &Rc<RefCell<ClickState>>) {
        render::render(&self.state, f, area, click_state);
    }
}

/// Strategy 切替時の共通処理。`logic::strategy_info` を引いて
/// 「ラベル + 副作用 (建設速度・収入修正)」を 1 行のイベントログにまとめる。
/// 切替時に「何が変わったか」が即座にログに見えるようにするのが目的。
fn set_strategy(city: &mut City, s: Strategy, icon: &str) {
    city.strategy = s;
    let info = logic::strategy_info(s);
    let mut suffix = String::new();
    if info.speed_bonus_pct != 0 {
        suffix.push_str(&format!(" / 建設{:+}%", info.speed_bonus_pct));
    }
    if info.income_penalty_pct != 0 {
        suffix.push_str(&format!(" / 収入{:+}%", info.income_penalty_pct));
    }
    city.push_event(format!(
        "{} 戦略: {} — {}{}",
        icon, info.label, info.tagline, suffix
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::ClickScope;

    fn click(id: u16) -> InputEvent {
        InputEvent::Click(ClickScope::Game(GameChoice::Metropolis), id)
    }

    #[test]
    fn strategy_keys_change_state() {
        let mut g = MetropolisGame::new();
        g.handle_input(&InputEvent::Key('g'));
        assert_eq!(g.state.strategy, Strategy::Growth);
        g.handle_input(&InputEvent::Key('i'));
        assert_eq!(g.state.strategy, Strategy::Income);
        g.handle_input(&click(ACT_STRATEGY_TECH));
        assert_eq!(g.state.strategy, Strategy::Tech);
    }

    #[test]
    fn unknown_input_returns_false() {
        let mut g = MetropolisGame::new();
        assert!(!g.handle_input(&InputEvent::Key('q')));
        assert!(!g.handle_input(&click(9999)));
    }

    #[test]
    fn hiring_costs_money() {
        let mut g = MetropolisGame::new();
        g.state.cash = 1000;
        let before = g.state.workers;
        assert!(g.handle_input(&InputEvent::Key('w')));
        assert_eq!(g.state.workers, before + 1);
        assert!(g.state.cash < 1000);
    }

    #[test]
    fn tick_advances_simulation() {
        let mut g = MetropolisGame::new();
        let before = g.state.tick;
        g.tick(50);
        assert_eq!(g.state.tick, before + 50);
    }
}
