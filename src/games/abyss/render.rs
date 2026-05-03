//! 深淵潜行 — UI レンダリング。読み取り専用 (state を一切変更しない)。

use std::cell::RefCell;
use std::rc::Rc;

use ratzilla::ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratzilla::ratatui::style::{Color, Modifier, Style};
use ratzilla::ratatui::text::{Line, Span};
use ratzilla::ratatui::widgets::{Block, Borders, Paragraph};
use ratzilla::ratatui::Frame;

use crate::input::{is_narrow_layout, ClickState};
use crate::widgets::{Clickable, ClickableList, TabBar};

use super::actions::*;
use super::state::{AbyssState, FloorKind, SoulPerk, Tab, UpgradeKind};

/// 主要パネルの Rect。effect 配置と通常描画の両方で同じ値を使うため切り出し。
///
/// AbyssGame::render から「敵パネルだけ」に effect を当てたい時、layout 計算を
/// render.rs と mod.rs で重複させないために共通化している。
pub struct AbyssLayout {
    pub header: Rect,
    pub combat: Rect,
    pub hero_panel: Rect,
    pub enemy_panel: Rect,
    pub toggle: Rect,
    pub tab_bar: Rect,
    pub body: Rect,
}

/// area から各パネル Rect を計算する。`render` の中でも、effect 配置時にも同じ式が使える。
pub fn compute_layout(area: Rect) -> AbyssLayout {
    let narrow = is_narrow_layout(area.width);
    // narrow 時は body に縦領域を渡したいので combat を 1 行詰める。
    // hero panel は narrow で stats 行を省略済 (最大 5 行) なので inner=5 で収まる。
    let combat_height: u16 = if narrow { 7 } else { 9 };
    // narrow 時はヘッダを 2 行 (B/floor_kind/最深 + 通貨 3 種) に分けるため
    // 内部 2 行 + ALL ボーダー 2 = 4。wide は従来通り 1 行 (= 3)。
    let header_height: u16 = if narrow { 4 } else { 3 };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(header_height),
            Constraint::Length(combat_height),
            // toggle bar: 浅瀬に戻るボタン 1 行のみ (枠なし)。
            // 自動潜行トグルは Settings タブに移動した。
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(8),
        ])
        .split(area);

    // render_combat 内と同じ Block::default().borders(ALL) で内側 Rect を計算する。
    // ここを式で揃えておかないと effect の領域がずれて「枠がフラッシュ対象外」みたいに
    // ちぐはぐになるので、Block を経由して inner を取るのがいちばん安全。
    let combat = chunks[1];
    let combat_inner = Block::default().borders(Borders::ALL).inner(combat);
    let halves = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(combat_inner);

    AbyssLayout {
        header: chunks[0],
        combat,
        hero_panel: halves[0],
        enemy_panel: halves[1],
        toggle: chunks[2],
        tab_bar: chunks[3],
        body: chunks[4],
    }
}

pub fn render(state: &AbyssState, f: &mut Frame, area: Rect, click_state: &Rc<RefCell<ClickState>>) {
    let narrow = is_narrow_layout(area.width);
    let l = compute_layout(area);

    render_header(state, f, l.header, narrow);
    render_combat(state, f, l.combat, narrow);
    render_toggle_bar(state, f, l.toggle, click_state);
    render_tab_bar(state, f, l.tab_bar, click_state);

    // タブコンテンツ + ログを縦分割
    let log_h: u16 = if narrow { 4 } else { 5 };
    let body_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(5), Constraint::Length(log_h)])
        .split(l.body);

    match state.tab {
        Tab::Upgrades => render_upgrades(state, f, body_chunks[0], click_state),
        Tab::Souls => render_souls(state, f, body_chunks[0], click_state),
        Tab::Stats => render_stats(state, f, body_chunks[0], click_state),
        Tab::Gacha => render_gacha(state, f, body_chunks[0], click_state),
        Tab::Settings => render_settings(state, f, body_chunks[0], click_state),
    }
    render_log(state, f, body_chunks[1]);
}

// ── ヘッダ ─────────────────────────────────────────────────

fn render_header(state: &AbyssState, f: &mut Frame, area: Rect, narrow: bool) {
    // 1 行目: フロア / 種別 / 最深
    let mut floor_spans: Vec<Span> = vec![Span::styled(
        format!(" B{}F ", state.floor),
        Style::default()
            .fg(Color::Black)
            .bg(floor_color(state.floor))
            .add_modifier(Modifier::BOLD),
    )];
    if !matches!(state.floor_kind, FloorKind::Normal) {
        floor_spans.push(Span::raw(" "));
        floor_spans.push(Span::styled(
            format!(
                "{}{}",
                state.floor_kind.short_label(),
                state.floor_kind.name()
            ),
            Style::default()
                .fg(floor_kind_color(state.floor_kind))
                .add_modifier(Modifier::BOLD),
        ));
    }
    floor_spans.push(Span::raw(" "));
    floor_spans.push(Span::styled(
        format!("最深: B{}F", state.deepest_floor_ever),
        Style::default().fg(Color::DarkGray),
    ));

    // 2 行目候補: 通貨 3 種 (gold / soul / key)
    let currency_spans: Vec<Span> = vec![
        Span::styled(
            format!(" 💰{}", format_num(state.gold)),
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(
            format!("✦{}", format_num(state.souls)),
            Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(
            format!("🔑{}", format_num(state.keys)),
            Style::default()
                .fg(Color::LightCyan)
                .add_modifier(Modifier::BOLD),
        ),
    ];

    let lines: Vec<Line> = if narrow {
        // narrow 時は 2 行構成。compute_layout の header_height=4 と整合。
        vec![Line::from(floor_spans), Line::from(currency_spans)]
    } else {
        // wide 時は従来通り 1 行に詰める。spaces で区切る。
        let mut all = floor_spans;
        all.push(Span::raw(" "));
        all.extend(currency_spans);
        vec![Line::from(all)]
    };

    let widget = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(" 深淵潜行 "),
    );
    f.render_widget(widget, area);
}

fn floor_color(floor: u32) -> Color {
    match floor {
        1..=4 => Color::Green,
        5..=9 => Color::Yellow,
        10..=14 => Color::LightRed,
        15..=24 => Color::Magenta,
        _ => Color::Red,
    }
}

fn floor_kind_color(kind: FloorKind) -> Color {
    match kind {
        FloorKind::Normal => Color::DarkGray,
        FloorKind::Treasure => Color::LightYellow,
        FloorKind::Elite => Color::LightRed,
        FloorKind::Bonanza => Color::LightCyan,
    }
}

// ── 戦闘表示 ───────────────────────────────────────────────

fn render_combat(state: &AbyssState, f: &mut Frame, area: Rect, narrow: bool) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(combat_title(state));
    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.height < 4 || inner.width < 20 {
        return;
    }

    // 横半分ずつ hero / enemy
    let halves = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(inner);
    render_hero_panel(state, f, halves[0], narrow);
    render_enemy_panel(state, f, halves[1], narrow);
}

fn combat_title(state: &AbyssState) -> String {
    let kind_tag = match state.floor_kind {
        FloorKind::Normal => String::new(),
        other => format!(" {} ", other.short_label()),
    };
    let until = state.enemies_until_boss();
    if state.current_enemy.is_boss {
        format!(" 戦闘{}— ボス戦 (B{}F) ", kind_tag, state.floor)
    } else if until == 0 {
        format!(" 戦闘{}— ボス出現中 (B{}F) ", kind_tag, state.floor)
    } else {
        format!(" 戦闘{}— ボスまであと {} 体 ", kind_tag, until)
    }
}

fn render_hero_panel(state: &AbyssState, f: &mut Frame, area: Rect, narrow: bool) {
    let max_hp = state.hero_max_hp();
    let bar_width = (area.width.saturating_sub(2)).clamp(8, 20);

    let hero_name_color = if state.hero_hurt_flash > 0 {
        Color::Red
    } else {
        Color::Cyan
    };

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(Span::styled(
        " 勇者",
        Style::default().fg(hero_name_color).add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(make_hp_bar_line(state.hero_hp, max_hp, bar_width, Color::Green)));

    // 攻撃進捗バー
    let progress = atk_progress(state.hero_atk_period(), state.hero_atk_cooldown);
    lines.push(Line::from(make_progress_line(
        " ATK ",
        progress,
        bar_width,
        Color::Yellow,
    )));

    // 戦闘集中バー (focus)。攻撃成功で溜まり、被弾で削れる。
    let focus_max = state.config.hero.focus_max.max(1);
    let focus_frac = state.combat_focus as f32 / focus_max as f32;
    lines.push(Line::from(make_progress_line(
        " 集中",
        focus_frac.clamp(0.0, 1.0),
        bar_width,
        Color::LightCyan,
    )));

    if !narrow {
        lines.push(Line::from(Span::styled(
            format!(
                " ⚔{} 🛡{} CRIT{}%",
                state.hero_atk(),
                state.hero_def(),
                (state.hero_crit_rate() * 100.0).round() as u32,
            ),
            Style::default().fg(Color::DarkGray),
        )));
    }

    if let Some((dmg, life)) = state.last_hero_damage {
        if life > 0 {
            lines.push(Line::from(Span::styled(
                format!(" -{}", dmg),
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            )));
        }
    }

    f.render_widget(Paragraph::new(lines), area);
}

fn render_enemy_panel(state: &AbyssState, f: &mut Frame, area: Rect, narrow: bool) {
    let bar_width = (area.width.saturating_sub(2)).clamp(8, 20);

    let name_color = if state.enemy_hurt_flash > 0 {
        Color::Yellow
    } else if state.current_enemy.is_boss {
        Color::Red
    } else {
        Color::White
    };

    let prefix = if state.current_enemy.is_boss { "👑 " } else { "  " };
    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(Span::styled(
        format!("{}{}", prefix, state.current_enemy.name),
        Style::default().fg(name_color).add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(make_hp_bar_line(
        state.current_enemy.hp,
        state.current_enemy.max_hp,
        bar_width,
        if state.current_enemy.is_boss { Color::Red } else { Color::LightRed },
    )));

    let e_progress = atk_progress(state.current_enemy.atk_period, state.current_enemy.atk_cooldown);
    lines.push(Line::from(make_progress_line(
        " ATK ",
        e_progress,
        bar_width,
        Color::LightRed,
    )));

    if !narrow {
        lines.push(Line::from(Span::styled(
            format!(
                " ⚔{} 🛡{} 💰{}",
                state.current_enemy.atk, state.current_enemy.def, state.current_enemy.gold,
            ),
            Style::default().fg(Color::DarkGray),
        )));
    }

    if let Some((dmg, life, crit)) = state.last_enemy_damage {
        if life > 0 {
            let label = if crit { format!(" -{} CRIT!", dmg) } else { format!(" -{}", dmg) };
            let color = if crit { Color::LightYellow } else { Color::Yellow };
            lines.push(Line::from(Span::styled(
                label,
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            )));
        }
    }

    f.render_widget(Paragraph::new(lines), area);
}

/// 0.0..=1.0 の進捗を返す。cooldown=period (まだ攻撃直後) → 0.0、cooldown=0 (発射寸前) → 1.0
fn atk_progress(period: u32, cooldown: u32) -> f32 {
    if period == 0 {
        return 1.0;
    }
    let elapsed = period.saturating_sub(cooldown) as f32;
    (elapsed / period as f32).clamp(0.0, 1.0)
}

fn make_hp_bar_line(cur: u64, max: u64, width: u16, color: Color) -> Vec<Span<'static>> {
    let frac = if max == 0 { 0.0 } else { (cur as f64 / max as f64).clamp(0.0, 1.0) };
    let filled = (frac * width as f64).round() as u16;
    let bar: String = (0..width)
        .map(|i| if i < filled { '█' } else { '░' })
        .collect();
    vec![
        Span::styled(format!(" {}", bar), Style::default().fg(color)),
        Span::styled(
            format!(" {}/{}", format_num(cur), format_num(max)),
            Style::default().fg(Color::White),
        ),
    ]
}

fn make_progress_line(label: &'static str, frac: f32, width: u16, color: Color) -> Vec<Span<'static>> {
    let filled = (frac as f64 * width as f64).round() as u16;
    let bar: String = (0..width)
        .map(|i| if i < filled { '▰' } else { '▱' })
        .collect();
    vec![
        Span::styled(label, Style::default().fg(Color::DarkGray)),
        Span::styled(bar, Style::default().fg(color)),
    ]
}

// ── トグル行 ───────────────────────────────────────────────

fn render_toggle_bar(
    _state: &AbyssState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    // 自動潜行は Settings タブに移したので、ここは「浅瀬に戻る」1 ボタンのみ。
    // 枠を外して 1 行に圧縮することで、強化リストに 2 行余分に渡せる。
    if area.height == 0 {
        return;
    }
    let retreat_para = Paragraph::new(Line::from(Span::styled(
        " 浅瀬に戻る △",
        Style::default().fg(Color::LightCyan),
    )))
    .alignment(Alignment::Right);

    let mut cs = click_state.borrow_mut();
    Clickable::new(retreat_para, RETREAT_TO_SURFACE).render(f, area, &mut cs);
}

// ── タブバー ───────────────────────────────────────────────

fn render_tab_bar(
    state: &AbyssState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let active_idx = match state.tab {
        Tab::Upgrades => 0,
        Tab::Souls => 1,
        Tab::Stats => 2,
        Tab::Gacha => 3,
        Tab::Settings => 4,
    };
    let style_for = |idx: usize, base: Color| -> Style {
        if idx == active_idx {
            Style::default()
                .fg(Color::Black)
                .bg(base)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(base)
        }
    };

    let separator = if is_narrow_layout(area.width) { "|" } else { " │ " };
    // narrow 40 列で 5 タブ並ぶので、ガチャの 🔑 絵文字を narrow では落として幅を圧縮。
    let gacha_label = if is_narrow_layout(area.width) { "ガチャ" } else { "ガチャ🔑" };
    let settings_label = if is_narrow_layout(area.width) { "設定" } else { "⚙設定" };
    let bar = TabBar::new(separator)
        .tab("強化", style_for(0, Color::Green), TAB_UPGRADES)
        .tab("魂", style_for(1, Color::Magenta), TAB_SOULS)
        .tab("統計", style_for(2, Color::Cyan), TAB_STATS)
        .tab(gacha_label, style_for(3, Color::LightCyan), TAB_GACHA)
        .tab(settings_label, style_for(4, Color::White), TAB_SETTINGS);

    let mut cs = click_state.borrow_mut();
    bar.render(f, area, &mut cs);
}

// ── スクロール共通ヘルパー ─────────────────────────────────

/// `body 内側の inner` を「コンテンツ領域」と「右端 1 列の ▲▼ スクロール列」に分割する。
/// inner.width が 1 以下なら scroll 列を作らず content_area = inner を返す
/// (極小幅でも壊れないように)。
fn split_for_scroll(inner: Rect) -> (Rect, Option<Rect>) {
    if inner.width <= 1 {
        return (inner, None);
    }
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(inner);
    (chunks[0], Some(chunks[1]))
}

/// `state.tab_scroll` を `[0, max_scroll]` に clamp して書き戻し、適用後の値を返す。
/// `max_scroll = content_h - content_area_h` (saturating)。
/// Cell 経由で Game::render の `&self` シグネチャを変えずに実現する。
fn clamp_tab_scroll(state: &AbyssState, content_h: u16, content_area_h: u16) -> u16 {
    let max_scroll = content_h.saturating_sub(content_area_h);
    let s = state.tab_scroll.get().min(max_scroll);
    state.tab_scroll.set(s);
    s
}

/// 右端のスクロール列に ▲▼ ボタンを描画する。
/// scroll 列の上半分が ▲ tap area、下半分が ▼ tap area。
/// ボタン自体は端に 1 セル文字で表示するが、Clickable の tap target は領域全体。
/// (タッチ操作時のヒット領域を稼ぐため。1 セルだけだとモバイルでは tap 困難)
fn render_scroll_indicators(
    f: &mut Frame,
    area: Rect,
    scroll: u16,
    max_scroll: u16,
    cs: &mut ClickState,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let half = area.height / 2;
    let style = Style::default()
        .fg(Color::LightCyan)
        .add_modifier(Modifier::BOLD);

    // 上半分: ▲ (scroll > 0 のときのみ表示・反応)
    if half > 0 && scroll > 0 {
        let up_rect = Rect::new(area.x, area.y, area.width, half);
        let para = Paragraph::new(Line::from(Span::styled("▲", style)));
        Clickable::new(para, SCROLL_UP).render(f, up_rect, cs);
    }
    // 下半分: ▼ (scroll < max_scroll のときのみ表示・反応)。
    // 下端に表示するため、上にスペーサ行を入れる。
    if scroll < max_scroll && area.height > half {
        let down_h = area.height - half;
        let down_rect = Rect::new(area.x, area.y + half, area.width, down_h);
        let mut lines: Vec<Line> = (0..down_h.saturating_sub(1))
            .map(|_| Line::from(""))
            .collect();
        lines.push(Line::from(Span::styled("▼", style)));
        let para = Paragraph::new(lines);
        Clickable::new(para, SCROLL_DOWN).render(f, down_rect, cs);
    }
}

// ── スクロール対応タブの抽象 ──────────────────────────────

/// 「スクロール可能なタブ本体に入る描画コンテンツ」を表す trait。
///
/// 各タブ (強化/魂/設定/統計) は同じ「block + content + ▲▼ 列」レイアウトを
/// 共有しているが、コンテンツの実体だけが `ClickableList` だったり `Paragraph`
/// だったりする。この trait を介して `render_scrollable_tab` 1 関数で扱う。
///
/// **記述と実行の分離**: impl 側はコンテンツの「データ」(行数の見積もりと
/// 描画手段) を提供するだけで、block 描画やスクロール列の管理は呼び出し側
/// (`render_scrollable_tab`) が effect として実行する。
trait ScrollableContent<'a> {
    /// 指定 `content_width` で描画した時の visual 行数。max_scroll 計算に使う。
    fn content_height(&self, content_width: u16) -> u16;

    /// `content_area` 内に描画する。`scroll` 分の縦オフセットは impl 側で適用。
    /// `cs` はクリックターゲット登録が必要な実装 (ClickableList 等) のためだけに渡す。
    fn render(self, f: &mut Frame, content_area: Rect, scroll: u16, cs: &mut ClickState);
}

/// `ClickableList` を `wrap` 設定とセットで扱うアダプタ。
struct WrappingClickableList<'a> {
    list: ClickableList<'a>,
    wrap: bool,
}

impl<'a> ScrollableContent<'a> for WrappingClickableList<'a> {
    fn content_height(&self, content_width: u16) -> u16 {
        if self.wrap {
            self.list.visual_height(content_width)
        } else {
            // wrap=false なら論理 1 行 = visual 1 行。
            self.list.lines().len().min(u16::MAX as usize) as u16
        }
    }
    fn render(self, f: &mut Frame, content_area: Rect, scroll: u16, cs: &mut ClickState) {
        // block は呼び出し側 (render_scrollable_tab) が既に描いているので、
        // ここでは Block::default() (無 borders) を渡して content_area に直接描画。
        self.list
            .render(f, content_area, Block::default(), cs, self.wrap, scroll);
    }
}

impl<'a> ScrollableContent<'a> for Vec<Line<'a>> {
    fn content_height(&self, _content_width: u16) -> u16 {
        // 統計タブは wrap しない固定幅前提。論理 = visual。
        self.len().min(u16::MAX as usize) as u16
    }
    fn render(self, f: &mut Frame, content_area: Rect, scroll: u16, _cs: &mut ClickState) {
        f.render_widget(Paragraph::new(self).scroll((scroll, 0)), content_area);
    }
}

/// スクロール対応タブの汎用 render エントリ。
///
/// 1. `border_color` の枠を `area` 全体に描く
/// 2. 内側を「コンテンツ + ▲▼ 列」に分割
/// 3. `content.content_height` から max_scroll を算出して `state.tab_scroll` を clamp
/// 4. コンテンツを content_area に描画 (clamp 後の scroll を渡す)
/// 5. 必要に応じて ▲▼ を描画
///
/// 各タブの呼び出し箇所は「コンテンツを組み立てて渡す」だけになる。
fn render_scrollable_tab<'a, C: ScrollableContent<'a>>(
    state: &AbyssState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
    border_color: Color,
    content: C,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));
    let (content_area, scroll_col) = split_for_scroll(block.inner(area));

    let content_h = content.content_height(content_area.width);
    let scroll = clamp_tab_scroll(state, content_h, content_area.height);
    let max_scroll = content_h.saturating_sub(content_area.height);

    f.render_widget(block, area);

    let mut cs = click_state.borrow_mut();
    content.render(f, content_area, scroll, &mut cs);
    if let Some(sc) = scroll_col {
        render_scroll_indicators(f, sc, scroll, max_scroll, &mut cs);
    }
}

// ── 強化タブ ───────────────────────────────────────────────

fn render_upgrades(
    state: &AbyssState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let narrow = is_narrow_layout(area.width);
    let mut cl = ClickableList::new();

    // narrow ではヘッダ装飾を最小化し、リストに 1 行余分に渡す。
    // 7 アイテム + ヘッダ 1 + 空行 = 9 行は 40x30 のタブ inner に入りきらないため、
    // 副題と空行を畳んでギリギリ全アイテム visible を確保する。
    if narrow {
        cl.push(Line::from(Span::styled(
            " 強化",
            Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
        )));
    } else {
        cl.push(Line::from(vec![
            Span::styled(
                " 強化",
                Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                " — gold で永続強化を購入",
                Style::default().fg(Color::DarkGray),
            ),
        ]));
        cl.push(Line::from(""));
    }

    for kind in UpgradeKind::all() {
        let lv = state.upgrades[kind.index()];
        let cost = state.upgrade_cost(*kind);
        let affordable = state.gold >= cost;

        let cost_str = format!("{}g", format_num(cost));
        let cost_style = if affordable {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let label_color = if affordable { Color::White } else { Color::DarkGray };
        let label = format!(" {} ", kind.name());
        // 次に 1 Lv 上げた時の実増分。curve 持ち強化は段階によって変わるので
        // `cumulative(lv+1) - cumulative(lv)` で次購入の実効値を出す。
        // 非 curve (Crit/Regen/Gold) は固定文字列にフォールバック。
        let effect = match state.upgrade_curve(*kind) {
            Some(curve) => {
                let lv = state.upgrades[kind.index()];
                let delta = curve.cumulative(lv + 1) - curve.cumulative(lv);
                match kind {
                    UpgradeKind::Sword => format!("ATK+{}", delta.round() as u64),
                    UpgradeKind::Vitality => format!("HP+{}", delta.round() as u64),
                    UpgradeKind::Armor => format!("DEF+{}", delta.round() as u64),
                    UpgradeKind::Speed => format!("速度+{}%", (delta * 100.0).round() as u64),
                    _ => kind.effect().to_string(),
                }
            }
            None => kind.effect().to_string(),
        };
        let lv_str = format!(" Lv.{}", lv);

        // 段階バッジ (curve 持ちのみ)
        let tier_badge = state
            .upgrade_tier(*kind)
            .map(|(name, _)| format!("[{}] ", name))
            .unwrap_or_default();

        cl.push_clickable(
            Line::from(vec![
                Span::styled(label, Style::default().fg(label_color).add_modifier(Modifier::BOLD)),
                Span::styled(tier_badge, Style::default().fg(Color::Yellow)),
                Span::styled(format!("{:<10}", effect), Style::default().fg(Color::Cyan)),
                Span::styled(format!("{:>10}", cost_str), cost_style),
                Span::styled(lv_str, Style::default().fg(Color::Magenta)),
            ]),
            BUY_UPGRADE_BASE + kind.index() as u16,
        );

        // 次段階プレビューは情報密度が高い行 (1〜2 visual row) なので、
        // narrow では完全に畳む。7 アイテム × 最大 2 行 = 14 行を一気に節約し、
        // 「下が見えなくなる」問題の最大要因を解消する。wide では従来通り表示。
        if !narrow {
            if let Some(curve) = state.upgrade_curve(*kind) {
                let lv_u = state.upgrades[kind.index()];
                let (cur_idx, _) = curve.tier_at(lv_u);
                let next = curve.tier(cur_idx + 1);
                let after_next = curve.tier(cur_idx + 2);
                if let Some((next_lv, _, next_name)) = next {
                    // start_level は「超えると次段階」境界なので、新段階が
                    // 実際に効く最初の Lv は start_level + 1。表示はこちらを使う。
                    let mut spans = vec![
                        Span::styled("        次: ", Style::default().fg(Color::DarkGray)),
                        Span::styled(
                            format!("Lv{} [{}]", next_lv + 1, next_name),
                            Style::default().fg(Color::Green),
                        ),
                    ];
                    if let Some((after_lv, _, _)) = after_next {
                        // 次のさらに先は silhouette (?) で見せる
                        spans.push(Span::styled(
                            format!("    その先: Lv{} [???]", after_lv + 1),
                            Style::default().fg(Color::DarkGray),
                        ));
                    }
                    cl.push(Line::from(spans));
                } else if cur_idx + 1 == curve.len() {
                    // 最終段階に到達済み
                    cl.push(Line::from(vec![Span::styled(
                        "        ▼ 最終段階到達",
                        Style::default().fg(Color::Magenta),
                    )]));
                }
            }
        }
    }

    render_scrollable_tab(
        state,
        f,
        area,
        click_state,
        Color::Green,
        WrappingClickableList { list: cl, wrap: narrow },
    );
}

// ── 魂タブ ─────────────────────────────────────────────────

fn render_souls(
    state: &AbyssState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let narrow = is_narrow_layout(area.width);
    let mut cl = ClickableList::new();

    cl.push(Line::from(vec![
        Span::styled(" 魂の強化", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
        Span::styled(
            " — 死亡しても残る永続バフ",
            Style::default().fg(Color::DarkGray),
        ),
    ]));
    cl.push(Line::from(Span::styled(
        format!(" 所持: ✦{}", format_num(state.souls)),
        Style::default().fg(Color::Magenta),
    )));
    cl.push(Line::from(""));

    for perk in SoulPerk::all().iter() {
        let lv = state.soul_perks[perk.index()];
        let cost = state.soul_perk_cost(*perk);
        let affordable = state.souls >= cost;

        let cost_style = if affordable {
            Style::default().fg(Color::Magenta)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let label_color = if affordable { Color::White } else { Color::DarkGray };

        let label = format!(" {} ", perk.name());
        let effect = perk.effect().to_string();
        let lv_str = format!(" Lv.{}", lv);
        let cost_str = format!("{}✦", format_num(cost));

        cl.push_clickable(
            Line::from(vec![
                Span::styled(label, Style::default().fg(label_color).add_modifier(Modifier::BOLD)),
                Span::styled(format!("{:<14}", effect), Style::default().fg(Color::Cyan)),
                Span::styled(format!("{:>10}", cost_str), cost_style),
                Span::styled(lv_str, Style::default().fg(Color::LightMagenta)),
            ]),
            BUY_SOUL_PERK_BASE + perk.index() as u16,
        );
    }

    render_scrollable_tab(
        state,
        f,
        area,
        click_state,
        Color::Magenta,
        WrappingClickableList { list: cl, wrap: narrow },
    );
}

// ── 設定タブ ───────────────────────────────────────────────

/// 設定タブ。「セッションを通して頻繁に切り替えない項目」を集約する場所。
/// 現状は自動潜行のみ。将来的に難易度・サウンド・倍速プレイ等を入れる想定。
fn render_settings(
    state: &AbyssState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let narrow = is_narrow_layout(area.width);
    let mut cl = ClickableList::new();

    // ヘッダ。narrow では副題を畳む (強化タブと同じポリシー)。
    if narrow {
        cl.push(Line::from(Span::styled(
            " 設定",
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
        )));
    } else {
        cl.push(Line::from(vec![
            Span::styled(
                " 設定",
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                " — プレイ全体に効くオプション",
                Style::default().fg(Color::DarkGray),
            ),
        ]));
        cl.push(Line::from(""));
    }

    // ── 自動潜行トグル ──
    // 行タップで TOGGLE_AUTO_DESCEND を発火 (action ID は toggle bar 時代から再利用)。
    let (state_label, state_color) = if state.auto_descend {
        ("ON ▼", Color::Green)
    } else {
        ("OFF ■", Color::Yellow)
    };
    cl.push_clickable(
        Line::from(vec![
            Span::styled(
                " 自動潜行  ",
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                state_label,
                Style::default().fg(state_color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(" (タップで切替)", Style::default().fg(Color::DarkGray)),
        ]),
        TOGGLE_AUTO_DESCEND,
    );
    if !narrow {
        cl.push(Line::from(Span::styled(
            "   ON: 雑魚を倒したら次フロアへ自動降下 / OFF: 現フロア周回",
            Style::default().fg(Color::DarkGray),
        )));
    }

    render_scrollable_tab(
        state,
        f,
        area,
        click_state,
        Color::White,
        WrappingClickableList { list: cl, wrap: narrow },
    );
}

// ── 統計タブ ───────────────────────────────────────────────

fn render_stats(
    state: &AbyssState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(Span::styled(
        " 統計",
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));
    lines.push(stat_line("最深到達", format!("B{}F", state.deepest_floor_ever)));
    lines.push(stat_line("現フロア", format!("B{}F", state.floor)));
    lines.push(stat_line("総撃破数", format_num(state.total_kills)));
    lines.push(stat_line("死亡回数", format_num(state.deaths)));
    lines.push(Line::from(""));
    lines.push(stat_line("ATK", format!("{}", state.hero_atk())));
    lines.push(stat_line("DEF", format!("{}", state.hero_def())));
    lines.push(stat_line("最大HP", format!("{}", state.hero_max_hp())));
    lines.push(stat_line(
        "クリ率",
        format!("{}%", (state.hero_crit_rate() * 100.0).round() as u32),
    ));
    lines.push(stat_line(
        "攻撃間隔",
        format!("{:.1}秒/回", state.hero_atk_period() as f32 / 10.0),
    ));
    lines.push(stat_line(
        "戦闘集中",
        format!(
            "{}/{} (-{}%)",
            state.combat_focus,
            state.config.hero.focus_max,
            ((1.0 - state.focus_factor()) * 100.0).round() as u32
        ),
    ));
    lines.push(stat_line(
        "HP回復",
        format!("{:.1}/秒", state.hero_regen_per_sec()),
    ));
    lines.push(stat_line(
        "ゴールド倍率",
        format!("×{:.2}", state.gold_multiplier()),
    ));

    // ScrollableContent の Vec<Line> impl 経由で render_scrollable_tab に委譲。
    // 強化/魂/設定タブと完全に同じパターンで描画される。
    render_scrollable_tab(state, f, area, click_state, Color::Cyan, lines);
}

fn stat_line(label: &'static str, value: String) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!(" {:<14}", label), Style::default().fg(Color::DarkGray)),
        Span::styled(value, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
    ])
}

// ── ガチャタブ ─────────────────────────────────────────────

fn render_gacha(
    state: &AbyssState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let narrow = is_narrow_layout(area.width);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::LightCyan));
    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.height < 3 || inner.width < 20 {
        return;
    }

    // narrow 時はボタンを縦積みにするので 2 倍の高さが必要 (3 → 6)。
    let buttons_height: u16 = if narrow { 6 } else { 3 };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),               // ヘッダ
            Constraint::Length(buttons_height),  // ボタン
            Constraint::Length(4),               // 直近結果
            Constraint::Min(3),                  // 確率テーブル
        ])
        .split(inner);

    render_gacha_header(state, f, chunks[0]);
    render_gacha_buttons(state, f, chunks[1], click_state, narrow);
    render_gacha_last_result(state, f, chunks[2]);
    render_gacha_table(state, f, chunks[3], narrow);
}

fn render_gacha_header(state: &AbyssState, f: &mut Frame, area: Rect) {
    let g = &state.config.gacha;
    let pity = g.gacha_pity;
    let until_pity = pity.saturating_sub(state.pulls_since_epic);
    let pity_str = if pity == 0 {
        "天井無効".to_string()
    } else if until_pity == 0 {
        "天井: 次回 Epic+ 確定！".to_string()
    } else {
        format!("天井: あと {} 連で Epic+ 確定", until_pity)
    };

    let lines = vec![
        Line::from(vec![
            Span::styled(
                " 🎲 深淵ガチャ",
                Style::default()
                    .fg(Color::LightCyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  所持: 🔑{}", format_num(state.keys)),
                Style::default()
                    .fg(Color::LightCyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  累計: {}回", format_num(state.total_pulls)),
                Style::default().fg(Color::DarkGray),
            ),
        ]),
        Line::from(Span::styled(
            format!(" {}", pity_str),
            Style::default().fg(Color::Yellow),
        )),
    ];
    f.render_widget(Paragraph::new(lines), area);
}

fn render_gacha_buttons(
    state: &AbyssState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
    narrow: bool,
) {
    // narrow: 縦並び (上 1 連, 下 10 連)。wide: 横並び 50/50。
    let halves = if narrow {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(area)
    } else {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(area)
    };

    let can_one = state.keys >= 1;
    let can_ten = state.keys >= 10;

    let one_color = if can_one { Color::LightCyan } else { Color::DarkGray };
    let ten_color = if can_ten { Color::LightYellow } else { Color::DarkGray };

    let one_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(one_color));
    let one_para = Paragraph::new(Line::from(Span::styled(
        " 1連 (🔑1) ",
        Style::default().fg(one_color).add_modifier(Modifier::BOLD),
    )))
    .alignment(Alignment::Center)
    .block(one_block);

    let ten_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ten_color));
    let ten_para = Paragraph::new(Line::from(Span::styled(
        " 10連 (🔑10) ",
        Style::default().fg(ten_color).add_modifier(Modifier::BOLD),
    )))
    .alignment(Alignment::Center)
    .block(ten_block);

    let mut cs = click_state.borrow_mut();
    Clickable::new(one_para, GACHA_PULL_1).render(f, halves[0], &mut cs);
    Clickable::new(ten_para, GACHA_PULL_10).render(f, halves[1], &mut cs);
}

fn render_gacha_last_result(state: &AbyssState, f: &mut Frame, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(" 直近結果 ");
    let mut lines: Vec<Line> = Vec::new();

    if let Some(r) = &state.last_gacha {
        // 1 行目: 等級分布
        lines.push(Line::from(vec![
            Span::styled(
                format!(" x{} ", r.count),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("C:{} ", r.by_tier[0]),
                Style::default().fg(Color::Gray),
            ),
            Span::styled(
                format!("R:{} ", r.by_tier[1]),
                Style::default().fg(Color::Cyan),
            ),
            Span::styled(
                format!("E:{} ", r.by_tier[2]),
                Style::default()
                    .fg(Color::Magenta)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("L:{} ", r.by_tier[3]),
                Style::default()
                    .fg(Color::LightYellow)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
        // 2 行目: 報酬合計
        let mut reward_spans: Vec<Span> = vec![Span::raw(" ")];
        if r.gained_gold > 0 {
            reward_spans.push(Span::styled(
                format!("💰+{} ", format_num(r.gained_gold)),
                Style::default().fg(Color::Yellow),
            ));
        }
        if r.gained_souls > 0 {
            reward_spans.push(Span::styled(
                format!("✦+{} ", format_num(r.gained_souls)),
                Style::default().fg(Color::Magenta),
            ));
        }
        if r.gained_keys > 0 {
            reward_spans.push(Span::styled(
                format!("🔑+{} ", r.gained_keys),
                Style::default().fg(Color::LightCyan),
            ));
        }
        if r.gained_upgrade_lv > 0 {
            reward_spans.push(Span::styled(
                format!("◆Lv+{} ", r.gained_upgrade_lv),
                Style::default().fg(Color::Green),
            ));
        }
        if reward_spans.len() == 1 {
            reward_spans.push(Span::styled("—", Style::default().fg(Color::DarkGray)));
        }
        lines.push(Line::from(reward_spans));
    } else {
        lines.push(Line::from(Span::styled(
            " (まだ引いていない)",
            Style::default().fg(Color::DarkGray),
        )));
    }

    f.render_widget(Paragraph::new(lines).block(block), area);
}

fn render_gacha_table(state: &AbyssState, f: &mut Frame, area: Rect, narrow: bool) {
    let g = &state.config.gacha;
    let total: u32 = g.gacha_weights_milli.iter().sum::<u32>().max(1);
    let pct = |w: u32| -> String { format!("{:.1}%", (w as f64 / total as f64) * 100.0) };

    // (label, label_color, weight_idx, weight_bold, reward_text)
    let rows: [(&'static str, Color, usize, bool, String); 4] = [
        ("Common   ", Color::Gray, 0, false, "💰 大量 gold".to_string()),
        ("Rare     ", Color::Cyan, 1, false, "◆ 強化レベル+1 (永続)".to_string()),
        ("Epic     ", Color::Magenta, 2, true, "✦ 魂 (現フロア依存)".to_string()),
        (
            "Legendary",
            Color::LightYellow,
            3,
            true,
            format!("🔑+{} (連鎖チャンス)", g.legendary_keys),
        ),
    ];

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(Span::styled(
        " 確率テーブル",
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    )));

    for (label, color, idx, bold, reward) in rows.iter() {
        let mut label_style = Style::default().fg(*color);
        let mut weight_style = Style::default().fg(*color);
        if *bold {
            label_style = label_style.add_modifier(Modifier::BOLD);
            weight_style = weight_style.add_modifier(Modifier::BOLD);
        }
        let pct_str = pct(g.gacha_weights_milli[*idx]);

        if narrow {
            // 1 行目: 等級 + 確率。2 行目: 報酬説明 (インデント)。
            lines.push(Line::from(vec![
                Span::styled(format!("  {} ", label), label_style),
                Span::styled(pct_str, weight_style),
            ]));
            lines.push(Line::from(Span::styled(
                format!("    → {}", reward),
                Style::default().fg(Color::DarkGray),
            )));
        } else {
            lines.push(Line::from(vec![
                Span::styled(format!("  {} ", label), label_style),
                Span::styled(pct_str, weight_style),
                Span::styled(format!("  → {}", reward), Style::default().fg(Color::DarkGray)),
            ]));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        " 鍵入手: ボス +1 (Elite +2 / 10F毎 +2)",
        Style::default().fg(Color::DarkGray),
    )));

    // narrow 時は行数が増えるので wrap=true で長文を折り返す保険を入れる。
    let para = Paragraph::new(lines);
    let para = if narrow {
        para.wrap(ratzilla::ratatui::widgets::Wrap { trim: false })
    } else {
        para
    };
    f.render_widget(para, area);
}

// ── ログ ───────────────────────────────────────────────────

fn render_log(state: &AbyssState, f: &mut Frame, area: Rect) {
    let inner_h = area.height.saturating_sub(2);
    let take = inner_h.max(1) as usize;
    let start = state.log.len().saturating_sub(take);
    let lines: Vec<Line> = state.log[start..]
        .iter()
        .map(|m| Line::from(Span::styled(format!(" {m}"), log_style(m))))
        .collect();

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(" ログ ");
    f.render_widget(Paragraph::new(lines).block(block), area);
}

fn log_style(msg: &str) -> Style {
    if msg.starts_with("✝") {
        Style::default().fg(Color::Red)
    } else if msg.starts_with("▼") || msg.contains("ボス") {
        Style::default().fg(Color::Yellow)
    } else if msg.starts_with("◆") {
        Style::default().fg(Color::Green)
    } else if msg.starts_with("✦") {
        Style::default().fg(Color::Magenta)
    } else {
        Style::default().fg(Color::DarkGray)
    }
}

// ── 数字フォーマット ───────────────────────────────────────

fn format_num(n: u64) -> String {
    if n < 10_000 {
        n.to_string()
    } else if n < 1_000_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else if n < 1_000_000_000 {
        format!("{:.2}M", n as f64 / 1_000_000.0)
    } else if n < 1_000_000_000_000 {
        format!("{:.2}B", n as f64 / 1_000_000_000.0)
    } else {
        format!("{:.2}T", n as f64 / 1_000_000_000_000.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::ClickState;
    use ratzilla::ratatui::backend::TestBackend;
    use ratzilla::ratatui::Terminal;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// ガチャタブを描画したとき、1連 / 10連ボタンが実描画位置に対して
    /// クリックターゲット登録されていることを TestBackend で確認。
    /// (CLAUDE.md: Widget Primitive 規約のチェックリスト)
    #[test]
    fn gacha_buttons_register_clickable_areas() {
        let mut state = AbyssState::new();
        state.tab = Tab::Gacha;
        state.keys = 100;
        let cs = Rc::new(RefCell::new(ClickState::new()));
        let mut terminal = Terminal::new(TestBackend::new(80, 30)).unwrap();
        terminal
            .draw(|f| {
                render(&state, f, f.area(), &cs);
            })
            .unwrap();

        // ボタン領域のいずれかのセルでヒットテストすると 1連/10連 ID が返る。
        // 描画位置を厳密に固定せず、どこかで両方検出されることを確認する。
        let cs = cs.borrow();
        let mut found_one = false;
        let mut found_ten = false;
        for y in 0..30 {
            for x in 0..80 {
                match cs.hit_test(x, y) {
                    Some(GACHA_PULL_1) => found_one = true,
                    Some(GACHA_PULL_10) => found_ten = true,
                    _ => {}
                }
            }
        }
        assert!(found_one, "GACHA_PULL_1 button not registered");
        assert!(found_ten, "GACHA_PULL_10 button not registered");
    }

    /// 5 タブ全てがクリック可能領域として登録されていることを確認。
    #[test]
    fn all_tabs_registered() {
        let state = AbyssState::new();
        let cs = Rc::new(RefCell::new(ClickState::new()));
        let mut terminal = Terminal::new(TestBackend::new(80, 30)).unwrap();
        terminal
            .draw(|f| render(&state, f, f.area(), &cs))
            .unwrap();
        let cs = cs.borrow();
        let mut found = [false; 5];
        for y in 0..30 {
            for x in 0..80 {
                match cs.hit_test(x, y) {
                    Some(TAB_UPGRADES) => found[0] = true,
                    Some(TAB_SOULS) => found[1] = true,
                    Some(TAB_STATS) => found[2] = true,
                    Some(TAB_GACHA) => found[3] = true,
                    Some(TAB_SETTINGS) => found[4] = true,
                    _ => {}
                }
            }
        }
        assert!(found.iter().all(|&b| b), "missing tab targets: {:?}", found);
    }

    /// narrow (40 列) でも全タブを描画してパニックしないこと、
    /// かつタブ・主要ボタンが登録されることをスモーク確認。
    /// レイアウト変更で行数が膨らんで Constraint が負になる回帰を防ぐ。
    #[test]
    fn narrow_layout_renders_all_tabs() {
        let mut state = AbyssState::new();
        state.keys = 100;
        let tabs = [
            Tab::Upgrades,
            Tab::Souls,
            Tab::Stats,
            Tab::Gacha,
            Tab::Settings,
        ];
        for &tab in &tabs {
            state.tab = tab;
            let cs = Rc::new(RefCell::new(ClickState::new()));
            let mut terminal = Terminal::new(TestBackend::new(40, 30)).unwrap();
            terminal
                .draw(|f| render(&state, f, f.area(), &cs))
                .unwrap();
            // narrow でも全タブターゲットが取れる (タブバーの幅切り詰めの回帰検知)
            let cs = cs.borrow();
            let mut found = [false; 5];
            for y in 0..30 {
                for x in 0..40 {
                    match cs.hit_test(x, y) {
                        Some(TAB_UPGRADES) => found[0] = true,
                        Some(TAB_SOULS) => found[1] = true,
                        Some(TAB_STATS) => found[2] = true,
                        Some(TAB_GACHA) => found[3] = true,
                        Some(TAB_SETTINGS) => found[4] = true,
                        _ => {}
                    }
                }
            }
            assert!(
                found.iter().all(|&b| b),
                "narrow tab {:?}: missing targets {:?}",
                tab,
                found
            );
        }
    }

    /// narrow (40 列) で 7 つの強化アイテム全てがクリック可能領域として
    /// 登録されることを確認 — 縦が足りずに後半が切れて「下が見えない」回帰を防ぐ。
    ///
    /// 40x30 はモバイル縦の中でもタイトな部類 (キーボード表示時等)。
    /// この高さで 7 アイテム全部 visible にできるかが narrow 設計の肝。
    /// 高さを 47 等の余裕ある値にすると、修正前のレイアウトでも偶然通って
    /// 回帰検出に失敗するため、わざと厳しめの高さで検証する。
    #[test]
    fn narrow_upgrades_all_visible() {
        let mut state = AbyssState::new();
        state.tab = Tab::Upgrades;
        let cs = Rc::new(RefCell::new(ClickState::new()));
        let mut terminal = Terminal::new(TestBackend::new(40, 30)).unwrap();
        terminal
            .draw(|f| render(&state, f, f.area(), &cs))
            .unwrap();
        let cs = cs.borrow();
        let mut found = [false; 7];
        for y in 0..30 {
            for x in 0..40 {
                if let Some(id) = cs.hit_test(x, y) {
                    if (BUY_UPGRADE_BASE..BUY_UPGRADE_BASE + 7).contains(&id) {
                        found[(id - BUY_UPGRADE_BASE) as usize] = true;
                    }
                }
            }
        }
        assert!(
            found.iter().all(|&b| b),
            "narrow upgrades not all visible at 40x30: {:?}",
            found
        );
    }

    /// タブ切替で `tab_scroll` が 0 にリセットされる。
    /// (logic::set_tab で `tab_scroll.set(0)` してる挙動の回帰検知)
    #[test]
    fn tab_switch_resets_scroll() {
        use crate::games::abyss::logic;
        use crate::games::abyss::policy::PlayerAction;

        let mut state = AbyssState::new();
        state.tab_scroll.set(99);
        logic::apply_action(&mut state, PlayerAction::SetTab(Tab::Souls));
        assert_eq!(state.tab_scroll.get(), 0);
    }

    /// 極小縦サイズ (40x20) で強化タブを開くと初期状態では下が切れるが、
    /// スクロールすれば最後の Gold (idx=6) のクリックターゲットに到達できる。
    /// scroll の round-trip (action 適用 → render → 反映) を網羅的に検証する。
    #[test]
    fn narrow_upgrades_scroll_to_bottom() {
        use crate::games::abyss::logic;
        use crate::games::abyss::policy::PlayerAction;

        let mut state = AbyssState::new();
        state.tab = Tab::Upgrades;
        let cs = Rc::new(RefCell::new(ClickState::new()));
        let mut terminal = Terminal::new(TestBackend::new(40, 20)).unwrap();

        // 初期 frame で render し、tab_scroll を clamp させる
        // (visual_height ベースで max_scroll が決定する)
        terminal
            .draw(|f| render(&state, f, f.area(), &cs))
            .unwrap();

        // 何度か ScrollDown を流し続けて、その都度 render すれば必ず Gold が
        // 見える位置に来る (clamp で max_scroll を超えない)。20 回以上で十分
        // (7 アイテム × 平均 2 visual = 14 行に対し step 3、即ち 5 回程度で底)。
        let mut found_gold = false;
        for _ in 0..20 {
            logic::apply_action(&mut state, PlayerAction::ScrollDown);
            cs.borrow_mut().clear_targets();
            terminal
                .draw(|f| render(&state, f, f.area(), &cs))
                .unwrap();
            let cs_ref = cs.borrow();
            for y in 0..20 {
                for x in 0..40 {
                    if cs_ref.hit_test(x, y) == Some(BUY_UPGRADE_BASE + 6) {
                        found_gold = true;
                    }
                }
            }
            if found_gold {
                break;
            }
        }
        assert!(found_gold, "Gold (idx=6) never reachable via ScrollDown at 40x20");
    }

    /// 設定タブを開いた時、自動潜行トグルがクリック可能領域として登録されること。
    /// (自動潜行を toggle bar から settings タブに移動した回帰検知用)
    #[test]
    fn settings_tab_registers_auto_descend_toggle() {
        let mut state = AbyssState::new();
        state.tab = Tab::Settings;
        let cs = Rc::new(RefCell::new(ClickState::new()));
        let mut terminal = Terminal::new(TestBackend::new(80, 30)).unwrap();
        terminal
            .draw(|f| render(&state, f, f.area(), &cs))
            .unwrap();
        let cs = cs.borrow();
        let mut found = false;
        for y in 0..30 {
            for x in 0..80 {
                if matches!(cs.hit_test(x, y), Some(TOGGLE_AUTO_DESCEND)) {
                    found = true;
                }
            }
        }
        assert!(found, "TOGGLE_AUTO_DESCEND not clickable in settings tab");
    }

    /// narrow (40 列) ガチャタブで、縦積みになったボタンが両方登録されること。
    /// ガチャは header + button + result + table と要素が多いので height は
    /// 余裕を持たせる (実機モバイルでも縦は十分確保できる前提)。
    #[test]
    fn narrow_gacha_buttons_registered() {
        let mut state = AbyssState::new();
        state.tab = Tab::Gacha;
        state.keys = 100;
        let cs = Rc::new(RefCell::new(ClickState::new()));
        let mut terminal = Terminal::new(TestBackend::new(40, 50)).unwrap();
        terminal
            .draw(|f| render(&state, f, f.area(), &cs))
            .unwrap();
        let cs = cs.borrow();
        let mut found_one = false;
        let mut found_ten = false;
        for y in 0..50 {
            for x in 0..40 {
                match cs.hit_test(x, y) {
                    Some(GACHA_PULL_1) => found_one = true,
                    Some(GACHA_PULL_10) => found_ten = true,
                    _ => {}
                }
            }
        }
        assert!(found_one && found_ten, "narrow gacha buttons missing");
    }

    #[test]
    fn format_num_basic() {
        assert_eq!(format_num(0), "0");
        assert_eq!(format_num(999), "999");
        assert_eq!(format_num(9_999), "9999");
        assert_eq!(format_num(10_000), "10.0K");
        assert_eq!(format_num(1_500_000), "1.50M");
    }

    #[test]
    fn atk_progress_bounds() {
        assert!((atk_progress(10, 10) - 0.0).abs() < 1e-6);
        assert!((atk_progress(10, 0) - 1.0).abs() < 1e-6);
        assert!((atk_progress(10, 5) - 0.5).abs() < 1e-6);
        assert!((atk_progress(0, 0) - 1.0).abs() < 1e-6);
    }
}
