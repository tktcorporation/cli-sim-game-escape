//! 破壊VFXラボの描画。威力Lvで船体・砲門・デブリ密度が変わる。

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
use crate::widgets::TabBar;

use super::actions::{
    tab_for_power, tab_for_style, TAB_POWER_AUTO, TAB_POWER_HIGH, TAB_POWER_LOW, TAB_POWER_MID,
};
use super::state::{
    DemoStyle, ParticleKind, PowerLevel, ShatterLabState, WORLD_H, WORLD_W,
};

const CX: f64 = WORLD_W * 0.5;

pub fn render(
    state: &ShatterLabState,
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
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(2),
        ])
        .split(area);

    render_header(state, f, chunks[0], borders);
    render_style_tabs(state, f, chunks[1], borders, click_state);
    render_power_tabs(state, f, chunks[2], borders, click_state);
    render_stage(state, f, chunks[3], borders);
    render_footer(state, f, chunks[4], borders);
}

fn render_header(state: &ShatterLabState, f: &mut Frame, area: Rect, borders: Borders) {
    let power_tag = if state.auto_power {
        format!("AUTO→{}", state.power.label())
    } else {
        state.power.label().to_string()
    };
    let title = format!(
        "破壊VFXラボ — {} [{}]",
        state.style.label(),
        power_tag
    );
    let p = Paragraph::new(Line::from(vec![
        Span::styled(
            title,
            Style::default()
                .fg(Color::LightYellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(state.style.blurb(), Style::default().fg(Color::Gray)),
        Span::raw("  "),
        Span::styled(
            format!("撃破 {}", state.cleared),
            Style::default().fg(Color::Cyan),
        ),
    ]))
    .block(Block::default().borders(borders).title(" 強化しがい比較 "));
    f.render_widget(p, area);
}

fn render_style_tabs(
    state: &ShatterLabState,
    f: &mut Frame,
    area: Rect,
    borders: Borders,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let mut cs = click_state.borrow_mut();
    let mut bar = TabBar::new("│").block(Block::default().borders(borders).title(" 舞台 "));
    for style in DemoStyle::ALL {
        let selected = style == state.style;
        let st = if selected {
            Style::default()
                .fg(Color::Black)
                .bg(Color::LightYellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        };
        bar = bar.tab(style.label(), st, tab_for_style(style));
    }
    bar.render(f, area, &mut cs);
}

fn render_power_tabs(
    state: &ShatterLabState,
    f: &mut Frame,
    area: Rect,
    borders: Borders,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let mut cs = click_state.borrow_mut();
    let mut bar = TabBar::new("│").block(
        Block::default()
            .borders(borders)
            .title(" 威力 (多い/速い/大きい/種類) "),
    );
    for power in PowerLevel::ALL {
        let selected = !state.auto_power && power == state.power;
        let st = if selected {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        };
        let label = match power {
            PowerLevel::Low => "弱:砲1",
            PowerLevel::Mid => "中:砲3",
            PowerLevel::High => "強:砲6",
        };
        bar = bar.tab(label, st, tab_for_power(power));
    }
    let auto_st = if state.auto_power {
        Style::default()
            .fg(Color::Black)
            .bg(Color::LightGreen)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Gray)
    };
    bar = bar.tab("自動強化", auto_st, TAB_POWER_AUTO);
    // silence unused import warnings for direct constants used via tab_for_power
    let _ = (TAB_POWER_LOW, TAB_POWER_MID, TAB_POWER_HIGH);
    bar.render(f, area, &mut cs);
}

fn render_footer(state: &ShatterLabState, f: &mut Frame, area: Rect, borders: Borders) {
    let hint = "[1-4]舞台  [Q/W/E]弱中強  [A]自動強化ループ";
    let shake = if state.shake_ticks > 0 { "  ※衝撃" } else { "" };
    let p = Paragraph::new(Line::from(Span::styled(
        format!("{hint}{shake}"),
        Style::default().fg(Color::DarkGray),
    )))
    .block(Block::default().borders(borders));
    f.render_widget(p, area);
}

pub fn render_stage(state: &ShatterLabState, f: &mut Frame, area: Rect, borders: Borders) {
    let shake_x = if state.shake_ticks > 0 {
        (((state.elapsed_ticks % 4) as f64) - 1.5) * 0.55
    } else {
        0.0
    };
    let shake_y = if state.shake_ticks > 0 {
        ((((state.elapsed_ticks / 2) % 3) as f64) - 1.0) * 0.35
    } else {
        0.0
    };

    let (ship_pts, gun_pts, star_pts) = build_ship_and_bg(state);
    let target_groups = build_targets(state);
    let beam_lines: Vec<(f64, f64, f64, f64)> = state
        .beams
        .iter()
        .map(|b| (b.x0 + shake_x, b.y0 + shake_y, b.x1 + shake_x, b.y1 + shake_y))
        .collect();

    let mut debris = Vec::new();
    let mut sparks = Vec::new();
    let mut dust = Vec::new();
    let mut embers = Vec::new();
    let mut shards = Vec::new();
    let mut beams = Vec::new();
    for p in &state.particles {
        let pt = (p.x + shake_x, p.y + shake_y);
        match p.kind {
            ParticleKind::Debris => debris.push(pt),
            ParticleKind::Spark => sparks.push(pt),
            ParticleKind::Dust => dust.push(pt),
            ParticleKind::Ember => embers.push(pt),
            ParticleKind::Shard => shards.push(pt),
            ParticleKind::Beam => beams.push(pt),
        }
    }

    let canvas = Canvas::default()
        .x_bounds([0.0, WORLD_W])
        .y_bounds([0.0, WORLD_H])
        .marker(Marker::Braille)
        .paint(move |ctx| {
            if !star_pts.is_empty() {
                ctx.draw(&Points {
                    coords: &star_pts,
                    color: Color::DarkGray,
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
            if !beams.is_empty() {
                ctx.draw(&Points {
                    coords: &beams,
                    color: Color::Cyan,
                });
            }
            for (pts, color) in &target_groups {
                if !pts.is_empty() {
                    ctx.draw(&Points {
                        coords: pts,
                        color: *color,
                    });
                }
            }
            if !ship_pts.is_empty() {
                ctx.draw(&Points {
                    coords: &ship_pts,
                    color: Color::LightYellow,
                });
            }
            if !gun_pts.is_empty() {
                ctx.draw(&Points {
                    coords: &gun_pts,
                    color: Color::White,
                });
            }
            if !dust.is_empty() {
                ctx.draw(&Points {
                    coords: &dust,
                    color: Color::Gray,
                });
            }
            if !debris.is_empty() {
                ctx.draw(&Points {
                    coords: &debris,
                    color: Color::Yellow,
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
                    format!(" 情景 Lv.{} ", state.power.label()),
                    Style::default().fg(Color::LightYellow),
                )),
        );
    f.render_widget(canvas, area);
}

fn variety_color(v: u8) -> Color {
    match v % 5 {
        0 => Color::Gray,
        1 => Color::Yellow,
        2 => Color::LightRed,
        3 => Color::LightCyan,
        _ => Color::LightMagenta,
    }
}

fn build_targets(state: &ShatterLabState) -> Vec<(Vec<(f64, f64)>, Color)> {
    let mut groups: Vec<(Vec<(f64, f64)>, Color)> = Vec::new();
    for t in &state.targets {
        let color = variety_color(t.variety);
        let pts = canvas_fx::filled_ellipse_points(t.x, t.y, t.radius, t.radius * 0.85, 0.7);
        if let Some(g) = groups.iter_mut().find(|(_, c)| *c == color) {
            g.0.extend(pts);
        } else {
            groups.push((pts, color));
        }
    }
    groups
}

fn build_ship_and_bg(state: &ShatterLabState) -> (Vec<(f64, f64)>, Vec<(f64, f64)>, Vec<(f64, f64)>) {
    let scale = state.power.ship_scale();
    let mut stars = Vec::new();
    // スクロールする星 / レール
    for i in 0..24 {
        let seed = i as f64 * 7.13;
        let x = ((seed * 11.0) % WORLD_W).abs();
        let y = (seed * 3.0 + state.scroll * 2.0) % WORLD_H;
        stars.extend(canvas_fx::filled_ellipse_points(x, y, 0.35, 0.35, 0.3));
    }

    let (mut ship, mut guns) = match state.style {
        DemoStyle::SpaceCruise => {
            let sy = 14.0;
            let body = canvas_fx::filled_ellipse_points(CX, sy, 5.0 * scale, 3.5 * scale, 0.55);
            let nose = canvas_fx::filled_ellipse_points(CX, sy + 5.0 * scale, 2.2 * scale, 2.8 * scale, 0.5);
            let mut s = body;
            s.extend(nose);
            // エンジン噴射
            s.extend(canvas_fx::filled_ellipse_points(
                CX,
                sy - 4.0 * scale,
                1.5 * scale,
                2.0 * scale,
                0.45,
            ));
            let g = gun_points_for(state);
            (s, g)
        }
        DemoStyle::OrbitMine => {
            let sx = 12.0;
            let sy = WORLD_H * 0.5;
            let body = canvas_fx::filled_rect_points(
                sx - 4.0 * scale,
                sy - 6.0 * scale,
                sx + 3.0 * scale,
                sy + 6.0 * scale,
                0.7,
            );
            let dish = canvas_fx::ring_points(sx + 2.0 * scale, sy, 4.0 * scale, 0.35);
            let mut s = body;
            s.extend(dish);
            (s, gun_points_for(state))
        }
        DemoStyle::RailBreak => {
            let sy = 16.0;
            let body = canvas_fx::filled_rect_points(
                CX - 4.0 * scale,
                sy,
                CX + 4.0 * scale,
                sy + 14.0 * scale,
                0.65,
            );
            let nose = canvas_fx::filled_ellipse_points(CX, sy + 16.0 * scale, 3.5 * scale, 3.0 * scale, 0.55);
            // レール
            for i in 0..8 {
                let y = (i as f64 * 10.0 + state.scroll * 1.5) % WORLD_H;
                stars.extend(canvas_fx::filled_rect_points(CX - 12.0, y, CX - 11.0, y + 4.0, 0.8));
                stars.extend(canvas_fx::filled_rect_points(CX + 11.0, y, CX + 12.0, y + 4.0, 0.8));
            }
            let mut s = body;
            s.extend(nose);
            (s, gun_points_for(state))
        }
        DemoStyle::SatDefense => {
            // 拠点コア
            let core = canvas_fx::filled_ellipse_points(CX, 22.0, 3.0 * scale, 3.0 * scale, 0.5);
            let ring = canvas_fx::ring_points(CX, 22.0, 6.0 * scale, 0.25);
            let mut s = core;
            s.extend(ring);
            // 地面
            stars.extend(canvas_fx::filled_rect_points(4.0, 2.0, WORLD_W - 4.0, 8.0, 1.0));
            (s, gun_points_for(state))
        }
    };

    // 砲門を大きめの点で
    let gpts = gun_points_for(state);
    for &(gx, gy) in &gpts {
        guns.extend(canvas_fx::filled_ellipse_points(gx, gy, 1.1, 1.1, 0.45));
    }
    let _ = &mut ship;
    (ship, guns, stars)
}

fn gun_points_for(state: &ShatterLabState) -> Vec<(f64, f64)> {
    let scale = state.power.ship_scale();
    let n = state.power.gun_count();
    match state.style {
        DemoStyle::SpaceCruise => {
            let sy = 14.0;
            let spread = 8.0 * scale;
            (0..n)
                .map(|i| {
                    let t = if n == 1 {
                        0.5
                    } else {
                        i as f64 / (n - 1) as f64
                    };
                    (CX - spread + spread * 2.0 * t, sy + 6.0 * scale)
                })
                .collect()
        }
        DemoStyle::OrbitMine => {
            let sx = 12.0;
            let sy = WORLD_H * 0.5;
            (0..n)
                .map(|i| {
                    let t = if n == 1 {
                        0.5
                    } else {
                        i as f64 / (n - 1) as f64
                    };
                    (sx + 5.0 * scale, sy - 10.0 * scale + 20.0 * scale * t)
                })
                .collect()
        }
        DemoStyle::RailBreak => {
            let sy = 18.0;
            let mut v = vec![(CX, sy + 10.0 * scale)];
            for i in 1..n {
                let side = if i % 2 == 0 { -1.0 } else { 1.0 };
                v.push((CX + side * (3.0 + i as f64), sy + 4.0 * scale));
            }
            v
        }
        DemoStyle::SatDefense => {
            let orbit_r = 10.0 + scale * 6.0;
            let cy = 22.0;
            (0..n)
                .map(|i| {
                    let a = state.elapsed_ticks as f64 * 0.04
                        + i as f64 * std::f64::consts::TAU / n.max(1) as f64;
                    (CX + a.cos() * orbit_r, cy + a.sin() * orbit_r * 0.45)
                })
                .collect()
        }
    }
}
