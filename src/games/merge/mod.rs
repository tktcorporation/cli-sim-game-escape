//! マージマンション風マージゲーム。
//!
//! 6×5 の盤面で同種同レベルのアイテムをマージして上位レベルを作り、
//! クエストにアイテムを納品してコインを稼ぐ。

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
use state::{MergeState, QUEST_SLOTS};

pub struct MergeGame {
    pub state: MergeState,
    save_countdown: u32,
}

impl MergeGame {
    pub fn new() -> Self {
        let mut state = MergeState::new();

        #[cfg(target_arch = "wasm32")]
        if save::load_game(&mut state) {
            state.add_log("セーブデータをロード");
        }

        // クエストが空ならその場で 1 回 tick して埋める (起動直後にクエスト
        // 表示が空白だと「何をすればいいか分からない」体験になる)。
        if state.quests.iter().all(|q| q.is_none()) {
            logic::tick(&mut state, 1);
        }

        Self {
            state,
            save_countdown: save::AUTOSAVE_INTERVAL,
        }
    }

    fn handle_click(&mut self, action_id: u16) -> bool {
        if let Some((col, row)) = actions::decode_grid(action_id) {
            logic::tap_cell(&mut self.state, col, row);
            return true;
        }
        if (ACT_QUEST_DELIVER_BASE..ACT_QUEST_DELIVER_BASE + QUEST_SLOTS as u16)
            .contains(&action_id)
        {
            let slot = (action_id - ACT_QUEST_DELIVER_BASE) as usize;
            logic::deliver_quest(&mut self.state, slot);
            return true;
        }
        if (ACT_QUEST_REROLL_BASE..ACT_QUEST_REROLL_BASE + QUEST_SLOTS as u16).contains(&action_id) {
            let slot = (action_id - ACT_QUEST_REROLL_BASE) as usize;
            logic::reroll_quest(&mut self.state, slot);
            return true;
        }
        match action_id {
            ACT_UPGRADE_GENERATORS => {
                logic::buy_upgrade(&mut self.state);
                true
            }
            ACT_CLEAR_SELECTION => {
                logic::clear_selection(&mut self.state);
                true
            }
            _ => false,
        }
    }

    fn handle_key(&mut self, key: char) -> bool {
        // 数字 1/2/3 でクエスト納品。盤面操作はキーボードでは提供しない
        // (グリッド操作は本質的にポインター UI なので)。
        match key {
            '1' => {
                logic::deliver_quest(&mut self.state, 0);
                true
            }
            '2' => {
                logic::deliver_quest(&mut self.state, 1);
                true
            }
            '3' => {
                logic::deliver_quest(&mut self.state, 2);
                true
            }
            'u' | 'U' => {
                logic::buy_upgrade(&mut self.state);
                true
            }
            'c' | 'C' => {
                logic::clear_selection(&mut self.state);
                true
            }
            _ => false,
        }
    }
}

impl Default for MergeGame {
    fn default() -> Self {
        Self::new()
    }
}

impl Game for MergeGame {
    fn choice(&self) -> GameChoice {
        GameChoice::Merge
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
    use super::state::{Cell, ItemType, Quest};
    use crate::input::ClickScope;

    fn click(id: u16) -> InputEvent {
        InputEvent::Click(ClickScope::Game(GameChoice::Merge), id)
    }

    #[test]
    fn click_generator_creates_item() {
        let mut g = MergeGame::new();
        // (0,0) = Flower generator → action id = GRID_CLICK_BASE + 0
        g.handle_input(&click(GRID_CLICK_BASE));
        let items: usize = g
            .state
            .grid
            .iter()
            .filter(|c| matches!(c, Cell::Item(_, _)))
            .count();
        assert!(items >= 1);
    }

    #[test]
    fn click_quest_deliver_pays_when_inventory_present() {
        let mut g = MergeGame::new();
        g.state.quests[0] = Some(Quest {
            item_type: ItemType::Flower,
            level: 1,
            needed: 1,
            reward: 20,
        });
        // 盤面に 1 個用意
        g.state.set(1, 1, Cell::Item(ItemType::Flower, 1));
        g.handle_input(&click(ACT_QUEST_DELIVER_BASE));
        assert!(g.state.coins >= 20);
    }

    #[test]
    fn key_1_delivers_first_quest() {
        let mut g = MergeGame::new();
        g.state.quests[0] = Some(Quest {
            item_type: ItemType::Gem,
            level: 1,
            needed: 1,
            reward: 20,
        });
        g.state.set(1, 1, Cell::Item(ItemType::Gem, 1));
        g.handle_input(&InputEvent::Key('1'));
        assert_eq!(g.state.coins, 20);
    }

    #[test]
    fn unknown_click_returns_false() {
        let mut g = MergeGame::new();
        assert!(!g.handle_input(&click(9999)));
    }

    #[test]
    fn tick_progresses_cooldown() {
        let mut g = MergeGame::new();
        g.handle_input(&click(GRID_CLICK_BASE));
        let cd0 = g.state.gen_cooldown[0];
        g.tick(5);
        assert!(g.state.gen_cooldown[0] < cd0);
    }
}
