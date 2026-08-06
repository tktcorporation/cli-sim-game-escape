//! 破壊VFXラボの描画。Everlight と同じ Canvas+Braille を使う。

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

use super::actions::tab_for_style;
use super::state::{
    DemoStyle, ParticleKind, Scene, ShatterLabState, WORLD_H, WORLD_W,
};

const CX: f64 = WORLD_W * 0.5;
const GROUND_Y: f64 = 12.0;

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
            Constraint::Min(8),
            Constraint::Length(2),
        ])
        .split(area);

    render_header(state, f, chunks[0], borders);
    render_tabs(state, f, chunks[1], borders, click_state);
    render_stage(state, f, chunks[2], borders);
    render_footer(state, f, chunks[3], borders);
}

fn render_header(state: &ShatterLabState, f: &mut Frame, area: Rect, borders: Borders) {
    let title = format!("破壊VFXラボ — {}", state.style.label());
    let p = Paragraph::new(Line::from(vec![
        Span::styled(
            title,
            Style::default()
                .fg(Color::LightYellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(state.style.blurb(), Style::default().fg(Color::Gray)),
    ]))
    .block(Block::default().borders(borders).title(" 試作比較 "));
    f.render_widget(p, area);
}

fn render_tabs(
    state: &ShatterLabState,
    f: &mut Frame,
    area: Rect,
    borders: Borders,
    click_state: &Rc<RefCell<ClickState>>,
) {
    let mut cs = click_state.borrow_mut();
    let mut bar = TabBar::new("│").block(Block::default().borders(borders));
    for style in DemoStyle::ALL {
        let selected = style == state.style;
        let style_fg = if selected {
            Style::default()
                .fg(Color::Black)
                .bg(Color::LightYellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        };
        bar = bar.tab(style.label(), style_fg, tab_for_style(style));
    }
    bar.render(f, area, &mut cs);
}

fn render_footer(state: &ShatterLabState, f: &mut Frame, area: Rect, borders: Borders) {
    let hint = "[1]鉱石 [2]油圧 [3]惑星 [4]都市  /  自動ループ再生中";
    let shake = if state.shake_ticks > 0 { "  ※衝撃" } else { "" };
    let p = Paragraph::new(Line::from(Span::styled(
        format!("{hint}{shake}"),
        Style::default().fg(Color::DarkGray),
    )))
    .block(Block::default().borders(borders));
    f.render_widget(p, area);
}

fn render_stage(state: &ShatterLabState, f: &mut Frame, area: Rect, borders: Borders) {
    let shake_x = if state.shake_ticks > 0 {
        (((state.elapsed_ticks % 4) as f64) - 1.5) * 0.6
    } else {
        0.0
    };
    let shake_y = if state.shake_ticks > 0 {
        ((((state.elapsed_ticks / 2) % 3) as f64) - 1.0) * 0.4
    } else {
        0.0
    };

    // 静物・動体を先に組み立てて paint に move
    let ground = canvas_fx::filled_rect_points(4.0, 2.0, WORLD_W - 4.0, GROUND_Y - 2.0, 1.2);
    let (solids, accents, rings, lines) = build_scene_geometry(state);

    let mut debris = Vec::new();
    let mut sparks = Vec::new();
    let mut dust = Vec::new();
    let mut embers = Vec::new();
    let mut shards = Vec::new();
    for p in &state.particles {
        let pt = (p.x + shake_x, p.y + shake_y);
        match p.kind {
            ParticleKind::Debris => debris.push(pt),
            ParticleKind::Spark => sparks.push(pt),
            ParticleKind::Dust => dust.push(pt),
            ParticleKind::Ember => embers.push(pt),
            ParticleKind::Shard => shards.push(pt),
        }
    }

    let canvas = Canvas::default()
        .x_bounds([0.0, WORLD_W])
        .y_bounds([0.0, WORLD_H])
        .marker(Marker::Braille)
        .paint(move |ctx| {
            if !ground.is_empty() {
                ctx.draw(&Points {
                    coords: &ground,
                    color: Color::DarkGray,
                });
            }
            for (pts, color) in &solids {
                if !pts.is_empty() {
                    ctx.draw(&Points {
                        coords: pts,
                        color: *color,
                    });
                }
            }
            for (pts, color) in &accents {
                if !pts.is_empty() {
                    ctx.draw(&Points {
                        coords: pts,
                        color: *color,
                    });
                }
            }
            for (pts, color) in &rings {
                if !pts.is_empty() {
                    ctx.draw(&Points {
                        coords: pts,
                        color: *color,
                    });
                }
            }
            for &(x1, y1, x2, y2, color) in &lines {
                ctx.draw(&CanvasLine {
                    x1: x1 + shake_x,
                    y1: y1 + shake_y,
                    x2: x2 + shake_x,
                    y2: y2 + shake_y,
                    color,
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
                    color: Color::LightCyan,
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
                    " 情景 ",
                    Style::default().fg(Color::LightYellow),
                )),
        );
    f.render_widget(canvas, area);
}

type Geom = (
    Vec<(Vec<(f64, f64)>, Color)>,
    Vec<(Vec<(f64, f64)>, Color)>,
    Vec<(Vec<(f64, f64)>, Color)>,
    Vec<(f64, f64, f64, f64, Color)>,
);

fn build_scene_geometry(state: &ShatterLabState) -> Geom {
    match &state.scene {
        Scene::OreBomb {
            phase,
            bomb_y,
            rock_hp_frac,
            ..
        } => geom_ore_bomb(*phase, *bomb_y, *rock_hp_frac, state.elapsed_ticks),
        Scene::PressCrush {
            press_y,
            rock_squash,
            phase,
            ..
        } => geom_press(*phase, *press_y, *rock_squash),
        Scene::PlanetPeel {
            layers_left,
            crack,
            ..
        } => geom_planet(*layers_left, *crack, state.elapsed_ticks),
        Scene::CityCollapse {
            floors_left,
            falling_y,
            ..
        } => geom_city(*floors_left, *falling_y),
    }
}

fn geom_ore_bomb(phase: u8, bomb_y: f64, rock_hp: f64, ticks: u64) -> Geom {
    let mut solids = Vec::new();
    let mut accents = Vec::new();
    let mut rings = Vec::new();
    let lines = Vec::new();

    if rock_hp > 0.05 {
        let wobble = if phase == 0 {
            ((ticks as f64) * 0.35).sin() * 0.4
        } else {
            0.0
        };
        let rx = 9.0 * rock_hp.sqrt();
        let ry = 7.0 * rock_hp.sqrt();
        solids.push((
            canvas_fx::filled_ellipse_points(CX + wobble, GROUND_Y + 8.0, rx, ry, 0.7),
            Color::Magenta,
        ));
        accents.push((
            canvas_fx::filled_ellipse_points(CX + wobble - 2.0, GROUND_Y + 10.0, 2.2, 1.6, 0.5),
            Color::LightMagenta,
        ));
    }

    if phase == 1 {
        accents.push((
            canvas_fx::filled_ellipse_points(CX, bomb_y, 2.0, 2.4, 0.5),
            Color::Red,
        ));
        // 導火線っぽい点
        accents.push((
            canvas_fx::filled_ellipse_points(CX, bomb_y + 3.2, 0.8, 0.8, 0.4),
            Color::Yellow,
        ));
    }

    if phase >= 2 {
        let t = (ticks % 20) as f64 / 20.0;
        let r = 3.0 + t * 14.0;
        rings.push((
            canvas_fx::ring_points(CX, GROUND_Y + 10.0, r, 0.25),
            Color::LightYellow,
        ));
        rings.push((
            canvas_fx::ring_points(CX, GROUND_Y + 10.0, r * 0.55, 0.35),
            Color::Red,
        ));
    }

    (solids, accents, rings, lines)
}

fn geom_press(phase: u8, press_y: f64, squash: f64) -> Geom {
    let mut solids = Vec::new();
    let mut accents = Vec::new();
    let rings = Vec::new();
    let mut lines = Vec::new();

    // 台座
    solids.push((
        canvas_fx::filled_rect_points(CX - 14.0, GROUND_Y, CX + 14.0, GROUND_Y + 3.0, 0.8),
        Color::DarkGray,
    ));

    // 岩 (潰れる)
    let rock_h = 10.0 * (1.0 - squash * 0.75);
    let rock_w = 8.0 + squash * 10.0;
    if phase < 3 || squash < 1.0 {
        solids.push((
            canvas_fx::filled_ellipse_points(CX, GROUND_Y + 3.0 + rock_h * 0.45, rock_w * 0.5, rock_h * 0.45, 0.65),
            Color::Cyan,
        ));
    }

    // プレス板
    solids.push((
        canvas_fx::filled_rect_points(CX - 12.0, press_y, CX + 12.0, press_y + 3.5, 0.7),
        Color::Gray,
    ));
    accents.push((
        canvas_fx::filled_rect_points(CX - 2.0, press_y + 3.5, CX + 2.0, WORLD_H - 4.0, 1.0),
        Color::DarkGray,
    ));

    // ガイド柱
    lines.push((CX - 13.0, GROUND_Y + 3.0, CX - 13.0, WORLD_H - 6.0, Color::DarkGray));
    lines.push((CX + 13.0, GROUND_Y + 3.0, CX + 13.0, WORLD_H - 6.0, Color::DarkGray));

    (solids, accents, rings, lines)
}

fn geom_planet(layers_left: u8, crack: f64, ticks: u64) -> Geom {
    let mut solids = Vec::new();
    let mut accents = Vec::new();
    let mut rings = Vec::new();
    let mut lines = Vec::new();
    let cy = WORLD_H * 0.48;

    if layers_left >= 3 {
        solids.push((
            canvas_fx::filled_ellipse_points(CX, cy, 14.0, 14.0, 0.85),
            Color::Green,
        ));
    }
    if layers_left >= 2 {
        solids.push((
            canvas_fx::filled_ellipse_points(CX, cy, 10.0, 10.0, 0.75),
            Color::Yellow,
        ));
    }
    if layers_left >= 1 {
        solids.push((
            canvas_fx::filled_ellipse_points(CX, cy, 6.0, 6.0, 0.65),
            Color::LightRed,
        ));
        accents.push((
            canvas_fx::filled_ellipse_points(CX, cy, 2.2, 2.2, 0.45),
            Color::White,
        ));
    }

    if layers_left > 0 && crack > 0.0 {
        let r = match layers_left {
            3 => 14.0,
            2 => 10.0,
            _ => 6.0,
        };
        // 亀裂ラジアル
        for i in 0..5 {
            let a = (i as f64) * std::f64::consts::TAU / 5.0 + ticks as f64 * 0.01;
            let len = r * (0.35 + crack * 0.65);
            lines.push((
                CX + a.cos() * (r * 0.2),
                cy + a.sin() * (r * 0.2),
                CX + a.cos() * len,
                cy + a.sin() * len,
                Color::White,
            ));
        }
        rings.push((
            canvas_fx::ring_points(CX, cy, r * (0.7 + crack * 0.35), 0.28),
            Color::LightYellow,
        ));
    }

    if layers_left == 0 {
        let t = ((ticks % 24) as f64) / 24.0;
        rings.push((
            canvas_fx::ring_points(CX, cy, 4.0 + t * 22.0, 0.22),
            Color::LightYellow,
        ));
        rings.push((
            canvas_fx::ring_points(CX, cy, 2.0 + t * 12.0, 0.3),
            Color::LightRed,
        ));
    }

    (solids, accents, rings, lines)
}

fn geom_city(floors_left: u8, falling_y: f64) -> Geom {
    let mut solids = Vec::new();
    let accents = Vec::new();
    let rings = Vec::new();
    let lines = Vec::new();
    let floor_h = 7.0;
    let width = 18.0;

    for i in 0..floors_left {
        let y0 = GROUND_Y + i as f64 * floor_h;
        let y1 = y0 + floor_h - 0.8;
        let is_top = i + 1 == floors_left;
        let y_off = if is_top { falling_y } else { 0.0 };
        let color = if i % 2 == 0 {
            Color::LightCyan
        } else {
            Color::Cyan
        };
        solids.push((
            canvas_fx::filled_rect_points(
                CX - width * 0.5,
                y0 - y_off,
                CX + width * 0.5,
                y1 - y_off,
                0.85,
            ),
            color,
        ));
        // 窓の点
        let mut windows = Vec::new();
        for wx in 0..4 {
            for wy in 0..2 {
                let x = CX - 6.0 + wx as f64 * 4.0;
                let y = y0 + 1.5 + wy as f64 * 2.5 - y_off;
                windows.extend(canvas_fx::filled_ellipse_points(x, y, 0.7, 0.7, 0.4));
            }
        }
        solids.push((windows, Color::Yellow));
    }

    (solids, accents, rings, lines)
}
