//! たまごっち風育成ゲーム — 卵から成長させて寿命を伸ばす。

pub mod actions;
pub mod logic;
pub mod render;
pub mod save;
pub mod state;

use std::cell::RefCell;
use std::rc::Rc;

use ratzilla::ratatui::layout::Rect;
use ratzilla::ratatui::Frame;

use crate::games::{Game, GameChoice};
use crate::input::{ClickState, InputEvent};

use actions::*;
use state::TamaState;

pub struct TamagotchiGame {
    pub state: TamaState,
    save_countdown: u32,
}

impl TamagotchiGame {
    pub fn new() -> Self {
        let state = TamaState::new();

        #[cfg(target_arch = "wasm32")]
        let state = {
            let mut s = state;
            if save::load_game(&mut s) {
                s.add_log("セーブデータをロード");
            }
            s
        };

        Self {
            state,
            save_countdown: save::AUTOSAVE_INTERVAL,
        }
    }

    fn handle_click(&mut self, action_id: u16) -> bool {
        match action_id {
            ACT_FEED => {
                logic::feed(&mut self.state);
                true
            }
            ACT_PLAY => {
                logic::play(&mut self.state);
                true
            }
            ACT_BATH => {
                logic::bath(&mut self.state);
                true
            }
            ACT_MEDICINE => {
                logic::medicine(&mut self.state);
                true
            }
            ACT_SLEEP_TOGGLE => {
                logic::toggle_sleep(&mut self.state);
                true
            }
            ACT_HATCH => {
                logic::hatch(&mut self.state);
                true
            }
            ACT_NEW_PET => {
                logic::start_new_generation(&mut self.state);
                true
            }
            ACT_PET => {
                logic::pet(&mut self.state);
                true
            }
            _ => false,
        }
    }

    fn handle_key(&mut self, key: char) -> bool {
        // 大文字小文字を等価に扱う。
        let key = key.to_ascii_lowercase();
        match key {
            'f' => {
                logic::feed(&mut self.state);
                true
            }
            'p' => {
                logic::play(&mut self.state);
                true
            }
            'b' => {
                logic::bath(&mut self.state);
                true
            }
            'm' => {
                logic::medicine(&mut self.state);
                true
            }
            's' => {
                logic::toggle_sleep(&mut self.state);
                true
            }
            'n' => {
                logic::start_new_generation(&mut self.state);
                true
            }
            // 卵タップ / なで操作は Space に集約。卵なら孵化、生きてればなで、
            // 死んでれば新世代開始 — 1 つのキーで「いま画面が促してる行動」を行う。
            ' ' => {
                if self.state.is_egg() {
                    logic::hatch(&mut self.state);
                } else if self.state.is_dead() {
                    logic::start_new_generation(&mut self.state);
                } else {
                    logic::pet(&mut self.state);
                }
                true
            }
            _ => false,
        }
    }
}

impl Default for TamagotchiGame {
    fn default() -> Self {
        Self::new()
    }
}

impl Game for TamagotchiGame {
    fn choice(&self) -> GameChoice {
        GameChoice::Tamagotchi
    }

    fn handle_input(&mut self, event: &InputEvent) -> bool {
        let consumed = match event {
            InputEvent::Key(c) => self.handle_key(*c),
            InputEvent::Click(_, id) => self.handle_click(*id),
        };

        // アクションでステータスが大きく動いた直後に保存しておくと、
        // ブラウザを閉じても直近の進行が落ちにくい。tick ベースの定期
        // 保存と二重防御。
        #[cfg(target_arch = "wasm32")]
        if consumed {
            save::save_game(&self.state);
        }

        consumed
    }

    fn tick(&mut self, delta_ticks: u32) {
        logic::tick(&mut self.state, delta_ticks);

        self.save_countdown = self.save_countdown.saturating_sub(delta_ticks);
        if self.save_countdown == 0 {
            #[cfg(target_arch = "wasm32")]
            save::save_game(&self.state);
            self.save_countdown = save::AUTOSAVE_INTERVAL;
        }
    }

    fn render(&self, f: &mut Frame, area: Rect, click_state: &Rc<RefCell<ClickState>>) {
        render::render(&self.state, f, area, click_state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::ClickScope;

    fn click(id: u16) -> InputEvent {
        InputEvent::Click(ClickScope::Game(GameChoice::Tamagotchi), id)
    }

    #[test]
    fn click_hatch_starts_baby() {
        let mut g = TamagotchiGame::new();
        assert!(g.state.is_egg());
        g.handle_input(&click(ACT_HATCH));
        assert_eq!(g.state.stage, state::Stage::Baby);
    }

    #[test]
    fn key_space_hatches_egg() {
        let mut g = TamagotchiGame::new();
        g.handle_input(&InputEvent::Key(' '));
        assert_eq!(g.state.stage, state::Stage::Baby);
    }

    #[test]
    fn key_space_pets_when_alive() {
        let mut g = TamagotchiGame::new();
        g.handle_input(&InputEvent::Key(' ')); // hatch
        g.state.stats.happiness = 50;
        g.handle_input(&InputEvent::Key(' ')); // pet
        assert_eq!(g.state.stats.happiness, 54);
    }

    #[test]
    fn click_feed_increases_hunger() {
        let mut g = TamagotchiGame::new();
        g.handle_input(&click(ACT_HATCH));
        g.state.stats.hunger = 40;
        g.handle_input(&click(ACT_FEED));
        assert_eq!(g.state.stats.hunger, 70);
    }

    #[test]
    fn key_f_feeds() {
        let mut g = TamagotchiGame::new();
        g.handle_input(&click(ACT_HATCH));
        g.state.stats.hunger = 40;
        g.handle_input(&InputEvent::Key('f'));
        assert_eq!(g.state.stats.hunger, 70);
        // 大文字も同じ
        g.state.stats.hunger = 40;
        g.handle_input(&InputEvent::Key('F'));
        assert_eq!(g.state.stats.hunger, 70);
    }

    #[test]
    fn click_sleep_toggles() {
        let mut g = TamagotchiGame::new();
        g.handle_input(&click(ACT_HATCH));
        assert!(!g.state.sleeping);
        g.handle_input(&click(ACT_SLEEP_TOGGLE));
        assert!(g.state.sleeping);
        g.handle_input(&click(ACT_SLEEP_TOGGLE));
        assert!(!g.state.sleeping);
    }

    #[test]
    fn unknown_click_action_returns_false() {
        let mut g = TamagotchiGame::new();
        assert!(!g.handle_input(&click(9999)));
    }

    #[test]
    fn tick_advances_state() {
        let mut g = TamagotchiGame::new();
        g.handle_input(&click(ACT_HATCH));
        g.tick(50);
        assert_eq!(g.state.age_ticks, 50);
    }
}
