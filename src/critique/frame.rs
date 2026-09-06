//! `TestBackend` から画面スナップショットを取る。

use std::cell::RefCell;
use std::rc::Rc;

use ratzilla::ratatui::backend::TestBackend;
use ratzilla::ratatui::Terminal;

use crate::input::ClickState;
use crate::tui_inspect::{buffer_occupancy, buffer_text};

/// 1フレーム分の「プレイヤーが見ているもの」。
#[derive(Clone, Debug)]
pub struct ScreenSnapshot {
    pub width: u16,
    pub height: u16,
    /// `buffer_text` の生テキスト（行は `\n` 区切り）。
    pub text: String,
    /// 登録されたクリック対象の action_id（重複除去・ソート済み）。
    pub action_ids: Vec<u16>,
    /// 空白以外のセル比率（全角は表示幅でカウント）。
    pub occupancy: f64,
}

impl ScreenSnapshot {
    pub fn contains_needle(&self, needle: &str) -> bool {
        !needle.is_empty() && self.text.contains(needle)
    }

    pub fn occupancy(&self) -> f64 {
        self.occupancy
    }

    pub fn has_action(&self, action_id: u16) -> bool {
        self.action_ids.binary_search(&action_id).is_ok()
    }

    /// 画面に登録されたクリック対象のうち、「迷い度」として数える個数。
    ///
    /// 連続した ID 帯が長いもの（グリッド配置のマス群）は、マスごとに
    /// 別のメニュー項目ではなく「盤面を1つ選ぶ」操作として 1 と数える。
    /// レーン数程度（≤12）の短い連続は個別に数える。
    pub fn semantic_choice_count(&self) -> usize {
        semantic_choice_count(&self.action_ids)
    }
}

/// 連続 ID の長いランを 1 操作に畳む。
pub(crate) fn semantic_choice_count(action_ids: &[u16]) -> usize {
    if action_ids.is_empty() {
        return 0;
    }
    let mut count = 0usize;
    let mut i = 0usize;
    while i < action_ids.len() {
        let mut j = i + 1;
        while j < action_ids.len() && action_ids[j] == action_ids[j - 1].saturating_add(1) {
            j += 1;
        }
        let run = j - i;
        if run > 12 {
            count += 1;
        } else {
            count += run;
        }
        i = j;
    }
    count
}

/// `draw` に渡した描画を `TestBackend` へ落とし、文字とクリック対象を返す。
pub fn capture_frame<F>(width: u16, height: u16, draw: F) -> ScreenSnapshot
where
    F: FnOnce(&mut ratzilla::ratatui::Frame, &Rc<RefCell<ClickState>>),
{
    let cs = Rc::new(RefCell::new(ClickState::new()));
    cs.borrow_mut().terminal_cols = width;
    cs.borrow_mut().terminal_rows = height;
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("TestBackend");
    terminal
        .draw(|f| {
            draw(f, &cs);
        })
        .expect("draw");
    let buf = terminal.backend().buffer();
    let text = buffer_text(buf);
    let occupancy = buffer_occupancy(buf);
    let borrowed = cs.borrow();
    let mut action_ids: Vec<u16> = borrowed.targets.iter().map(|t| t.action_id).collect();
    action_ids.sort_unstable();
    action_ids.dedup();
    ScreenSnapshot {
        width,
        height,
        text,
        action_ids,
        occupancy,
    }
}
