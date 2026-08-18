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
/// 定数は `GRAVITY` 単体ではない。`rail_start` / `HORIZONTAL_DRAG` /
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
/// レールを離す角度のばらつき。同じ強度でも 12 時の左右に割れ、釘帯の入口が
/// 一本にならない。
const RAIL_UNTIL_JITTER: f64 = 0.18;
/// 到達角が出っ張りからこの内側なら、壁を沿って跳ね 12時へ戻す。
/// 届かない打ち出しと「当たって戻る」打ち出しを強度で分けられる幅。
const BUMP_REACH: f64 = 0.22;
/// 出っ張りで跳ね返したあとの速さの下限。12時まで登り返す勢いを残す。
const BUMP_RETURN_SPEED: f64 = 0.82;
/// 出っ張り衝突の反発。1 だと行きの速さをそのまま持ち帰り、0 だとその場で落ちる。
const BUMP_RESTITUTION: f64 = 0.92;
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

/// ハンドル強度 (0〜100) から、逆U字レールを離す角度を決める。
///
/// 強度 0 は右足 (3時) のすぐ先、既定の 62 は頂点 (12時)、100 は 10時の
/// 出っ張り。出っ張りに届いた玉はそこで跳ね、12時まで戻ってから落ちる。
/// 天井を壁として跳ね返すと、どの強度でも右肩で落ちて頂点まで届かない。
pub fn rail_release_theta(power: u8) -> f64 {
    let p = (power as f64 / 100.0).clamp(0.0, 1.0);
    let start = Arch::THETA_RIGHT - 0.22;
    let twelve = Arch::THETA_TOP;
    let bump = Arch::THETA_BUMP;
    if p <= 0.62 {
        start + (twelve - start) * (p / 0.62)
    } else {
        twelve + (bump - twelve) * ((p - 0.62) / 0.38)
    }
}

/// レール上の速さ。頂点まで届くだけの勢いを持ち、`MAX_SPEED` は超えない。
fn rail_speed(power: u8) -> f64 {
    let p = (power as f64 / 100.0).clamp(0.0, 1.0);
    0.62 + p * 0.42
}

/// 打ち出し1発分のレール始点。logic が玉を置くときに使う。
pub struct RailStart {
    pub x: f64,
    pub y: f64,
    pub vx: f64,
    pub vy: f64,
    pub theta: f64,
    pub until: f64,
}

pub fn rail_start(power: u8, seed: &mut u32) -> RailStart {
    let arch = Arch::TABLE;
    let theta = Arch::THETA_RIGHT;
    let speed_j = 1.0 + (rand01(seed) - 0.5) * 2.0 * LAUNCH_SPEED_JITTER;
    let speed = (rail_speed(power) * speed_j).clamp(0.2, MAX_SPEED);
    let aimed = rail_release_theta(power) + (rand01(seed) - 0.5) * 2.0 * RAIL_UNTIL_JITTER;
    let until = if aimed <= Arch::THETA_BUMP + BUMP_REACH {
        Arch::THETA_BUMP
    } else {
        aimed.clamp(Arch::THETA_BUMP, Arch::THETA_RIGHT - 0.12)
    };
    let (x, y) = arch.inner_point(theta, BALL_R);
    let (tx, ty) = arch.tangent_decreasing(theta, BALL_R);
    RailStart {
        x,
        y,
        vx: tx * speed,
        vy: ty * speed,
        theta,
        until,
    }
}

/// 1サブステップ、内壁に沿って進む。行きは θ を減らし、出っ張りに届いたら
/// 向きを反転して 12時まで戻す。`rail_until` に達するか速さが尽きると離す。
fn step_rail(ball: &mut Ball) {
    let arch = Arch::TABLE;
    let (_, mut ty) = arch.tangent_decreasing(ball.rail_theta, BALL_R);
    if ball.rail_returning {
        ty = -ty;
    }
    let mut speed = (ball.vx * ball.vx + ball.vy * ball.vy).sqrt();
    if speed < 1e-6 {
        speed = 0.5;
    }
    speed += GRAVITY * ty * 0.4;
    speed = speed.clamp(0.06, MAX_SPEED);
    let metric = arch.arc_metric(ball.rail_theta, BALL_R).max(1e-6);
    let dtheta = speed / metric;
    if ball.rail_returning {
        ball.rail_theta += dtheta;
    } else {
        ball.rail_theta -= dtheta;
    }

    let mut leave = false;
    if ball.rail_returning {
        if ball.rail_theta >= ball.rail_until {
            ball.rail_theta = ball.rail_until;
            leave = true;
        }
    } else if ball.rail_theta <= ball.rail_until {
        if Arch::rail_hits_bump(ball.rail_until) {
            ball.rail_returning = true;
            ball.rail_until = Arch::THETA_TOP;
            ball.rail_theta = Arch::THETA_BUMP;
            speed = (speed * BUMP_RESTITUTION).max(BUMP_RETURN_SPEED);
        } else {
            ball.rail_theta = ball.rail_until;
            leave = true;
        }
    } else if speed <= 0.07 {
        leave = true;
    }

    let theta = ball.rail_theta;
    let (x, y) = arch.inner_point(theta, BALL_R);
    ball.x = x;
    ball.y = y;
    let (mut tx, mut ty) = arch.tangent_decreasing(theta, BALL_R);
    if ball.rail_returning {
        tx = -tx;
        ty = -ty;
    }
    let leave_speed = if leave { speed.max(0.55) } else { speed };
    ball.vx = tx * leave_speed;
    ball.vy = ty * leave_speed;
    if leave {
        ball.on_rail = false;
        let (nx, ny) = arch.outward_normal(theta, BALL_R);
        ball.vx -= nx * 0.10;
        ball.vy -= ny * 0.10;
    }
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
            if ball.on_rail {
                step_rail(ball);
                continue;
            }
            let prev_y = ball.y;
            ball.vy += GRAVITY;
            ball.x += ball.vx;
            ball.y += ball.vy;
            // レールを離したあとの横速さを、釘帯に入るまで残す。頂点から
            // 落ちた玉の横成分をすぐ殺すと、中央の隙間を縦に抜ける。
            if ball.y >= Arch::TABLE.b {
                ball.vx *= HORIZONTAL_DRAG;
            }
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
    bounce_bump(ball, seed);
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

/// 10時の出っ張り。レール上の跳ねは `step_rail` が角度で扱い、ここは盤内を
/// 落ちる玉が塗りつぶしをすり抜けないための円衝突。材質はアーチと同じ。
fn bounce_bump(ball: &mut Ball, seed: &mut u32) {
    let (cx, cy) = Arch::TABLE.bump_center();
    let dx = cx - ball.x;
    let dy = cy - ball.y;
    let contact = BALL_R + Arch::BUMP_R;
    let dist_sq = dx * dx + dy * dy;
    if dist_sq >= contact * contact {
        return;
    }
    let dist = dist_sq.sqrt();
    let (nx, ny) = if dist < 1e-6 {
        (0.0, -1.0)
    } else {
        (-dx / dist, -dy / dist)
    };
    ball.x = cx + nx * contact;
    ball.y = cy + ny * contact;
    let vn = ball.vx * nx + ball.vy * ny;
    if vn < 0.0 {
        ball.vx -= (1.0 + ARCH_RESTITUTION) * vn * nx;
        ball.vy -= (1.0 + ARCH_RESTITUTION) * vn * ny;
        ball.vx += (rand01(seed) - 0.5) * 2.0 * ARCH_SCATTER;
    }
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
    use super::super::nails::{generate_nails, RAIL_ROW_DY, RAIL_TOP_Y};
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

    fn launched_at(power: u8, seed: u32) -> Ball {
        let mut seed = seed;
        let start = rail_start(power, &mut seed);
        Ball::falling(
            start.x,
            start.y,
            start.vx,
            start.vy,
            true,
            BallTint::Gold,
        )
        .with_rail(start.theta, start.until)
    }

    fn test_ball(x: f64, y: f64, vx: f64, vy: f64) -> Ball {
        Ball::falling(x, y, vx, vy, true, BallTint::Gold)
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
    fn a_default_launch_rides_the_arch_to_twelve() {
        // 天井を壁として跳ね返すと右肩 (3時) で落ちる。レールとして滑らせると
        // 既定の強度で頂点 (12時) まで届く。
        let mut state = state_with_nails(Vec::new());
        state.balls.push(launched_at(62, 1));
        let mut min_y = LAUNCH_Y;
        let mut x_at_peak = LAUNCH_X;
        let mut left_rail = false;
        for _ in 0..200 {
            step_balls(&mut state);
            let Some(ball) = state.balls.first() else {
                break;
            };
            if ball.y < min_y {
                min_y = ball.y;
                x_at_peak = ball.x;
            }
            if !ball.on_rail {
                left_rail = true;
                break;
            }
        }
        assert!(left_rail, "レールを離れていない");
        assert!(
            min_y < Arch::TABLE.b * 0.22,
            "12時まで届いていない (min_y={min_y:.2} x={x_at_peak:.2})"
        );
        assert!(
            (x_at_peak - BOARD_W / 2.0).abs() < BOARD_W * 0.22,
            "頂点付近で離していない (x={x_at_peak:.2} y={min_y:.2})"
        );
    }

    #[test]
    fn a_strong_launch_reaches_the_ten_oclock_bump_and_returns_to_twelve() {
        // 強い打ち出しは 12時を越えて 10時の出っ張りまで壁を沿い、跳ねて
        // 頂点へ戻ってから落ちる。行きっぱなしだと左壁沿いに落ちる。
        let mut state = state_with_nails(Vec::new());
        state.balls.push(launched_at(100, 5));
        let bump = Arch::TABLE.inner_point(Arch::THETA_BUMP, BALL_R);
        let mut min_x = LAUNCH_X;
        let mut reached_bump = false;
        let mut x_leave = LAUNCH_X;
        let mut y_leave = LAUNCH_Y;
        for _ in 0..400 {
            step_balls(&mut state);
            let Some(ball) = state.balls.first() else {
                break;
            };
            if ball.on_rail {
                min_x = min_x.min(ball.x);
                if (ball.x - bump.0).abs() < 3.0 && (ball.y - bump.1).abs() < 3.0 {
                    reached_bump = true;
                }
            } else {
                x_leave = ball.x;
                y_leave = ball.y;
                break;
            }
        }
        assert!(
            reached_bump,
            "強い打ち出しが出っ張りまで届いていない (min_x={min_x:.2})"
        );
        assert!(
            min_x < BOARD_W * 0.28,
            "10時側まで回っていない (min_x={min_x:.2})"
        );
        assert!(
            (x_leave - BOARD_W / 2.0).abs() < BOARD_W * 0.22,
            "跳ね返り後に12時付近で離していない (x={x_leave:.2} y={y_leave:.2})"
        );
        assert!(
            y_leave < Arch::TABLE.b * 0.22,
            "12時の高さで離していない (y={y_leave:.2})"
        );
    }

    #[test]
    fn a_default_launch_does_not_reach_the_ten_oclock_bump() {
        // 既定の強度は 12時で離し、出っ張りまで行かない。届くかどうかが
        // ハンドルの強さの差になる。
        let mut state = state_with_nails(Vec::new());
        state.balls.push(launched_at(62, 1));
        let bump = Arch::TABLE.inner_point(Arch::THETA_BUMP, BALL_R);
        let mut min_x = LAUNCH_X;
        let mut x_leave = LAUNCH_X;
        for _ in 0..200 {
            step_balls(&mut state);
            let Some(ball) = state.balls.first() else {
                break;
            };
            if ball.on_rail {
                min_x = min_x.min(ball.x);
            } else {
                x_leave = ball.x;
                break;
            }
        }
        assert!(
            min_x > bump.0 + 8.0,
            "既定の打ち出しが出っ張りまで届いている (min_x={min_x:.2} bump_x={:.2})",
            bump.0
        );
        assert!(
            (x_leave - BOARD_W / 2.0).abs() < BOARD_W * 0.22,
            "既定が12時付近で離していない (x={x_leave:.2})"
        );
    }

    #[test]
    fn a_falling_ball_bounces_off_the_ten_oclock_bump() {
        let (cx, cy) = Arch::TABLE.bump_center();
        let mut state = state_with_nails(Vec::new());
        let gap = Arch::BUMP_R + BALL_R - 0.04;
        state.balls.push(test_ball(cx + gap, cy + 0.2, -0.45, 0.05));
        let x_before = state.balls[0].x;
        step_balls(&mut state);
        let ball = state.balls.first().expect("玉が消えている");
        assert!(
            ball.x >= x_before - 0.15,
            "出っ張りを左へすり抜けている (x={:.2} from {x_before:.2})",
            ball.x
        );
        assert!(
            ball.vx > -0.15,
            "出っ張りで跳ねていない (vx={:.3})",
            ball.vx
        );
    }

    #[test]
    fn a_weak_launch_falls_near_three_oclock() {
        let mut state = state_with_nails(Vec::new());
        state.balls.push(launched_at(8, 2));
        let mut x_leave = LAUNCH_X;
        for _ in 0..200 {
            step_balls(&mut state);
            let Some(ball) = state.balls.first() else {
                break;
            };
            if !ball.on_rail {
                x_leave = ball.x;
                break;
            }
        }
        assert!(
            x_leave > BOARD_W * 0.72,
            "弱い打ち出しが頂点まで回っている (x={x_leave:.2})"
        );
    }

    #[test]
    fn a_ball_that_leaves_the_arch_falls_into_the_playfield() {
        // 12時で離した玉は右端に張り付かず、釘帯の中央付近へ落ちる。
        let mut state = state_with_nails(Vec::new());
        state.balls.push(launched_at(62, 3));
        let mut x_at_nails = None;
        for _ in 0..200 {
            step_balls(&mut state);
            let Some(ball) = state.balls.first() else {
                break;
            };
            if !ball.on_rail && ball.vy > 0.0 && ball.y >= RAIL_TOP_Y {
                x_at_nails = Some(ball.x);
                break;
            }
        }
        let x = x_at_nails.expect("釘帯まで届いていない");
        assert!(
            x < LAUNCH_X - 8.0,
            "レールを離した玉が右端へ戻っている (x={x:.2})"
        );
        assert!(
            (x - BOARD_W / 2.0).abs() < BOARD_W * 0.35,
            "12時から落ちた玉が中央を外している (x={x:.2})"
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
    fn a_ball_keeps_hitting_nails_after_the_mid_rail() {
        // 中段を抜けると隙間を真っ直ぐ落ち、跳ねる絵が消える。千鳥はヘソ前まで
        // 届き、下半分でも釘に当たる。
        let mut seed = 0xA11C_E5ED;
        let nails = generate_nails(&mut seed, 0.55, 0.0);
        let mut state = state_with_nails(nails);
        state
            .balls
            .push(test_ball(BOARD_W / 2.0, RAIL_TOP_Y - 1.5, 0.12, 0.22));
        let mid_y = RAIL_TOP_Y + RAIL_ROW_DY * 3.0;
        let mut lower_contacts = 0u32;
        for _ in 0..500 {
            step_balls(&mut state);
            let Some(ball) = state.balls.first() else {
                break;
            };
            if ball.y > mid_y && ball.hit_glow == HIT_GLOW_TICKS {
                lower_contacts += 1;
            }
            if ball.y > START_POCKET_Y {
                break;
            }
        }
        assert!(
            lower_contacts >= 2,
            "中段より下で釘に当たらず落ちている (接触={lower_contacts}tick)"
        );
    }

    #[test]
    fn balls_stay_inside_the_inverted_u() {
        let mut state = state_with_nails(Vec::new());
        state.balls.push(launched_at(80, 4));
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

    /// 既定強度の打ち出しが 3時から 12時へ沿う軌跡と、強い打ち出しが
    /// 10時の出っ張りで跳ねて 12時へ戻る軌跡を文字で出す。
    ///
    /// `cargo test --lib games::pachinko::physics::tests::dump_launch_path_to_twelve -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn dump_launch_path_to_twelve() {
        dump_launch_path(62, "default 62");
        dump_launch_path(100, "strong 100");
    }

    fn dump_launch_path(power: u8, label: &str) {
        let mut state = state_with_nails(Vec::new());
        state.balls.push(launched_at(power, 7));
        let cols = 64usize;
        let rows = 20usize;
        let mut grid = vec![vec![' '; cols]; rows];
        let plot = |grid: &mut [Vec<char>], x: f64, y: f64, mark: char| {
            let c = (x / BOARD_W * cols as f64).floor() as i32;
            let r = (y / 20.0 * rows as f64).floor() as i32;
            if (0..cols as i32).contains(&c) && (0..rows as i32).contains(&r) {
                grid[r as usize][c as usize] = mark;
            }
        };
        for theta_i in 0..=32 {
            let theta = Arch::THETA_LEFT + (Arch::THETA_RIGHT - Arch::THETA_LEFT) * theta_i as f64 / 32.0;
            let (x, y) = Arch::TABLE.inner_point(theta, BALL_R);
            plot(&mut grid, x, y, '.');
        }
        let (bx, by) = Arch::TABLE.bump_center();
        plot(&mut grid, bx, by, '#');
        for _ in 0..240 {
            let Some(ball) = state.balls.first().copied() else {
                break;
            };
            let mark = if !ball.on_rail {
                'o'
            } else if ball.rail_returning {
                '*'
            } else {
                '@'
            };
            plot(&mut grid, ball.x, ball.y, mark);
            if !ball.on_rail && ball.y > 18.0 {
                break;
            }
            step_balls(&mut state);
        }
        eprintln!("=== launch path {label} (64x20, y=0..20) @=out * =return o=free .=arch #=bump ===");
        for (i, row) in grid.iter().enumerate() {
            eprintln!("{:2}|{}|", i, row.iter().collect::<String>());
        }
    }
}
