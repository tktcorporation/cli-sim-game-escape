//! 破壊VFX比較ラボ — 放置ゲーム本編ではなく、破壊表現の手触りを
//! 並べて見比べるための試作画面。Everlight と同じ Canvas+Braille を使う。

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

use actions::style_for_tab;
use state::{DemoStyle, ShatterLabState};

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
                self.state.set_style(DemoStyle::OreBomb);
                true
            }
            InputEvent::Key('2') => {
                self.state.set_style(DemoStyle::PressCrush);
                true
            }
            InputEvent::Key('3') => {
                self.state.set_style(DemoStyle::PlanetPeel);
                true
            }
            InputEvent::Key('4') => {
                self.state.set_style(DemoStyle::CityCollapse);
                true
            }
            InputEvent::Click(_, id) => {
                if let Some(style) = style_for_tab(*id) {
                    self.state.set_style(style);
                    true
                } else {
                    false
                }
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
