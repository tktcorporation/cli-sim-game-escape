//! 破壊VFX比較ラボ — 放置ゲーム本編ではなく、
//! 「強化しがいが見える破壊舞台」を並べて見比べる試作。

mod actions;
mod logic;
mod render;
mod state;

use std::cell::RefCell;
use std::rc::Rc;

use ratzilla::ratatui::layout::Rect;
use ratzilla::ratatui::Frame;

use crate::games::{Game, GameChoice};
use crate::input::{ClickState, InputEvent};

use actions::{power_for_tab, style_for_tab, TAB_POWER_AUTO};
use state::{DemoStyle, PowerLevel, ShatterLabState};

pub struct ShatterLabGame {
    state: ShatterLabState,
}

impl Default for ShatterLabGame {
    fn default() -> Self {
        Self::new()
    }
}

impl ShatterLabGame {
    pub fn new() -> Self {
        Self {
            state: ShatterLabState::new(),
        }
    }
}

impl Game for ShatterLabGame {
    fn choice(&self) -> GameChoice {
        GameChoice::ShatterLab
    }

    fn handle_input(&mut self, event: &InputEvent) -> bool {
        match event {
            InputEvent::Key('1') => {
                self.state.set_style(DemoStyle::SpaceCruise);
                true
            }
            InputEvent::Key('2') => {
                self.state.set_style(DemoStyle::OrbitMine);
                true
            }
            InputEvent::Key('3') => {
                self.state.set_style(DemoStyle::RailBreak);
                true
            }
            InputEvent::Key('4') => {
                self.state.set_style(DemoStyle::SatDefense);
                true
            }
            InputEvent::Key('q') | InputEvent::Key('Q') => {
                self.state.set_power(PowerLevel::Low);
                true
            }
            InputEvent::Key('w') | InputEvent::Key('W') => {
                self.state.set_power(PowerLevel::Mid);
                true
            }
            InputEvent::Key('e') | InputEvent::Key('E') => {
                self.state.set_power(PowerLevel::High);
                true
            }
            InputEvent::Key('a') | InputEvent::Key('A') => {
                self.state.enable_auto_power();
                true
            }
            InputEvent::Click(_, id) => {
                if *id == TAB_POWER_AUTO {
                    self.state.enable_auto_power();
                    return true;
                }
                if let Some(power) = power_for_tab(*id) {
                    self.state.set_power(power);
                    return true;
                }
                if let Some(style) = style_for_tab(*id) {
                    self.state.set_style(style);
                    return true;
                }
                false
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
