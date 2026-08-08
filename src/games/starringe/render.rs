//! 星環の描画。読み取り専用。クリック登録は widgets 経由のみ。

use std::cell::RefCell;
use std::rc::Rc;

use ratzilla::ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratzilla::ratatui::style::{Color, Modifier, Style};
use ratzilla::ratatui::symbols::Marker;
use ratzilla::ratatui::text::{Line, Span};
use ratzilla::ratatui::widgets::canvas::{Canvas, Line as CanvasLine, Points};
use ratzilla::ratatui::widgets::{Block, Borders, Paragraph};
use ratzilla::ratatui::Frame;

use crate::canvas_fx;
use crate::input::{is_narrow_layout, ClickState};
use crate::widgets::{Clickable, ClickableList, TabBar};

use super::actions::{
    buy_ring_id, buy_weapon_stat_id, select_weapon_id, OPEN_LAYER, TAB_ARMORY, TAB_CODEX, TAB_RING,
    TAP_STRIKE, WEAPON_NEXT, WEAPON_PREV,
};
use super::logic::{
    can_unlock_next_layer, can_upgrade_ring, can_upgrade_weapon_stat, layer_unlock_cost,
    ring_upgrade_cost, turret_positions, weapon_stat_cost,
};
use super::state::{
    Layer, OreKind, ParticleKind, RingUpgrade, StarRingState, Tab, WeaponKind, WeaponStat, CX, CY,
    ORBIT_Y_SQUASH, WORLD_H, WORLD_W,
};

pub fn render(
    state: &StarRingState,
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

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(2),
        ])
        .split(area);

    render_header(state, f, chunks[0], borders);
    render_tabs(state, f, chunks[1], borders, click_state);

    if is_narrow {
        let body = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(46), Constraint::Percentage(54)])
            .split(chunks[2]);
        render_stage(state, f, body[0], borders, click_state);
        match state.tab {
            Tab::Armory => render_armory(state, f, body[1], borders, click_state),
            Tab::Ring => render_ring(state, f, body[1], borders, click_state),
            Tab::Codex => render_codex(state, f, body[1], borders),
        }
    } else {
        let body = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(46), Constraint::Percentage(54)])
            .split(chunks[2]);
        match state.tab {
            Tab::Armory => render_armory(state, f, body[0], borders, click_state),
            Tab::Ring => render_ring(state, f, body[0], borders, click_state),
            Tab::Codex => render_codex(state, f, body[0], borders),
        }
        render_stage(state, f, body[1], borders, click_state);
    }

    render_footer(state, f, chunks[3], borders);
}

fn format_shards(n: f64) -> String {
    if n >= 1_000_000.0 {
        format!("{:.2}M", n / 1_000_000.0)
    } else if n >= 10_000.0 {
        format!("{:.1}K", n / 1_000.0)
    } else if n >= 100.0 {
        format!("{:.0}", n)
    } else {
        format!("{:.1}", n)
    }
}

fn render_header(state: &StarRingState, f: &mut Frame, area: Rect, borders: Borders) {
    let sps = state.shards_per_sec();
    let layer = state.layer();
    let boost = if state.boost_ticks > 0 {
        " ⚡ブースト"
    } else {
        ""
    };
    let layer_fx = if state.layer_flash_ticks > 0 {
        " ◆層開放"
    } else if can_unlock_next_layer(state) {
        " ◆開放可[!]"
    } else if state.kills_ready_for_next_layer() {
        " ◆星屑不足"
    } else if state.layer_ready_flash_ticks > 0 {
        " ◆条件達成"
    } else {
        ""
    };
    let p = Paragraph::new(Line::from(vec![
        Span::styled(
            "星環",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(
            format!("第{layer}層 {}", Layer::title(layer)),
            Style::default()
                .fg(layer_color(layer))
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(
            format!("✦{}", format_shards(state.shards)),
            Style::default().fg(Color::LightYellow),
        ),
        Span::raw("  "),
        Span::styled(
            format!("{:.1}/秒", sps),
            Style::default().fg(Color::Cyan),
        ),
        Span::styled(boost, Style::default().fg(Color::LightRed)),
        Span::styled(layer_fx, Style::default().fg(Color::LightMagenta)),
    ]))
    .block(
        Block::default()
            .borders(borders)
            .border_style(Style::default().fg(Color::Yellow))
            .title(" 軌道採掘 "),
    );
    f.render_widget(p, area);
}

fn layer_color(layer: u32) -> Color {
    match layer {
        1 => Color::Gray,
        2 => Color::Yellow,
        3 => Color::LightCyan,
        4 => Color::LightMagenta,
        5 => Color::LightRed,
        6 => Color::Cyan,
        7 => Color::Magenta,
        _ => Color::White,
    }
}

fn render_tabs(
    state: &StarRingState,
    f: &mut Frame,
    area: Rect,
    borders: Borders,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let mut cs = click_state.borrow_mut();
    let sel = |active: bool| {
        if active {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        }
    };
    TabBar::new("│")
        .block(Block::default().borders(borders).title(" 画面 "))
        .tab("武装", sel(state.tab == Tab::Armory), TAB_ARMORY)
        .tab("環", sel(state.tab == Tab::Ring), TAB_RING)
        .tab("図鑑", sel(state.tab == Tab::Codex), TAB_CODEX)
        .render(f, area, &mut cs);
}

/// 武装タブ: 武器ピッカー + ビジュアル説明 + 個別強化 (余白多め)。
fn render_armory(
    state: &StarRingState,
    f: &mut Frame,
    area: Rect,
    borders: Borders,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Yellow))
        .title(" 武装 ");
    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.height < 6 || inner.width < 12 {
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // 武器ピッカー
            Constraint::Length(5), // ビジュアル + 説明
            Constraint::Min(6),    // 強化3種
        ])
        .split(inner);

    render_weapon_picker(state, f, chunks[0], click_state);
    render_weapon_showcase(state, f, chunks[1]);
    render_weapon_upgrades(state, f, chunks[2], click_state);
}

fn render_weapon_picker(
    state: &StarRingState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(3),
        ])
        .split(area);

    let prev = Paragraph::new(Line::from(Span::styled(
        "◀",
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
    )))
    .alignment(Alignment::Center)
    .block(Block::default().borders(Borders::NONE));
    Clickable::new(prev, WEAPON_PREV).render(f, chunks[0], &mut click_state.borrow_mut());

    let next = Paragraph::new(Line::from(Span::styled(
        "▶",
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
    )))
    .alignment(Alignment::Center);
    Clickable::new(next, WEAPON_NEXT).render(f, chunks[2], &mut click_state.borrow_mut());

    // 中央: 解放済み武器を横並びで選択
    let mut spans = Vec::new();
    for w in WeaponKind::ALL {
        let unlocked = state.is_weapon_unlocked(w);
        let selected = state.selected_weapon == w;
        let style = if !unlocked {
            Style::default().fg(Color::DarkGray)
        } else if selected {
            Style::default()
                .fg(Color::Black)
                .bg(weapon_color(w))
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(weapon_color(w))
        };
        let label = if unlocked {
            format!(" {}{} ", w.glyph(), w.label())
        } else {
            format!(" ？L{} ", w.unlock_layer())
        };
        spans.push((label, style, unlocked, w));
    }

    // クリック可能な武器チップを等分
    let n = spans.len().max(1) as u16;
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(vec![Constraint::Ratio(1, n as u32); spans.len()])
        .split(chunks[1]);

    for (i, (label, style, unlocked, w)) in spans.into_iter().enumerate() {
        let p = Paragraph::new(Line::from(Span::styled(label, style))).alignment(Alignment::Center);
        if unlocked {
            Clickable::new(p, select_weapon_id(w)).render(
                f,
                cols[i],
                &mut click_state.borrow_mut(),
            );
        } else {
            f.render_widget(p, cols[i]);
        }
    }
}

fn render_weapon_showcase(state: &StarRingState, f: &mut Frame, area: Rect) {
    let w = state.selected_weapon;
    let unlocked = state.is_weapon_unlocked(w);
    let art = weapon_art(w);
    let dmg = state.weapon_damage(w);
    let interval = state.fire_interval(w);
    let volley = state.volley_count(w);

    let mut lines = vec![
        Line::from(vec![
            Span::styled(
                format!(" {}  {}", w.glyph(), w.label()),
                Style::default()
                    .fg(weapon_color(w))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("   "),
            Span::styled(art, Style::default().fg(weapon_color(w))),
        ]),
        Line::from(Span::styled(
            if unlocked {
                format!("  {}", w.blurb())
            } else {
                format!("  第{}層で解放", w.unlock_layer())
            },
            Style::default().fg(Color::Gray),
        )),
    ];
    if unlocked {
        lines.push(Line::from(Span::styled(
            format!("  威力{dmg:.2}  間隔{interval}  斉射×{volley}"),
            Style::default().fg(Color::DarkGray),
        )));
        // 簡易ステータスバー
        let power_lv = state.weapon_stat(w, WeaponStat::Power);
        let rate_lv = state.weapon_stat(w, WeaponStat::Rate);
        let count_lv = state.weapon_stat(w, WeaponStat::Count);
        lines.push(Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(
                format!("弾{}", bar(count_lv, 7)),
                Style::default().fg(Color::Cyan),
            ),
            Span::raw(" "),
            Span::styled(
                format!("連{}", bar(rate_lv, 8)),
                Style::default().fg(Color::LightYellow),
            ),
            Span::raw(" "),
            Span::styled(
                format!("威{}", bar(power_lv.min(8), 8)),
                Style::default().fg(Color::LightRed),
            ),
        ]));
    } else {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  次層を開放して手札を増やそう",
            Style::default().fg(Color::DarkGray),
        )));
    }
    f.render_widget(Paragraph::new(lines), area);
}

fn bar(lv: u32, width: u32) -> String {
    let filled = lv.min(width) as usize;
    let empty = width as usize - filled;
    format!("{}{}", "█".repeat(filled), "░".repeat(empty))
}

fn weapon_art(w: WeaponKind) -> &'static str {
    match w {
        WeaponKind::Pulse => "· › · › · ›",
        WeaponKind::Ray => "════════▷",
        WeaponKind::Scatter => "  ※ ※ ※",
        WeaponKind::Arc => "  ☾  ～▷",
        WeaponKind::Nova => "  ·→✸←·",
    }
}

fn weapon_color(w: WeaponKind) -> Color {
    match w {
        WeaponKind::Pulse => Color::Cyan,
        WeaponKind::Ray => Color::White,
        WeaponKind::Scatter => Color::Yellow,
        WeaponKind::Arc => Color::LightMagenta,
        WeaponKind::Nova => Color::LightRed,
    }
}

fn render_weapon_upgrades(
    state: &StarRingState,
    f: &mut Frame,
    area: Rect,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let w = state.selected_weapon;
    if !state.is_weapon_unlocked(w) {
        let p = Paragraph::new(Line::from(Span::styled(
            "  (解放後に強化できます)",
            Style::default().fg(Color::DarkGray),
        )));
        f.render_widget(p, area);
        return;
    }

    // 3強化を縦に余白付きで配置
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(0),
        ])
        .split(area);

    let keys = ['A', 'S', 'D'];
    for (i, stat) in WeaponStat::ALL.iter().copied().enumerate() {
        if i >= rows.len() {
            break;
        }
        let lv = state.weapon_stat(w, stat);
        let maxed = !can_upgrade_weapon_stat(state, w, stat);
        let cost = weapon_stat_cost(state, w, stat);
        let can = !maxed && state.shards + 1e-9 >= cost;
        let cost_label = if maxed {
            "MAX".to_string()
        } else {
            format!("✦{}", format_shards(cost))
        };
        let style = if maxed {
            Style::default().fg(Color::DarkGray)
        } else if can {
            Style::default()
                .fg(Color::LightYellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        };
        let lines = vec![
            Line::from(vec![
                Span::styled(
                    format!(" [{}] ", keys[i]),
                    Style::default().fg(Color::Yellow),
                ),
                Span::styled(
                    format!("{}  Lv.{}", stat.label(), lv),
                    style.add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                Span::styled(cost_label, Style::default().fg(Color::Cyan)),
            ]),
            Line::from(Span::styled(
                format!("      {}", stat.blurb()),
                Style::default().fg(Color::DarkGray),
            )),
        ];
        let p = Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::LEFT)
                .border_style(Style::default().fg(if can {
                    weapon_color(w)
                } else {
                    Color::DarkGray
                })),
        );
        Clickable::new(p, buy_weapon_stat_id(w, stat)).render(
            f,
            rows[i],
            &mut click_state.borrow_mut(),
        );
    }
}

fn render_ring(
    state: &StarRingState,
    f: &mut Frame,
    area: Rect,
    borders: Borders,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let layer = state.layer();
    let next = Layer::next_threshold(layer);
    let progress = match next {
        Some(th) => {
            let prev = Layer::entry_threshold(layer);
            let span = th.saturating_sub(prev).max(1);
            let done = state.total_kills.saturating_sub(prev);
            ((done * 10) / span).min(10)
        }
        None => 10,
    };
    let bar = format!(
        "{}{}",
        "█".repeat(progress as usize),
        "░".repeat(10 - progress as usize)
    );

    let mut cl = ClickableList::new();
    cl.push(Line::from(Span::styled(
        format!(
            " 第{}層 {}  {}",
            layer,
            Layer::title(layer),
            bar
        ),
        Style::default()
            .fg(layer_color(layer))
            .add_modifier(Modifier::BOLD),
    )));
    cl.push(Line::from(Span::styled(
        match next {
            Some(th) => format!(
                " 次層まで 撃破 {} / {}",
                state.total_kills, th
            ),
            None => format!(" 撃破 {}", state.total_kills),
        },
        Style::default().fg(Color::DarkGray),
    )));
    cl.push(Line::from(Span::styled(
        format!(
            " 湧き×{}  HP×{:.1}  星屑×{:.1}",
            Layer::spawn_batch(layer),
            Layer::hp_mult(layer),
            Layer::value_mult(layer)
        ),
        Style::default().fg(Color::Gray),
    )));
    cl.push(Line::from(""));

    if let Some(th) = next {
        let next_layer = layer + 1;
        let cost = layer_unlock_cost(state);
        let kills_ready = state.total_kills >= th;
        let can = can_unlock_next_layer(state);
        if kills_ready {
            let style = if can {
                Style::default()
                    .fg(Color::LightMagenta)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            let label = if can {
                format!(
                    " [!] 第{}層「{}」を開放  ✦{}",
                    next_layer,
                    Layer::title(next_layer),
                    format_shards(cost)
                )
            } else {
                format!(
                    " [!] 第{}層「{}」 要✦{} (不足)",
                    next_layer,
                    Layer::title(next_layer),
                    format_shards(cost)
                )
            };
            if can {
                cl.push_clickable(Line::from(Span::styled(label, style)), OPEN_LAYER);
            } else {
                cl.push(Line::from(Span::styled(label, style)));
            }
            cl.push(Line::from(Span::styled(
                "      撃破条件達成 — 星屑を払って層を開く",
                Style::default().fg(Color::DarkGray),
            )));
        } else {
            cl.push(Line::from(Span::styled(
                format!(
                    " 次層「{}」 開放条件: 撃破{}  費用✦{}",
                    Layer::title(next_layer),
                    th,
                    format_shards(cost)
                ),
                Style::default().fg(Color::DarkGray),
            )));
        }
        cl.push(Line::from(""));
    }

    cl.push(Line::from(Span::styled(
        " 環の強化",
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    )));
    cl.push(Line::from(""));

    let keys = ['1', '2'];
    for (i, kind) in RingUpgrade::ALL.iter().copied().enumerate() {
        let unlocked = state.is_ring_unlocked(kind);
        let lv = state.ring_level(kind);
        let maxed = unlocked && !can_upgrade_ring(state, kind);
        let cost = ring_upgrade_cost(state, kind);
        let can = unlocked && !maxed && state.shards + 1e-9 >= cost;
        let key = keys.get(i).copied().unwrap_or('?');
        let style = if !unlocked {
            Style::default().fg(Color::DarkGray)
        } else if can {
            Style::default()
                .fg(Color::LightYellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        };
        let cost_label = if !unlocked {
            format!("L{}", kind.unlock_layer())
        } else if maxed {
            "MAX".to_string()
        } else {
            format!("✦{}", format_shards(cost))
        };
        if unlocked {
            cl.push_clickable(
                Line::from(vec![
                    Span::styled(format!(" [{key}] "), Style::default().fg(Color::Yellow)),
                    Span::styled(format!("{} Lv.{} ", kind.label(), lv), style),
                    Span::styled(cost_label, Style::default().fg(Color::Cyan)),
                ]),
                buy_ring_id(kind),
            );
        } else {
            cl.push(Line::from(vec![
                Span::styled(format!(" [{key}] "), Style::default().fg(Color::DarkGray)),
                Span::styled(format!("{}  ", kind.label()), style),
                Span::styled(cost_label, Style::default().fg(Color::DarkGray)),
            ]));
        }
        cl.push(Line::from(Span::styled(
            format!("      {}", kind.blurb()),
            Style::default().fg(Color::DarkGray),
        )));
        cl.push(Line::from(""));
    }

    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Yellow))
        .title(" 環 ");
    let mut cs = click_state.borrow_mut();
    cl.render(f, area, block, &mut cs, false, 0);
}

fn render_codex(state: &StarRingState, f: &mut Frame, area: Rect, borders: Borders) {
    let unlocked = state.unlocked_ore_kinds();
    let layer = state.layer();
    let mut lines = vec![
        Line::from(Span::styled(
            format!(
                " 第{}層  累計撃破 {}  逸失 {}",
                layer, state.total_kills, state.missed_count
            ),
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(""),
    ];
    for kind in OreKind::ALL {
        let open = unlocked.contains(&kind);
        if open {
            lines.push(Line::from(vec![
                Span::styled(" ◆ ", Style::default().fg(ore_color(kind))),
                Span::styled(
                    format!("{} ", kind.label()),
                    Style::default()
                        .fg(ore_color(kind))
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!(
                        "価値{} HP{:.0}",
                        kind.base_value(),
                        kind.base_hp() * Layer::hp_mult(layer)
                    ),
                    Style::default().fg(Color::Gray),
                ),
            ]));
        } else {
            lines.push(Line::from(Span::styled(
                format!(" ？ 第{}層で出現", kind.unlock_layer()),
                Style::default().fg(Color::DarkGray),
            )));
        }
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        format!(" 獲得累計 ✦{}", format_shards(state.shards_earned)),
        Style::default().fg(Color::DarkGray),
    )));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        " 武装解放",
        Style::default().fg(Color::Yellow),
    )));
    for w in WeaponKind::ALL {
        let open = state.is_weapon_unlocked(w);
        if open {
            lines.push(Line::from(Span::styled(
                format!("  {} {} 解放済", w.glyph(), w.label()),
                Style::default().fg(weapon_color(w)),
            )));
        } else {
            lines.push(Line::from(Span::styled(
                format!("  ？ {}  第{}層", w.label(), w.unlock_layer()),
                Style::default().fg(Color::DarkGray),
            )));
        }
    }
    let p = Paragraph::new(lines).block(
        Block::default()
            .borders(borders)
            .border_style(Style::default().fg(Color::Yellow))
            .title(" 図鑑 "),
    );
    f.render_widget(p, area);
}

fn ore_color(kind: OreKind) -> Color {
    match kind {
        OreKind::Dust => Color::Gray,
        OreKind::Rock => Color::Yellow,
        OreKind::Crystal => Color::LightCyan,
        OreKind::Wisp => Color::White,
        OreKind::Prism => Color::LightMagenta,
        OreKind::Shell => Color::DarkGray,
        OreKind::Splitter => Color::LightYellow,
        OreKind::Nova => Color::LightRed,
    }
}

fn render_stage(
    state: &StarRingState,
    f: &mut Frame,
    area: Rect,
    borders: Borders,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let shake_x = if state.shake_ticks > 0 {
        (((state.elapsed_ticks % 4) as f64) - 1.5) * 0.4
    } else {
        0.0
    };
    let shake_y = if state.shake_ticks > 0 {
        ((((state.elapsed_ticks / 2) % 3) as f64) - 1.0) * 0.3
    } else {
        0.0
    };

    let layer = state.layer();
    let ring_r = state.ring_radius();
    let mut orbit_pts = Vec::new();
    for i in 0..48 {
        let a = i as f64 * std::f64::consts::TAU / 48.0;
        orbit_pts.push((
            CX + a.cos() * ring_r + shake_x,
            CY + a.sin() * ring_r * ORBIT_Y_SQUASH + shake_y,
        ));
    }

    let core_scale = if state.layer_flash_ticks > 0 {
        1.55
    } else if state.layer_ready_flash_ticks > 0 {
        1.30
    } else if state.core_flash_ticks > 0 {
        1.25
    } else if can_unlock_next_layer(state) && state.elapsed_ticks % 20 < 10 {
        1.12
    } else {
        1.0 + (layer.saturating_sub(1) as f64) * 0.04
    };
    let core_pts = canvas_fx::filled_ellipse_points(
        CX + shake_x,
        CY + shake_y,
        2.6 * core_scale,
        2.2 * core_scale,
        0.45,
    );
    let core_ring = canvas_fx::ring_points(CX + shake_x, CY + shake_y, 4.2 * core_scale, 0.28);

    // 層の外縁リング (場面転換の視覚アンカー)
    let mut layer_ring = Vec::new();
    let lr = SPAWN_RING_VISUAL + layer as f64 * 0.8;
    for i in 0..36 {
        let a = i as f64 * std::f64::consts::TAU / 36.0;
        layer_ring.push((
            CX + a.cos() * lr + shake_x,
            CY + a.sin() * lr * ORBIT_Y_SQUASH.max(0.5) + shake_y,
        ));
    }

    let turrets = turret_positions(state);
    let mut gun_near = Vec::new();
    let mut gun_far = Vec::new();
    for &(gx, gy, depth) in &turrets {
        let size = if depth > 0.0 { 1.35 } else { 0.85 };
        let pts = canvas_fx::filled_ellipse_points(gx + shake_x, gy + shake_y, size, size * 0.85, 0.4);
        if depth >= 0.0 {
            gun_near.extend(pts);
        } else {
            gun_far.extend(pts);
        }
    }

    let mut ore_groups: Vec<(Vec<(f64, f64)>, Color)> = Vec::new();
    let mut approach_trails: Vec<(f64, f64, f64, f64, Color)> = Vec::new();
    for ore in &state.ores {
        let color = ore_color(ore.kind);
        let pts = canvas_fx::filled_ellipse_points(
            ore.x + shake_x,
            ore.y + shake_y,
            ore.radius,
            ore.radius * 0.85,
            0.65,
        );
        if let Some(g) = ore_groups.iter_mut().find(|(_, c)| *c == color) {
            g.0.extend(pts);
        } else {
            ore_groups.push((pts, color));
        }
        let speed = ore.vx.hypot(ore.vy).max(0.01);
        let trail = ore.radius * 2.8 + speed * 3.0;
        approach_trails.push((
            ore.x + shake_x,
            ore.y + shake_y,
            ore.x - ore.vx / speed * trail + shake_x,
            ore.y - ore.vy / speed * trail + shake_y,
            color,
        ));
    }

    // 飛翔弾を武器色で描画
    let mut proj_groups: Vec<(Vec<(f64, f64)>, Color)> = Vec::new();
    let mut proj_trails: Vec<(f64, f64, f64, f64, Color)> = Vec::new();
    for p in &state.projectiles {
        let color = weapon_color(p.kind);
        let pts = canvas_fx::filled_ellipse_points(
            p.x + shake_x,
            p.y + shake_y,
            p.radius.max(0.4),
            p.radius.max(0.4) * 0.8,
            0.5,
        );
        if let Some(g) = proj_groups.iter_mut().find(|(_, c)| *c == color) {
            g.0.extend(pts);
        } else {
            proj_groups.push((pts, color));
        }
        let speed = p.vx.hypot(p.vy).max(0.01);
        let len = match p.kind {
            WeaponKind::Ray => 4.5,
            WeaponKind::Pulse => 1.8,
            _ => 2.4,
        };
        proj_trails.push((
            p.x + shake_x,
            p.y + shake_y,
            p.x - p.vx / speed * len + shake_x,
            p.y - p.vy / speed * len + shake_y,
            color,
        ));
    }

    let mut sparks = Vec::new();
    let mut dust = Vec::new();
    let mut shards = Vec::new();
    let mut embers = Vec::new();
    for p in &state.particles {
        let pt = (p.x + shake_x, p.y + shake_y);
        match p.kind {
            ParticleKind::Spark => sparks.push(pt),
            ParticleKind::Dust => dust.push(pt),
            ParticleKind::Shard => shards.push(pt),
            ParticleKind::Ember => embers.push(pt),
        }
    }

    // 核脈動の波紋
    let mut pulse_ring_pts: Vec<(f64, f64)> = Vec::new();
    for ring in &state.pulse_rings {
        let alpha = ring.life as f64 / ring.max_life.max(1) as f64;
        let step = if alpha > 0.5 { 20 } else { 14 };
        for i in 0..step {
            let a = i as f64 * std::f64::consts::TAU / step as f64;
            pulse_ring_pts.push((
                CX + a.cos() * ring.radius + shake_x,
                CY + a.sin() * ring.radius * ORBIT_Y_SQUASH.max(0.55) + shake_y,
            ));
        }
    }

    let star_count = 16 + layer as usize * 4;
    let mut stars = Vec::new();
    for i in 0..star_count {
        let seed = i as f64 * 7.13 + (state.elapsed_ticks as f64 * 0.01);
        let x = ((seed * 11.0) % WORLD_W).abs();
        let y = ((seed * 3.7 + state.elapsed_ticks as f64 * 0.02) % WORLD_H).abs();
        stars.push((x, y));
    }

    let core_color = if state.layer_flash_ticks > 0 {
        layer_color(layer)
    } else if state.layer_ready_flash_ticks > 0 || can_unlock_next_layer(state) {
        Color::LightMagenta
    } else if state.boost_ticks > 0 {
        Color::LightYellow
    } else {
        Color::Yellow
    };
    let star_color = match layer {
        1 => Color::DarkGray,
        2 => Color::Indexed(240),
        3 => Color::Indexed(81),
        4 => Color::Indexed(177),
        _ => Color::Indexed(210),
    };
    let layer_ring_color = layer_color(layer);

    let title = if state.layer_flash_ticks > 0 {
        format!(" ◆開放 第{}層 {} ", layer, Layer::title(layer))
    } else if can_unlock_next_layer(state) {
        format!(
            " 次層開放可「{}」[!]",
            Layer::title(layer + 1)
        )
    } else if state.layer_ready_flash_ticks > 0 {
        " 撃破条件達成 — 星屑で開放 ".to_string()
    } else {
        format!(" 情景 砲×{} ", state.turret_count())
    };

    let canvas = Canvas::default()
        .x_bounds([0.0, WORLD_W])
        .y_bounds([0.0, WORLD_H])
        .marker(Marker::Braille)
        .paint(move |ctx| {
            if !stars.is_empty() {
                ctx.draw(&Points {
                    coords: &stars,
                    color: star_color,
                });
            }
            if !layer_ring.is_empty() {
                ctx.draw(&Points {
                    coords: &layer_ring,
                    color: layer_ring_color,
                });
            }
            if !orbit_pts.is_empty() {
                ctx.draw(&Points {
                    coords: &orbit_pts,
                    color: Color::Indexed(240),
                });
            }
            for &(x1, y1, x2, y2, color) in &proj_trails {
                ctx.draw(&CanvasLine {
                    x1,
                    y1,
                    x2,
                    y2,
                    color,
                });
            }
            for (pts, color) in &proj_groups {
                if !pts.is_empty() {
                    ctx.draw(&Points {
                        coords: pts,
                        color: *color,
                    });
                }
            }
            if !gun_far.is_empty() {
                ctx.draw(&Points {
                    coords: &gun_far,
                    color: Color::Gray,
                });
            }
            for &(x1, y1, x2, y2, color) in &approach_trails {
                ctx.draw(&CanvasLine {
                    x1,
                    y1,
                    x2,
                    y2,
                    color,
                });
            }
            for (pts, color) in &ore_groups {
                if !pts.is_empty() {
                    ctx.draw(&Points {
                        coords: pts,
                        color: *color,
                    });
                }
            }
            if !pulse_ring_pts.is_empty() {
                ctx.draw(&Points {
                    coords: &pulse_ring_pts,
                    color: Color::LightCyan,
                });
            }
            if !core_ring.is_empty() {
                ctx.draw(&Points {
                    coords: &core_ring,
                    color: Color::DarkGray,
                });
            }
            if !core_pts.is_empty() {
                ctx.draw(&Points {
                    coords: &core_pts,
                    color: core_color,
                });
            }
            if !gun_near.is_empty() {
                ctx.draw(&Points {
                    coords: &gun_near,
                    color: Color::White,
                });
            }
            if !dust.is_empty() {
                ctx.draw(&Points {
                    coords: &dust,
                    color: Color::Gray,
                });
            }
            if !shards.is_empty() {
                ctx.draw(&Points {
                    coords: &shards,
                    color: Color::LightMagenta,
                });
            }
            if !embers.is_empty() {
                ctx.draw(&Points {
                    coords: &embers,
                    color: Color::LightRed,
                });
            }
            if !sparks.is_empty() {
                ctx.draw(&Points {
                    coords: &sparks,
                    color: Color::White,
                });
            }
        })
        .block(
            Block::default()
                .borders(borders)
                .border_style(Style::default().fg(layer_color(layer)))
                .title(Span::styled(title, Style::default().fg(Color::Yellow))),
        );

    Clickable::new(canvas, TAP_STRIKE).render(f, area, &mut click_state.borrow_mut());
}

const SPAWN_RING_VISUAL: f64 = 30.0;

fn render_footer(state: &StarRingState, f: &mut Frame, area: Rect, borders: Borders) {
    let hint = match state.tab {
        Tab::Armory => "[◀▶]武装  [A/S/D]弾数/連射/威力  情景タップでブースト  [Q]戻る",
        Tab::Ring => "[!]次層開放  [1-2]収率/核脈動  [Q]戻る",
        Tab::Codex => "図鑑: 層開放で鉱石と武装が増える  [Q]戻る",
    };
    let p = Paragraph::new(Line::from(Span::styled(
        hint,
        Style::default().fg(Color::DarkGray),
    )))
    .block(
        Block::default()
            .borders(borders)
            .border_style(Style::default().fg(Color::DarkGray)),
    );
    f.render_widget(p, area);
}
