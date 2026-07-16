//! 穴掘り長屋の描画。

use std::cell::RefCell;
use std::rc::Rc;

use ratzilla::ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratzilla::ratatui::style::{Color, Modifier, Style};
use ratzilla::ratatui::text::{Line, Span};
use ratzilla::ratatui::widgets::{Block, Borders, Paragraph};
use ratzilla::ratatui::Frame;

use crate::input::{is_narrow_layout, ClickState};
use crate::widgets::{Clickable, ClickableGrid, ClickableList, ScrollableTab, TabBar};

use super::actions::{
    ACT_MUSEUM_SCROLL_DOWN, ACT_MUSEUM_SCROLL_UP, ACT_RADAR, ACT_TAB_MUSEUM, ACT_TAB_SITE,
    GRID_CLICK_BASE,
};
use super::logic;
use super::state::{
    CollectionSet, DigState, DigTab, ItemKind, SITE_H, SITE_W,
};

/// セル1つの表示幅 (terminal columns)。
const CELL_W: u16 = 4;
const CELL_H: u16 = 1;

pub fn render(state: &DigState, f: &mut Frame, area: Rect, click_state: &Rc<RefCell<ClickState>>) {
    let is_narrow = is_narrow_layout(area.width);
    let borders = if is_narrow {
        Borders::TOP | Borders::BOTTOM
    } else {
        Borders::ALL
    };

    // メイン領域の最小高: グリッド5行 + 枠2 + ステータス1 + 羅盤行1。
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(2),
            Constraint::Min(SITE_H as u16 * CELL_H + 4),
            Constraint::Length(4),
        ])
        .split(area);

    render_header(state, f, chunks[0], borders);
    render_tabs(state, f, chunks[1], click_state);
    match state.selected_tab {
        DigTab::Site => {
            // 現場グリッドの下に、羅盤の全幅タップ行を置く。ヘッダーへの
            // オーバーレイだと狭い画面でコイン表示と衝突するのと、全幅行の
            // 方がタッチで確実に押せる。
            let site_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(SITE_H as u16 * CELL_H + 3),
                    Constraint::Length(1),
                ])
                .split(chunks[2]);
            render_site(state, f, site_chunks[0], click_state, borders);
            render_radar_row(state, f, site_chunks[1], click_state);
        }
        DigTab::Museum => render_museum(state, f, chunks[2], click_state, borders),
    }
    render_log(state, f, chunks[3], borders);
}

fn render_header(state: &DigState, f: &mut Frame, area: Rect, borders: Borders) {
    let line = Line::from(vec![
        Span::styled(
            " ⛏ 穴掘り長屋 ",
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("⛏×{} ", state.shovels),
            shovel_style(state.shovels),
        ),
        Span::styled(
            format!("💰{} ", state.coins),
            Style::default().fg(Color::LightYellow).add_modifier(Modifier::BOLD),
        ),
    ]);
    let para = Paragraph::new(line)
        .block(
            Block::default()
                .borders(borders)
                .border_style(Style::default().fg(Color::Yellow)),
        )
        .alignment(Alignment::Left);
    f.render_widget(para, area);
}

fn shovel_style(shovels: u8) -> Style {
    if shovels == 0 {
        Style::default().fg(Color::DarkGray)
    } else {
        Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD)
    }
}

/// 羅盤の全幅タップ行。状態 (待機/照準中/本日終了) でラベルと色が変わる。
fn render_radar_row(
    state: &DigState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let (label, style) = if state.radar_armed {
        (
            " ◎ 調べるマスをタップ (もう一度押すと解除)".to_string(),
            Style::default()
                .fg(Color::LightCyan)
                .add_modifier(Modifier::BOLD | Modifier::REVERSED),
        )
    } else {
        match logic::radar_cost(state.radar_uses) {
            Some(cost) => {
                let usable = state.coins >= cost && state.remaining_treasures() > 0;
                (
                    format!(" ◎ 羅盤 💰{cost} — 掘らずにヒントだけ見る"),
                    if usable {
                        Style::default().fg(Color::LightCyan).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    },
                )
            }
            None => (
                " ◎ 羅盤 — 本日は使い切った".to_string(),
                Style::default().fg(Color::DarkGray),
            ),
        }
    };
    let para = Paragraph::new(Line::from(Span::styled(label, style)));
    Clickable::new(para, ACT_RADAR).render(f, area, &mut click_state.borrow_mut());
}

fn render_tabs(state: &DigState, f: &mut Frame, area: Rect, click_state: &Rc<RefCell<ClickState>>) {
    let style_for = |tab: DigTab| {
        if state.selected_tab == tab {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        }
    };
    let bar = TabBar::new(" │ ")
        .tab("発掘", style_for(DigTab::Site), ACT_TAB_SITE)
        .tab("図鑑", style_for(DigTab::Museum), ACT_TAB_MUSEUM)
        .block(
            Block::default()
                .borders(Borders::BOTTOM)
                .border_style(Style::default().fg(Color::DarkGray)),
        );
    let mut cs = click_state.borrow_mut();
    bar.render(f, area, &mut cs);
}

fn render_site(
    state: &DigState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
    borders: Borders,
) {
    let title = if state.radar_armed {
        " 発掘現場 (◎羅盤: 調べるマスをタップ) "
    } else {
        " 発掘現場 (タップで掘る) "
    };
    let block = Block::default()
        .borders(borders)
        .border_style(if state.radar_armed {
            Style::default().fg(Color::LightCyan)
        } else {
            Style::default().fg(Color::Green)
        })
        .title(title);

    let mut lines: Vec<Line> = Vec::with_capacity(SITE_H + 1);
    for y in 0..SITE_H {
        let mut spans: Vec<Span> = Vec::with_capacity(SITE_W);
        for x in 0..SITE_W {
            let idx = DigState::idx(x, y);
            let (text, style) = cell_display(state, idx);
            spans.push(Span::styled(text, style));
        }
        lines.push(Line::from(spans));
    }
    lines.push(status_line(state));

    let grid = ClickableGrid::new(SITE_W, SITE_H, GRID_CLICK_BASE, CELL_W).with_cell_height(CELL_H);
    {
        let mut cs = click_state.borrow_mut();
        grid.register_targets(area, &block, &mut cs, 0);
    }

    // ClickableGrid::register_targets は inner の左端からのオフセットでクリック
    // 領域を計算するため (widgets.rs)、Paragraph 側も Left で描画して両者の
    // 座標系を一致させる。Center にすると inner が広い時に描画だけ右へずれ、
    // タップ判定と表示位置がずれる。
    let para = Paragraph::new(lines).block(block).alignment(Alignment::Left);
    f.render_widget(para, area);
}

/// グリッド最下段のステータス行。
fn status_line(state: &DigState) -> Line<'static> {
    let remaining = state.remaining_treasures();
    if !state.treasures.is_empty() && remaining == 0 {
        return Line::from(Span::styled(
            " ☆ 完全制覇！ また明日 ☆",
            Style::default().fg(Color::LightYellow).add_modifier(Modifier::BOLD),
        ));
    }
    Line::from(vec![
        Span::styled(
            format!(" お宝 残り{remaining} "),
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "数字=近くのお宝までの歩数",
            Style::default().fg(Color::DarkGray),
        ),
    ])
}

/// セル1つの表示文字列と色。全バリアント CELL_W (4桁) 固定幅。
/// (CJK/絵文字は表示幅が揺れるため、単幅が保証される記号だけを使う。)
fn cell_display(state: &DigState, idx: usize) -> (String, Style) {
    let flashing = state
        .flash
        .as_ref()
        .is_some_and(|fl| fl.cells.contains(&(idx as u16)));

    let (text, mut style) = if state.dug[idx] {
        match state.treasure_at(idx) {
            Some((t_idx, kind)) => {
                if state.treasure_complete(t_idx) {
                    (
                        " ★  ".to_string(),
                        Style::default().fg(Color::LightYellow).add_modifier(Modifier::BOLD),
                    )
                } else {
                    (
                        " ◆  ".to_string(),
                        Style::default()
                            .fg(set_color(kind.collection()))
                            .add_modifier(Modifier::BOLD),
                    )
                }
            }
            None => hint_display(logic::hint_at(state, idx), false),
        }
    } else if state.scanned[idx] {
        hint_display(logic::hint_at(state, idx), true)
    } else {
        (" ▒▒ ".to_string(), Style::default().fg(Color::DarkGray))
    };

    if flashing {
        style = style.add_modifier(Modifier::REVERSED);
    }
    (text, style)
}

/// ヒント数字の表示。羅盤由来 (`scanned`) は ◎ 付きで区別する。
/// 数字の色は熱度: 1=激アツ, 2=近い, 3=まあまあ, 4以上=遠い。
fn hint_display(hint: Option<u32>, scanned: bool) -> (String, Style) {
    match hint {
        Some(n) => {
            let text = if scanned {
                format!("◎{n:>2} ")
            } else {
                format!(" {n:>2} ")
            };
            let style = match n {
                0..=1 => Style::default().fg(Color::LightRed).add_modifier(Modifier::BOLD),
                2 => Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                3 => Style::default().fg(Color::White),
                _ => Style::default().fg(Color::DarkGray),
            };
            (text, style)
        }
        None => (" ·  ".to_string(), Style::default().fg(Color::DarkGray)),
    }
}

fn set_color(set: CollectionSet) -> Color {
    match set {
        CollectionSet::Jomon => Color::LightGreen,
        CollectionSet::Dragon => Color::LightRed,
        CollectionSet::Fuku => Color::LightMagenta,
    }
}

fn render_museum(
    state: &DigState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
    borders: Borders,
) {
    // コンテンツ (3セット×最大5行) は狭い画面の表示域を超えるので
    // ScrollableTab で ▲▼ スクロールできるようにする。
    let mut cl = ClickableList::new();
    for set in CollectionSet::all() {
        cl.push(set_header_line(state, set));
        for kind in set.kinds() {
            cl.push(kind_line(state, kind));
        }
        cl.push(Line::from(""));
    }

    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::LightMagenta))
        .title(" 図鑑 ");
    let mut cs = click_state.borrow_mut();
    ScrollableTab::new(
        cl,
        &state.museum_scroll,
        ACT_MUSEUM_SCROLL_UP,
        ACT_MUSEUM_SCROLL_DOWN,
    )
    .block(block)
    .arrow_color(Color::LightMagenta)
    .render(f, area, &mut cs);
}

fn set_header_line(state: &DigState, set: CollectionSet) -> Line<'static> {
    let kinds = set.kinds();
    let found = kinds
        .iter()
        .filter(|k| state.museum_counts[k.to_save_id() as usize] >= 1)
        .count();
    if state.completed_sets[set.index()] {
        Line::from(vec![
            Span::styled(
                " ★ ",
                Style::default().fg(Color::LightYellow).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                set.display_name(),
                Style::default().fg(set_color(set)).add_modifier(Modifier::BOLD),
            ),
            Span::styled(" — コンプリート済み", Style::default().fg(Color::LightGreen)),
        ])
    } else {
        Line::from(vec![
            Span::styled(" ○ ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                set.display_name(),
                Style::default().fg(set_color(set)),
            ),
            Span::styled(
                format!(" ({}/{})", found, kinds.len()),
                Style::default().fg(Color::Yellow),
            ),
        ])
    }
}

/// 未発見の種類は名前も大きさも隠す — 掘り当てて初めて正体が分かる。
fn kind_line(state: &DigState, kind: ItemKind) -> Line<'static> {
    let count = state.museum_counts[kind.to_save_id() as usize];
    if count == 0 {
        return Line::from(Span::styled(
            "    ？ ？？？",
            Style::default().fg(Color::DarkGray),
        ));
    }
    Line::from(vec![
        Span::styled(
            "    ◆ ",
            Style::default().fg(set_color(kind.collection())),
        ),
        Span::styled(kind.name(), Style::default().fg(Color::White)),
        Span::styled(
            format!(" ({}マス) ×{}", kind.size(), count),
            Style::default().fg(Color::Gray),
        ),
    ])
}

fn render_log(state: &DigState, f: &mut Frame, area: Rect, borders: Borders) {
    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(" ログ ");
    let inner = block.inner(area);
    let visible = inner.height as usize;
    let start = state.log.len().saturating_sub(visible);
    let lines: Vec<Line> = state.log[start..]
        .iter()
        .map(|m| Line::from(Span::styled(m.clone(), Style::default().fg(Color::Gray))))
        .collect();
    let para = Paragraph::new(lines).block(block);
    f.render_widget(para, area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::games::dig::state::Treasure;
    use crate::input::ClickState;
    use ratzilla::ratatui::backend::TestBackend;
    use ratzilla::ratatui::Terminal;

    /// (2,1)-(3,1) 勾玉 / (5,3) 鏃 の既知配置。
    fn fixed_state() -> DigState {
        let mut s = DigState::new();
        s.treasures = vec![
            Treasure {
                kind: ItemKind::Magatama,
                cells: vec![DigState::idx(2, 1) as u16, DigState::idx(3, 1) as u16],
            },
            Treasure {
                kind: ItemKind::ObsidianArrow,
                cells: vec![DigState::idx(5, 3) as u16],
            },
        ];
        s
    }

    #[test]
    fn セル表示は全バリアントで幅が揃う() {
        let mut s = fixed_state();
        s.dug[DigState::idx(0, 0)] = true; // 空振り (ヒント数字)
        s.dug[DigState::idx(2, 1)] = true; // 宝の一部 ◆
        s.dug[DigState::idx(5, 3)] = true; // 完掘 ★
        s.scanned[DigState::idx(6, 4)] = true; // 羅盤 ◎N

        let probes = [
            (DigState::idx(1, 0), "hidden"),
            (DigState::idx(0, 0), "hint"),
            (DigState::idx(2, 1), "partial-treasure"),
            (DigState::idx(5, 3), "complete-treasure"),
            (DigState::idx(6, 4), "scanned"),
        ];
        for (idx, label) in probes {
            let (text, _) = cell_display(&s, idx);
            assert_eq!(
                text.chars().count(),
                CELL_W as usize,
                "{label} の表示幅がずれている: {text:?}"
            );
        }

        // 全回収後のヒントなし "·" と、2桁ヒントも同幅
        let mut all_dug = fixed_state();
        for t in all_dug.treasures.clone() {
            for c in t.cells {
                all_dug.dug[c as usize] = true;
            }
        }
        all_dug.dug[0] = true;
        let (text, _) = cell_display(&all_dug, 0);
        assert_eq!(text.chars().count(), CELL_W as usize, "no-hint: {text:?}");
        let (text, _) = hint_display(Some(10), false);
        assert_eq!(text.chars().count(), CELL_W as usize, "2桁ヒント: {text:?}");
        let (text, _) = hint_display(Some(10), true);
        assert_eq!(text.chars().count(), CELL_W as usize, "2桁羅盤: {text:?}");
    }

    #[test]
    fn ヒント数字は近いほど熱く色分けされる() {
        let (_, hot) = hint_display(Some(1), false);
        let (_, near) = hint_display(Some(2), false);
        let (_, far) = hint_display(Some(6), false);
        assert_eq!(hot.fg, Some(Color::LightRed));
        assert_eq!(near.fg, Some(Color::Yellow));
        assert_eq!(far.fg, Some(Color::DarkGray));
    }

    #[test]
    fn 現場グリッドのクリックターゲットは実描画位置と一致する() {
        let mut s = fixed_state();
        // (0,0) を空振り済みにしてヒント数字 "3" が描画される状態にする。
        s.dug[DigState::idx(0, 0)] = true;

        let cs = Rc::new(RefCell::new(ClickState::new()));
        // inner 幅がグリッド内容 (SITE_W*CELL_W=28) より広いエリアで検証。
        // Alignment が Left でないと描画だけ右へずれてここで検出される。
        let mut terminal = Terminal::new(TestBackend::new(60, 20)).unwrap();
        terminal
            .draw(|f| {
                render_site(&s, f, Rect::new(0, 0, 60, 9), &cs, Borders::ALL);
            })
            .unwrap();

        // inner = (1,1,...) with Borders::ALL。cell (0,0) は col 1..5, row 1。
        assert_eq!(cs.borrow().hit_test(1, 1), Some(GRID_CLICK_BASE));
        assert_eq!(cs.borrow().hit_test(5, 1), Some(GRID_CLICK_BASE + 1));

        // クリック座標に実際にヒント数字 "3" が描画されていることをバッファで確認。
        let buffer = terminal.backend().buffer();
        let cell0_text: String = (1..5)
            .map(|x| buffer.cell((x, 1)).unwrap().symbol().to_string())
            .collect();
        assert!(
            cell0_text.contains('3'),
            "セル(0,0)のクリック座標に描画された内容: {cell0_text:?}"
        );
    }

    #[test]
    fn 羅盤行はコストを表示し全幅がタップ対象になる() {
        let mut s = fixed_state();
        s.coins = 999; // コスト30と混同しない値
        let cs = Rc::new(RefCell::new(ClickState::new()));
        let mut terminal = Terminal::new(TestBackend::new(60, 24)).unwrap();
        terminal
            .draw(|f| {
                render_radar_row(&s, f, Rect::new(0, 5, 60, 1), &cs);
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        let row: String = (0..60)
            .map(|x| buffer.cell((x, 5)).unwrap().symbol().to_string())
            .collect();
        assert!(row.contains("30"), "羅盤コストが描画されていない: {row:?}");
        // 行のどこをタップしても羅盤が反応する (タッチ前提の全幅ターゲット)
        assert_eq!(cs.borrow().hit_test(0, 5), Some(super::ACT_RADAR));
        assert_eq!(cs.borrow().hit_test(59, 5), Some(super::ACT_RADAR));
    }

    #[test]
    fn 図鑑はコンテンツが収まらない高さでスクロールターゲットが登録される() {
        let s = fixed_state();
        let cs = Rc::new(RefCell::new(ClickState::new()));
        // 図鑑コンテンツは13行 (3セット分)。inner 6行に収まらない高さで描画。
        let mut terminal = Terminal::new(TestBackend::new(40, 8)).unwrap();
        terminal
            .draw(|f| {
                render_museum(&s, f, Rect::new(0, 0, 40, 8), &cs, Borders::ALL);
            })
            .unwrap();

        let cs_ref = cs.borrow();
        let mut found_down = false;
        for y in 1..7 {
            if cs_ref.hit_test(38, y) == Some(ACT_MUSEUM_SCROLL_DOWN) {
                found_down = true;
            }
        }
        assert!(found_down, "スクロール▼ターゲットが右端に登録されるべき");
    }

    #[test]
    fn 図鑑は未発見の宝の名前を隠す() {
        let s = fixed_state();
        let line = kind_line(&s, ItemKind::Magatama);
        let rendered: String = line.spans.iter().map(|sp| sp.content.to_string()).collect();
        assert!(rendered.contains("？？？"));
        assert!(!rendered.contains("勾玉"));

        let mut found = fixed_state();
        found.museum_counts[ItemKind::Magatama.to_save_id() as usize] = 2;
        let line = kind_line(&found, ItemKind::Magatama);
        let rendered: String = line.spans.iter().map(|sp| sp.content.to_string()).collect();
        assert!(rendered.contains("翡翠の勾玉"));
        assert!(rendered.contains("×2"));
        assert!(rendered.contains("2マス"), "大きさは推理素材として表示する");
    }

    #[test]
    fn ステータス行は全回収で完全制覇表示になる() {
        let mut s = fixed_state();
        let line = status_line(&s);
        let rendered: String = line.spans.iter().map(|sp| sp.content.to_string()).collect();
        assert!(rendered.contains("残り2"));

        for t in s.treasures.clone() {
            for c in t.cells {
                s.dug[c as usize] = true;
            }
        }
        let line = status_line(&s);
        let rendered: String = line.spans.iter().map(|sp| sp.content.to_string()).collect();
        assert!(rendered.contains("完全制覇"));
    }

    #[test]
    fn 全タブが描画してもpanicしない() {
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        for tab in [DigTab::Site, DigTab::Museum] {
            let mut state = fixed_state();
            state.selected_tab = tab;
            state.radar_armed = tab == DigTab::Site;
            let cs = Rc::new(RefCell::new(ClickState::new()));
            terminal
                .draw(|f| {
                    render(&state, f, f.area(), &cs);
                })
                .unwrap();
        }
        // 狭い幅でも panic しない
        let mut narrow = Terminal::new(TestBackend::new(32, 20)).unwrap();
        let state = fixed_state();
        let cs = Rc::new(RefCell::new(ClickState::new()));
        narrow
            .draw(|f| {
                render(&state, f, f.area(), &cs);
            })
            .unwrap();
    }
}
