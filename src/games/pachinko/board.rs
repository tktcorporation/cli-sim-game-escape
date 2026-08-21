//! 盤面の空間。逆U字のアーチ、遊技領域、ヘソの口幅。
//!
//! 物理も描画も釘生成も、同じ楕円と同じ口幅を見る。座標の式を logic に置くと
//! 描画だけが別の形を描き、当たった場所と見えている壁がずれる。規則は値の
//! 側へ閉じ、呼び出し側は `Arch::TABLE` / `Playfield::TABLE` を渡す。

use super::state::{
    PachinkoState, ATTACKER_Y, BALL_R, BOARD_H, BOARD_W, NAIL_R, START_POCKET_BASE_HALF_W,
    START_POCKET_Y,
};

/// 玉と釘が接触する距離。衝突判定と釘格子のピッチ制約が同じ値を見る。
pub const CONTACT_DIST: f64 = BALL_R + NAIL_R;

/// `nail_spread` がヘソの受け口へ効く強さ。ヘソ釘の位置と当たり判定の幅は
/// 同じ係数を共有しないと、見た目の開きと実際の入りやすさが食い違って
/// 釘読みが嘘になる。
const POCKET_SPREAD_GAIN: f64 = 2.0;

/// 盤面上部の逆U字。楕円の上半分が天井と左右の肩になり、その下は垂直の壁。
///
/// 打ち出しは右足 (3時) から内壁を滑る。弱い玉は途中で落ち、既定は頂点
/// (12時) で離す。強い玉は 10時の出っ張りまで沿って跳ね、12時へ戻ってから
/// 落ちる。天井をただの壁として跳ね返すと、玉は右肩で落ちて頂点まで届かない。
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

    /// 左足 (9時)。`polyline` と同じ媒介変数。
    pub const THETA_LEFT: f64 = std::f64::consts::PI;
    /// 10時。頂点から左へ 60°。強い打ち出しが壁を沿って届く出っ張りの位置。
    pub const THETA_BUMP: f64 = Self::THETA_TOP - std::f64::consts::PI / 3.0;
    /// 頂点 (12時)。
    pub const THETA_TOP: f64 = std::f64::consts::PI * 1.5;
    /// 右足 (3時)。打ち出しの始点。
    pub const THETA_RIGHT: f64 = std::f64::consts::PI * 2.0;
    /// 出っ張りが内壁から盤内へ張り出す半径。釘より大きく、壁の一部として読める。
    pub const BUMP_R: f64 = 1.55;

    /// 天井の y (下向き正)。中央が最も浅く、左右の肩で `b` まで下がる。
    pub fn ceiling_y(self, x: f64) -> f64 {
        let u = ((x - self.cx) / self.a).clamp(-1.0, 1.0);
        self.b * (1.0 - (1.0 - u * u).sqrt())
    }

    /// 10時の出っ張りの中心。内壁に接し、盤の内側へ円として張り出す。
    /// 物理の跳ね位置と描画が同じ点を見る。
    pub fn bump_center(self) -> (f64, f64) {
        let theta = Self::THETA_BUMP;
        let wall_x = self.cx + self.a * theta.cos();
        let wall_y = self.cy + self.b * theta.sin();
        let (nx, ny) = self.outward_normal(theta, 0.0);
        (
            wall_x - nx * Self::BUMP_R,
            wall_y - ny * Self::BUMP_R,
        )
    }

    /// 出っ張りの輪郭。壁に接する円なので、アーチの折れ線と重ねて描く。
    pub fn bump_polyline(self, segments: usize) -> Vec<(f64, f64)> {
        let (cx, cy) = self.bump_center();
        let n = segments.max(8);
        (0..=n)
            .map(|i| {
                let t = std::f64::consts::PI * 2.0 * i as f64 / n as f64;
                (cx + Self::BUMP_R * t.cos(), cy + Self::BUMP_R * t.sin())
            })
            .collect()
    }

    /// レールの到達角が出っ張りに届くか。届いた玉はここで跳ねて 12時へ戻る。
    pub fn rail_hits_bump(until: f64) -> bool {
        until <= Self::THETA_BUMP + 1e-6
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

    /// 玉半径だけ縮めた楕円上の点。レールに乗った玉の中心。
    pub fn inner_point(self, theta: f64, radius: f64) -> (f64, f64) {
        let rx = (self.a - radius).max(0.1);
        let ry = (self.b - radius).max(0.1);
        (
            self.cx + rx * theta.cos(),
            self.cy + ry * theta.sin(),
        )
    }

    /// 右足から頂点へ向かう向きの単位接線。θ を減らす方向。
    pub fn tangent_decreasing(self, theta: f64, radius: f64) -> (f64, f64) {
        let rx = (self.a - radius).max(0.1);
        let ry = (self.b - radius).max(0.1);
        let dx = rx * theta.sin();
        let dy = -ry * theta.cos();
        let len = (dx * dx + dy * dy).sqrt().max(1e-9);
        (dx / len, dy / len)
    }

    /// 楕円の外へ向かう単位法線。レールを離すときに盤内へ蹴る向きの逆。
    pub fn outward_normal(self, theta: f64, radius: f64) -> (f64, f64) {
        let (x, y) = self.inner_point(theta, radius);
        let rx = (self.a - radius).max(0.1);
        let ry = (self.b - radius).max(0.1);
        let gx = (x - self.cx) / (rx * rx);
        let gy = (y - self.cy) / (ry * ry);
        let len = (gx * gx + gy * gy).sqrt().max(1e-9);
        (gx / len, gy / len)
    }

    /// `dθ` を弧長に換算する係数。レール上の移動量を速さから決める。
    pub fn arc_metric(self, theta: f64, radius: f64) -> f64 {
        let rx = (self.a - radius).max(0.1);
        let ry = (self.b - radius).max(0.1);
        (rx * theta.sin()).hypot(ry * theta.cos())
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

/// 盤面下側の液晶。ヘソとアタッカーのあいだ、空いている帯を横長の楕円で埋める。
///
/// 玉は液晶の手前を落ちる（当たり判定は持たない）。実機と同じく、下の
/// 空きは数字や釘ではなく「今なにかが動いている」場所として視線を置く。
/// 物理は当たらない。描画だけが同じ楕円を見る。円だと左右の空きが死ぬ。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stage {
    pub cx: f64,
    pub cy: f64,
    pub rx: f64,
    pub ry: f64,
}

/// 液晶の見せ方。同じ回転だけだと数秒で目が慣れるので、席の位相から
/// 種類を切り替える。数字は出さない。速さは描画側の `stage_rate` が決める。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StageShow {
    /// 外周が回り、スポークが追いかけて輪だと読める。
    Roulette,
    /// 縦の光が左右へ往復する。横長を一番使う。
    Sweep,
    /// 二つの明かりが逆向きに追う。
    Twin,
    /// 楕円の輪が内側から膨らんで消える。
    Pulse,
    /// 光点が Lissajous で画面を渡り歩く。
    Comet,
}

impl StageShow {
    pub const ALL: [Self; 5] = [
        Self::Roulette,
        Self::Sweep,
        Self::Twin,
        Self::Pulse,
        Self::Comet,
    ];
    /// 1種類を見せる tick 数。10 ticks/sec なので約 8 秒で次へ移る。
    pub const LEN: u32 = 80;

    pub fn at(ticks: u32) -> Self {
        Self::ALL[((ticks / Self::LEN) as usize) % Self::ALL.len()]
    }
}

impl Stage {
    pub const TABLE: Self = Self {
        cx: BOARD_W / 2.0,
        cy: 65.0,
        rx: 22.0,
        ry: 8.5,
    };
    /// 外周のランプ数。横長だと 12 では目盛りがスカスカになる。
    pub const LAMPS: usize = 16;
    /// 内周のランプ数。外周と逆向きに回し、止まって見えないのを防ぐ。
    pub const INNER_LAMPS: usize = 8;
    /// スポーク数。ランプだけだと円の塗りに見え、輪が回っていると読めない。
    pub const SPOKES: usize = 6;
    /// 内周の速さ倍率。外周と同じ位相だと二重の円が一体に見えてしまう。
    pub const INNER_PHASE_MULT: f64 = 1.35;

    /// 楕円上の点。`frac` は半径に対する割合。
    fn point_on_ring(self, index: usize, count: usize, frac: f64, phase: f64) -> (f64, f64) {
        let a = phase + std::f64::consts::TAU * index as f64 / count as f64;
        (
            self.cx + self.rx * frac * a.cos(),
            self.cy + self.ry * frac * a.sin(),
        )
    }

    /// 外周ランプの位置。`phase` は時計回りに進む位相 (ラジアン)。
    pub fn lamp(self, index: usize, phase: f64) -> (f64, f64) {
        self.point_on_ring(index, Self::LAMPS, 0.88, phase)
    }

    /// 内周ランプの位置。外周と逆位相で回す。
    pub fn inner_lamp(self, index: usize, phase: f64) -> (f64, f64) {
        self.point_on_ring(
            index,
            Self::INNER_LAMPS,
            0.42,
            Self::inner_phase(phase),
        )
    }

    /// スポーク上の点。
    pub fn spoke_point(self, index: usize, phase: f64, frac: f64) -> (f64, f64) {
        self.point_on_ring(index, Self::SPOKES, frac, phase)
    }

    /// スポーク先端。
    pub fn spoke_tip(self, index: usize, phase: f64) -> (f64, f64) {
        self.spoke_point(index, phase, 0.70)
    }

    /// 12時の指針。y は下向き正なので、頂点は中心より浅い。
    pub fn pointer(self) -> (f64, f64) {
        (self.cx, self.cy - self.ry * 0.96)
    }

    /// 掃引バーの x。`t.sin()` で左右へ往復し、横幅を使い切る。
    pub fn sweep_x(self, t: f64) -> f64 {
        self.cx + self.rx * 0.78 * t.sin()
    }

    /// 脈動する輪の半径割合。`offset` をずらすと二重の波になる。
    pub fn pulse_frac(t: f64, offset: f64) -> f64 {
        0.20 + 0.68 * (t * 0.18 + offset).rem_euclid(1.0)
    }

    /// 画面を横断する光点。周波数を 2:3 にして同じ軌跡を辿らせない。
    pub fn comet(self, t: f64) -> (f64, f64) {
        (
            self.cx + self.rx * 0.78 * (t * 0.85).sin(),
            self.cy + self.ry * 0.62 * (t * 1.3 + 0.7).sin(),
        )
    }

    /// 点が液晶の内側かどうか。サイド入賞口と重ならないことの検査に使う。
    pub fn contains(self, x: f64, y: f64) -> bool {
        let nx = (x - self.cx) / self.rx;
        let ny = (y - self.cy) / self.ry;
        nx * nx + ny * ny <= 1.0
    }

    /// 内周の位相。符号をここで一箇所に閉じ、位置と点灯が打ち消し合わないようにする。
    pub const fn inner_phase(phase: f64) -> f64 {
        -phase * Self::INNER_PHASE_MULT
    }

    /// 位相に対して点灯するランプ番号。位置の計算と同じ `phase` を渡す。
    pub fn hot_index(phase: f64, count: usize) -> usize {
        let step = std::f64::consts::TAU / count as f64;
        ((phase / step).rem_euclid(count as f64).floor() as usize) % count
    }

    /// ルーレットの回転方向。同じ向きだけだと目が慣れる。
    pub fn spin_sign(ticks: u32) -> f64 {
        if (ticks / StageShow::LEN / StageShow::ALL.len() as u32).is_multiple_of(2) {
            1.0
        } else {
            -1.0
        }
    }
}

const _: () = assert!(Stage::TABLE.cy - Stage::TABLE.ry > START_POCKET_Y + 1.5);
const _: () = assert!(Stage::TABLE.cy + Stage::TABLE.ry < ATTACKER_Y - 2.0);
const _: () = assert!(Stage::TABLE.rx > Stage::TABLE.ry * 2.0);

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
    use crate::games::pachinko::state::{
        SIDE_POCKET_LEFT_X, SIDE_POCKET_RIGHT_X, SIDE_POCKET_Y,
    };

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
        let top = arch.inner_point(Arch::THETA_TOP, BALL_R);
        assert!(
            (top.0 - arch.cx).abs() < 0.2 && top.1 < BALL_R + 0.2,
            "12時の内側点が頂点に無い ({:?})",
            top
        );
        let right = arch.inner_point(Arch::THETA_RIGHT, BALL_R);
        assert!(
            right.0 > BOARD_W - BALL_R - 0.2 && (right.1 - arch.b).abs() < 0.2,
            "3時の内側点が右足に無い ({:?})",
            right
        );
        assert!(
            (crate::games::pachinko::state::LAUNCH_Y - arch.b).abs() < 1e-9,
            "LAUNCH_Y がアーチの右足の高さとずれている"
        );
    }

    #[test]
    fn the_ten_oclock_bump_sits_on_the_inner_wall() {
        // 出っ張りは 10時の内壁に接する円。描画と跳ね位置が同じ中心を見る。
        let arch = Arch::TABLE;
        let (bx, by) = arch.bump_center();
        assert!(
            bx < arch.cx - arch.a * 0.35,
            "出っ張りが10時側に無い (x={bx:.2})"
        );
        assert!(
            by > 2.0 && by < arch.b * 0.75,
            "出っ張りがアーチの左肩に無い (y={by:.2})"
        );
        let wall = (
            arch.cx + arch.a * Arch::THETA_BUMP.cos(),
            arch.cy + arch.b * Arch::THETA_BUMP.sin(),
        );
        let dist = (bx - wall.0).hypot(by - wall.1);
        assert!(
            (dist - Arch::BUMP_R).abs() < 0.05,
            "出っ張りが壁から浮いている (dist={dist:.2})"
        );
        let outline = arch.bump_polyline(12);
        assert!(outline.len() >= 9, "出っ張りの輪郭が閉じていない");
        assert!(Arch::rail_hits_bump(Arch::THETA_BUMP));
        assert!(!Arch::rail_hits_bump(Arch::THETA_TOP));
    }

    #[test]
    fn the_stage_sits_in_the_empty_band_below_the_heso() {
        // 液晶はヘソとアタッカーのあいだに置く。釘帯や漏斗に重ねると、
        // 玉の通り道と絵が食い違って釘読みの対象が消える。
        let stage = Stage::TABLE;
        assert!(
            (stage.cx - BOARD_W / 2.0).abs() < 1e-9,
            "液晶が盤面の中央に無い"
        );
        assert!(
            stage.cy > START_POCKET_Y + 2.0,
            "液晶がヘソに重なっている (cy={:.2})",
            stage.cy
        );
        assert!(
            stage.rx > stage.ry * 2.0,
            "液晶が横長になっていない (rx={:.1} ry={:.1})",
            stage.rx,
            stage.ry
        );
        assert!(
            stage.cy + stage.ry < ATTACKER_Y - 1.0,
            "液晶がアタッカーに重なっている"
        );
        assert!(
            stage.cy - stage.ry > START_POCKET_Y,
            "液晶の上端がヘソより上にある"
        );
        assert!(
            !stage.contains(SIDE_POCKET_LEFT_X, SIDE_POCKET_Y),
            "液晶が左のサイド入賞口に被っている"
        );
        assert!(
            !stage.contains(SIDE_POCKET_RIGHT_X, SIDE_POCKET_Y),
            "液晶が右のサイド入賞口に被っている"
        );
        let sweep_span = (stage.sweep_x(std::f64::consts::FRAC_PI_2)
            - stage.sweep_x(-std::f64::consts::FRAC_PI_2))
        .abs();
        assert!(
            sweep_span > stage.rx,
            "掃引が横幅を使っていない (span={sweep_span:.1})"
        );
        let (c0x, _) = stage.comet(0.0);
        let (c1x, _) = stage.comet(2.0);
        assert!(
            (c0x - c1x).abs() > stage.rx * 0.5,
            "コメットが横に動いていない"
        );
        assert_ne!(
            StageShow::at(0),
            StageShow::at(StageShow::LEN),
            "tick が進んでも見せ方が変わらない"
        );
        assert_eq!(StageShow::at(0), StageShow::Roulette);
        assert_eq!(StageShow::at(StageShow::LEN), StageShow::Sweep);
        let (px, py) = stage.pointer();
        assert!(
            (px - stage.cx).abs() < 0.2 && py < stage.cy,
            "指針が12時に無い ({px:.2}, {py:.2})"
        );
        let (x0, y0) = stage.lamp(0, 0.0);
        let (x1, y1) = stage.lamp(0, 0.4);
        assert!(
            (x0 - x1).hypot(y0 - y1) > 1.0,
            "位相を変えてもランプが動いていない"
        );
        let inner_a = stage.inner_lamp(Stage::hot_index(Stage::inner_phase(0.0), Stage::INNER_LAMPS), 0.0);
        let inner_b = stage.inner_lamp(Stage::hot_index(Stage::inner_phase(0.8), Stage::INNER_LAMPS), 0.8);
        assert!(
            (inner_a.0 - inner_b.0).hypot(inner_a.1 - inner_b.1) > 1.0,
            "内周の点灯が位相打ち消しで止まっている ({inner_a:?} / {inner_b:?})"
        );
        let spoke_a = stage.spoke_tip(0, 0.0);
        let spoke_b = stage.spoke_tip(0, 0.5);
        assert!(
            (spoke_a.0 - spoke_b.0).hypot(spoke_a.1 - spoke_b.1) > 1.0,
            "位相を変えてもスポークが動いていない"
        );
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
