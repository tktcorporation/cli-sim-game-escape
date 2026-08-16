//! 盤面の空間。逆U字のアーチ、遊技領域、ヘソの口幅。
//!
//! 物理も描画も釘生成も、同じ楕円と同じ口幅を見る。座標の式を logic に置くと
//! 描画だけが別の形を描き、当たった場所と見えている壁がずれる。規則は値の
//! 側へ閉じ、呼び出し側は `Arch::TABLE` / `Playfield::TABLE` を渡す。

use super::state::{PachinkoState, BALL_R, BOARD_H, BOARD_W, NAIL_R, START_POCKET_BASE_HALF_W};

/// 玉と釘が接触する距離。衝突判定と釘格子のピッチ制約が同じ値を見る。
pub const CONTACT_DIST: f64 = BALL_R + NAIL_R;

/// `nail_spread` がヘソの受け口へ効く強さ。ヘソ釘の位置と当たり判定の幅は
/// 同じ係数を共有しないと、見た目の開きと実際の入りやすさが食い違って
/// 釘読みが嘘になる。
const POCKET_SPREAD_GAIN: f64 = 2.0;

/// 盤面上部の逆U字。楕円の上半分が天井と左右の肩になり、その下は垂直の壁。
/// 打ち出した玉は右端に沿って上がり、肩のカーブに当たって釘帯へ落ちる。
/// 平面の天井は反射角が揃い、同じ列へ落ちる。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Arch {
    pub cx: f64,
    pub cy: f64,
    pub a: f64,
    pub b: f64,
}

/// 面上へ押し戻した位置と、盤面の外へ向かう単位法線。
#[derive(Clone, Copy, Debug)]
pub struct ArchPush {
    pub x: f64,
    pub y: f64,
    pub nx: f64,
    pub ny: f64,
}

impl Arch {
    pub const TABLE: Self = Self {
        cx: BOARD_W / 2.0,
        cy: 15.0,
        a: BOARD_W / 2.0,
        b: 15.0,
    };

    /// 天井の y (下向き正)。中央が最も浅く、左右の肩で `b` まで下がる。
    pub fn ceiling_y(self, x: f64) -> f64 {
        let u = ((x - self.cx) / self.a).clamp(-1.0, 1.0);
        self.b * (1.0 - (1.0 - u * u).sqrt())
    }

    /// 逆U字の上端を左足から右足まで辿る折れ線。描画と物理が同じ楕円を共有する。
    pub fn polyline(self, segments: usize) -> Vec<(f64, f64)> {
        let n = segments.max(8);
        (0..=n)
            .map(|i| {
                let theta = std::f64::consts::PI * (1.0 + i as f64 / n as f64);
                (
                    self.cx + self.a * theta.cos(),
                    self.cy + self.b * theta.sin(),
                )
            })
            .collect()
    }

    /// 玉半径だけ縮めた楕円の上半分の外側に中心があるとき、面上へ押し戻した
    /// 位置と外向き単位法線を返す。縮めないと、中心が楕円上に乗ったときに
    /// 玉の上半分が盤外へ出る。内側・楕円の下側は `None`。
    pub fn push_inside(self, x: f64, y: f64, radius: f64) -> Option<ArchPush> {
        let rx = self.a - radius;
        let ry = self.b - radius;
        if rx <= 0.0 || ry <= 0.0 {
            return None;
        }
        let dx = x - self.cx;
        let dy = y - self.cy;
        if dy > 0.0 {
            return None;
        }
        let fx = dx / rx;
        let fy = dy / ry;
        let f = fx * fx + fy * fy;
        if f <= 1.0 {
            return None;
        }
        let inv = 1.0 / f.sqrt();
        let gx = dx / (rx * rx);
        let gy = dy / (ry * ry);
        let glen = (gx * gx + gy * gy).sqrt().max(1e-9);
        Some(ArchPush {
            x: self.cx + dx * inv,
            y: self.cy + dy * inv,
            nx: gx / glen,
            ny: gy / glen,
        })
    }
}

/// 遊技領域。上部はアーチ、その下は矩形。角の切り欠きは矩形の内包だけでは足りない。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Playfield {
    pub w: f64,
    pub h: f64,
    pub arch: Arch,
}

impl Playfield {
    pub const TABLE: Self = Self {
        w: BOARD_W,
        h: BOARD_H,
        arch: Arch::TABLE,
    };

    /// 玉の中心が盤面の内側にいるか。
    pub fn contains(self, x: f64, y: f64, radius: f64) -> bool {
        if !(radius..=self.w - radius).contains(&x) || !(0.0..=self.h).contains(&y) {
            return false;
        }
        if y >= self.arch.b {
            return true;
        }
        let rx = self.arch.a - radius;
        let ry = self.arch.b - radius;
        if rx <= 0.0 || ry <= 0.0 {
            return false;
        }
        let dx = x - self.arch.cx;
        let dy = y - self.arch.cy;
        if dy > 0.0 {
            return true;
        }
        (dx / rx) * (dx / rx) + (dy / ry) * (dy / ry) <= 1.0 + 1e-6
    }
}

/// ヘソの受け口半幅。決まるのは台のヘソ釘の開きと電サポの有無だけなので、
/// 着席中の台に限らずホールに並ぶ台にも同じ式で引ける。ホールの盤面
/// プレビューが着席後と同じヘソを描けるのはこのため。
pub fn pocket_half_w(nail_spread: f64, assisted: bool) -> f64 {
    let base = START_POCKET_BASE_HALF_W + nail_spread * POCKET_SPREAD_GAIN;
    if assisted {
        // 電サポ中は羽根が開いてヘソが広がる。確変・時短の価値をヘソの
        // 見た目そのもので伝えるため、確率ではなく受け口を触る。
        base + 1.0
    } else {
        base
    }
}

/// 着席中の台のヘソの実効受け口半幅。台に着いていない間は中庸な開きの台と
/// して扱い、受け口が 0 幅に潰れた盤面を描かせない。
pub fn effective_pocket_half_w(state: &PachinkoState) -> f64 {
    let spread = state.seated_machine().map(|m| m.nail_spread).unwrap_or(0.5);
    pocket_half_w(spread, state.mode.is_assisted())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arch_ceiling_is_an_inverted_u() {
        // 中央が最も浅く、左右の肩で下がる。平面だと打ち出しの反射角が揃う。
        let arch = Arch::TABLE;
        let center = arch.ceiling_y(BOARD_W / 2.0);
        let side = arch.ceiling_y(4.0);
        assert!(
            center < 0.5,
            "アーチの頂点が天井から離れている (y={center:.2})"
        );
        assert!(
            side > arch.b * 0.4,
            "左右の肩がカーブしていない (y={side:.2})"
        );
        assert!(side > center + 4.0, "逆U字になっていない");
        let pts = arch.polyline(16);
        assert_eq!(pts.len(), 17);
        assert!((pts[0].0 - 0.0).abs() < 0.2 && (pts[0].1 - arch.b).abs() < 0.2);
        assert!(
            (pts[8].0 - arch.cx).abs() < 0.2 && pts[8].1 < 0.5,
            "折れ線の頂点が中央の天井に無い ({:?})",
            pts[8]
        );
        assert!((pts[16].0 - BOARD_W).abs() < 0.2 && (pts[16].1 - arch.b).abs() < 0.2);
    }

    #[test]
    fn playfield_cuts_the_corners_above_the_arch() {
        let field = Playfield::TABLE;
        assert!(field.contains(BOARD_W / 2.0, 1.0, 0.0));
        assert!(
            !field.contains(1.0, 1.0, 0.0),
            "アーチの外の角を盤内として扱っている"
        );
        assert!(field.contains(1.0, arch_foot_y(), 0.0));
    }

    fn arch_foot_y() -> f64 {
        Arch::TABLE.b + 1.0
    }
}
