//! Card screen and gacha result rendering.

use std::cell::RefCell;
use std::rc::Rc;

use ratzilla::ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratzilla::ratatui::style::{Color, Modifier, Style};
use ratzilla::ratatui::text::{Line, Span};
use ratzilla::ratatui::widgets::{Block, Borders, Paragraph};
use ratzilla::ratatui::Frame;

use crate::input::{is_narrow_layout, ClickState};
use crate::widgets::Clickable;

use super::super::actions::*;
use super::super::gacha::{self, card_def};
use super::super::state::CafeState;

pub(super) fn render_card_screen(state: &CafeState, f: &mut Frame, area: Rect, click_state: &Rc<RefCell<ClickState>>) {
    // Delegate to hub cards view
    super::hub::render_hub_cards(state, f, area, click_state);
}

pub(super) fn render_gacha_result(f: &mut Frame, area: Rect, click_state: &Rc<RefCell<ClickState>>, card_ids: &[u32]) {
    let is_narrow = is_narrow_layout(area.width);

    // 外枠 (タイトル付き) を先に描画し、内側を「カード一覧」と「固定 OK ボタン」に分割。
    // 10連 narrow (24 行) でも OK は最終行に必ず残ってクリック可能にする。
    let borders = if is_narrow { Borders::TOP | Borders::BOTTOM } else { Borders::ALL };
    let outer = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Yellow))
        .title(" ガチャ ");
    let inner = outer.inner(area);
    f.render_widget(outer, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),    // カード一覧 (overflow 時はここが切れる)
            Constraint::Length(1), // OK ボタン (常に最終行に固定)
        ])
        .split(inner);
    let cards_area = chunks[0];
    let ok_area = chunks[1];

    // カード一覧
    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(Span::styled(" ガチャ結果！", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))));
    lines.push(Line::from(""));

    for &id in card_ids {
        if let Some(def) = card_def(id) {
            let color = match def.rarity {
                gacha::Rarity::Star3 => Color::Yellow,
                gacha::Rarity::Star2 => Color::Cyan,
                gacha::Rarity::Star1 => Color::White,
            };
            if is_narrow {
                // Narrow: rarity+name と description を 2 行に分ける
                lines.push(Line::from(vec![
                    Span::styled(format!(" {} ", def.rarity.label()), Style::default().fg(color).add_modifier(Modifier::BOLD)),
                    Span::styled(def.name, Style::default().fg(color)),
                ]));
                lines.push(Line::from(Span::styled(
                    format!("   {}", def.description),
                    Style::default().fg(Color::DarkGray),
                )));
            } else {
                lines.push(Line::from(vec![
                    Span::styled(format!(" {} ", def.rarity.label()), Style::default().fg(color).add_modifier(Modifier::BOLD)),
                    Span::styled(def.name, Style::default().fg(color)),
                    Span::styled(format!(" — {}", def.description), Style::default().fg(Color::DarkGray)),
                ]));
            }
        }
    }
    f.render_widget(Paragraph::new(lines), cards_area);

    // OK ボタンは Clickable で 1 行領域全体をクリック可能にして固定描画する。
    let ok_para = Paragraph::new(Line::from(Span::styled(
        " ▶ OK",
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
    )));
    {
        let mut cs = click_state.borrow_mut();
        Clickable::new(ok_para, GACHA_RESULT_OK).render(f, ok_area, &mut cs);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratzilla::ratatui::backend::TestBackend;
    use ratzilla::ratatui::Terminal;

    /// 32 列幅でガチャ結果を描画したとき、各カード情報行が
    /// バッファ幅 (32) を超えないことを確認。
    /// (CJK 2 セル目が空白 (' ') として現れる TestBackend の特性に依存しないよう、
    ///  Line の visual width と area の width を直接比較する。)
    #[test]
    fn gacha_result_fits_in_narrow_width() {
        let cs = Rc::new(RefCell::new(ClickState::new()));
        let mut terminal = Terminal::new(TestBackend::new(32, 30)).unwrap();
        // 全レアリティのサンプル + 最長 description のもの
        let card_ids = vec![20, 22, 24, 10, 14, 1, 6];
        terminal
            .draw(|f| render_gacha_result(f, f.area(), &cs, &card_ids))
            .unwrap();
        // 描画後のバッファでパニックしないこと自体が narrow 安全性の primary check。
        // 加えて ★3 の name は単独 1 行で完結するので、その行の visual width が
        // 32 列以下であることも論理的に検証する。
        for &id in &card_ids {
            if let Some(def) = card_def(id) {
                let name_line_w = format!(" {} {}", def.rarity.label(), def.name);
                let visual = ratzilla::ratatui::text::Line::from(name_line_w.as_str()).width();
                assert!(
                    visual <= 32,
                    "card name line for id={id} ({}) is {} cells > 32",
                    def.name,
                    visual
                );
                let desc_line_w = format!("   {}", def.description);
                let visual_d = ratzilla::ratatui::text::Line::from(desc_line_w.as_str()).width();
                assert!(
                    visual_d <= 32,
                    "card desc line for id={id} ({}) is {} cells > 32",
                    def.description,
                    visual_d
                );
            }
        }
    }

    /// narrow 幅で OK ボタンが描画範囲内にクリック登録されること。
    #[test]
    fn gacha_result_ok_button_clickable_in_narrow() {
        let cs = Rc::new(RefCell::new(ClickState::new()));
        let mut terminal = Terminal::new(TestBackend::new(32, 30)).unwrap();
        let card_ids = vec![1, 2, 3];
        terminal
            .draw(|f| render_gacha_result(f, f.area(), &cs, &card_ids))
            .unwrap();
        let cs = cs.borrow();
        let mut found = false;
        for y in 0..30 {
            for x in 0..32 {
                if cs.hit_test(x, y) == Some(GACHA_RESULT_OK) {
                    found = true;
                }
            }
        }
        assert!(found, "GACHA_RESULT_OK button not registered at narrow width");
    }

    /// narrow 10連 (20+ 行) でも、画面が短くて中身が overflow する条件で
    /// OK ボタンが必ず最終行に固定描画され、クリック可能であることを確認。
    /// (Codex review #79: P1 — overflow 時に OK が clip されて触れなくなる回帰防止)
    #[test]
    fn gacha_result_ok_button_pinned_when_content_overflows() {
        let cs = Rc::new(RefCell::new(ClickState::new()));
        // 32x10: 10連 narrow (24 行) は全く入らない overflow ケース
        let mut terminal = Terminal::new(TestBackend::new(32, 10)).unwrap();
        // 10連 想定の 10 枚
        let card_ids: Vec<u32> = vec![20, 21, 22, 23, 24, 10, 11, 12, 13, 14];
        terminal
            .draw(|f| render_gacha_result(f, f.area(), &cs, &card_ids))
            .unwrap();
        let cs = cs.borrow();
        let mut found = false;
        for y in 0..10 {
            for x in 0..32 {
                if cs.hit_test(x, y) == Some(GACHA_RESULT_OK) {
                    found = true;
                }
            }
        }
        assert!(
            found,
            "GACHA_RESULT_OK must remain clickable when card list overflows (10連 on short terminal)"
        );
    }
}

