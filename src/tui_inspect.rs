//! TUI を文字として取り出す。
//!
//! このクレートの画面は端末セルである。判断はバッファの記号列で行い、
//! ビットマップへ変換しない。画像にすると Braille 盤面の情報が落ち、
//! 読む側のトークンも食う。

use ratzilla::ratatui::buffer::Buffer;
use ratzilla::ratatui::text::Span;

/// `TestBackend` が持っているセルを、1行1行の文字列に畳む。
///
/// 全角文字は幅2セルで、続きセルの symbol は空になる。セルを素直に連結
/// すると文字の間に空白が挟まり、「台を選ぶ」が `contains` できない。
/// 記号の表示幅だけ進めて続きセルを飛ばす。
pub fn buffer_text(buf: &Buffer) -> String {
    let area = buf.area();
    let mut out = String::with_capacity((area.width as usize + 1) * area.height as usize);
    for y in area.y..area.y.saturating_add(area.height) {
        let mut x = area.x;
        let end_x = area.x.saturating_add(area.width);
        while x < end_x {
            let sym = buf[(x, y)].symbol();
            if sym.is_empty() {
                out.push(' ');
                x = x.saturating_add(1);
            } else {
                out.push_str(sym);
                let w = Span::raw(sym).width().max(1) as u16;
                x = x.saturating_add(w);
            }
        }
        if y + 1 < area.y.saturating_add(area.height) {
            out.push('\n');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratzilla::ratatui::layout::Rect;

    #[test]
    fn buffer_text_keeps_rows_and_symbols() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 3, 2));
        buf[(0, 0)].set_symbol("A");
        buf[(1, 0)].set_symbol("B");
        buf[(2, 0)].set_symbol("C");
        buf[(0, 1)].set_symbol("d");
        assert_eq!(buffer_text(&buf), "ABC\nd  ");
    }

    #[test]
    fn buffer_text_skips_wide_char_continuation_cells() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
        // 全角1文字 + 続きセル空 + ASCII。
        buf[(0, 0)].set_symbol("台");
        buf[(1, 0)].set_symbol("");
        buf[(2, 0)].set_symbol("A");
        buf[(3, 0)].set_symbol("B");
        let text = buffer_text(&buf);
        assert_eq!(text, "台AB");
        assert!(text.contains("台"));
        assert!(!text.contains("台 "));
    }
}
