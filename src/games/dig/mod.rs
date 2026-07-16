//! 穴掘り長屋 — 1日5本のシャベルで、ヒントの数字から宝の位置を推理して
//! 掘り当てる発掘パズル。現場は日付から決定的に生成されるため全員共通。

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

use actions::{
    ACT_MUSEUM_SCROLL_DOWN, ACT_MUSEUM_SCROLL_UP, ACT_RADAR, ACT_TAB_MUSEUM, ACT_TAB_SITE,
};
use state::{DigState, DigTab};

/// ▲▼ 1タップあたりのスクロール量 (行)。
const MUSEUM_SCROLL_STEP: u16 = 3;

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

    /// セーブをロードし、直後に日付チェックまで済ませた state を返す。
    /// ロード直後にチェックしておくことで、開いた瞬間から「今日の現場」が
    /// 正しく表示される。
    #[cfg(target_arch = "wasm32")]
    fn load_or_new_state() -> DigState {
        let mut state = DigState::new();
        if save::load_game(&mut state) {
            state.add_log("セーブデータをロード");
        } else {
            logic::setup_site(&mut state, 0);
        }
        let now_ms = save::wall_clock_now_ms();
        logic::maybe_reset_day(&mut state, now_ms);
        state
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn load_or_new_state() -> DigState {
        let mut state = DigState::new();
        logic::setup_site(&mut state, 0);
        state
    }

    fn handle_click(&mut self, action_id: u16) -> bool {
        match action_id {
            ACT_TAB_SITE => {
                self.state.selected_tab = DigTab::Site;
                true
            }
            ACT_TAB_MUSEUM => {
                self.state.selected_tab = DigTab::Museum;
                true
            }
            ACT_RADAR => self.toggle_radar(),
            ACT_MUSEUM_SCROLL_UP => {
                let cur = self.state.museum_scroll.get();
                self.state.museum_scroll.set(cur.saturating_sub(MUSEUM_SCROLL_STEP));
                true
            }
            ACT_MUSEUM_SCROLL_DOWN => {
                // 上限は描画時に ScrollableTab が実コンテンツ高で clamp する。
                let cur = self.state.museum_scroll.get();
                self.state.museum_scroll.set(cur.saturating_add(MUSEUM_SCROLL_STEP));
                true
            }
            _ => {
                if let Some(idx) = actions::decode_grid(action_id) {
                    if self.state.radar_armed {
                        logic::scan(&mut self.state, idx)
                    } else {
                        logic::dig(&mut self.state, idx)
                    }
                } else {
                    false
                }
            }
        }
    }

    /// 羅盤モードの切り替え。使えない状態 (上限・コイン不足・全回収後) では
    /// 入らない。
    fn toggle_radar(&mut self) -> bool {
        if self.state.radar_armed {
            self.state.radar_armed = false;
            return true;
        }
        if self.state.remaining_treasures() == 0 {
            self.state.add_log("もうお宝は残っていない。羅盤の出番なし".to_string());
            return true;
        }
        match logic::radar_cost(self.state.radar_uses) {
            Some(cost) if self.state.coins >= cost => {
                self.state.radar_armed = true;
                self.state.selected_tab = DigTab::Site;
                true
            }
            Some(cost) => {
                self.state.add_log(format!("羅盤にはコインが足りない (💰{cost})"));
                true
            }
            None => {
                self.state.add_log("羅盤は本日もう使えない".to_string());
                true
            }
        }
    }

    fn handle_key(&mut self, key: char) -> bool {
        // グリッドのマス単体を狙う操作はポインター前提なのでキー未対応。
        // タブ切替と羅盤だけキーで提供する。
        match key {
            '1' => {
                self.state.selected_tab = DigTab::Site;
                true
            }
            '2' => {
                self.state.selected_tab = DigTab::Museum;
                true
            }
            'r' | 'R' => self.toggle_radar(),
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
    use actions::GRID_CLICK_BASE;
    use state::{SHOVELS_PER_DAY, SITE_LEN};

    fn click(id: u16) -> InputEvent {
        InputEvent::Click(ClickScope::Game(GameChoice::Dig), id)
    }

    #[test]
    fn 新規ゲームは現場に宝が埋まっている() {
        let g = DigGame::new();
        assert!(!g.state.treasures.is_empty());
    }

    #[test]
    fn グリッドクリックで掘れてシャベルが減る() {
        let mut g = DigGame::new();
        // 宝のないマスを選んで空振りの消費を確認する。
        let empty_idx = (0..SITE_LEN)
            .find(|&i| g.state.treasure_at(i).is_none())
            .expect("空きマスは必ずある");
        assert!(g.handle_input(&click(GRID_CLICK_BASE + empty_idx as u16)));
        assert_eq!(g.state.shovels, SHOVELS_PER_DAY - 1);
        assert!(g.state.dug[empty_idx]);
    }

    #[test]
    fn タブクリックで切り替わる() {
        let mut g = DigGame::new();
        assert_eq!(g.state.selected_tab, DigTab::Site);
        g.handle_input(&click(actions::ACT_TAB_MUSEUM));
        assert_eq!(g.state.selected_tab, DigTab::Museum);
        g.handle_input(&click(actions::ACT_TAB_SITE));
        assert_eq!(g.state.selected_tab, DigTab::Site);
    }

    #[test]
    fn 羅盤モード中のグリッドクリックは掘らずに調べる() {
        let mut g = DigGame::new();
        g.state.coins = 100;
        assert!(g.handle_input(&InputEvent::Key('r')));
        assert!(g.state.radar_armed);
        assert!(g.handle_input(&click(GRID_CLICK_BASE)));
        assert!(g.state.scanned[0]);
        assert!(!g.state.dug[0], "羅盤では掘れない");
        assert_eq!(g.state.shovels, SHOVELS_PER_DAY, "シャベルは減らない");
        assert!(!g.state.radar_armed, "使用後は解除");
    }

    #[test]
    fn コイン不足では羅盤モードに入れない() {
        let mut g = DigGame::new();
        g.state.coins = 0;
        assert!(g.handle_input(&InputEvent::Key('r')));
        assert!(!g.state.radar_armed);
    }

    #[test]
    fn 羅盤モードは再タップで解除できる() {
        let mut g = DigGame::new();
        g.state.coins = 100;
        g.handle_input(&click(actions::ACT_RADAR));
        assert!(g.state.radar_armed);
        g.handle_input(&click(actions::ACT_RADAR));
        assert!(!g.state.radar_armed);
        assert_eq!(g.state.coins, 100, "解除だけではコインを消費しない");
    }

    #[test]
    fn 数字キーでタブが切り替わる() {
        let mut g = DigGame::new();
        assert!(g.handle_input(&InputEvent::Key('2')));
        assert_eq!(g.state.selected_tab, DigTab::Museum);
        assert!(g.handle_input(&InputEvent::Key('1')));
        assert_eq!(g.state.selected_tab, DigTab::Site);
    }

    #[test]
    fn 未知のクリックとキーはfalseを返す() {
        let mut g = DigGame::new();
        assert!(!g.handle_input(&click(9999)));
        assert!(!g.handle_input(&InputEvent::Key('z')));
    }

    #[test]
    fn 図鑑スクロールのタップで位置が動く() {
        let mut g = DigGame::new();
        g.handle_input(&click(actions::ACT_MUSEUM_SCROLL_DOWN));
        assert_eq!(g.state.museum_scroll.get(), MUSEUM_SCROLL_STEP);
        g.handle_input(&click(actions::ACT_MUSEUM_SCROLL_UP));
        assert_eq!(g.state.museum_scroll.get(), 0);
        // 0未満にはならない
        g.handle_input(&click(actions::ACT_MUSEUM_SCROLL_UP));
        assert_eq!(g.state.museum_scroll.get(), 0);
    }

    #[test]
    fn 全回収後は羅盤モードに入れない() {
        let mut g = DigGame::new();
        g.state.coins = 1000;
        for t in g.state.treasures.clone() {
            for c in t.cells {
                g.state.dug[c as usize] = true;
            }
        }
        assert!(g.handle_input(&InputEvent::Key('r')));
        assert!(!g.state.radar_armed);
        assert_eq!(g.state.coins, 1000);
    }

    #[test]
    fn tickはフラッシュを減衰させる() {
        let mut g = DigGame::new();
        g.state.flash = Some(state::Flash { cells: vec![0], ttl: 3 });
        g.tick(2);
        assert_eq!(g.state.flash.as_ref().unwrap().ttl, 1);
        g.tick(5);
        assert!(g.state.flash.is_none());
    }
}
