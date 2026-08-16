//! 玉の運動。重力、壁とアーチ、釘衝突、ヘソの縁揺れ。
//!
//! 入賞の賞球と抽選は logic が決める。ここは位置と速度だけを進め、口を跨いだ
//! 事実を `PocketHits` として返す。物理が当落を知ると、釘の跳ねと確率が同じ
//! モジュールに混ざり、どちらを触っても両方壊れる。

use super::board::{effective_pocket_half_w, Arch, Playfield, CONTACT_DIST};
use super::rng::{rand01, rng_below};
use super::state::{
    Ball, Mode, Nail, PachinkoState, ATTACKER_HALF_W, ATTACKER_X, ATTACKER_Y, BALL_R, BOARD_W,
    HIT_GLOW_TICKS, SIDE_POCKET_HALF_W, SIDE_POCKET_LEFT_X, SIDE_POCKET_RIGHT_X, SIDE_POCKET_Y,
    START_POCKET_X, START_POCKET_Y,
};

/// 1 tick あたりの物理サブステップ数。10 ticks/sec のまま1回で進めると、
/// 1ステップの移動量が釘の直径を超えて釘をすり抜ける。
pub const PHYSICS_SUBSTEPS: u32 = 5;
/// 1サブステップあたりの重力加速度。玉が落ちる速さはここが決める。
///
/// 盤面は 10 ticks/sec でしか描き直されないので、1 tick の移動距離が玉の直径
/// (`BALL_R * 2`) を大きく超えると、玉は毎コマ離れた位置に現れ、釘に弾かれる
/// 瞬間が絵として残らない。`simulator::ball_motion_report` がこの移動距離を
/// 実測する。
///
/// 重力は、釘で跳ねた弧が次の段を飛び越えない強さにする。軽すぎると玉は
/// 釘の上を hop して隙間を落ち、当たりながら落ちる絵が消える。速さを決める
/// 定数は `GRAVITY` 単体ではない。`launch_velocity` / `HORIZONTAL_DRAG` /
/// `NAIL_SCATTER` と組で「玉道の形」を決めており、どれか1つだけを触ると
/// 形が変わって回転率が動く。玉道を保ったまま T 倍の時間をかけさせたい
/// 場合は、位置が速度の積分・速度が加速度の積分であることから、速度を
/// 1/T・加速度を 1/T²・1サブステップあたりの減衰を T 乗根にした組で動かす。
const GRAVITY: f64 = 0.028;
/// 釘との衝突の反発係数。弾かれた玉が次の釘まで飛ぶ軌跡が絵として残る程度に
/// 跳ね返す。上げすぎると玉が釘の上で跳ね続けて落ちてこない。
const NAIL_RESTITUTION: f64 = 0.90;
/// 法線速度がこれ未満の衝突は「絡み」。実機では起こした釘の根元に玉が絡み、
/// 盤面へ擦りながら落ちる。反発を落とすことで、速い衝突の「ポン」と遅い
/// 衝突の「引っかかり」を同じ式で出し分ける。
const NAIL_TANGLE_VN: f64 = 0.12;
const NAIL_TANGLE_RESTITUTION: f64 = 0.55;
/// 接線方向の速度を残す割合。1 だと表面を滑って去り、0 だと釘に貼り付く。
/// 実機の玉は釘の側面を転がって方向を変えるので、跳ね返しだけでは出ない
/// 「沿って落ちる」動きをここで作る。ここを上げすぎると寄り釘で横へ弾かれて
/// ヘソへ届く玉が減り、ヘソ釘の開きを読む判断軸が弱くなる。
const NAIL_TANGENTIAL_KEEP: f64 = 0.86;
/// 左右の垂直壁との衝突の反発係数。
const WALL_RESTITUTION: f64 = 0.45;
/// アーチ内壁の反発係数。釘より落とす。高いとカーブを滑って中央のヘソ
/// 真上へ集まり、出玉率が 1 を超える。
const ARCH_RESTITUTION: f64 = 0.55;
/// アーチに沿う接線速度を残す割合。1 だと天井を滑って中央のヘソ真上へ
/// 集まり、0 だと当たった場所から真下へ落ちる。強度で「どこまで回るか」
/// を残しつつ、ヘソ真上への収束は避ける。
const ARCH_TANGENTIAL_KEEP: f64 = 0.50;
/// アーチ衝突時の横kick。同じ強度・同じ肩でも落ちる列が割れる。
/// 釘の `NAIL_SCATTER` と同じ役割を、釘帯へ入る前の入口で担う。
const ARCH_SCATTER: f64 = 0.14;
/// 速度の上限。`CONTACT_DIST` 以下に収めてあるので、釘へ真っ直ぐ向かう玉は
/// 必ず1回はサブステップの標本が接触範囲へ入る。ここが接触距離を超えると
/// 速い玉だけが釘をすり抜け、釘に当たるかどうかが速度で変わってしまう。
pub(super) const MAX_SPEED: f64 = 1.10;
/// 釘に当たった時に横方向へ乗るばらつきの最大幅。同じ軌道で入っても結果が
/// 割れる「パチンコらしさ」の源で、0 にすると釘配置だけで結果が決まる
/// 決定論的な機械になってしまう。速度と同じ次元なので、落下の速さを変える
/// ときは `GRAVITY` の説明にある組で一緒に動かす。
///
/// 実機の玉は盤面とガラスの 2.5mm 隙間で面外に揺れ、2D では再現できない。
/// その代わりに衝突のたびに横kickを乗せ、一本の溝に全弾が落ちるのを防ぐ。
const NAIL_SCATTER: f64 = 0.18;

/// 1サブステップごとに横方向の速度へ掛かる減衰。打ち出した勢いは盤面を
/// 横切る間に抜け、玉は釘の間をほぼ真下へ落ちていく。減衰が無いと初速の
/// まま左端まで飛んで壁沿いに落ちるだけになり、ハンドル強度が「どこへ
/// 落とすか」を決める操作にならない。
const HORIZONTAL_DRAG: f64 = 0.958;
/// 打ち出し1発ごとの初速のばらつき (割合)。同じ強度でも玉道が完全に一致
/// すると、釘の間に一本の溝ができて全弾が同じ場所へ落ちる。実機のハンドル
/// と同じく、わずかな揺らぎが玉道を散らす。
const LAUNCH_SPEED_JITTER: f64 = 0.11;
/// 打ち出し方向の横kick。速さだけを振ると同じ角度のまま着地点がほとんど
/// 動かない。左右に独立した分を足して、同じ強度でも落ちる列が分かれる
/// ようにする。
const LAUNCH_VX_JITTER: f64 = 0.12;
/// 打ち出しの縦成分のばらつき。アーチの高い位置まで届くかどうかの境を
/// またぐと、右肩で落ちるか中央近くまで回るかが割れ、同じ強度でも落ちる
/// 列が分かれる。
const LAUNCH_VY_JITTER: f64 = 0.14;
/// 縁揺れの最短 / 最長 (tick)。最短は `delta_ticks` の上限 (5) より長くし、
/// 遅れをまとめて消化しても「縁に乗っている」絵が1フレームは残るようにする。
pub(super) const TEETER_TICKS_MIN: u8 = 6;
pub(super) const TEETER_TICKS_MAX: u8 = 11;
/// 縁揺れの振幅と、1 tick あたりの位相。10 ticks/sec で 1〜2 往復見える速さ。
/// 振幅は受け口半幅より小さくし、中央へ届いた玉が揺れの途中で口の外へ
/// 飛ばないようにする。外へ出るのは、縁ぎりぎりに乗った玉だけ。
const TEETER_AMP: f64 = 0.38;
const TEETER_OMEGA: f64 = 1.15;
/// 縁揺れ中の横方向の乱れ。実機の面外ゆらぎの代わり。
const TEETER_JITTER: f64 = 0.07;
/// 揺れの中心を受け口中央へ寄せる割合。1 だと到着位置のまま、0 だと中央へ
/// 吸い寄せる。縁に乗った玉ほど残り幅が狭いので、寄せが無いと次の揺れで
/// 口の外へ出る。
const TEETER_CENTER_KEEP: f64 = 0.55;
/// 到着位置が受け口半幅のこの割合より外側なら、揺れの末に滑り落ちうる。
/// 狭いヘソほど縁に乗る玉が増え、広いヘソほど中央で落ち着いて入る。
const TEETER_SLIP_EDGE: f64 = 0.62;
const TEETER_SLIP_PERCENT: u32 = 40;
/// ヘソの口として扱う、受け口中心からの上下幅。
const POCKET_MOUTH_ABOVE: f64 = 1.7;
pub(super) const POCKET_MOUTH_BELOW: f64 = 0.9;

const _: () = assert!(MAX_SPEED <= CONTACT_DIST);
const _: () = assert!(TEETER_TICKS_MIN as u32 > 5);

/// 1 tick の運動で口を跨いだ玉。賞球と抽選は logic が処理する。
#[derive(Default)]
pub(super) struct PocketHits {
    pub start: Vec<bool>,
    pub attacker: u32,
    pub side: u32,
}

/// ハンドル強度 (0〜100) から打ち出し初速 (1サブステップあたり) を決める。
/// 玉は逆U字の右肩に当たり、`HORIZONTAL_DRAG` で横の勢いが抜けたところから
/// 釘の間へ落ちるので、強度は「盤面のどこへ落とすか」を決める操作になる。
/// 適正値は台ごとの釘配置で変わるため、ここでは素直な線形写像だけを行い、
/// 良し悪しの判断は盤面に委ねる。
pub fn launch_velocity(power: u8) -> (f64, f64) {
    let p = (power as f64 / 100.0).clamp(0.0, 1.0);
    // 横は右端から離れない程度。左へ出すと空中で失速し、肩に当たらない。
    // 縦が主で、当たったあとの跳ねが強度で「どこまで回るか」を分ける。
    (-(0.05 + p * 0.22), -0.72 - p * 0.42)
}

pub(super) fn jitter_launch((vx, vy): (f64, f64), seed: &mut u32) -> (f64, f64) {
    let speed = 1.0 + (rand01(seed) - 0.5) * 2.0 * LAUNCH_SPEED_JITTER;
    let vx = vx * speed + (rand01(seed) - 0.5) * 2.0 * LAUNCH_VX_JITTER;
    let vy = vy * speed + (rand01(seed) - 0.5) * 2.0 * LAUNCH_VY_JITTER;
    (vx, vy)
}

/// サブステップの前後で入賞口の高さを跨いだか。矩形の内包判定にすると、
/// 1サブステップの移動量 (最大 `MAX_SPEED`) が入賞口の高さを超えたときに
/// 素通りする。
fn crossed_downward(prev_y: f64, y: f64, line: f64) -> bool {
    prev_y <= line && y > line
}

/// 1 tick 分の玉の運動。口を跨いだ事実だけを返す。
pub(super) fn step_balls(state: &mut PachinkoState) -> PocketHits {
    let mut hits = PocketHits::default();
    if state.seat >= state.machines.len() {
        return hits;
    }
    // 釘と玉は所有権ごと借り出す。`state` の他のフィールド (乱数 seed) を
    // 玉のループ中に触るため、借用を分離する必要がある。
    let nails = std::mem::take(&mut state.machines[state.seat].nails);
    let mut balls = std::mem::take(&mut state.balls);
    let pocket_half_w = effective_pocket_half_w(state);
    let attacker_open = matches!(state.mode, Mode::Jackpot(_));
    let seed = &mut state.rng_state;

    balls.retain_mut(|ball| {
        if ball.teeter > 0 {
            return match step_teeter(ball, pocket_half_w, seed) {
                TeeterEnd::Capture => {
                    hits.start.push(ball.fired_in_normal);
                    false
                }
                TeeterEnd::Stay => true,
            };
        }
        for _ in 0..PHYSICS_SUBSTEPS {
            let prev_y = ball.y;
            ball.vy += GRAVITY;
            ball.x += ball.vx;
            ball.y += ball.vy;
            ball.vx *= HORIZONTAL_DRAG;
            bounce_walls(ball, seed);
            bounce_nails(ball, &nails, seed);
            clamp_speed(ball);

            if in_start_mouth(ball, pocket_half_w) {
                start_teeter(ball, pocket_half_w);
                break;
            }
            if crossed_downward(prev_y, ball.y, SIDE_POCKET_Y)
                && ((ball.x - SIDE_POCKET_LEFT_X).abs() < SIDE_POCKET_HALF_W
                    || (ball.x - SIDE_POCKET_RIGHT_X).abs() < SIDE_POCKET_HALF_W)
            {
                hits.side += 1;
                return false;
            }
            if attacker_open
                && crossed_downward(prev_y, ball.y, ATTACKER_Y)
                && (ball.x - ATTACKER_X).abs() < ATTACKER_HALF_W
            {
                hits.attacker += 1;
                return false;
            }
            if ball.y > Playfield::TABLE.h {
                return false;
            }
        }
        true
    });

    state.balls = balls;
    state.machines[state.seat].nails = nails;
    hits
}

fn in_start_mouth(ball: &Ball, pocket_half_w: f64) -> bool {
    (ball.x - START_POCKET_X).abs() < pocket_half_w
        && ball.y > START_POCKET_Y - POCKET_MOUTH_ABOVE
        && ball.y < START_POCKET_Y + POCKET_MOUTH_BELOW
        && ball.vy >= 0.0
}

fn teeter_duration(vy: f64) -> u8 {
    let t = 1.0 - (vy / MAX_SPEED).clamp(0.0, 1.0);
    let span = f64::from(TEETER_TICKS_MAX - TEETER_TICKS_MIN);
    (f64::from(TEETER_TICKS_MIN) + t * span).round() as u8
}

fn start_teeter(ball: &mut Ball, pocket_half_w: f64) {
    ball.teeter = teeter_duration(ball.vy);
    ball.teeter_x = ball.x.clamp(
        START_POCKET_X - pocket_half_w + 0.05,
        START_POCKET_X + pocket_half_w - 0.05,
    );
    ball.y = START_POCKET_Y - 0.35;
    ball.vx = 0.0;
    ball.vy = 0.0;
}

enum TeeterEnd {
    Capture,
    Stay,
}

/// ヘソの縁で1 tick 分揺する。口の外へ出たら落下を再開し、揺れが尽きたら入る。
fn step_teeter(ball: &mut Ball, pocket_half_w: f64, seed: &mut u32) -> TeeterEnd {
    ball.teeter = ball.teeter.saturating_sub(1);
    let phase = f64::from(ball.teeter) * TEETER_OMEGA;
    let amp = TEETER_AMP * (f64::from(ball.teeter) / f64::from(TEETER_TICKS_MAX)).max(0.25);
    let origin = START_POCKET_X + (ball.teeter_x - START_POCKET_X) * TEETER_CENTER_KEEP;
    ball.x = origin + amp * phase.sin() + (rand01(seed) - 0.5) * TEETER_JITTER;
    ball.y = START_POCKET_Y - 0.35 + 0.16 * (phase * 1.7).sin();
    ball.vx = 0.0;
    ball.vy = 0.0;

    let offset = (ball.x - START_POCKET_X).abs();
    if offset > pocket_half_w {
        slip_off_lip(ball);
        return TeeterEnd::Stay;
    }
    if ball.teeter == 0 {
        let arrival = (ball.teeter_x - START_POCKET_X).abs() / pocket_half_w.max(0.1);
        if arrival > TEETER_SLIP_EDGE && rng_below(seed, 100) < TEETER_SLIP_PERCENT {
            slip_off_lip(ball);
            return TeeterEnd::Stay;
        }
        TeeterEnd::Capture
    } else {
        TeeterEnd::Stay
    }
}

/// 縁から外す。口の判定矩形の外へ出さないと、次の tick でまた縁に乗ってしまう。
fn slip_off_lip(ball: &mut Ball) {
    ball.teeter = 0;
    ball.vx = (ball.x - START_POCKET_X).signum() * 0.38;
    ball.vy = 0.28;
    ball.y = START_POCKET_Y + POCKET_MOUTH_BELOW + 0.15;
}

fn bounce_walls(ball: &mut Ball, seed: &mut u32) {
    bounce_arch(ball, seed);
    if ball.x < BALL_R {
        ball.x = BALL_R;
        ball.vx = -ball.vx * WALL_RESTITUTION;
    } else if ball.x > BOARD_W - BALL_R {
        ball.x = BOARD_W - BALL_R;
        ball.vx = -ball.vx * WALL_RESTITUTION;
    }
    // 平面の天井は持たない。上部は楕円アーチが受け止める。
    // y < 0 は数値誤差の逃げ。
    if ball.y < 0.0 {
        ball.y = 0.0;
        if ball.vy < 0.0 {
            ball.vy = -ball.vy * WALL_RESTITUTION;
        }
    }
}

/// 逆U字の内壁。幾何は `Arch`、反発と接線とばらけはここの材質定数。
fn bounce_arch(ball: &mut Ball, seed: &mut u32) {
    let Some(hit) = Arch::TABLE.push_inside(ball.x, ball.y, BALL_R) else {
        return;
    };
    ball.x = hit.x;
    ball.y = hit.y;
    let vn = ball.vx * hit.nx + ball.vy * hit.ny;
    if vn > 0.0 {
        ball.vx -= (1.0 + ARCH_RESTITUTION) * vn * hit.nx;
        ball.vy -= (1.0 + ARCH_RESTITUTION) * vn * hit.ny;
        ball.vx += (rand01(seed) - 0.5) * 2.0 * ARCH_SCATTER;
    }
    let tx = -hit.ny;
    let ty = hit.nx;
    let vt = ball.vx * tx + ball.vy * ty;
    let lost = vt * (1.0 - ARCH_TANGENTIAL_KEEP);
    ball.vx -= lost * tx;
    ball.vy -= lost * ty;
}

fn bounce_nails(ball: &mut Ball, nails: &[Nail], seed: &mut u32) {
    for nail in nails {
        // 玉数×釘数×サブステップ分の距離計算になるため、y 差だけで先に弾く。
        let dy = nail.y - ball.y;
        if dy.abs() > CONTACT_DIST {
            continue;
        }
        let dx = nail.x - ball.x;
        let dist_sq = dx * dx + dy * dy;
        if dist_sq >= CONTACT_DIST * CONTACT_DIST {
            continue;
        }
        // 釘中心から玉へ向かう法線。真上から落ちて距離 0 になった場合だけは
        // 法線が定まらないので、真上へ逃がす。
        let dist = dist_sq.sqrt();
        let (nx, ny) = if dist < 1e-6 {
            (0.0, -1.0)
        } else {
            (-dx / dist, -dy / dist)
        };
        ball.x = nail.x + nx * CONTACT_DIST;
        ball.y = nail.y + ny * CONTACT_DIST;
        let vn = ball.vx * nx + ball.vy * ny;
        if vn < 0.0 {
            let rest = if vn.abs() < NAIL_TANGLE_VN {
                NAIL_TANGLE_RESTITUTION
            } else {
                NAIL_RESTITUTION
            };
            ball.vx -= (1.0 + rest) * vn * nx;
            ball.vy -= (1.0 + rest) * vn * ny;
            // 接線成分を落とすと、釘の側面を転がって方向が変わる。
            let tx = -ny;
            let ty = nx;
            let vt = ball.vx * tx + ball.vy * ty;
            let keep = NAIL_TANGENTIAL_KEEP;
            ball.vx -= vt * (1.0 - keep) * tx;
            ball.vy -= vt * (1.0 - keep) * ty;
        }
        ball.vx += (rand01(seed) - 0.5) * NAIL_SCATTER;
        ball.hit_glow = HIT_GLOW_TICKS;
    }
}

fn clamp_speed(ball: &mut Ball) {
    let speed = (ball.vx * ball.vx + ball.vy * ball.vy).sqrt();
    if speed > MAX_SPEED {
        let scale = MAX_SPEED / speed;
        ball.vx *= scale;
        ball.vy *= scale;
    }
}

#[cfg(test)]
mod tests {
    use super::super::nails::{generate_nails, RAIL_TOP_Y};
    use super::super::state::{
        BallTint, Machine, Phase, LAUNCH_X, LAUNCH_Y, MACHINE_SPECS, START_POCKET_Y,
    };
    use super::*;

    fn empty_machine(nails: Vec<Nail>) -> Machine {
        Machine {
            name: MACHINE_SPECS[0].0,
            spec: MACHINE_SPECS[0].1,
            nail_spread: 0.5,
            rail_bias: 0.0,
            nails,
            balls_spent: 0,
            spins_seen: 0,
            normal_balls_spent: 0,
            normal_spins_seen: 0,
        }
    }

    fn state_with_nails(nails: Vec<Nail>) -> PachinkoState {
        let mut state = PachinkoState::new();
        state.machines = vec![empty_machine(nails)];
        state.seat = 0;
        state.phase = Phase::Playing;
        state
    }

    fn test_ball(x: f64, y: f64, vx: f64, vy: f64) -> Ball {
        Ball::falling(x, y, vx, vy, true, BallTint::Gold)
    }

    fn shrunk_arch_factor(x: f64, y: f64) -> f64 {
        let arch = Arch::TABLE;
        let rx = arch.a - BALL_R;
        let ry = arch.b - BALL_R;
        let fx = (x - arch.cx) / rx;
        let fy = (y - arch.cy) / ry;
        fx * fx + fy * fy
    }

    #[test]
    fn a_ball_bounces_off_a_nail_instead_of_passing_through() {
        let mut state = state_with_nails(vec![Nail { x: 32.0, y: 30.0 }]);
        state.balls.push(test_ball(32.0, 28.0, 0.0, 0.5));
        step_balls(&mut state);
        let ball = state.balls.first().expect("玉が消えている");
        assert_eq!(
            ball.hit_glow, HIT_GLOW_TICKS,
            "釘に接触したのに衝突が記録されていない"
        );
        assert!(
            ball.y < 30.0,
            "玉が釘をすり抜けて下へ抜けている (y={:.2})",
            ball.y
        );
    }

    #[test]
    fn a_downward_hit_rebounds_up_with_most_of_its_speed() {
        // 釘に当たってもすぐ下へ滑ると、跳ねる弧が見えない。速い衝突は
        // 入射の大部分を保って上へ返し、次の釘まで飛ぶ軌跡を残す。
        let incoming = 0.45;
        let mut ball = test_ball(32.0, 30.0 - CONTACT_DIST + 0.02, 0.0, incoming);
        let nails = [Nail { x: 32.0, y: 30.0 }];
        let mut seed = 1u32;
        bounce_nails(&mut ball, &nails, &mut seed);
        assert!(
            ball.vy < 0.0,
            "下向きの衝突なのに上へ跳ねていない (vy={:.3})",
            ball.vy
        );
        let returned = -ball.vy;
        assert!(
            returned >= incoming * 0.88,
            "釘の跳ねが弱く、弧が見えない (入射={incoming:.3} 跳ね={returned:.3})"
        );
    }

    #[test]
    fn a_ball_going_up_the_right_hits_the_arch_and_falls() {
        // 右肩のカーブに当たってから落ちる。空中で重力だけで折り返すと
        // 反射角が無く、落ちる列が初速だけで決まる。
        // 衝突はサブステップ内で完結するので、tick 境界の vy 反転位置ではなく
        // 天井へ最も近づいた距離で当たったことを見る。
        let mut state = state_with_nails(Vec::new());
        let (vx, vy) = launch_velocity(62);
        state.balls.push(test_ball(LAUNCH_X, LAUNCH_Y, vx, vy));
        let mut min_gap = f64::MAX;
        let mut closest = (LAUNCH_X, LAUNCH_Y);
        let mut min_y = LAUNCH_Y;
        let arch = Arch::TABLE;
        for _ in 0..120 {
            step_balls(&mut state);
            let Some(ball) = state.balls.first() else {
                break;
            };
            min_y = min_y.min(ball.y);
            let gap = ball.y - arch.ceiling_y(ball.x);
            if gap < min_gap {
                min_gap = gap;
                closest = (ball.x, ball.y);
            }
            if ball.y >= RAIL_TOP_Y && ball.vy > 0.0 {
                break;
            }
        }
        assert!(
            min_gap < BALL_R * 2.5,
            "アーチに当たっていない (min_gap={min_gap:.2} at x={:.2} y={:.2} 天井={:.2} min_y={min_y:.2})",
            closest.0,
            closest.1,
            arch.ceiling_y(closest.0)
        );
        assert!(
            closest.0 > BOARD_W * 0.7,
            "右肩以外で天井に近づいている (x={:.2} y={:.2})",
            closest.0,
            closest.1
        );
        let f = shrunk_arch_factor(closest.0, closest.1);
        assert!(
            f > 0.85,
            "空中で失速して落ちている (x={:.2} y={:.2} f={f:.2})",
            closest.0,
            closest.1
        );
    }

    #[test]
    fn a_ball_that_hits_the_arch_falls_into_the_playfield() {
        // 肩に当たった玉は右端に張り付かず、釘帯の内側へ落ちる。
        let mut state = state_with_nails(Vec::new());
        let (vx, vy) = launch_velocity(62);
        state.balls.push(test_ball(LAUNCH_X, LAUNCH_Y, vx, vy));
        let mut x_at_nails = None;
        for _ in 0..200 {
            step_balls(&mut state);
            let Some(ball) = state.balls.first() else {
                break;
            };
            if ball.vy > 0.0 && ball.y >= RAIL_TOP_Y {
                x_at_nails = Some(ball.x);
                break;
            }
        }
        let x = x_at_nails.expect("釘帯まで届いていない");
        assert!(
            x < LAUNCH_X - 1.0,
            "アーチに当たった玉が右端へ戻っている (x={x:.2})"
        );
        assert!(
            x > BOARD_W * 0.35,
            "右肩から落ちた玉が左へ飛びすぎている (x={x:.2})"
        );
    }

    #[test]
    fn a_ball_falling_through_the_rail_hits_nails_on_the_way_down() {
        // 千鳥は「段を落ちるたびに釘へ当たる」ための打ち方。隙間を外して
        // ヘソまで届くなら、格子の位相がずれている。
        let mut seed = 0xA11C_E5ED;
        let nails = generate_nails(&mut seed, 0.55, 0.0);
        let mut state = state_with_nails(nails);
        state
            .balls
            .push(test_ball(BOARD_W / 2.0, RAIL_TOP_Y - 1.5, 0.15, 0.25));
        let mut contacts = 0u32;
        for _ in 0..400 {
            step_balls(&mut state);
            let Some(ball) = state.balls.first() else {
                break;
            };
            if ball.hit_glow == HIT_GLOW_TICKS {
                contacts += 1;
            }
            if ball.y > START_POCKET_Y {
                break;
            }
        }
        assert!(
            contacts >= 3,
            "千鳥を落ちても釘にほとんど当たらない (接触={contacts}tick)"
        );
    }

    #[test]
    fn balls_stay_inside_the_inverted_u() {
        let mut state = state_with_nails(Vec::new());
        let (vx, vy) = launch_velocity(80);
        state.balls.push(test_ball(LAUNCH_X, LAUNCH_Y, vx, vy));
        for _ in 0..400 {
            step_balls(&mut state);
            for ball in &state.balls {
                assert!(
                    Playfield::TABLE.contains(ball.x, ball.y, 0.0),
                    "玉が逆U字の盤面の外へ出た ({:.2}, {:.2})",
                    ball.x,
                    ball.y
                );
            }
        }
    }
}
