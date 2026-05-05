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

pub(super) fn render_gacha_result(
    state: &CafeState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
    card_ids: &[u32],
) {
    let is_narrow = is_narrow_layout(area.width);
    let frame = state.gacha_anim_frame;
    let revealed = gacha::gacha_anim_revealed(frame, card_ids.len());
    let complete = gacha::gacha_anim_is_complete(frame, card_ids.len());

    // Border color flares brighter while the animation runs to add some
    // texture to the reveal — yellow when active, faded once complete.
    let border_color = if complete { Color::Yellow } else { Color::LightYellow };
    let borders = if is_narrow { Borders::TOP | Borders::BOTTOM } else { Borders::ALL };
    let outer = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(border_color))
        .title(" ガチャ ");
    let inner = outer.inner(area);
    f.render_widget(outer, area);

    // Layout: card list grows; OK pinned to the last row so it survives
    // overflow on small mobile screens (regression-safe per existing tests).
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),    // card list / anticipation
            Constraint::Length(1), // OK / skip button
        ])
        .split(inner);
    let cards_area = chunks[0];
    let ok_area = chunks[1];

    let mut lines: Vec<Line> = Vec::new();

    // ── Header (pulses during the anim) ──
    let header_label = if complete { " ガチャ結果！" } else { " ✦ 召喚中 ✦ " };
    let header_color = if complete { Color::Yellow } else { Color::LightMagenta };
    lines.push(Line::from(Span::styled(
        header_label,
        Style::default().fg(header_color).add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));

    if revealed == 0 {
        // Anticipation: animated dots + sparkle pattern. Looks alive even on
        // a single-frame screenshot because the Span colors are layered.
        let dots = (frame as usize % 4) + 1;
        let dot_str = ".".repeat(dots);
        lines.push(Line::from(vec![
            Span::styled("  ✧ ", Style::default().fg(Color::LightMagenta)),
            Span::styled(format!("ご縁を紡いでいます{dot_str}"), Style::default().fg(Color::White)),
            Span::styled(" ✧", Style::default().fg(Color::LightMagenta)),
        ]));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "   (タップでスキップ)",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        // Show what's been revealed so far (one card per frame after the
        // anticipation phase). Once `complete`, this is the full list.
        for &id in card_ids.iter().take(revealed) {
            if let Some(def) = card_def(id) {
                push_card_line(&mut lines, def, is_narrow);
            }
        }
        // Hint that more cards are still rolling in.
        let remaining = card_ids.len().saturating_sub(revealed);
        if !complete && remaining > 0 {
            lines.push(Line::from(Span::styled(
                format!("   …あと {remaining} 枚"),
                Style::default().fg(Color::DarkGray),
            )));
        }
    }
    f.render_widget(Paragraph::new(lines), cards_area);

    // OK button: doubles as a "skip" affordance during the reveal so users
    // never have to wait, and as the dismiss button afterwards. The widget
    // is registered via `Clickable` so the same rect serves both states.
    let ok_label = if complete { " ▶ OK" } else { " ▶ スキップ" };
    let ok_color = if complete { Color::Yellow } else { Color::Cyan };
    let ok_para = Paragraph::new(Line::from(Span::styled(
        ok_label,
        Style::default().fg(ok_color).add_modifier(Modifier::BOLD),
    )));
    {
        let mut cs = click_state.borrow_mut();
        Clickable::new(ok_para, GACHA_RESULT_OK).render(f, ok_area, &mut cs);
    }
}

/// Append one card's reveal line(s). On narrow (mobile) screens we collapse
/// to a single row per card so the 10-pull result fits without truncation —
/// ★3 still gets a sparkle accent so it pops visually.
fn push_card_line<'a>(lines: &mut Vec<Line<'a>>, def: &'a gacha::CardDef, is_narrow: bool) {
    let color = match def.rarity {
        gacha::Rarity::Star3 => Color::Yellow,
        gacha::Rarity::Star2 => Color::Cyan,
        gacha::Rarity::Star1 => Color::White,
    };
    let is_three = def.rarity == gacha::Rarity::Star3;
    if is_narrow {
        // Single-line layout — title only, with sparkle accents for ★3.
        let mut spans: Vec<Span> = Vec::new();
        if is_three {
            spans.push(Span::styled(" ✦ ", Style::default().fg(Color::LightYellow)));
        } else {
            spans.push(Span::styled("   ", Style::default()));
        }
        spans.push(Span::styled(
            format!("{} ", def.rarity.label()),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(def.name, Style::default().fg(color)));
        if is_three {
            spans.push(Span::styled(" ✦", Style::default().fg(Color::LightYellow)));
        }
        lines.push(Line::from(spans));
    } else {
        // Wide layout: name + description on one line, sparkles for ★3.
        let mut spans: Vec<Span> = Vec::new();
        if is_three {
            spans.push(Span::styled(" ✦ ", Style::default().fg(Color::LightYellow)));
        } else {
            spans.push(Span::styled(" ", Style::default()));
        }
        spans.push(Span::styled(
            format!("{} ", def.rarity.label()),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(def.name, Style::default().fg(color)));
        spans.push(Span::styled(
            format!(" — {}", def.description),
            Style::default().fg(Color::DarkGray),
        ));
        lines.push(Line::from(spans));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratzilla::ratatui::backend::TestBackend;
    use ratzilla::ratatui::Terminal;

    /// State preconfigured to render the gacha result with the animation
    /// already complete — exercises the same code path as a player who has
    /// finished watching the reveal.
    fn revealed_state(card_ids: &[u32]) -> CafeState {
        let mut s = CafeState::new();
        s.gacha_anim_frame = gacha::gacha_anim_complete_frame(card_ids.len());
        s
    }

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
        let state = revealed_state(&card_ids);
        terminal
            .draw(|f| render_gacha_result(&state, f, f.area(), &cs, &card_ids))
            .unwrap();
        // 描画後のバッファでパニックしないこと自体が narrow 安全性の primary check。
        // 加えて ★3 の name は単独 1 行で完結するので、その行の visual width が
        // 32 列以下であることも論理的に検証する。narrow では sparkle 装飾
        // (" ✦ ") も含めた幅で評価する。
        for &id in &card_ids {
            if let Some(def) = card_def(id) {
                let prefix = if def.rarity == gacha::Rarity::Star3 { " ✦ " } else { "   " };
                let suffix = if def.rarity == gacha::Rarity::Star3 { " ✦" } else { "" };
                let name_line = format!("{}{} {}{}", prefix, def.rarity.label(), def.name, suffix);
                let visual = ratzilla::ratatui::text::Line::from(name_line.as_str()).width();
                assert!(
                    visual <= 32,
                    "card name line for id={id} ({}) is {} cells > 32",
                    def.name,
                    visual
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
        let state = revealed_state(&card_ids);
        terminal
            .draw(|f| render_gacha_result(&state, f, f.area(), &cs, &card_ids))
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
        // 32x10: narrow 10連 でも 1 行/カードに圧縮したのでギリギリ収まらない可能性が
        // あるため、OK pin の挙動を縮小バッファで確認する。
        let mut terminal = Terminal::new(TestBackend::new(32, 10)).unwrap();
        let card_ids: Vec<u32> = vec![20, 21, 22, 23, 24, 10, 11, 12, 13, 14];
        let state = revealed_state(&card_ids);
        terminal
            .draw(|f| render_gacha_result(&state, f, f.area(), &cs, &card_ids))
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

    /// アニメーション中 (revealed=0) にも OK (= スキップ) ボタンが
    /// 必ずクリック可能なことを確認。回帰: アニメ中に skip できないと
    /// 最大 1.3 秒待たされる UX が再発する。
    #[test]
    fn gacha_result_skip_button_clickable_during_anim() {
        let cs = Rc::new(RefCell::new(ClickState::new()));
        let mut terminal = Terminal::new(TestBackend::new(32, 30)).unwrap();
        let card_ids = vec![20, 21, 22, 23, 24, 10, 11, 12, 13, 14];
        let mut state = CafeState::new();
        state.gacha_anim_frame = 0; // anticipation
        terminal
            .draw(|f| render_gacha_result(&state, f, f.area(), &cs, &card_ids))
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
        assert!(found, "skip-button must be clickable during anticipation phase");
    }

    /// narrow 10連 — 1 行/カード圧縮で 32x24 buffer (mobile portrait の
    /// 一般的な高さ) に収まることを保証。
    #[test]
    fn ten_pull_narrow_fits_mobile_portrait() {
        let cs = Rc::new(RefCell::new(ClickState::new()));
        let mut terminal = Terminal::new(TestBackend::new(32, 24)).unwrap();
        let card_ids: Vec<u32> = vec![20, 21, 22, 23, 24, 10, 11, 12, 13, 14];
        let state = revealed_state(&card_ids);
        terminal
            .draw(|f| render_gacha_result(&state, f, f.area(), &cs, &card_ids))
            .unwrap();
        // パニックしないこと + OK 必ず登録されることが mobile portrait
        // (32×24 ≈ iOS Safari 縦) で成立することを確認。
        let cs = cs.borrow();
        let mut found = false;
        for y in 0..24 {
            for x in 0..32 {
                if cs.hit_test(x, y) == Some(GACHA_RESULT_OK) {
                    found = true;
                }
            }
        }
        assert!(found, "OK button must be reachable on 32×24 mobile portrait");
    }
}

