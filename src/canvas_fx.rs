//! `ratatui::widgets::canvas::Canvas` で疑似ピクセル表現を作るための純粋な
//! 幾何計算ヘルパー。
//!
//! Canvas 自体が world 座標→セル内マーカーの変換を担うため、ここでは「どの
//! 座標に何を描くか」だけを返す。実際の `Canvas`/`Shape` の組み立て・色・
//! x_bounds/y_bounds・`Marker` の選択は呼び出し側 (各ゲームの render.rs)
//! の責務にする — パネルのサイズや配色はゲームごとに違うため、ここで固定
//! するとかえって使い回しにくくなる。
//!
//! # 端末ビジュアルの技術メニュー（このリポジトリ向け）
//!
//! 描画先はブラウザ DOM の `<pre>`（Ratzilla）。Sixel / Kitty graphics /
//! 別 `<canvas>` オーバーレイは使えない（クリックヒットが壊れる）。
//! 使えるのは **Unicode セル + 前景/背景色** だけ。
//!
//! | 技術 | 解像度/性質 | この repo での状態 | 向いている用途 |
//! | --- | --- | --- | --- |
//! | `Marker::Braille` | 2×4 点/セル。点描 | **主力**（戦場・情景） | 曲線・粒子・滑らかな形 |
//! | `Marker::HalfBlock` | 1×2。fg+bg 2色/セル | **未使用**（ratatui にはある） | 塗り面・グラデ帯 |
//! | `Marker::Quadrant` | 2×2。帯が出にくい | **未使用**（手動 ▖▗ は Metropolis） | ソリッドなシルエット |
//! | `Marker::Sextant` / `Octant` | 2×3 / 2×4 密充填 | **未使用・フォント要注意** | 高密度シルエット（要検証） |
//! | `Marker::Dot` / `Block` / `Bar` | 1×1 | **未使用** | 粗い HUD チャート |
//! | 手動 box-drawing / ░▒▓█ | セル単位 | Factory / Metropolis | 機械・タイル・道路 |
//! | `Color::Rgb` | 24bit | Everlight / Pachinko 等 | 連続パレット |
//! | tachyonfx (`effects`) | Buffer post-process | Abyss / RPG / Everlight | フラッシュ・シェーダ風 |
//!
//! フォントカバレッジの目視確認は
//! `dump_marker_resolution_catalog`（`--ignored --nocapture`）を使う。

/// 塗りつぶした楕円の内部座標を返す。
///
/// `step` は世界座標系でのサンプリング間隔。braille は 1 セルが 2×4 の
/// 疑似ピクセルなので、`step` が粗すぎると塗りに隙間が目立つ。
pub fn filled_ellipse_points(cx: f64, cy: f64, rx: f64, ry: f64, step: f64) -> Vec<(f64, f64)> {
    let mut points = Vec::new();
    if rx <= 0.0 || ry <= 0.0 || step <= 0.0 {
        return points;
    }
    let mut y = cy - ry;
    while y <= cy + ry {
        let mut x = cx - rx;
        while x <= cx + rx {
            let nx = (x - cx) / rx;
            let ny = (y - cy) / ry;
            if nx * nx + ny * ny <= 1.0 {
                points.push((x, y));
            }
            x += step;
        }
        y += step;
    }
    points
}

/// 数値の推移 `values` (各要素は 0.0〜1.0 に正規化済み) を、連続する線分
/// `(x1, y1, x2, y2)` の列にマッピングする。折れ線グラフを `Line` shape の
/// 並びとして描く時に使う。
///
/// `y_at_zero`/`y_at_one` で value=0/1 に対応する y 座標を指定する
/// (Canvas の y_bounds の向き次第でどちらが上か変わるため、呼び出し側に
/// 委ねる)。
pub fn history_line_segments(
    values: &[f64],
    x_start: f64,
    x_end: f64,
    y_at_zero: f64,
    y_at_one: f64,
) -> Vec<(f64, f64, f64, f64)> {
    if values.len() < 2 {
        return Vec::new();
    }
    let n = values.len();
    let point_at = |i: usize| {
        let px = x_start + (x_end - x_start) * (i as f64 / (n - 1) as f64);
        let v = values[i].clamp(0.0, 1.0);
        let py = y_at_zero + (y_at_one - y_at_zero) * v;
        (px, py)
    };
    (1..n)
        .map(|i| {
            let (px0, py0) = point_at(i - 1);
            let (px1, py1) = point_at(i);
            (px0, py0, px1, py1)
        })
        .collect()
}

/// 塗りつぶした軸並行矩形の内部座標を返す。`(x0,y0)`〜`(x1,y1)` の大小関係は
/// 呼び出し側で正規化不要 (内部でmin/maxを取る)。
pub fn filled_rect_points(x0: f64, y0: f64, x1: f64, y1: f64, step: f64) -> Vec<(f64, f64)> {
    let mut points = Vec::new();
    if step <= 0.0 {
        return points;
    }
    let (min_x, max_x) = if x0 <= x1 { (x0, x1) } else { (x1, x0) };
    let (min_y, max_y) = if y0 <= y1 { (y0, y1) } else { (y1, y0) };
    let mut y = min_y;
    while y <= max_y {
        let mut x = min_x;
        while x <= max_x {
            points.push((x, y));
            x += step;
        }
        y += step;
    }
    points
}

/// 円環 (アウトラインのみ、塗りつぶさない) の座標を、中心角 0〜2π を
/// `step_rad` 間隔でサンプリングして返す。
pub fn ring_points(cx: f64, cy: f64, radius: f64, step_rad: f64) -> Vec<(f64, f64)> {
    ellipse_ring_points(cx, cy, radius, radius, step_rad)
}

/// 楕円の輪郭。横長の液晶のように、円では幅が足りない形を描く。
pub fn ellipse_ring_points(
    cx: f64,
    cy: f64,
    rx: f64,
    ry: f64,
    step_rad: f64,
) -> Vec<(f64, f64)> {
    let mut points = Vec::new();
    if rx <= 0.0 || ry <= 0.0 || step_rad <= 0.0 {
        return points;
    }
    let mut angle = 0.0;
    while angle < std::f64::consts::TAU {
        let (sin, cos) = angle.sin_cos();
        points.push((cx + cos * rx, cy + sin * ry));
        angle += step_rad;
    }
    points
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filled_ellipse_points_stays_within_radius() {
        let pts = filled_ellipse_points(0.0, 0.0, 5.0, 3.0, 0.5);
        assert!(!pts.is_empty());
        for (x, y) in pts {
            let nx = x / 5.0;
            let ny = y / 3.0;
            assert!(nx * nx + ny * ny <= 1.0 + 1e-9);
        }
    }

    #[test]
    fn filled_ellipse_points_empty_for_non_positive_radius() {
        assert!(filled_ellipse_points(0.0, 0.0, 0.0, 3.0, 0.5).is_empty());
        assert!(filled_ellipse_points(0.0, 0.0, 5.0, -1.0, 0.5).is_empty());
    }

    #[test]
    fn history_line_segments_connects_every_consecutive_pair() {
        let values = [0.0, 0.5, 1.0, 0.25];
        let segs = history_line_segments(&values, 0.0, 30.0, 0.0, 10.0);
        assert_eq!(segs.len(), values.len() - 1);
        // 最初の点は x_start・value=0 (=y_at_zero)、最後は x_end・value=0.25。
        assert_eq!((segs[0].0, segs[0].1), (0.0, 0.0));
        let (last_x2, last_y2) = (segs.last().unwrap().2, segs.last().unwrap().3);
        assert_eq!(last_x2, 30.0);
        assert!((last_y2 - 2.5).abs() < 1e-9);
    }

    #[test]
    fn history_line_segments_clamps_out_of_range_values() {
        let values = [-1.0, 2.0];
        let segs = history_line_segments(&values, 0.0, 10.0, 0.0, 10.0);
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0].1, 0.0); // -1.0 は 0.0 にクランプ
        assert_eq!(segs[0].3, 10.0); // 2.0 は 1.0 にクランプ
    }

    #[test]
    fn history_line_segments_needs_at_least_two_points() {
        assert!(history_line_segments(&[0.5], 0.0, 10.0, 0.0, 10.0).is_empty());
        assert!(history_line_segments(&[], 0.0, 10.0, 0.0, 10.0).is_empty());
    }

    #[test]
    fn filled_rect_points_covers_full_span_regardless_of_argument_order() {
        let forward = filled_rect_points(0.0, 0.0, 4.0, 2.0, 1.0);
        let reversed = filled_rect_points(4.0, 2.0, 0.0, 0.0, 1.0);
        assert_eq!(forward.len(), reversed.len());
        assert!(forward.iter().all(|&(x, y)| (0.0..=4.0).contains(&x) && (0.0..=2.0).contains(&y)));
    }

    #[test]
    fn filled_rect_points_empty_for_non_positive_step() {
        assert!(filled_rect_points(0.0, 0.0, 4.0, 2.0, 0.0).is_empty());
    }

    #[test]
    fn ring_points_stays_on_circle() {
        let pts = ring_points(1.0, 2.0, 5.0, 0.2);
        assert!(!pts.is_empty());
        for (x, y) in pts {
            let dist = ((x - 1.0).powi(2) + (y - 2.0).powi(2)).sqrt();
            assert!((dist - 5.0).abs() < 1e-9);
        }
    }

    #[test]
    fn ring_points_empty_for_non_positive_radius() {
        assert!(ring_points(0.0, 0.0, 0.0, 0.2).is_empty());
    }

    #[test]
    fn ellipse_ring_points_stays_on_ellipse() {
        let pts = ellipse_ring_points(0.0, 0.0, 8.0, 3.0, 0.2);
        assert!(!pts.is_empty());
        for (x, y) in &pts {
            let n = (x / 8.0).powi(2) + (y / 3.0).powi(2);
            assert!((n - 1.0).abs() < 1e-9, "楕円上に無い ({x:.2},{y:.2}) n={n:.4}");
        }
        let xs: Vec<f64> = pts.iter().map(|p| p.0).collect();
        assert!(xs.iter().copied().fold(f64::MIN, f64::max) > 7.5);
        assert!(xs.iter().copied().fold(f64::MAX, f64::min) < -7.5);
    }

    #[test]
    fn ellipse_ring_points_empty_for_non_positive_radii() {
        assert!(ellipse_ring_points(0.0, 0.0, 0.0, 3.0, 0.2).is_empty());
        assert!(ellipse_ring_points(0.0, 0.0, 4.0, -1.0, 0.2).is_empty());
    }

    /// 同一の円＋矩形を各 `Marker` で描き、解像度とフォントカバレッジを目視する。
    ///
    /// ```bash
    /// cargo test --lib canvas_fx::tests::dump_marker_resolution_catalog -- --ignored --nocapture
    /// ```
    #[test]
    #[ignore]
    fn dump_marker_resolution_catalog() {
        use ratzilla::ratatui::backend::TestBackend;
        use ratzilla::ratatui::layout::Rect;
        use ratzilla::ratatui::style::Color;
        use ratzilla::ratatui::symbols::Marker;
        use ratzilla::ratatui::widgets::canvas::{Canvas, Points};
        use ratzilla::ratatui::widgets::{Block, Borders};
        use ratzilla::ratatui::Terminal;

        use crate::tui_inspect::buffer_text;

        let markers = [
            ("Braille", Marker::Braille),
            ("HalfBlock", Marker::HalfBlock),
            ("Quadrant", Marker::Quadrant),
            ("Sextant", Marker::Sextant),
            ("Octant", Marker::Octant),
            ("Block", Marker::Block),
            ("Dot", Marker::Dot),
            ("Bar", Marker::Bar),
        ];

        let disc = filled_ellipse_points(20.0, 12.0, 8.0, 7.0, 0.35);
        let ring = ring_points(20.0, 12.0, 10.0, 0.12);
        let bar = filled_rect_points(34.0, 6.0, 42.0, 18.0, 0.4);

        for (name, marker) in markers {
            let backend = TestBackend::new(48, 14);
            let mut terminal = Terminal::new(backend).unwrap();
            terminal
                .draw(|f| {
                    let area = Rect::new(0, 0, 48, 14);
                    let canvas = Canvas::default()
                        .block(
                            Block::default()
                                .borders(Borders::ALL)
                                .title(format!(" {name} ")),
                        )
                        .x_bounds([0.0, 48.0])
                        .y_bounds([0.0, 24.0])
                        .marker(marker)
                        .paint(|ctx| {
                            ctx.draw(&Points {
                                coords: &ring,
                                color: Color::DarkGray,
                            });
                            ctx.draw(&Points {
                                coords: &disc,
                                color: Color::LightYellow,
                            });
                            ctx.draw(&Points {
                                coords: &bar,
                                color: Color::LightGreen,
                            });
                        });
                    f.render_widget(canvas, area);
                })
                .unwrap();
            eprintln!("=== {name} ===\n{}", buffer_text(terminal.backend().buffer()));
        }
    }
}
