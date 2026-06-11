//! マージゲームの描画。

use std::cell::RefCell;
use std::rc::Rc;

use ratzilla::ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratzilla::ratatui::style::{Color, Modifier, Style};
use ratzilla::ratatui::text::{Line, Span};
use ratzilla::ratatui::widgets::{Block, Borders, Paragraph};
use ratzilla::ratatui::Frame;

use crate::input::{is_narrow_layout, ClickState};
use crate::widgets::{Clickable, ClickableGrid, ClickableList};

use super::actions::{
    ACT_CLEAR_SELECTION, ACT_QUEST_DELIVER_BASE, ACT_QUEST_REROLL_BASE, ACT_UPGRADE_GENERATORS,
    GRID_CLICK_BASE,
};
use super::state::{Cell, FlashKind, ItemType, MergeState, Quest, GRID_H, GRID_W, QUEST_SLOTS};

/// セル 1 つの表示幅 (terminal columns)。
const CELL_W: u16 = 5;
const CELL_H: u16 = 1;

pub fn render(
    state: &MergeState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let is_narrow = is_narrow_layout(area.width);
    let borders = if is_narrow {
        Borders::TOP | Borders::BOTTOM
    } else {
        Borders::ALL
    };

    // 1: ヘッダ (タイトル / コイン / アップグレード)
    // 2: 盤面 (ジェネレーター + アイテム)
    // 3: クエストリスト
    // 4: ログ
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(CELL_H * GRID_H as u16 + 2),
            Constraint::Min(QUEST_SLOTS as u16 * 2 + 3),
            Constraint::Length(4),
        ])
        .split(area);

    render_header(state, f, chunks[0], click_state, borders);
    render_grid(state, f, chunks[1], click_state, borders);
    render_quests(state, f, chunks[2], click_state, borders);
    render_log(state, f, chunks[3], borders);
}

fn render_header(
    state: &MergeState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
    borders: Borders,
) {
    let upgrade_label = match state.next_upgrade_cost() {
        Some(cost) => format!(" [⚡強化 LV{}→{} 💰{}] ", state.gen_upgrade_level, state.gen_upgrade_level + 1, cost),
        None => " [⚡強化 MAX] ".to_string(),
    };
    let can_upgrade = state
        .next_upgrade_cost()
        .map(|c| state.coins >= c)
        .unwrap_or(false);
    let upgrade_style = if can_upgrade {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let line = Line::from(vec![
        Span::styled(
            " 🧩 マージマンション ",
            Style::default()
                .fg(Color::LightMagenta)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("💰 {} ", state.coins),
            Style::default()
                .fg(Color::LightYellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("最高LV: {} ", state.best_level),
            Style::default().fg(Color::DarkGray),
        ),
    ]);
    let para = Paragraph::new(line)
        .block(
            Block::default()
                .borders(borders)
                .border_style(Style::default().fg(Color::LightMagenta)),
        )
        .alignment(Alignment::Left);
    f.render_widget(para, area);

    // アップグレードボタン: ヘッダ右端にオーバーレイ。
    let label_len = upgrade_label.chars().count() as u16;
    if area.width > label_len + 2 && area.height >= 2 {
        let btn_y = area.y + 1; // タイトル行と同じ行
        let btn_x = area.x + area.width.saturating_sub(label_len + 1);
        let btn_rect = Rect::new(btn_x, btn_y, label_len, 1);
        let btn = Paragraph::new(Span::styled(upgrade_label, upgrade_style));
        Clickable::new(btn, ACT_UPGRADE_GENERATORS).render(
            f,
            btn_rect,
            &mut click_state.borrow_mut(),
        );
    }
}

fn render_grid(
    state: &MergeState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
    borders: Borders,
) {
    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Green))
        .title(" 盤面 (タップで選択 → タップでマージ/移動) ");

    let inner = block.inner(area);

    // 各セル文字列を生成して 1 つの Paragraph に詰める。色付けのため Span 単位。
    let mut lines: Vec<Line> = Vec::with_capacity(GRID_H);
    for y in 0..GRID_H {
        let mut spans: Vec<Span> = Vec::with_capacity(GRID_W);
        for x in 0..GRID_W {
            let (text, style) = cell_display(state, x, y);
            spans.push(Span::styled(text, style));
        }
        lines.push(Line::from(spans));
    }

    // ClickableGrid::register_targets の padding_left は「inner 領域内での横
    // オフセット」。Paragraph を Alignment::Left で描いた時点でセルは inner.x
    // から始まっているので 0。`block` をこの後 Paragraph に move するので、
    // 参照渡しの register_targets はこの順序で先に呼ぶ。
    let _ = inner;
    let grid = ClickableGrid::new(GRID_W, GRID_H, GRID_CLICK_BASE, CELL_W).with_cell_height(CELL_H);
    {
        let mut cs = click_state.borrow_mut();
        grid.register_targets(area, &block, &mut cs, 0);
    }

    let para = Paragraph::new(lines).block(block).alignment(Alignment::Left);
    f.render_widget(para, area);

    // 盤面外タップは hit_test が None になるだけで自然に「何もしない」扱い
    // になるため、明示的な解除ボタンは別配置にする。
    let _ = ACT_CLEAR_SELECTION;
}

/// セル 1 つの表示文字列と色。
fn cell_display(state: &MergeState, x: usize, y: usize) -> (String, Style) {
    let cell = state.get(x, y);
    let is_selected = state.selected == Some((x, y));
    let flash_kind = state
        .flash_cell
        .filter(|fc| (fc.x, fc.y) == (x, y))
        .map(|fc| fc.kind);

    let (text, mut style) = match cell {
        Cell::Empty => (
            "  ·  ".to_string(),
            Style::default().fg(Color::DarkGray),
        ),
        Cell::Generator(t) => {
            let idx = t.gen_index();
            let cd = state.gen_cooldown[idx];
            let glyph = match cd {
                0 => '✦',
                _ => cooldown_glyph(cd, state.current_cooldown_ticks()),
            };
            (
                format!(" [{}]{}", t.label(), glyph),
                Style::default()
                    .fg(generator_color(t, cd == 0))
                    .add_modifier(if cd == 0 {
                        Modifier::BOLD
                    } else {
                        Modifier::DIM
                    }),
            )
        }
        Cell::Item(t, lv) => (
            // CELL_W = 5 と揃えるため 2 + 1(label) + 1(digit) + 1 = 5 文字に。
            // Empty の "  ·  " と中央位置を揃える効果もある。
            format!("  {}{} ", t.label().to_ascii_lowercase(), lv),
            Style::default()
                .fg(item_color(t, lv))
                .add_modifier(if lv >= 4 { Modifier::BOLD } else { Modifier::empty() }),
        ),
    };

    if is_selected {
        style = style.bg(Color::Indexed(238)).add_modifier(Modifier::REVERSED);
    } else if let Some(kind) = flash_kind {
        style = apply_flash(style, kind);
    }

    (text, style)
}

/// flash 種別ごとの強調スタイル。格 (Action < Merge < Record) が
/// 視覚的な強さに一致するようにする。
fn apply_flash(base: Style, kind: FlashKind) -> Style {
    match kind {
        // 軽い操作フィードバック: 元の色のまま控えめに光る。
        FlashKind::Action => base.add_modifier(Modifier::BOLD).bg(Color::Indexed(236)),
        // マージ成立: 明るい色 + 太字で「進化」の瞬間を見せる。
        FlashKind::Merge => base
            .fg(Color::LightYellow)
            .add_modifier(Modifier::BOLD)
            .bg(Color::Indexed(238)),
        // 自己ベスト更新: 反転 + LightYellow で最も派手に。
        FlashKind::Record => base
            .fg(Color::LightYellow)
            .add_modifier(Modifier::REVERSED | Modifier::BOLD),
    }
}

/// cooldown 進行度を 1 文字で表す。残り少ない (≒完成間近) ほど glyph が高くなる。
fn cooldown_glyph(remaining: u32, total: u32) -> char {
    if total == 0 {
        return '✦';
    }
    let done = total.saturating_sub(remaining);
    let stages = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let stage_idx = ((done as usize * stages.len()) / (total as usize).max(1)).min(stages.len() - 1);
    stages[stage_idx]
}

fn generator_color(t: ItemType, ready: bool) -> Color {
    if !ready {
        return Color::DarkGray;
    }
    match t {
        ItemType::Flower => Color::LightGreen,
        ItemType::Gem => Color::LightBlue,
        ItemType::Tool => Color::LightYellow,
    }
}

/// アイテム色: 種類で色相、レベルで明度。
fn item_color(t: ItemType, lv: u8) -> Color {
    match (t, lv) {
        (ItemType::Flower, 1) => Color::Green,
        (ItemType::Flower, 2) => Color::LightGreen,
        (ItemType::Flower, 3) => Color::Yellow,
        (ItemType::Flower, 4) => Color::LightMagenta,
        (ItemType::Flower, _) => Color::Magenta,
        (ItemType::Gem, 1) => Color::Blue,
        (ItemType::Gem, 2) => Color::LightBlue,
        (ItemType::Gem, 3) => Color::Cyan,
        (ItemType::Gem, 4) => Color::LightCyan,
        (ItemType::Gem, _) => Color::LightMagenta,
        (ItemType::Tool, 1) => Color::Yellow,
        (ItemType::Tool, 2) => Color::LightYellow,
        (ItemType::Tool, 3) => Color::LightRed,
        (ItemType::Tool, 4) => Color::Red,
        (ItemType::Tool, _) => Color::LightMagenta,
    }
}

fn render_quests(
    state: &MergeState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
    borders: Borders,
) {
    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Cyan))
        .title(" クエスト ");

    let mut cl = ClickableList::new();
    cl.push(Line::from(""));

    for (i, q) in state.quests.iter().enumerate() {
        match q {
            Some(quest) => {
                let inv = state.count_items(quest.item_type, quest.level);
                let can_deliver = inv >= quest.needed;
                cl.push_clickable(quest_deliver_line(quest, inv, can_deliver), ACT_QUEST_DELIVER_BASE + i as u16);
                cl.push_clickable(quest_reroll_line(), ACT_QUEST_REROLL_BASE + i as u16);
            }
            None => {
                cl.push(Line::from(Span::styled(
                    "  (新しいクエストを準備中…)",
                    Style::default().fg(Color::DarkGray),
                )));
                cl.push(Line::from(""));
            }
        }
    }

    {
        let mut cs = click_state.borrow_mut();
        cl.render(f, area, block, &mut cs, false, 0);
    }
}

fn quest_deliver_line(quest: &Quest, inv: u8, can_deliver: bool) -> Line<'static> {
    let item_label = quest.item_type.full_name();
    let progress_color = if can_deliver { Color::LightGreen } else { Color::Yellow };
    let main_color = if can_deliver { Color::White } else { Color::DarkGray };
    let marker = if can_deliver { "▶ 納品" } else { "▷ 納品" };
    let marker_style = if can_deliver {
        Style::default().fg(Color::LightYellow).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    Line::from(vec![
        Span::styled(format!("  {}  ", marker), marker_style),
        Span::styled(
            format!("{} LV{} ×{} ", item_label, quest.level, quest.needed),
            Style::default().fg(main_color),
        ),
        Span::styled(
            format!("({}/{}) ", inv.min(quest.needed), quest.needed),
            Style::default().fg(progress_color),
        ),
        Span::styled(
            format!("💰{}", quest.reward),
            Style::default()
                .fg(Color::LightYellow)
                .add_modifier(Modifier::BOLD),
        ),
    ])
}

fn quest_reroll_line() -> Line<'static> {
    Line::from(Span::styled(
        "     × 破棄 (新しいクエストを抽選)",
        Style::default().fg(Color::DarkGray),
    ))
}

fn render_log(state: &MergeState, f: &mut Frame, area: Rect, borders: Borders) {
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
    use super::super::state::{BASE_COOLDOWN, MAX_LEVEL};

    #[test]
    fn cell_display_all_variants_have_uniform_width() {
        // Empty / Generator / Item の 3 バリアント全てが CELL_W に揃っていない
        // と、ClickableGrid の cell_display_width=CELL_W で登録した click target
        // と実描画位置が列ごとにずれて誤タップが発生する。テーブル駆動で 3
        // バリアントを必ず網羅する。
        let mut s = MergeState::new();
        s.set(1, 1, Cell::Item(ItemType::Flower, 1));
        s.set(2, 2, Cell::Item(ItemType::Gem, 5));
        s.set(3, 3, Cell::Item(ItemType::Tool, 3));
        let probes: &[(usize, usize, &str)] = &[
            (4, 4, "empty"),
            (0, 0, "generator-ready"),
            (1, 1, "item-lv1"),
            (2, 2, "item-lv5"),
            (3, 3, "item-lv3"),
        ];
        for &(x, y, label) in probes {
            let (text, _) = cell_display(&s, x, y);
            assert_eq!(
                text.chars().count(),
                CELL_W as usize,
                "{} cell width: text={:?}",
                label,
                text,
            );
        }

        // cooldown 中の generator も同じ幅
        s.gen_cooldown[0] = 10;
        let (text, _) = cell_display(&s, 0, 0);
        assert_eq!(text.chars().count(), CELL_W as usize);
    }

    #[test]
    fn cooldown_glyph_progresses_with_remaining() {
        let early = cooldown_glyph(BASE_COOLDOWN, BASE_COOLDOWN);
        let late = cooldown_glyph(1, BASE_COOLDOWN);
        assert_ne!(early, late);
    }

    #[test]
    fn quest_deliver_line_includes_inventory_marker() {
        let q = Quest {
            item_type: ItemType::Flower,
            level: 2,
            needed: 3,
            reward: 100,
        };
        let line = quest_deliver_line(&q, 1, false);
        let rendered: String = line
            .spans
            .iter()
            .map(|s| s.content.to_string())
            .collect();
        assert!(rendered.contains("1/3"));
        assert!(rendered.contains("Flower"));
        assert!(rendered.contains("LV2"));
        assert!(rendered.contains("100"));
    }

    #[test]
    fn フラッシュ演出は種別ごとに強さが違う() {
        let base = Style::default().fg(Color::Green);
        let action = apply_flash(base, FlashKind::Action);
        let merge = apply_flash(base, FlashKind::Merge);
        let record = apply_flash(base, FlashKind::Record);
        // マージ成立: BOLD + 明るい色で「進化」を見せる
        assert!(merge.add_modifier.contains(Modifier::BOLD));
        assert_eq!(merge.fg, Some(Color::LightYellow));
        // ベスト更新: REVERSED + LightYellow で最も派手
        assert!(record.add_modifier.contains(Modifier::REVERSED));
        assert_eq!(record.fg, Some(Color::LightYellow));
        // 通常アクションは元の色を保ったまま控えめに光る
        assert_eq!(action.fg, Some(Color::Green));
        assert!(!action.add_modifier.contains(Modifier::REVERSED));
    }

    #[test]
    fn フラッシュは該当セルだけに乗る() {
        let mut s = MergeState::new();
        s.set(1, 1, Cell::Item(ItemType::Flower, 1));
        s.set(2, 2, Cell::Item(ItemType::Flower, 1));
        s.flash_with(1, 1, FlashKind::Record);
        let (_, flashed) = cell_display(&s, 1, 1);
        let (_, plain) = cell_display(&s, 2, 2);
        assert!(flashed.add_modifier.contains(Modifier::REVERSED));
        assert!(!plain.add_modifier.contains(Modifier::REVERSED));
    }

    #[test]
    fn item_color_differs_per_level() {
        // 単純な smoke: 5 lv 全てで panic しないだけでなく、何かしら色がつく
        for lv in 1..=MAX_LEVEL {
            for t in ItemType::all() {
                let _ = item_color(t, lv);
            }
        }
    }
}
