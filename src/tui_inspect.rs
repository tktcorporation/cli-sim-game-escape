//! TUI を文字として取り出す。
//!
//! このクレートの画面は端末セルである。判断はバッファの記号列で行い、
//! ビットマップへ変換しない。画像にすると Braille 盤面の情報が落ち、
//! 読む側のトークンも食う。

use ratzilla::ratatui::buffer::Buffer;
use ratzilla::ratatui::text::Span;

/// `TestBackend` が持っているセルを、1行1行の文字列に畳む。
///
/// 全角文字は幅2セルで、続きセルは空または空白になる。セルを素直に連結
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

/// 空白以外のセル比率。全角は表示幅ぶん埋まったものとして数える。
pub fn buffer_occupancy(buf: &Buffer) -> f64 {
    let area = buf.area();
    let total = (area.width as usize).saturating_mul(area.height as usize);
    if total == 0 {
        return 0.0;
    }
    let mut filled = 0usize;
    for y in area.y..area.y.saturating_add(area.height) {
        let mut x = area.x;
        let end_x = area.x.saturating_add(area.width);
        while x < end_x {
            let sym = buf[(x, y)].symbol();
            if sym.is_empty() {
                x = x.saturating_add(1);
                continue;
            }
            let w = Span::raw(sym).width().max(1) as u16;
            if sym != " " {
                filled = filled.saturating_add(w as usize);
            }
            x = x.saturating_add(w);
        }
    }
    filled as f64 / total as f64
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratzilla::ratatui::backend::TestBackend;
    use ratzilla::ratatui::layout::Rect;
    use ratzilla::ratatui::widgets::Paragraph;
    use ratzilla::ratatui::Terminal;

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
    fn buffer_text_joins_fullwidth_run_without_gaps() {
        // 本番と同じ経路: Paragraph 経由で全角を描く。続きセルは空文字ではなく
        // 空白になることがあり、手で `set_symbol("")` しただけでは退行を
        // ロックできない。
        let mut terminal = Terminal::new(TestBackend::new(10, 1)).unwrap();
        terminal
            .draw(|f| {
                f.render_widget(Paragraph::new("台を選ぶ"), f.area());
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        let naive: String = (0..10).map(|x| buf[(x, 0)].symbol().to_string()).collect();
        let text = buffer_text(buf);
        assert!(
            text.contains("台を選ぶ"),
            "幅スキップ後も連続全角が繋がっていない: text={text:?} naive={naive:?}"
        );
        assert!(
            naive.contains(' ') || naive != text,
            "前提崩れ: naive 連結が既に隙間無しならこのテストは意味を失う"
        );
    }

    #[test]
    fn buffer_occupancy_counts_wide_chars_by_display_width() {
        let mut terminal = Terminal::new(TestBackend::new(4, 1)).unwrap();
        terminal
            .draw(|f| {
                f.render_widget(Paragraph::new("台A"), f.area());
            })
            .unwrap();
        let occ = buffer_occupancy(terminal.backend().buffer());
        // 「台」(幅2) + 「A」(幅1) = 3/4
        assert!(
            (occ - 0.75).abs() < 0.01,
            "全角を1セル扱いにすると密度が歪む: occ={occ}"
        );
    }
}
