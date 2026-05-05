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
use super::logic;
use super::state::{
    AbyssState, EquipmentId, EquipmentLane, FloorKind, SoulPerk, Tab, TabGroup,
};

/// 主要パネルの Rect。
pub struct AbyssLayout {
    pub header: Rect,
    pub combat: Rect,
    pub hero_panel: Rect,
    pub enemy_panel: Rect,
    pub toggle: Rect,
    pub tab_bar: Rect,
    pub body: Rect,
}

pub fn compute_layout(area: Rect) -> AbyssLayout {
    let narrow = is_narrow_layout(area.width);
    let combat_height: u16 = if narrow { 7 } else { 9 };
    let header_height: u16 = if narrow { 4 } else { 3 };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(header_height),
            Constraint::Length(combat_height),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(8),
        ])
        .split(area);

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

    let group = TabGroup::from_tab(state.tab);
    let log_h: u16 = if narrow { 4 } else { 5 };
    let constraints: Vec<Constraint> = if group.has_subtabs() {
        vec![
            Constraint::Length(1),
            Constraint::Min(5),
            Constraint::Length(log_h),
        ]
    } else {
        vec![Constraint::Min(5), Constraint::Length(log_h)]
    };
    let body_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(l.body);
    let (content_idx, log_idx) = if group.has_subtabs() {
        render_subtab_bar(state, f, body_chunks[0], click_state);
        (1, 2)
    } else {
        (0, 1)
    };

    match state.tab {
        Tab::Upgrades => render_upgrades(state, f, body_chunks[content_idx], click_state),
        Tab::Roadmap => render_roadmap(state, f, body_chunks[content_idx], click_state),
        Tab::Stats => render_stats(state, f, body_chunks[content_idx], click_state),
        Tab::Gacha => render_gacha(state, f, body_chunks[content_idx], click_state),
        Tab::Settings => render_settings(state, f, body_chunks[content_idx], click_state),
        Tab::Shop => render_shop(state, f, body_chunks[content_idx], click_state),
        Tab::Souls => render_souls(state, f, body_chunks[content_idx], click_state),
    }
    render_log(state, f, body_chunks[log_idx]);
}

// ── ヘッダ ─────────────────────────────────────────────────

fn render_header(state: &AbyssState, f: &mut Frame, area: Rect, narrow: bool) {
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
        vec![Line::from(floor_spans), Line::from(currency_spans)]
    } else {
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

    let progress = atk_progress(state.hero_atk_period(), state.hero_atk_cooldown);
    lines.push(Line::from(make_progress_line(
        " ATK ",
        progress,
        bar_width,
        Color::Yellow,
    )));

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
    let active_group = TabGroup::from_tab(state.tab);
    let style_for = |group: TabGroup, base: Color| -> Style {
        if group == active_group {
            Style::default()
                .fg(Color::Black)
                .bg(base)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(base)
        }
    };

    let narrow = is_narrow_layout(area.width);
    let separator = if narrow { "|" } else { " │ " };
    let growth_label: &str = if narrow { TabGroup::Growth.name() } else { "🛡育成" };
    let info_label: &str = if narrow { TabGroup::Info.name() } else { "📊情報" };
    let gacha_label: &str = if narrow { TabGroup::Gacha.name() } else { "ガチャ🔑" };
    let settings_label: &str = if narrow { TabGroup::Settings.name() } else { "⚙設定" };
    let bar = TabBar::new(separator)
        .tab(growth_label, style_for(TabGroup::Growth, Color::Green), TAB_GROUP_GROWTH)
        .tab(info_label, style_for(TabGroup::Info, Color::Cyan), TAB_GROUP_INFO)
        .tab(gacha_label, style_for(TabGroup::Gacha, Color::LightCyan), TAB_GROUP_GACHA)
        .tab(settings_label, style_for(TabGroup::Settings, Color::White), TAB_GROUP_SETTINGS);

    let mut cs = click_state.borrow_mut();
    bar.render(f, area, &mut cs);
}

fn render_subtab_bar(
    state: &AbyssState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let group = TabGroup::from_tab(state.tab);
    let active_tab = state.tab;
    let mut bar = TabBar::new(" · ");
    for &tab in group.tabs() {
        let label = subtab_label(tab);
        let click_id = subtab_click_id(tab);
        let base = subtab_color(tab);
        let style = if tab == active_tab {
            Style::default()
                .fg(Color::Black)
                .bg(base)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(base)
        };
        bar = bar.tab(label, style, click_id);
    }
    let mut cs = click_state.borrow_mut();
    bar.render(f, area, &mut cs);
}

fn subtab_label(tab: Tab) -> &'static str {
    match tab {
        Tab::Upgrades => "強化",
        Tab::Shop => "装備",
        Tab::Souls => "魂",
        Tab::Roadmap => "進捗",
        Tab::Stats => "統計",
        Tab::Gacha => "ガチャ",
        Tab::Settings => "設定",
    }
}

fn subtab_click_id(tab: Tab) -> u16 {
    match tab {
        Tab::Upgrades => TAB_UPGRADES,
        Tab::Shop => TAB_SHOP,
        Tab::Souls => TAB_SOULS,
        Tab::Roadmap => TAB_ROADMAP,
        Tab::Stats => TAB_STATS,
        Tab::Gacha => TAB_GACHA,
        Tab::Settings => TAB_SETTINGS,
    }
}

fn subtab_color(tab: Tab) -> Color {
    match tab {
        Tab::Upgrades => Color::Green,
        Tab::Shop => Color::Yellow,
        Tab::Souls => Color::Magenta,
        Tab::Roadmap | Tab::Stats => Color::Cyan,
        Tab::Gacha => Color::LightCyan,
        Tab::Settings => Color::White,
    }
}

// ── スクロール共通ヘルパー ─────────────────────────────────

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

fn clamp_tab_scroll(state: &AbyssState, content_h: u16, content_area_h: u16) -> u16 {
    let max_scroll = content_h.saturating_sub(content_area_h);
    let s = state.tab_scroll.get().min(max_scroll);
    state.tab_scroll.set(s);
    s
}

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

    if half > 0 && scroll > 0 {
        let up_rect = Rect::new(area.x, area.y, area.width, half);
        let para = Paragraph::new(Line::from(Span::styled("▲", style)));
        Clickable::new(para, SCROLL_UP).render(f, up_rect, cs);
    }
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

trait ScrollableContent<'a> {
    fn content_height(&self, content_width: u16) -> u16;
    fn render(self, f: &mut Frame, content_area: Rect, scroll: u16, cs: &mut ClickState);
}

struct WrappingClickableList<'a> {
    list: ClickableList<'a>,
    wrap: bool,
}

impl<'a> ScrollableContent<'a> for WrappingClickableList<'a> {
    fn content_height(&self, content_width: u16) -> u16 {
        if self.wrap {
            self.list.visual_height(content_width)
        } else {
            self.list.lines().len().min(u16::MAX as usize) as u16
        }
    }
    fn render(self, f: &mut Frame, content_area: Rect, scroll: u16, cs: &mut ClickState) {
        self.list
            .render(f, content_area, Block::default(), cs, self.wrap, scroll);
    }
}

impl<'a> ScrollableContent<'a> for Vec<Line<'a>> {
    fn content_height(&self, _content_width: u16) -> u16 {
        self.len().min(u16::MAX as usize) as u16
    }
    fn render(self, f: &mut Frame, content_area: Rect, scroll: u16, _cs: &mut ClickState) {
        f.render_widget(Paragraph::new(self).scroll((scroll, 0)), content_area);
    }
}

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

// ── 強化サブタブ ───────────────────────────────────────────

/// 強化サブタブ — **装着中の装備を gold で強化** する画面。
///
/// 表示:
/// - 各 lane (武器/防具/装飾) ごとに 1 ブロック
/// - 装着中の装備があれば: 名前 + 強化 Lv + 効果ラベル + 次の強化コスト + [強化] ボタン
/// - 未装着なら: 「装備未装着」プレースホルダ + 装備タブ誘導文言
///
/// クリック: 装着中装備の行をタップで `EnhanceEquipment` を発火。
/// gold 不足は灰色表示でも logic 側でブロックされる (押せはする)。
fn render_upgrades(
    state: &AbyssState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let narrow = is_narrow_layout(area.width);
    let mut cl = ClickableList::new();

    if !narrow {
        cl.push(Line::from(vec![
            Span::styled(
                " 装備強化",
                Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                " — 装着中の 3 装備を gold で強化",
                Style::default().fg(Color::DarkGray),
            ),
        ]));
        cl.push(Line::from(""));
    }

    for &lane in EquipmentLane::all() {
        push_lane_enhance_block(state, &mut cl, lane);
        cl.push(Line::from(""));
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

/// 強化タブの 1 lane 分のブロックを cl に push する。
fn push_lane_enhance_block<'a>(
    state: &AbyssState,
    cl: &mut ClickableList<'a>,
    lane: EquipmentLane,
) {
    let lane_color = lane_color(lane);
    let header = format!(" ▣ {}", lane.name());

    match state.equipped_at(lane) {
        Some(id) => {
            let def = match state.config.equipment.get(id.index()) {
                Some(d) => d,
                None => {
                    cl.push(Line::from(Span::styled(
                        header,
                        Style::default().fg(lane_color).add_modifier(Modifier::BOLD),
                    )));
                    return;
                }
            };
            let lv = state.equipment_levels[id.index()];
            let cost = state.enhance_cost(id);
            let affordable = state.gold >= cost;

            // ヘッダ行: lane 名 + 装着中装備の名前 + 強化 Lv バッジ。
            cl.push(Line::from(vec![
                Span::styled(
                    format!(" ▣ {} ", lane.name()),
                    Style::default().fg(lane_color).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    def.name.to_string(),
                    Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                Span::styled(
                    format!("+{}", lv),
                    Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD),
                ),
            ]));

            // 効果プレビュー (ベース + 現 Lv 適用後の値)。
            let effective = super::state::EquipmentBonus::scaled(
                &def.base_bonus,
                &def.per_level_bonus,
                lv,
            );
            cl.push(Line::from(Span::styled(
                format!("   現効果: {}", format_bonus_summary(&effective)),
                Style::default().fg(Color::Cyan),
            )));

            // 強化ボタン行 (この行をクリックで強化発火)。
            let cost_color = if affordable { Color::Yellow } else { Color::DarkGray };
            let button_color = if affordable { Color::Green } else { Color::DarkGray };
            cl.push_clickable(
                Line::from(vec![
                    Span::styled(
                        "   ◆ 強化 ",
                        Style::default()
                            .fg(button_color)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled("→ Lv+1: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        format!("{}g", format_num(cost)),
                        Style::default().fg(cost_color).add_modifier(Modifier::BOLD),
                    ),
                ]),
                ENHANCE_EQUIPMENT_BASE + id.index() as u16,
            );
        }
        None => {
            cl.push(Line::from(Span::styled(
                header,
                Style::default().fg(lane_color).add_modifier(Modifier::BOLD),
            )));
            cl.push(Line::from(Span::styled(
                "   (装備未装着 — 装備タブで購入・装着してください)",
                Style::default().fg(Color::DarkGray),
            )));
        }
    }
}

/// EquipmentBonus を 1 行サマリ (主要な数値だけ)。0 のフィールドは出さない。
fn format_bonus_summary(b: &super::state::EquipmentBonus) -> String {
    let mut parts: Vec<String> = Vec::new();
    if b.atk_pct > 0.0 || b.atk_flat > 0 {
        let mut s = String::from("ATK");
        if b.atk_pct > 0.0 {
            s += &format!(" +{}%", (b.atk_pct * 100.0).round() as u64);
        }
        if b.atk_flat > 0 {
            s += &format!("/+{}", b.atk_flat);
        }
        parts.push(s);
    }
    if b.hp_pct > 0.0 || b.hp_flat > 0 {
        let mut s = String::from("HP");
        if b.hp_pct > 0.0 {
            s += &format!(" +{}%", (b.hp_pct * 100.0).round() as u64);
        }
        if b.hp_flat > 0 {
            s += &format!("/+{}", b.hp_flat);
        }
        parts.push(s);
    }
    if b.def_flat > 0 {
        parts.push(format!("DEF +{}", b.def_flat));
    }
    if b.crit_bonus > 0.0 {
        parts.push(format!("CRIT +{:.1}%", b.crit_bonus * 100.0));
    }
    if b.speed_pct > 0.0 {
        parts.push(format!("速度 +{}%", (b.speed_pct * 100.0).round() as u64));
    }
    if b.regen_per_sec > 0.0 {
        parts.push(format!("回復 +{:.1}/s", b.regen_per_sec));
    }
    if b.gold_pct > 0.0 {
        parts.push(format!("金 +{}%", (b.gold_pct * 100.0).round() as u64));
    }
    if parts.is_empty() {
        "—".to_string()
    } else {
        parts.join(" / ")
    }
}

// ── 魂サブタブ ─────────────────────────────────────────────

fn render_souls(
    state: &AbyssState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let narrow = is_narrow_layout(area.width);
    let mut cl = ClickableList::new();

    cl.push(Line::from(Span::styled(
        format!(" 所持: ✦{}", format_num(state.souls)),
        Style::default()
            .fg(Color::Magenta)
            .add_modifier(Modifier::BOLD),
    )));

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

// ── 進捗タブ ───────────────────────────────────────────────

fn render_roadmap(
    state: &AbyssState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let inner_width = area.width.saturating_sub(7);
    let goal = state.goal_floor().max(1);
    let cur = state.floor.min(goal);
    let deepest = state.deepest_floor_ever.min(goal);

    let mut lines: Vec<Line> = Vec::new();

    lines.push(Line::from(vec![
        Span::styled(
            " 進捗",
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("  — {}階のゴールまで", goal),
            Style::default().fg(Color::DarkGray),
        ),
    ]));
    lines.push(Line::from(""));

    let pct = (cur as f64 / goal as f64 * 100.0).round() as u32;
    lines.push(Line::from(vec![
        Span::styled("  現在: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!("B{}F", cur),
            Style::default()
                .fg(floor_color(cur))
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  /  "),
        Span::styled(format!("B{}F", goal), Style::default().fg(Color::White)),
        Span::styled(format!("  ({}%)", pct), Style::default().fg(Color::Yellow)),
    ]));

    let bar_width: u16 = inner_width.max(10);
    let filled = ((cur as u64 * bar_width as u64) / goal as u64) as u16;
    let deepest_pos = ((deepest as u64 * bar_width as u64) / goal as u64) as u16;
    let deepest_pos = deepest_pos.min(bar_width.saturating_sub(1));

    let mut bar = String::with_capacity(bar_width as usize + 2);
    bar.push('[');
    for i in 0..bar_width {
        let ch = if i == deepest_pos && deepest > cur {
            '*'
        } else if i < filled {
            '█'
        } else {
            '░'
        };
        bar.push(ch);
    }
    bar.push(']');
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(bar, Style::default().fg(floor_color(cur))),
    ]));

    if deepest > cur {
        lines.push(Line::from(vec![
            Span::styled("  最深記録: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("B{}F", deepest),
                Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD),
            ),
            Span::styled("  (バー上の * 印)", Style::default().fg(Color::DarkGray)),
        ]));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        " 節目フロア",
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
    )));
    let mut last_milestone: u32 = 0;
    for pct in [10u32, 25, 50, 75, 100] {
        let milestone = (((goal as u64 * pct as u64) / 100).max(1) as u32).min(goal);
        if milestone == last_milestone {
            continue;
        }
        last_milestone = milestone;
        let reached = cur >= milestone;
        let ever_reached = deepest >= milestone;
        let (mark, mark_color) = if reached {
            ("✓", Color::Green)
        } else if ever_reached {
            ("◇", Color::Magenta)
        } else {
            ("·", Color::DarkGray)
        };
        let line_color = if reached || ever_reached { Color::White } else { Color::DarkGray };
        let status = if reached {
            "(到達済)".to_string()
        } else if ever_reached {
            "(過去最深で踏破)".to_string()
        } else {
            format!("あと {}F", milestone - cur)
        };
        let status_color = if reached {
            Color::Green
        } else if ever_reached {
            Color::Magenta
        } else {
            Color::Yellow
        };
        lines.push(Line::from(vec![
            Span::styled(format!("  {} ", mark), Style::default().fg(mark_color)),
            Span::styled(format!("B{}F", milestone), Style::default().fg(line_color)),
            Span::raw("  "),
            Span::styled(status, Style::default().fg(status_color)),
        ]));
    }

    render_scrollable_tab(state, f, area, click_state, Color::Cyan, lines);
}

// ── 装備ショップタブ ───────────────────────────────────────

/// 装備ショップタブ。lane ごとに購入と装着切替を行う中心ハブ。
///
/// 行の状態:
/// - 装着中: ✓ + " (装着中)"
/// - 所持済み (未装着): ✓ + [装着] ボタン (= EQUIP_ITEM クリックターゲット)
/// - 次に解放可能: ◆/◇ + [購入] ボタン (= BUY_EQUIPMENT クリックターゲット)
/// - その先以降: `???` で隠す (1 個だけ表示してリストを圧縮)
fn render_shop(
    state: &AbyssState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let narrow = is_narrow_layout(area.width);
    let mut cl = ClickableList::new();

    if narrow {
        cl.push(Line::from(Span::styled(
            " 装備",
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        )));
    } else {
        cl.push(Line::from(vec![
            Span::styled(
                " 装備",
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                " — 各 lane で購入と装着切替",
                Style::default().fg(Color::DarkGray),
            ),
        ]));
    }

    for lane in EquipmentLane::all().iter().copied() {
        cl.push(Line::from(""));
        cl.push(Line::from(Span::styled(
            format!(" ▣ {}", lane.name()),
            Style::default().fg(lane_color(lane)).add_modifier(Modifier::BOLD),
        )));

        let mut lane_items: Vec<EquipmentId> = EquipmentId::all()
            .iter()
            .copied()
            .filter(|id| id.lane() == lane)
            .collect();
        lane_items.sort_by_key(|id| id.lane_index());

        // 「次に見える」装備の lane_index = 最後の所持装備の lane_index + 1。
        let next_visible_idx = lane_items
            .iter()
            .rev()
            .find(|id| state.owned_equipment[id.index()])
            .map(|id| id.lane_index() + 1)
            .unwrap_or(0);

        let mut hid_a_step = false;
        for &id in &lane_items {
            let owned = state.owned_equipment[id.index()];
            let li = id.lane_index();

            if owned {
                push_owned_equipment_line(state, &mut cl, id, lane);
            } else if li == next_visible_idx {
                push_buyable_equipment_line(state, &mut cl, id, narrow);
            } else {
                if !hid_a_step {
                    cl.push(Line::from(vec![
                        Span::raw("   "),
                        Span::styled("???", Style::default().fg(Color::DarkGray)),
                    ]));
                    hid_a_step = true;
                }
            }
        }
    }

    render_scrollable_tab(
        state,
        f,
        area,
        click_state,
        Color::Yellow,
        WrappingClickableList { list: cl, wrap: narrow },
    );
}

fn lane_color(lane: EquipmentLane) -> Color {
    match lane {
        EquipmentLane::Weapon => Color::Red,
        EquipmentLane::Armor => Color::Blue,
        EquipmentLane::Accessory => Color::Magenta,
    }
}

/// 所持済み装備の 1 行表示。装着中なら "(装着中)" タグ、それ以外なら [装着] ボタン。
fn push_owned_equipment_line<'a>(
    state: &AbyssState,
    cl: &mut ClickableList<'a>,
    id: EquipmentId,
    lane: EquipmentLane,
) {
    let def = match state.config.equipment.get(id.index()) {
        Some(d) => d,
        None => return,
    };
    let lv = state.equipment_levels[id.index()];
    let is_equipped = state.equipped_at(lane) == Some(id);

    let name_str = format!("{} +{}", def.name, lv);
    let mut spans = vec![
        Span::styled(" ✓ ", Style::default().fg(Color::Green)),
        Span::styled(
            name_str,
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(
            def.effect_label.to_string(),
            Style::default().fg(Color::Cyan),
        ),
    ];

    if is_equipped {
        // 装着中: タグだけ。クリック不可な行 (push、push_clickable ではなく)。
        spans.push(Span::styled(
            "  [装着中]",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ));
        cl.push(Line::from(spans));
    } else {
        // 未装着: [装着] ボタン (= EQUIP_ITEM クリックターゲット)。
        spans.push(Span::styled(
            "  [装着]",
            Style::default()
                .fg(Color::Black)
                .bg(Color::LightGreen)
                .add_modifier(Modifier::BOLD),
        ));
        cl.push_clickable(Line::from(spans), EQUIP_ITEM_BASE + id.index() as u16);
    }
}

/// 「次に解放可能」装備の 1 行表示。条件達成度で色を変える。
fn push_buyable_equipment_line<'a>(
    state: &AbyssState,
    cl: &mut ClickableList<'a>,
    id: EquipmentId,
    narrow: bool,
) {
    let def = match state.config.equipment.get(id.index()) {
        Some(d) => d,
        None => return,
    };
    let req_met = logic::equipment_requirements_met(state, id);
    let cost = def.gold_cost;
    let gold_ok = state.gold >= cost;
    let buyable = req_met && gold_ok;

    let marker = if buyable { " ◆ " } else { " ◇ " };
    let marker_color = if buyable { Color::Yellow } else { Color::DarkGray };
    let label_color = if buyable { Color::White } else { Color::Gray };
    let cost_color = if gold_ok { Color::Yellow } else { Color::DarkGray };

    let mut spans = vec![
        Span::styled(marker, Style::default().fg(marker_color)),
        Span::styled(
            def.name.to_string(),
            Style::default().fg(label_color).add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(def.effect_label.to_string(), Style::default().fg(Color::Cyan)),
    ];

    spans.push(Span::raw("  "));
    spans.push(Span::styled(
        format!("{}g", format_num(cost)),
        Style::default().fg(cost_color),
    ));
    if buyable {
        spans.push(Span::styled(
            " [購入]",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ));
    }

    if !narrow && !req_met {
        let mut missing: Vec<String> = Vec::new();
        if let Some(prereq) = def.prerequisite {
            if !state.owned_equipment[prereq.index()] {
                if let Some(p_def) = state.config.equipment.get(prereq.index()) {
                    missing.push(format!("要 {}", p_def.name));
                }
            }
        }
        if !missing.is_empty() {
            spans.push(Span::styled(
                format!("  ({})", missing.join(", ")),
                Style::default().fg(Color::DarkGray),
            ));
        }
    }

    cl.push_clickable(Line::from(spans), BUY_EQUIPMENT_BASE + id.index() as u16);
}

// ── 設定タブ ───────────────────────────────────────────────

fn render_settings(
    state: &AbyssState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let narrow = is_narrow_layout(area.width);
    let mut cl = ClickableList::new();

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

    // 装着中装備一覧。
    lines.push(Line::from(Span::styled(
        " 装着中装備",
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
    )));
    for &lane in EquipmentLane::all() {
        let label = match state.equipped_at(lane) {
            Some(id) => {
                let name = state
                    .config
                    .equipment
                    .get(id.index())
                    .map(|d| d.name)
                    .unwrap_or("?");
                let lv = state.equipment_levels[id.index()];
                format!("{} +{}", name, lv)
            }
            None => "(未装着)".to_string(),
        };
        lines.push(stat_line(lane.name(), label));
    }

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
    let mut cl = ClickableList::new();

    push_gacha_header(state, &mut cl);
    cl.push(Line::from(""));

    push_gacha_button(&mut cl, "1連 (🔑1)", state.keys >= 1, GACHA_PULL_1, Color::LightCyan);
    cl.push(Line::from(""));
    push_gacha_button(&mut cl, "10連 (🔑10)", state.keys >= 10, GACHA_PULL_10, Color::LightYellow);
    cl.push(Line::from(""));

    push_gacha_last_result(state, &mut cl);
    cl.push(Line::from(""));

    push_gacha_table(state, &mut cl, narrow);

    render_scrollable_tab(
        state,
        f,
        area,
        click_state,
        Color::LightCyan,
        WrappingClickableList { list: cl, wrap: narrow },
    );
}

fn push_gacha_header<'a>(state: &AbyssState, cl: &mut ClickableList<'a>) {
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

    cl.push(Line::from(vec![
        Span::styled(
            " 🎲 深淵ガチャ",
            Style::default().fg(Color::LightCyan).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("  所持: 🔑{}", format_num(state.keys)),
            Style::default().fg(Color::LightCyan).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("  累計: {}回", format_num(state.total_pulls)),
            Style::default().fg(Color::DarkGray),
        ),
    ]));
    cl.push(Line::from(Span::styled(
        format!(" {}", pity_str),
        Style::default().fg(Color::Yellow),
    )));
}

fn push_gacha_button<'a>(
    cl: &mut ClickableList<'a>,
    label: &str,
    affordable: bool,
    action_id: u16,
    active_color: Color,
) {
    let color = if affordable { active_color } else { Color::DarkGray };
    let style = Style::default().fg(color);
    let bold = style.add_modifier(Modifier::BOLD);

    let inner_w: usize = 18;
    let label_padded = format!("▶ {} ◀", label);
    let pad_total = inner_w.saturating_sub(label_padded.chars().count());
    let pad_left = pad_total / 2;
    let pad_right = pad_total - pad_left;

    cl.push_clickable(
        Line::from(Span::styled(format!(" ┌{}┐", "─".repeat(inner_w)), style)),
        action_id,
    );
    cl.push_clickable(
        Line::from(vec![
            Span::styled(" │", style),
            Span::styled(" ".repeat(pad_left), bold),
            Span::styled(label_padded, bold),
            Span::styled(" ".repeat(pad_right), bold),
            Span::styled("│", style),
        ]),
        action_id,
    );
    cl.push_clickable(
        Line::from(Span::styled(format!(" └{}┘", "─".repeat(inner_w)), style)),
        action_id,
    );
}

fn push_gacha_last_result<'a>(state: &AbyssState, cl: &mut ClickableList<'a>) {
    cl.push(Line::from(Span::styled(
        " ── 直近結果 ──",
        Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
    )));

    let Some(r) = &state.last_gacha else {
        cl.push(Line::from(Span::styled(
            " (まだ引いていない)",
            Style::default().fg(Color::DarkGray),
        )));
        return;
    };

    cl.push(Line::from(vec![
        Span::styled(
            format!(" x{} ", r.count),
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!("C:{} ", r.by_tier[0]), Style::default().fg(Color::Gray)),
        Span::styled(format!("R:{} ", r.by_tier[1]), Style::default().fg(Color::Cyan)),
        Span::styled(
            format!("E:{} ", r.by_tier[2]),
            Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("L:{} ", r.by_tier[3]),
            Style::default().fg(Color::LightYellow).add_modifier(Modifier::BOLD),
        ),
    ]));

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
    if r.gained_enh_lv > 0 {
        reward_spans.push(Span::styled(
            format!("◆強化+{} ", r.gained_enh_lv),
            Style::default().fg(Color::Green),
        ));
    }
    if reward_spans.len() == 1 {
        reward_spans.push(Span::styled("—", Style::default().fg(Color::DarkGray)));
    }
    cl.push(Line::from(reward_spans));
}

fn push_gacha_table<'a>(state: &AbyssState, cl: &mut ClickableList<'a>, narrow: bool) {
    let g = &state.config.gacha;
    let total: u32 = g.gacha_weights_milli.iter().sum::<u32>().max(1);
    let pct = |w: u32| -> String { format!("{:.1}%", (w as f64 / total as f64) * 100.0) };

    let rows: [(&'static str, Color, usize, bool, String); 4] = [
        ("Common   ", Color::Gray, 0, false, "💰 大量 gold".to_string()),
        ("Rare     ", Color::Cyan, 1, false, "◆ 装着中装備の強化 +1".to_string()),
        ("Epic     ", Color::Magenta, 2, true, "✦ 魂 (現フロア依存)".to_string()),
        (
            "Legendary",
            Color::LightYellow,
            3,
            true,
            format!("🔑+{} (連鎖チャンス)", g.legendary_keys),
        ),
    ];

    cl.push(Line::from(Span::styled(
        " 確率テーブル",
        Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
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
            cl.push(Line::from(vec![
                Span::styled(format!("  {} ", label), label_style),
                Span::styled(pct_str, weight_style),
            ]));
            cl.push(Line::from(Span::styled(
                format!("    → {}", reward),
                Style::default().fg(Color::DarkGray),
            )));
        } else {
            cl.push(Line::from(vec![
                Span::styled(format!("  {} ", label), label_style),
                Span::styled(pct_str, weight_style),
                Span::styled(format!("  → {}", reward), Style::default().fg(Color::DarkGray)),
            ]));
        }
    }

    cl.push(Line::from(""));
    cl.push(Line::from(Span::styled(
        " 鍵入手: ボス +1 (Elite +2 / 10F毎 +2)",
        Style::default().fg(Color::DarkGray),
    )));
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

    /// ガチャタブの 1連/10連 ボタンが click target として登録されること。
    #[test]
    fn gacha_buttons_register_clickable_areas() {
        let mut state = AbyssState::new();
        state.tab = Tab::Gacha;
        state.keys = 100;
        let cs = Rc::new(RefCell::new(ClickState::new()));
        let mut terminal = Terminal::new(TestBackend::new(80, 30)).unwrap();
        terminal.draw(|f| render(&state, f, f.area(), &cs)).unwrap();

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
        assert!(found_one);
        assert!(found_ten);
    }

    #[test]
    fn all_top_groups_registered() {
        let state = AbyssState::new();
        let cs = Rc::new(RefCell::new(ClickState::new()));
        let mut terminal = Terminal::new(TestBackend::new(80, 30)).unwrap();
        terminal.draw(|f| render(&state, f, f.area(), &cs)).unwrap();
        let cs = cs.borrow();
        let mut found = [false; 4];
        for y in 0..30 {
            for x in 0..80 {
                match cs.hit_test(x, y) {
                    Some(TAB_GROUP_GROWTH) => found[0] = true,
                    Some(TAB_GROUP_INFO) => found[1] = true,
                    Some(TAB_GROUP_GACHA) => found[2] = true,
                    Some(TAB_GROUP_SETTINGS) => found[3] = true,
                    _ => {}
                }
            }
        }
        assert!(found.iter().all(|&b| b));
    }

    #[test]
    fn growth_group_renders_three_subtabs() {
        let state = AbyssState::new();
        let cs = Rc::new(RefCell::new(ClickState::new()));
        let mut terminal = Terminal::new(TestBackend::new(80, 30)).unwrap();
        terminal.draw(|f| render(&state, f, f.area(), &cs)).unwrap();
        let cs = cs.borrow();
        let mut found_subtabs = [false; 3];
        for y in 0..30 {
            for x in 0..80 {
                match cs.hit_test(x, y) {
                    Some(TAB_UPGRADES) => found_subtabs[0] = true,
                    Some(TAB_SHOP) => found_subtabs[1] = true,
                    Some(TAB_SOULS) => found_subtabs[2] = true,
                    _ => {}
                }
            }
        }
        assert!(found_subtabs.iter().all(|&b| b));
    }

    /// 強化タブで装着中装備の行が ENHANCE_EQUIPMENT_BASE+id クリックターゲットを持つこと。
    #[test]
    fn upgrades_tab_registers_enhance_target_for_equipped() {
        let mut state = AbyssState::new();
        state.gold = 1_000_000;
        // 銅剣を購入 → 自動装着。
        super::super::logic::buy_equipment(&mut state, EquipmentId::BronzeSword);
        state.tab = Tab::Upgrades;

        let cs = Rc::new(RefCell::new(ClickState::new()));
        let mut terminal = Terminal::new(TestBackend::new(80, 30)).unwrap();
        terminal.draw(|f| render(&state, f, f.area(), &cs)).unwrap();
        let cs = cs.borrow();
        let target = ENHANCE_EQUIPMENT_BASE + EquipmentId::BronzeSword.index() as u16;
        let mut found = false;
        for y in 0..30 {
            for x in 0..80 {
                if cs.hit_test(x, y) == Some(target) {
                    found = true;
                }
            }
        }
        assert!(found, "強化ターゲット {} が登録されていない", target);
    }

    /// 装備タブで lane 入口装備が BUY_EQUIPMENT_BASE クリックターゲットを持つこと。
    #[test]
    fn shop_tab_registers_buy_target_for_lane_entry() {
        let mut state = AbyssState::new();
        state.gold = 1_000;
        state.tab = Tab::Shop;

        let cs = Rc::new(RefCell::new(ClickState::new()));
        let mut terminal = Terminal::new(TestBackend::new(80, 40)).unwrap();
        terminal.draw(|f| render(&state, f, f.area(), &cs)).unwrap();
        let cs = cs.borrow();
        let target = BUY_EQUIPMENT_BASE + EquipmentId::BronzeSword.index() as u16;
        let mut found = false;
        for y in 0..40 {
            for x in 0..80 {
                if cs.hit_test(x, y) == Some(target) {
                    found = true;
                }
            }
        }
        assert!(found, "購入ターゲット {} が登録されていない", target);
    }

    /// 所持済み (未装着) の装備に EQUIP_ITEM_BASE クリックターゲットが登録されること。
    /// 銅剣を買って装着 → 鋼鉄の剣を買って自動装着 (新装備) → 銅剣は所持中だが未装着 → [装着] ボタン化。
    #[test]
    fn shop_tab_registers_equip_target_for_owned_unequipped() {
        let mut state = AbyssState::new();
        state.gold = 1_000_000_000;
        super::super::logic::buy_equipment(&mut state, EquipmentId::BronzeSword);
        super::super::logic::buy_equipment(&mut state, EquipmentId::SteelSword);
        // ここで装着中は SteelSword、所持中で未装着は BronzeSword。
        state.tab = Tab::Shop;

        let cs = Rc::new(RefCell::new(ClickState::new()));
        let mut terminal = Terminal::new(TestBackend::new(80, 40)).unwrap();
        terminal.draw(|f| render(&state, f, f.area(), &cs)).unwrap();
        let cs = cs.borrow();
        let target = EQUIP_ITEM_BASE + EquipmentId::BronzeSword.index() as u16;
        let mut found = false;
        for y in 0..40 {
            for x in 0..80 {
                if cs.hit_test(x, y) == Some(target) {
                    found = true;
                }
            }
        }
        assert!(found, "装着切替ターゲット {} が登録されていない", target);
    }
}
