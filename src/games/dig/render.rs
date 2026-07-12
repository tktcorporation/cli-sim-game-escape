//! 穴掘り長屋の描画。

use std::cell::RefCell;
use std::rc::Rc;

use ratzilla::ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratzilla::ratatui::style::{Color, Modifier, Style};
use ratzilla::ratatui::text::{Line, Span};
use ratzilla::ratatui::widgets::{Block, Borders, Paragraph};
use ratzilla::ratatui::Frame;

use crate::input::{is_narrow_layout, ClickState};
use crate::widgets::{Clickable, ClickableGrid, ClickableList, TabBar};

use super::actions::{ACT_NEIGHBOR_DIG_BASE, ACT_TAB_COLLECTION, ACT_TAB_NEIGHBORS, ACT_TAB_YARD, ACT_UPGRADE_SHOVEL, GRID_CLICK_BASE};
use super::logic;
use super::state::{CollectionSet, DigState, DigTab, ItemKind, Neighbor, MAX_ACTIONS_PER_DAY, YARD_H, YARD_W};

/// セル1つの表示幅 (terminal columns)。
const CELL_W: u16 = 5;
const CELL_H: u16 = 1;

pub fn render(state: &DigState, f: &mut Frame, area: Rect, click_state: &Rc<RefCell<ClickState>>) {
    let is_narrow = is_narrow_layout(area.width);
    let borders = if is_narrow {
        Borders::TOP | Borders::BOTTOM
    } else {
        Borders::ALL
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(2),
            Constraint::Min(CELL_H * YARD_H as u16 + 2),
            Constraint::Length(4),
        ])
        .split(area);

    render_header(state, f, chunks[0], click_state, borders);
    render_tabs(state, f, chunks[1], click_state);
    match state.selected_tab {
        DigTab::Yard => render_yard(state, f, chunks[2], click_state, borders),
        DigTab::Neighbors => render_neighbors(state, f, chunks[2], click_state, borders),
        DigTab::Collection => render_collection(state, f, chunks[2], borders),
    }
    render_log(state, f, chunks[3], borders);
}

fn render_header(
    state: &DigState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
    borders: Borders,
) {
    let upgrade_label = match logic::shovel_upgrade_cost(state.shovel_level) {
        Some(cost) => format!(
            " [⚡強化 Lv{}→{} 💰{}] ",
            state.shovel_level,
            state.shovel_level + 1,
            cost
        ),
        None => " [⚡シャベル MAX] ".to_string(),
    };
    let can_upgrade = logic::shovel_upgrade_cost(state.shovel_level)
        .map(|c| state.coins >= c)
        .unwrap_or(false);
    let upgrade_style = if can_upgrade {
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let line = Line::from(vec![
        Span::styled(
            " ⛏ 穴掘り長屋 ",
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("💰{} ", state.coins),
            Style::default().fg(Color::LightYellow).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("行動力 {}/{} ", state.actions_remaining, MAX_ACTIONS_PER_DAY),
            action_style(state.actions_remaining),
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

    let label_len = upgrade_label.chars().count() as u16;
    if area.width > label_len + 2 && area.height >= 2 {
        let btn_y = area.y + 1;
        let btn_x = area.x + area.width.saturating_sub(label_len + 1);
        let btn_rect = Rect::new(btn_x, btn_y, label_len, 1);
        let btn = Paragraph::new(Span::styled(upgrade_label, upgrade_style));
        Clickable::new(btn, ACT_UPGRADE_SHOVEL).render(f, btn_rect, &mut click_state.borrow_mut());
    }
}

fn action_style(actions_remaining: u8) -> Style {
    if actions_remaining == 0 {
        Style::default().fg(Color::DarkGray)
    } else {
        Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD)
    }
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
        .tab("庭", style_for(DigTab::Yard), ACT_TAB_YARD)
        .tab("ご近所", style_for(DigTab::Neighbors), ACT_TAB_NEIGHBORS)
        .tab("図鑑", style_for(DigTab::Collection), ACT_TAB_COLLECTION)
        .block(Block::default().borders(Borders::BOTTOM).border_style(Style::default().fg(Color::DarkGray)));
    let mut cs = click_state.borrow_mut();
    bar.render(f, area, &mut cs);
}

fn render_yard(
    state: &DigState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
    borders: Borders,
) {
    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Green))
        .title(" 庭 (タップで掘る) ");

    let mut lines: Vec<Line> = Vec::with_capacity(YARD_H);
    for y in 0..YARD_H {
        let mut spans: Vec<Span> = Vec::with_capacity(YARD_W);
        for x in 0..YARD_W {
            let idx = y * YARD_W + x;
            let (text, style) = yard_cell_display(state, idx);
            spans.push(Span::styled(text, style));
        }
        lines.push(Line::from(spans));
    }

    let grid = ClickableGrid::new(YARD_W, YARD_H, GRID_CLICK_BASE, CELL_W).with_cell_height(CELL_H);
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

/// セル1つの表示文字列と色。未掘は "  ·  " (行動力切れなら "  ✗  ")、
/// 掘った後は2文字ASCIIグリフ。
/// (CJK グリフは terminal 表示幅が chars().count() と食い違うため使わない。)
fn yard_cell_display(state: &DigState, idx: usize) -> (String, Style) {
    match state.yard[idx] {
        None if state.actions_remaining == 0 => {
            ("  ✗  ".to_string(), Style::default().fg(Color::DarkGray))
        }
        None => ("  ·  ".to_string(), Style::default().fg(Color::DarkGray)),
        Some(item) => (format!("  {} ", item.glyph()), item_style(item)),
    }
}

fn item_style(item: ItemKind) -> Style {
    match item.collection() {
        Some(CollectionSet::Pottery) => {
            Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD)
        }
        Some(CollectionSet::Dragon) => {
            Style::default().fg(Color::LightRed).add_modifier(Modifier::BOLD)
        }
        Some(CollectionSet::Maneki) => {
            Style::default().fg(Color::LightMagenta).add_modifier(Modifier::BOLD)
        }
        None => match item {
            ItemKind::Dirt => Style::default().fg(Color::Gray),
            ItemKind::Pebble => Style::default().fg(Color::White),
            ItemKind::CopperCoin => Style::default().fg(Color::Yellow),
            ItemKind::SilverChunk => Style::default().fg(Color::LightCyan),
            ItemKind::GoldNugget => {
                Style::default().fg(Color::LightYellow).add_modifier(Modifier::BOLD)
            }
            // 通貨アイテムを網羅しているため到達しないが、網羅性のため残す。
            _ => Style::default(),
        },
    }
}

fn render_neighbors(
    state: &DigState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
    borders: Borders,
) {
    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::LightBlue))
        .title(" ご近所 (お福分け穴を掘らせてもらう) ");

    let mut cl = ClickableList::new();
    cl.push(Line::from(""));
    for (i, n) in state.neighbors.iter().enumerate() {
        let level = logic::friendship_level(n.total_digs);
        let can_dig = !n.dug_today && state.actions_remaining > 0;
        let line = neighbor_line(n, level, can_dig);
        if can_dig {
            cl.push_clickable(line, ACT_NEIGHBOR_DIG_BASE + i as u16);
        } else {
            cl.push(line);
        }
        cl.push(Line::from(""));
    }

    let mut cs = click_state.borrow_mut();
    cl.render(f, area, block, &mut cs, false, 0);
}

fn neighbor_line(n: &Neighbor, level: u8, can_dig: bool) -> Line<'static> {
    let (marker, marker_style) = if can_dig {
        (
            "▶ 掘らせてもらう",
            Style::default().fg(Color::LightYellow).add_modifier(Modifier::BOLD),
        )
    } else if n.dug_today {
        ("✓ 本日は済み", Style::default().fg(Color::DarkGray))
    } else {
        ("✗ 行動力不足", Style::default().fg(Color::DarkGray))
    };

    Line::from(vec![
        Span::styled(format!("  {}  ", marker), marker_style),
        Span::styled(format!("{} ", n.name), Style::default().fg(Color::White)),
        Span::styled(
            format!("({}専門) ", n.specialty.display_name()),
            Style::default().fg(Color::Gray),
        ),
        Span::styled(format!("友好度Lv{level}"), Style::default().fg(Color::LightMagenta)),
    ])
}

fn render_collection(state: &DigState, f: &mut Frame, area: Rect, borders: Borders) {
    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::LightMagenta))
        .title(" 図鑑 ");

    let mut lines: Vec<Line> = vec![Line::from("")];
    for set in CollectionSet::all() {
        lines.push(collection_progress_line(state, set));
        lines.push(Line::from(""));
    }

    let para = Paragraph::new(lines).block(block);
    f.render_widget(para, area);
}

/// 未発見のかけらが残るセットは名前を「？？？」で隠す — 見つけて初めて
/// 何のコレクションだったか分かる「アハ」体験のため。
fn collection_progress_line(state: &DigState, set: CollectionSet) -> Line<'static> {
    if state.completed_sets[set.index()] {
        return Line::from(vec![
            Span::styled(" ★ ", Style::default().fg(Color::LightYellow).add_modifier(Modifier::BOLD)),
            Span::styled(
                set.display_name(),
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
            ),
            Span::styled(" — コンプリート済み", Style::default().fg(Color::LightGreen)),
        ]);
    }

    let pieces = set.pieces();
    let found = pieces
        .iter()
        .filter(|p| state.piece_counts[p.piece_slot().unwrap()] >= 1)
        .count();
    let name_span = if found > 0 {
        Span::styled(set.display_name(), Style::default().fg(Color::White))
    } else {
        Span::styled("？？？", Style::default().fg(Color::DarkGray))
    };

    Line::from(vec![
        Span::styled(" ○ ", Style::default().fg(Color::DarkGray)),
        name_span,
        Span::styled(format!(" ({}/{})", found, pieces.len()), Style::default().fg(Color::Yellow)),
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
    use crate::input::ClickState;

    #[test]
    fn 庭セルは未掘も全種類も表示幅が揃う() {
        let mut s = DigState::new();
        for (i, item) in ItemKind::all().into_iter().enumerate() {
            if i < YARD_W * YARD_H {
                s.yard[i] = Some(item);
            }
        }
        let (text, _) = yard_cell_display(&s, YARD_W * YARD_H - 1);
        // 未掘セル (最後の1つは12種のitemを詰めきれないので確実に空きが残る位置を確認)
        let (undug_text, _) = yard_cell_display(&DigState::new(), 0);
        assert_eq!(undug_text.chars().count(), CELL_W as usize);
        assert_eq!(text.chars().count(), CELL_W as usize);

        for item in ItemKind::all() {
            let mut probe = DigState::new();
            probe.yard[0] = Some(item);
            let (t, _) = yard_cell_display(&probe, 0);
            assert_eq!(t.chars().count(), CELL_W as usize, "{item:?} の表示幅がずれている");
        }
    }

    #[test]
    fn 庭グリッドのクリックターゲットは実描画位置と一致する() {
        use ratzilla::ratatui::backend::TestBackend;
        use ratzilla::ratatui::Terminal;

        let mut state = DigState::new();
        state.yard[0] = Some(ItemKind::GoldNugget); // glyph "gd"
        let cs = Rc::new(RefCell::new(ClickState::new()));
        // inner 幅 (58) がグリッド内容の幅 (YARD_W*CELL_W=25) よりずっと広い
        // エリアで検証する。もし Paragraph が Alignment::Center で描画されると
        // 実際の文字は右へずれるが、register_targets は常に左端基準で座標を
        // 登録するため、幅に余裕がある時だけこのズレが露見する。
        let mut terminal = Terminal::new(TestBackend::new(60, 20)).unwrap();
        terminal
            .draw(|f| {
                render_yard(&state, f, Rect::new(0, 0, 60, 5), &cs, Borders::ALL);
            })
            .unwrap();

        // inner = (1,1,58,3) with Borders::ALL, cell (0,0): col=1+0*5=1, row=1
        let hit = cs.borrow().hit_test(1, 1);
        assert_eq!(hit, Some(GRID_CLICK_BASE));
        // cell (1,0): col = 1 + 1*5 = 6
        let hit1 = cs.borrow().hit_test(6, 1);
        assert_eq!(hit1, Some(GRID_CLICK_BASE + 1));

        // register_targets が指すクリック座標に、実際に "gd" が描画されている
        // ことをバッファから直接確認する (クリック領域と表示位置の一致を
        // register_targets の計算式の再現ではなく実描画結果で検証する)。
        let buffer = terminal.backend().buffer();
        let cell0_text: String = (1..6)
            .map(|x| buffer.cell((x, 1)).unwrap().symbol().to_string())
            .collect();
        assert!(
            cell0_text.contains("gd"),
            "セル(0,0)のクリック座標に実際に描画された内容: {cell0_text:?}"
        );
    }

    #[test]
    fn 庭セルは行動力切れだと未掘セルがバツ表示になり幅も揃う() {
        let mut s = DigState::new();
        s.actions_remaining = 0;
        let (text, _) = yard_cell_display(&s, 0);
        assert!(text.contains('✗'));
        assert_eq!(text.chars().count(), CELL_W as usize);
    }

    #[test]
    fn neighbor_lineは掘れる時だけ強調される() {
        let n = Neighbor {
            name: "テスト隣人",
            specialty: CollectionSet::Dragon,
            dug_today: false,
            total_digs: 0,
        };
        let line = neighbor_line(&n, 0, true);
        let rendered: String = line.spans.iter().map(|s| s.content.to_string()).collect();
        assert!(rendered.contains("掘らせてもらう"));
        assert!(rendered.contains("テスト隣人"));
        assert!(rendered.contains("小さな竜の骨格"));

        let done = neighbor_line(&n, 0, false);
        let rendered_done: String = done.spans.iter().map(|s| s.content.to_string()).collect();
        assert!(!rendered_done.contains("掘らせてもらう"));
    }

    #[test]
    fn collection_progress_lineは未発見なら名前を隠す() {
        let s = DigState::new();
        let line = collection_progress_line(&s, CollectionSet::Pottery);
        let rendered: String = line.spans.iter().map(|s| s.content.to_string()).collect();
        assert!(rendered.contains("？？？"));
        assert!(!rendered.contains("唐草文様の土器"));
    }

    #[test]
    fn collection_progress_lineは1つでも見つければ名前を出す() {
        let mut s = DigState::new();
        s.piece_counts[ItemKind::PotteryTop.piece_slot().unwrap()] = 1;
        let line = collection_progress_line(&s, CollectionSet::Pottery);
        let rendered: String = line.spans.iter().map(|s| s.content.to_string()).collect();
        assert!(rendered.contains("唐草文様の土器"));
        assert!(rendered.contains("(1/2)"));
    }

    #[test]
    fn collection_progress_lineはコンプリート済みなら達成表示になる() {
        let mut s = DigState::new();
        s.completed_sets[CollectionSet::Maneki.index()] = true;
        let line = collection_progress_line(&s, CollectionSet::Maneki);
        let rendered: String = line.spans.iter().map(|s| s.content.to_string()).collect();
        assert!(rendered.contains("コンプリート済み"));
    }

    #[test]
    fn 全タブが描画してもpanicしない() {
        use ratzilla::ratatui::backend::TestBackend;
        use ratzilla::ratatui::Terminal;

        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        for tab in [DigTab::Yard, DigTab::Neighbors, DigTab::Collection] {
            let mut state = DigState::new();
            state.selected_tab = tab;
            let cs = Rc::new(RefCell::new(ClickState::new()));
            terminal
                .draw(|f| {
                    render(&state, f, f.area(), &cs);
                })
                .unwrap();
        }
    }
}
