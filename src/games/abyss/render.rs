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
use super::state::{AbyssState, SoulPerk, Tab, UpgradeKind};

pub fn render(state: &AbyssState, f: &mut Frame, area: Rect, click_state: &Rc<RefCell<ClickState>>) {
    let narrow = is_narrow_layout(area.width);

    // 縦分割
    //   - 1行: ステータス (フロア, ゴールド, 魂)
    //   - 4-5行: hero vs enemy 戦闘表示
    //   - 1行: トグル行 (自動潜行 / 撤退)
    //   - 1行: タブバー
    //   - 残り: タブコンテンツ + ログ
    let combat_height: u16 = if narrow { 7 } else { 8 };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),                  // ヘッダ (ステータス)
            Constraint::Length(combat_height),       // 戦闘
            Constraint::Length(3),                   // トグル行
            Constraint::Length(1),                   // タブバー
            Constraint::Min(8),                      // タブコンテンツ + ログ
        ])
        .split(area);

    render_header(state, f, chunks[0]);
    render_combat(state, f, chunks[1], narrow);
    render_toggle_bar(state, f, chunks[2], click_state);
    render_tab_bar(state, f, chunks[3], click_state);

    // タブコンテンツ + ログを縦分割
    let log_h: u16 = if narrow { 4 } else { 5 };
    let body = chunks[4];
    let body_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(5), Constraint::Length(log_h)])
        .split(body);

    match state.tab {
        Tab::Upgrades => render_upgrades(state, f, body_chunks[0], click_state),
        Tab::Souls => render_souls(state, f, body_chunks[0], click_state),
        Tab::Stats => render_stats(state, f, body_chunks[0]),
    }
    render_log(state, f, body_chunks[1]);
}

// ── ヘッダ ─────────────────────────────────────────────────

fn render_header(state: &AbyssState, f: &mut Frame, area: Rect) {
    let line = Line::from(vec![
        Span::styled(
            format!(" B{}F ", state.floor),
            Style::default()
                .fg(Color::Black)
                .bg(floor_color(state.floor))
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(
            format!("最深: B{}F", state.deepest_floor_ever),
            Style::default().fg(Color::DarkGray),
        ),
        Span::raw("  "),
        Span::styled(
            format!("💰{}", format_num(state.gold)),
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(
            format!("✦{}", format_num(state.souls)),
            Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(
            format!("撃破:{}", format_num(state.total_kills)),
            Style::default().fg(Color::DarkGray),
        ),
    ]);

    let widget = Paragraph::new(line).block(
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
    let until = state.enemies_until_boss();
    if state.current_enemy.is_boss {
        format!(" 戦闘 — ボス戦 (B{}F) ", state.floor)
    } else if until == 0 {
        format!(" 戦闘 — ボス出現中 (B{}F) ", state.floor)
    } else {
        format!(" 戦闘 — ボスまであと {} 体 ", until)
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
    state: &AbyssState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));
    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.height == 0 {
        return;
    }

    let halves = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(inner);

    let auto_label = if state.auto_descend {
        " [A] 自動潜行: ON ▼ "
    } else {
        " [A] 自動潜行: OFF ■ "
    };
    let auto_color = if state.auto_descend { Color::Green } else { Color::Yellow };
    let auto_para = Paragraph::new(Line::from(Span::styled(
        auto_label,
        Style::default().fg(auto_color).add_modifier(Modifier::BOLD),
    )));

    let retreat_para = Paragraph::new(Line::from(Span::styled(
        " [P] 浅瀬に戻る △",
        Style::default().fg(Color::LightCyan),
    )))
    .alignment(Alignment::Right);

    {
        let mut cs = click_state.borrow_mut();
        Clickable::new(auto_para, TOGGLE_AUTO_DESCEND).render(f, halves[0], &mut cs);
        Clickable::new(retreat_para, RETREAT_TO_SURFACE).render(f, halves[1], &mut cs);
    }
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
    let bar = TabBar::new(separator)
        .tab("強化", style_for(0, Color::Green), TAB_UPGRADES)
        .tab("魂", style_for(1, Color::Magenta), TAB_SOULS)
        .tab("統計", style_for(2, Color::Cyan), TAB_STATS);

    let mut cs = click_state.borrow_mut();
    bar.render(f, area, &mut cs);
}

// ── 強化タブ ───────────────────────────────────────────────

fn render_upgrades(
    state: &AbyssState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let mut cl = ClickableList::new();

    cl.push(Line::from(vec![
        Span::styled(" 強化", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
        Span::styled(
            " — gold で永続強化を購入",
            Style::default().fg(Color::DarkGray),
        ),
    ]));
    cl.push(Line::from(""));

    for kind in UpgradeKind::all() {
        let lv = state.upgrades[kind.index()];
        let cost = state.upgrade_cost(*kind);
        let key_char = upgrade_key(*kind);
        let affordable = state.gold >= cost;

        let cost_str = format!("{}g", format_num(cost));
        let cost_style = if affordable {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let label_color = if affordable { Color::White } else { Color::DarkGray };
        let label = format!(" [{}] {} ", key_char, kind.name());
        let effect = kind.effect().to_string();
        let lv_str = format!(" Lv.{}", lv);

        cl.push_clickable(
            Line::from(vec![
                Span::styled(label, Style::default().fg(label_color).add_modifier(Modifier::BOLD)),
                Span::styled(format!("{:<10}", effect), Style::default().fg(Color::Cyan)),
                Span::styled(format!("{:>10}", cost_str), cost_style),
                Span::styled(lv_str, Style::default().fg(Color::Magenta)),
            ]),
            BUY_UPGRADE_BASE + kind.index() as u16,
        );
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Green));
    let mut cs = click_state.borrow_mut();
    cl.render(f, area, block, &mut cs, false, 0);
}

fn upgrade_key(kind: UpgradeKind) -> char {
    match kind {
        UpgradeKind::Sword => '1',
        UpgradeKind::Vitality => '2',
        UpgradeKind::Armor => '3',
        UpgradeKind::Crit => '4',
        UpgradeKind::Speed => '5',
        UpgradeKind::Regen => '6',
        UpgradeKind::Gold => '7',
    }
}

// ── 魂タブ ─────────────────────────────────────────────────

fn render_souls(
    state: &AbyssState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
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

    for (i, perk) in SoulPerk::all().iter().enumerate() {
        let lv = state.soul_perks[perk.index()];
        let cost = state.soul_perk_cost(*perk);
        let affordable = state.souls >= cost;

        let cost_style = if affordable {
            Style::default().fg(Color::Magenta)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let label_color = if affordable { Color::White } else { Color::DarkGray };

        let key_char = (b'q' + i as u8) as char;
        let label = format!(" [{}] {} ", key_char, perk.name());
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

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Magenta));
    let mut cs = click_state.borrow_mut();
    cl.render(f, area, block, &mut cs, false, 0);
}

// ── 統計タブ ───────────────────────────────────────────────

fn render_stats(state: &AbyssState, f: &mut Frame, area: Rect) {
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
        "HP回復",
        format!("{:.1}/秒", state.hero_regen_per_sec()),
    ));
    lines.push(stat_line(
        "ゴールド倍率",
        format!("×{:.2}", state.gold_multiplier()),
    ));

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    f.render_widget(Paragraph::new(lines).block(block), area);
}

fn stat_line(label: &'static str, value: String) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!(" {:<14}", label), Style::default().fg(Color::DarkGray)),
        Span::styled(value, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
    ])
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
