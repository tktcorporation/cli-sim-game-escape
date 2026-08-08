//! 星環の描画。読み取り専用。クリック登録は widgets 経由のみ。

use std::cell::RefCell;
use std::rc::Rc;

use ratzilla::ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratzilla::ratatui::style::{Color, Modifier, Style};
use ratzilla::ratatui::symbols::Marker;
use ratzilla::ratatui::text::{Line, Span};
use ratzilla::ratatui::widgets::canvas::{Canvas, Line as CanvasLine, Points};
use ratzilla::ratatui::widgets::{Block, Borders, Paragraph};
use ratzilla::ratatui::Frame;

use crate::canvas_fx;
use crate::input::{is_narrow_layout, ClickState};
use crate::widgets::{Clickable, ClickableList, TabBar};

use super::actions::{buy_upgrade_id, TAB_CODEX, TAB_UPGRADES, TAP_STRIKE};
use super::logic::{can_upgrade_further, turret_positions, upgrade_cost};
use super::state::{
    OreKind, ParticleKind, StarRingState, Tab, UpgradeKind, CX, CY, ORBIT_Y_SQUASH, WORLD_H,
    WORLD_W,
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
            .constraints([Constraint::Percentage(48), Constraint::Percentage(52)])
            .split(chunks[2]);
        render_stage(state, f, body[0], borders, click_state);
        match state.tab {
            Tab::Upgrades => render_upgrades(state, f, body[1], borders, click_state),
            Tab::Codex => render_codex(state, f, body[1], borders),
        }
    } else {
        let body = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(42), Constraint::Percentage(58)])
            .split(chunks[2]);
        match state.tab {
            Tab::Upgrades => render_upgrades(state, f, body[0], borders, click_state),
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
    let boost = if state.boost_ticks > 0 {
        " ⚡火力ブースト"
    } else {
        ""
    };
    let leak = if state.core_flash_ticks > 0 {
        " ⚠漏洩"
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
            format!("✦{}", format_shards(state.shards)),
            Style::default().fg(Color::LightYellow),
        ),
        Span::raw("  "),
        Span::styled(
            format!("{:.1}/秒", sps),
            Style::default().fg(Color::Cyan),
        ),
        Span::raw("  "),
        Span::styled(
            format!("撃破 {}", state.total_kills),
            Style::default().fg(Color::LightCyan),
        ),
        Span::styled(boost, Style::default().fg(Color::LightRed)),
        Span::styled(leak, Style::default().fg(Color::Red)),
    ]))
    .block(
        Block::default()
            .borders(borders)
            .border_style(Style::default().fg(Color::Yellow))
            .title(" 軌道採掘 "),
    );
    f.render_widget(p, area);
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
        .tab("強化", sel(state.tab == Tab::Upgrades), TAB_UPGRADES)
        .tab("図鑑", sel(state.tab == Tab::Codex), TAB_CODEX)
        .render(f, area, &mut cs);
}

fn render_upgrades(
    state: &StarRingState,
    f: &mut Frame,
    area: Rect,
    borders: Borders,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let mut cl = ClickableList::new();
    cl.push(Line::from(Span::styled(
        format!(
            " 砲{}  速Lv{}  火力{:.1}  間隔{}",
            state.turret_count(),
            state.level(UpgradeKind::OrbitSpeed),
            state.damage(),
            state.fire_interval()
        ),
        Style::default().fg(Color::DarkGray),
    )));
    cl.push(Line::from(""));

    for kind in UpgradeKind::ALL {
        let lv = state.level(kind);
        let maxed = !can_upgrade_further(state, kind);
        let cost = upgrade_cost(state, kind);
        let can = !maxed && state.shards + 1e-9 >= cost;
        let key = match kind.index() {
            0 => '1',
            1 => '2',
            2 => '3',
            3 => '4',
            4 => '5',
            _ => '6',
        };
        let cost_label = if maxed {
            "MAX".to_string()
        } else {
            format_shards(cost)
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
        let line = Line::from(vec![
            Span::styled(format!("[{key}] "), Style::default().fg(Color::Yellow)),
            Span::styled(format!("{} Lv.{} ", kind.label(), lv), style),
            Span::styled(format!("✦{cost_label}"), Style::default().fg(Color::Cyan)),
            Span::styled(
                format!("  {}", kind.blurb()),
                Style::default().fg(Color::DarkGray),
            ),
        ]);
        cl.push_clickable(line, buy_upgrade_id(kind));
    }

    let block = Block::default()
        .borders(borders)
        .border_style(Style::default().fg(Color::Yellow))
        .title(" 強化 ");
    let mut cs = click_state.borrow_mut();
    cl.render(f, area, block, &mut cs, false, 0);
}

fn render_codex(state: &StarRingState, f: &mut Frame, area: Rect, borders: Borders) {
    let unlocked = state.unlocked_ore_kinds();
    let mut lines = vec![Line::from(Span::styled(
        format!(" 累計撃破 {} / 漏洩 {}", state.total_kills, state.leak_count),
        Style::default().fg(Color::DarkGray),
    ))];
    lines.push(Line::from(""));
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
                    format!("価値{} HP{}", kind.base_value(), kind.base_hp()),
                    Style::default().fg(Color::Gray),
                ),
            ]));
        } else {
            lines.push(Line::from(Span::styled(
                format!(" ？ 撃破{}で解放", kind.unlock_kills()),
                Style::default().fg(Color::DarkGray),
            )));
        }
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        format!(
            " 獲得累計 ✦{} / 漏洩損失 ✦{}",
            format_shards(state.shards_earned),
            format_shards(state.shards_leaked)
        ),
        Style::default().fg(Color::DarkGray),
    )));
    let p = Paragraph::new(lines).block(
        Block::default()
            .borders(borders)
            .border_style(Style::default().fg(Color::Yellow))
            .title(" 鉱石図鑑 "),
    );
    f.render_widget(p, area);
}

fn ore_color(kind: OreKind) -> Color {
    match kind {
        OreKind::Dust => Color::Gray,
        OreKind::Rock => Color::Yellow,
        OreKind::Crystal => Color::LightCyan,
        OreKind::Prism => Color::LightMagenta,
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

    let ring_r = state.ring_radius();
    let mut orbit_pts = Vec::new();
    for i in 0..48 {
        let a = i as f64 * std::f64::consts::TAU / 48.0;
        orbit_pts.push((
            CX + a.cos() * ring_r + shake_x,
            CY + a.sin() * ring_r * ORBIT_Y_SQUASH + shake_y,
        ));
    }

    let core_scale = if state.core_flash_ticks > 0 { 1.35 } else { 1.0 };
    let core_pts = canvas_fx::filled_ellipse_points(
        CX + shake_x,
        CY + shake_y,
        2.6 * core_scale,
        2.2 * core_scale,
        0.45,
    );
    let core_ring = canvas_fx::ring_points(CX + shake_x, CY + shake_y, 4.2 * core_scale, 0.28);

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
    // 迫ってくる感: 進行方向と逆側に短い尾を引く
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

    let beam_lines: Vec<(f64, f64, f64, f64)> = state
        .beams
        .iter()
        .map(|b| {
            (
                b.x0 + shake_x,
                b.y0 + shake_y,
                b.x1 + shake_x,
                b.y1 + shake_y,
            )
        })
        .collect();

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

    let mut stars = Vec::new();
    for i in 0..20 {
        let seed = i as f64 * 7.13 + (state.elapsed_ticks as f64 * 0.01);
        let x = ((seed * 11.0) % WORLD_W).abs();
        let y = ((seed * 3.7 + state.elapsed_ticks as f64 * 0.02) % WORLD_H).abs();
        stars.push((x, y));
    }

    let core_color = if state.core_flash_ticks > 0 {
        Color::LightRed
    } else if state.boost_ticks > 0 {
        Color::LightYellow
    } else {
        Color::Yellow
    };

    let canvas = Canvas::default()
        .x_bounds([0.0, WORLD_W])
        .y_bounds([0.0, WORLD_H])
        .marker(Marker::Braille)
        .paint(move |ctx| {
            if !stars.is_empty() {
                ctx.draw(&Points {
                    coords: &stars,
                    color: Color::DarkGray,
                });
            }
            if !orbit_pts.is_empty() {
                ctx.draw(&Points {
                    coords: &orbit_pts,
                    color: Color::Indexed(240),
                });
            }
            for &(x1, y1, x2, y2) in &beam_lines {
                ctx.draw(&CanvasLine {
                    x1,
                    y1,
                    x2,
                    y2,
                    color: Color::LightCyan,
                });
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
                .border_style(Style::default().fg(Color::DarkGray))
                .title(Span::styled(
                    format!(" 情景 砲×{} ", state.turret_count()),
                    Style::default().fg(Color::Yellow),
                )),
        );

    // 情景全体をタップ可能にして手動ブースト
    Clickable::new(canvas, TAP_STRIKE).render(f, area, &mut click_state.borrow_mut());
}

fn render_footer(state: &StarRingState, f: &mut Frame, area: Rect, borders: Borders) {
    let hint = if state.tab == Tab::Upgrades {
        "[1-6]強化  情景タップで火力ブースト  [Q]戻る"
    } else {
        "図鑑: 撃破で鉱石種が増える  [Q]戻る"
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
