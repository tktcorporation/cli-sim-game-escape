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
        Tab::Gacha => render_gacha(state, f, body_chunks[0], click_state),
    }
    render_log(state, f, body_chunks[1]);
}

// ── ヘッダ ─────────────────────────────────────────────────

fn render_header(state: &AbyssState, f: &mut Frame, area: Rect) {
    let mut spans: Vec<Span> = vec![
        Span::styled(
            format!(" B{}F ", state.floor),
            Style::default()
                .fg(Color::Black)
                .bg(floor_color(state.floor))
                .add_modifier(Modifier::BOLD),
        ),
    ];
    if !matches!(state.floor_kind, FloorKind::Normal) {
        spans.push(Span::raw(" "));
        spans.push(Span::styled(
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
    spans.extend([
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
            format!("🔑{}", format_num(state.keys)),
            Style::default()
                .fg(Color::LightCyan)
                .add_modifier(Modifier::BOLD),
        ),
    ]);
    let line = Line::from(spans);

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
        Tab::Gacha => 3,
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
        .tab("統計", style_for(2, Color::Cyan), TAB_STATS)
        .tab("ガチャ🔑", style_for(3, Color::LightCyan), TAB_GACHA);

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

// ── ガチャタブ ─────────────────────────────────────────────

fn render_gacha(
    state: &AbyssState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::LightCyan));
    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.height < 3 || inner.width < 20 {
        return;
    }

    // 縦分割: ヘッダ + ボタン行 + 結果サマリ + テーブル
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // ヘッダ
            Constraint::Length(3), // ボタン
            Constraint::Length(4), // 直近結果
            Constraint::Min(3),    // 確率テーブル
        ])
        .split(inner);

    render_gacha_header(state, f, chunks[0]);
    render_gacha_buttons(state, f, chunks[1], click_state);
    render_gacha_last_result(state, f, chunks[2]);
    render_gacha_table(state, f, chunks[3]);
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
) {
    let halves = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    let can_one = state.keys >= 1;
    let can_ten = state.keys >= 10;

    let one_color = if can_one { Color::LightCyan } else { Color::DarkGray };
    let ten_color = if can_ten { Color::LightYellow } else { Color::DarkGray };

    let one_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(one_color));
    let one_para = Paragraph::new(Line::from(Span::styled(
        " [S] 1連 (🔑1) ",
        Style::default().fg(one_color).add_modifier(Modifier::BOLD),
    )))
    .alignment(Alignment::Center)
    .block(one_block);

    let ten_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ten_color));
    let ten_para = Paragraph::new(Line::from(Span::styled(
        " [X] 10連 (🔑10) ",
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

fn render_gacha_table(state: &AbyssState, f: &mut Frame, area: Rect) {
    let g = &state.config.gacha;
    let total: u32 = g.gacha_weights_milli.iter().sum::<u32>().max(1);
    let pct = |w: u32| -> String { format!("{:.1}%", (w as f64 / total as f64) * 100.0) };

    let lines = vec![
        Line::from(Span::styled(
            " 確率テーブル",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(vec![
            Span::styled(
                "  Common    ",
                Style::default().fg(Color::Gray),
            ),
            Span::styled(pct(g.gacha_weights_milli[0]), Style::default().fg(Color::Gray)),
            Span::styled(
                "  → 💰 大量 gold",
                Style::default().fg(Color::DarkGray),
            ),
        ]),
        Line::from(vec![
            Span::styled("  Rare      ", Style::default().fg(Color::Cyan)),
            Span::styled(pct(g.gacha_weights_milli[1]), Style::default().fg(Color::Cyan)),
            Span::styled(
                "  → ◆ 強化レベル+1 (永続)",
                Style::default().fg(Color::DarkGray),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                "  Epic      ",
                Style::default()
                    .fg(Color::Magenta)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                pct(g.gacha_weights_milli[2]),
                Style::default()
                    .fg(Color::Magenta)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "  → ✦ 魂 (現フロア依存)",
                Style::default().fg(Color::DarkGray),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                "  Legendary ",
                Style::default()
                    .fg(Color::LightYellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                pct(g.gacha_weights_milli[3]),
                Style::default()
                    .fg(Color::LightYellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  → 🔑+{} (連鎖チャンス)", g.legendary_keys),
                Style::default().fg(Color::DarkGray),
            ),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            " 鍵入手: ボス +1 (Elite +2 / 10F毎 +2)",
            Style::default().fg(Color::DarkGray),
        )),
    ];

    f.render_widget(Paragraph::new(lines), area);
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

    /// ガチャタブを描画したとき、`[S]` 1連 / `[X]` 10連ボタンが
    /// 実描画位置に対してクリックターゲット登録されていることを TestBackend で確認。
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

    /// 4 タブ全てがクリック可能領域として登録されていることを確認。
    #[test]
    fn all_tabs_registered() {
        let state = AbyssState::new();
        let cs = Rc::new(RefCell::new(ClickState::new()));
        let mut terminal = Terminal::new(TestBackend::new(80, 30)).unwrap();
        terminal
            .draw(|f| render(&state, f, f.area(), &cs))
            .unwrap();
        let cs = cs.borrow();
        let mut found = [false; 4];
        for y in 0..30 {
            for x in 0..80 {
                match cs.hit_test(x, y) {
                    Some(TAB_UPGRADES) => found[0] = true,
                    Some(TAB_SOULS) => found[1] = true,
                    Some(TAB_STATS) => found[2] = true,
                    Some(TAB_GACHA) => found[3] = true,
                    _ => {}
                }
            }
        }
        assert!(found.iter().all(|&b| b), "missing tab targets: {:?}", found);
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
