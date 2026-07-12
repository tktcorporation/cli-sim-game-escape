//! 穴掘り長屋 — 1日5回の行動力で自分の庭を掘るか、ご近所のお福分け穴を
//! 掘らせてもらうかを選ぶ、日付駆動のソーシャル穴掘りゲーム。

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

use actions::{ACT_TAB_COLLECTION, ACT_TAB_NEIGHBORS, ACT_TAB_YARD, ACT_UPGRADE_SHOVEL};
use state::{DigState, DigTab, NEIGHBOR_COUNT};

pub struct DigGame {
    pub state: DigState,
    save_countdown: u32,
}

impl DigGame {
    pub fn new() -> Self {
        Self {
            state: Self::load_or_new_state(),
            save_countdown: save::AUTOSAVE_INTERVAL,
        }
    }

    /// セーブデータをロードし、直後に日付チェックまで済ませた state を返す。
    /// ロード直後にチェックしておくことで、タブを開いた瞬間から「今日」の
    /// 行動力・庭が正しい状態で表示される。
    #[cfg(target_arch = "wasm32")]
    fn load_or_new_state() -> DigState {
        let mut state = DigState::new();
        if save::load_game(&mut state) {
            state.add_log("セーブデータをロード");
        }
        let now_ms = save::wall_clock_now_ms();
        logic::maybe_reset_day(&mut state, now_ms);
        state
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn load_or_new_state() -> DigState {
        DigState::new()
    }

    fn handle_click(&mut self, action_id: u16) -> bool {
        match action_id {
            ACT_TAB_YARD => {
                self.state.selected_tab = DigTab::Yard;
                true
            }
            ACT_TAB_NEIGHBORS => {
                self.state.selected_tab = DigTab::Neighbors;
                true
            }
            ACT_TAB_COLLECTION => {
                self.state.selected_tab = DigTab::Collection;
                true
            }
            ACT_UPGRADE_SHOVEL => logic::buy_shovel_upgrade(&mut self.state),
            _ => {
                if let Some(idx) = actions::decode_grid(action_id) {
                    logic::dig_yard(&mut self.state, idx)
                } else if let Some(idx) = actions::decode_neighbor(action_id, NEIGHBOR_COUNT) {
                    logic::dig_neighbor(&mut self.state, idx)
                } else {
                    false
                }
            }
        }
    }

    fn handle_key(&mut self, key: char) -> bool {
        // グリッドのマス単体を狙う操作はポインター前提なのでキー未対応
        // (merge と同じ方針)。タブ切替と強化購入だけキーで提供する。
        match key {
            '1' => {
                self.state.selected_tab = DigTab::Yard;
                true
            }
            '2' => {
                self.state.selected_tab = DigTab::Neighbors;
                true
            }
            '3' => {
                self.state.selected_tab = DigTab::Collection;
                true
            }
            'u' | 'U' => logic::buy_shovel_upgrade(&mut self.state),
            _ => false,
        }
    }
}

impl Default for DigGame {
    fn default() -> Self {
        Self::new()
    }
}

impl Game for DigGame {
    fn choice(&self) -> GameChoice {
        GameChoice::Dig
    }

    fn handle_input(&mut self, event: &InputEvent) -> bool {
        let consumed = match event {
            InputEvent::Key(c) => self.handle_key(*c),
            InputEvent::Click(_, id) => self.handle_click(*id),
        };

        #[cfg(target_arch = "wasm32")]
        if consumed {
            save::save_game(&self.state);
        }

        consumed
    }

    fn tick(&mut self, delta_ticks: u32) {
        #[cfg(target_arch = "wasm32")]
        {
            let now_ms = save::wall_clock_now_ms();
            logic::maybe_reset_day(&mut self.state, now_ms);
        }
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
    use actions::{ACT_NEIGHBOR_DIG_BASE, GRID_CLICK_BASE};

    fn click(id: u16) -> InputEvent {
        InputEvent::Click(ClickScope::Game(GameChoice::Dig), id)
    }

    #[test]
    fn 庭セルをクリックすると掘って消費する() {
        let mut g = DigGame::new();
        let before = g.state.actions_remaining;
        assert!(g.handle_input(&click(GRID_CLICK_BASE)));
        assert_eq!(g.state.actions_remaining, before - 1);
        assert!(g.state.yard[0].is_some());
    }

    #[test]
    fn タブクリックでselected_tabが切り替わる() {
        let mut g = DigGame::new();
        assert_eq!(g.state.selected_tab, DigTab::Yard);
        g.handle_input(&click(ACT_TAB_NEIGHBORS));
        assert_eq!(g.state.selected_tab, DigTab::Neighbors);
        g.handle_input(&click(ACT_TAB_COLLECTION));
        assert_eq!(g.state.selected_tab, DigTab::Collection);
        g.handle_input(&click(ACT_TAB_YARD));
        assert_eq!(g.state.selected_tab, DigTab::Yard);
    }

    #[test]
    fn 未知のクリックはfalseを返す() {
        let mut g = DigGame::new();
        assert!(!g.handle_input(&click(9999)));
    }

    #[test]
    fn ご近所のお福分け穴クリックで行動力を消費する() {
        let mut g = DigGame::new();
        let before = g.state.actions_remaining;
        assert!(g.handle_input(&click(ACT_NEIGHBOR_DIG_BASE)));
        assert_eq!(g.state.actions_remaining, before - 1);
        assert!(g.state.neighbors[0].dug_today);
    }

    #[test]
    fn 数字キーでタブが切り替わる() {
        let mut g = DigGame::new();
        assert!(g.handle_input(&InputEvent::Key('2')));
        assert_eq!(g.state.selected_tab, DigTab::Neighbors);
        assert!(g.handle_input(&InputEvent::Key('3')));
        assert_eq!(g.state.selected_tab, DigTab::Collection);
        assert!(g.handle_input(&InputEvent::Key('1')));
        assert_eq!(g.state.selected_tab, DigTab::Yard);
    }

    #[test]
    fn uキーでシャベル強化を購入する() {
        let mut g = DigGame::new();
        g.state.coins = 100;
        assert!(g.handle_input(&InputEvent::Key('u')));
        assert_eq!(g.state.shovel_level, 1);
    }

    #[test]
    fn 未知のキーはfalseを返す() {
        let mut g = DigGame::new();
        assert!(!g.handle_input(&InputEvent::Key('z')));
    }

    #[test]
    fn tickは図鑑フラッシュを減衰させる() {
        let mut g = DigGame::new();
        g.state.collection_flash = Some((state::CollectionSet::Dragon, 3));
        g.tick(2);
        assert_eq!(g.state.collection_flash, Some((state::CollectionSet::Dragon, 1)));
        g.tick(5);
        assert_eq!(g.state.collection_flash, None);
    }
}
