//! `ratatui::widgets::canvas::Canvas` + `Marker::Braille` で疑似ピクセル表現を
//! 作るための純粋な幾何計算ヘルパー。
//!
//! Canvas 自体が world 座標→braille セルの変換を担うため、ここでは「どの
//! 座標に何を描くか」だけを返す。実際の `Canvas`/`Shape` の組み立て・色・
//! x_bounds/y_bounds の設定は呼び出し側 (各ゲームの render.rs) の責務にする
//! — パネルのサイズや配色はゲームごとに違うため、ここで固定すると
//! かえって使い回しにくくなる。

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
}
