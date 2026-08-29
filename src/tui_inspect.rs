//! TUI を文字として取り出す。
//!
//! このクレートの画面は端末セルである。判断はバッファの記号列で行い、
//! ビットマップへ変換しない。画像にすると Braille 盤面の情報が落ち、
//! 読む側のトークンも食う。

use ratzilla::ratatui::buffer::Buffer;

/// `TestBackend` が持っているセルを、1行1行の文字列に畳む。
pub fn buffer_text(buf: &Buffer) -> String {
    let area = buf.area();
    let mut out = String::with_capacity((area.width as usize + 1) * area.height as usize);
    for y in area.y..area.y.saturating_add(area.height) {
        for x in area.x..area.x.saturating_add(area.width) {
            out.push_str(buf[(x, y)].symbol());
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
}
