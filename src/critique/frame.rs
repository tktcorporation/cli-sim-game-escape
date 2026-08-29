//! `TestBackend` から画面スナップショットを取る。

use std::cell::RefCell;
use std::rc::Rc;

use ratzilla::ratatui::backend::TestBackend;
use ratzilla::ratatui::layout::Rect;
use ratzilla::ratatui::Terminal;

use crate::input::ClickState;
use crate::tui_inspect::buffer_text;

/// 1フレーム分の「プレイヤーが見ているもの」。
#[derive(Clone, Debug)]
pub struct ScreenSnapshot {
    pub width: u16,
    pub height: u16,
    /// `buffer_text` の生テキスト（行は `\n` 区切り）。
    pub text: String,
    /// 登録されたクリック対象の action_id（重複除去・ソート済み）。
    pub action_ids: Vec<u16>,
    /// クリック対象の矩形。画面上のどの帯が押せるかの密度判定に使う。
    pub target_rects: Vec<Rect>,
}

impl ScreenSnapshot {
    pub fn contains_needle(&self, needle: &str) -> bool {
        !needle.is_empty() && self.text.contains(needle)
    }

    /// 空白以外のセル比率。読みやすさの粗い代理指標。
    pub fn occupancy(&self) -> f64 {
        let total = (self.width as usize).saturating_mul(self.height as usize);
        if total == 0 {
            return 0.0;
        }
        let filled = self.text.chars().filter(|c| *c != ' ' && *c != '\n').count();
        filled as f64 / total as f64
    }

    pub fn has_action(&self, action_id: u16) -> bool {
        self.action_ids.binary_search(&action_id).is_ok()
    }
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
    let text = buffer_text(terminal.backend().buffer());
    let borrowed = cs.borrow();
    let mut action_ids: Vec<u16> = borrowed.targets.iter().map(|t| t.action_id).collect();
    action_ids.sort_unstable();
    action_ids.dedup();
    let target_rects = borrowed.targets.iter().map(|t| t.rect).collect();
    ScreenSnapshot {
        width,
        height,
        text,
        action_ids,
        target_rects,
    }
}
