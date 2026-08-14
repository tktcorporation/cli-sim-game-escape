//! 玉響 — ゲームロジック (純粋関数)。
//!
//! `tick()` が1tick分の全処理 (演出タイマー減衰→打ち出し→物理と入賞判定→
//! ラウンド進行→デジタル→保留消化→玉の補充) を順に進める。render.rs は
//! ここで更新された `PachinkoState` を読むだけで、書き込まない。
//!
//! 台の個性 (`Machine::nail_spread` / `rail_bias`) は `generate_nails` が
//! 釘の座標へ落とし込み、そこから先は物理だけが結果を決める。数値を直接
//! 当落へ掛けないことで、盤面の見た目と実測回転率が食い違わない。

use super::state::{
    Ball, Digit, HistoryEntry, JackpotState, Machine, MachineSpec, Mode, Nail, PachinkoState,
    Pending, PendingRank, Phase, ReachKind, SpinOutcome, StopStyle, ATTACKER_HALF_W,
    ATTACKER_PAYOUT, ATTACKER_X, ATTACKER_Y, BALL_LOAN_COUNT, BALL_LOAN_YEN, BALL_R, BOARD_H,
    BOARD_W, FIRE_INTERVAL_TICKS, HALL_SIZE, HISTORY_LEN, HIT_GLOW_TICKS, INITIAL_REELS, LAUNCH_X,
    LAUNCH_Y, MACHINE_SPECS, MAX_BALLS, MAX_PENDING, NAIL_R, PENDING_PROMOTE_FLASH_TICKS,
    RANK_WEIGHT_TOTAL, REACH_FLASH_TICKS, ROUND_COUNT, ROUND_LIMIT_TICKS, SIDE_PAYOUT,
    SIDE_POCKET_HALF_W, SIDE_POCKET_LEFT_X, SIDE_POCKET_RIGHT_X, SIDE_POCKET_Y, START_FLASH_TICKS,
    START_PAYOUT, START_POCKET_BASE_HALF_W, START_POCKET_X, START_POCKET_Y,
};

// ── 乱数 (xorshift32。seed を state に持たせてセーブ・シミュレーターで再現可能にする) ──

fn rng_next(seed: &mut u32) -> u32 {
    let mut x = *seed;
    if x == 0 {
        // xorshift32 は 0 が不動点で、一度落ちると乱数列が止まる。
        x = 0xDEAD_BEEF;
    }
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *seed = x;
    x
}

fn rng_below(seed: &mut u32, bound: u32) -> u32 {
    if bound == 0 {
        return 0;
    }
    rng_next(seed) % bound
}

fn rand01(seed: &mut u32) -> f64 {
    (rng_next(seed) as f64) / (u32::MAX as f64)
}

fn rand_range(seed: &mut u32, lo: f64, hi: f64) -> f64 {
    lo + (hi - lo) * rand01(seed)
}

// ── 物理 ───────────────────────────────────────────────────────

/// 1 tick あたりの物理サブステップ数。10 ticks/sec のまま1回で進めると、
/// 1ステップの移動量が釘の直径を超えて釘をすり抜ける。
pub const PHYSICS_SUBSTEPS: u32 = 4;
/// 1サブステップあたりの重力加速度。玉が落ちる速さはここが決める。
///
/// 盤面は 10 ticks/sec でしか描き直されないので、1 tick の移動距離が玉の直径
/// (`BALL_R * 2`) を大きく超えると、玉は毎コマ離れた位置に現れ、釘に弾かれる
/// 瞬間が絵として残らない。`simulator::ball_motion_report` がこの移動距離を
/// 実測する。
///
/// 速さを決める定数は `GRAVITY` 単体ではない。`launch_velocity` /
/// `HORIZONTAL_DRAG` / `NAIL_SCATTER` と組で「玉道の形」を決めており、
/// どれか1つだけを触ると形が変わって回転率が動く。玉道を保ったまま T 倍の
/// 時間をかけさせたい場合は、位置が速度の積分・速度が加速度の積分である
/// ことから、速度を 1/T・加速度を 1/T²・1サブステップあたりの減衰を T 乗根
/// にした組で動かす。
const GRAVITY: f64 = 0.030;
/// 釘との衝突の反発係数。弾かれた玉が次の釘まで飛ぶ軌跡が絵として残る程度に
/// 跳ね返す。上げすぎると玉が釘の上で跳ね続けて落ちてこない。
const NAIL_RESTITUTION: f64 = 0.75;
/// 壁・天井との衝突の反発係数。
const WALL_RESTITUTION: f64 = 0.45;
/// 速度の上限。`CONTACT_DIST` 以下に収めてあるので、釘へ真っ直ぐ向かう玉は
/// 必ず1回はサブステップの標本が接触範囲へ入る。ここが接触距離を超えると
/// 速い玉だけが釘をすり抜け、釘に当たるかどうかが速度で変わってしまう。
const MAX_SPEED: f64 = 1.6;
/// 釘に当たった時に横方向へ乗るばらつきの最大幅。同じ軌道で入っても結果が
/// 割れる「パチンコらしさ」の源で、0 にすると釘配置だけで結果が決まる
/// 決定論的な機械になってしまう。速度と同じ次元なので、落下の速さを変える
/// ときは `GRAVITY` の説明にある組で一緒に動かす。
const NAIL_SCATTER: f64 = 0.163;
/// `nail_spread` がヘソの受け口へ効く強さ。ヘソ釘の位置 (`generate_nails`)
/// と当たり判定の幅 (`effective_pocket_half_w`) は同じ係数を共有しないと、
/// 見た目の開きと実際の入りやすさが食い違って釘読みが嘘になる。
const POCKET_SPREAD_GAIN: f64 = 1.3;
/// 玉と釘が接触する距離。
const CONTACT_DIST: f64 = BALL_R + NAIL_R;
/// 1サブステップごとに横方向の速度へ掛かる減衰。打ち出した勢いは盤面を
/// 横切る間に抜け、玉は釘の間をほぼ真下へ落ちていく。減衰が無いと初速の
/// まま左端まで飛んで壁沿いに落ちるだけになり、ハンドル強度が「どこへ
/// 落とすか」を決める操作にならない。
const HORIZONTAL_DRAG: f64 = 0.955;
/// 打ち出し1発ごとの初速のばらつき (割合)。同じ強度でも玉道が完全に一致
/// すると、釘の間に一本の溝ができて全弾が同じ場所へ落ちる。実機のハンドル
/// と同じく、わずかな揺らぎが玉道を散らす。
const LAUNCH_JITTER: f64 = 0.03;

/// ハンドル強度 (0〜100) から打ち出し初速 (1サブステップあたり) を決める。
/// 玉は天井で折り返し、`HORIZONTAL_DRAG` で横の勢いが抜けたところから釘の
/// 間へ落ちるので、強度は「盤面のどこへ落とすか」を決める操作になる。
/// 適正値は台ごとの釘配置で変わるため、ここでは素直な線形写像だけを行い、
/// 良し悪しの判断は盤面に委ねる。
pub fn launch_velocity(power: u8) -> (f64, f64) {
    let p = (power as f64 / 100.0).clamp(0.0, 1.0);
    (-(0.407 + p * 0.963), -0.556 - p * 0.407)
}

/// ヘソの受け口半幅。決まるのは台のヘソ釘の開きと電サポの有無だけなので、
/// 着席中の台に限らずホールに並ぶ台にも同じ式で引ける — ホールの盤面
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

/// サブステップの前後で入賞口の高さを跨いだか。矩形の内包判定にすると、
/// 1サブステップの移動量 (最大 `MAX_SPEED`) が入賞口の高さを超えたときに
/// 素通りする。
fn crossed_downward(prev_y: f64, y: f64, line: f64) -> bool {
    prev_y <= line && y > line
}

/// 1 tick 分の玉の運動と入賞判定。
fn step_balls(state: &mut PachinkoState) {
    if state.seat >= state.machines.len() {
        return;
    }
    // 釘と玉は所有権ごと借り出す。`state` の他のフィールド (乱数 seed) を
    // 玉のループ中に触るため、借用を分離する必要がある。
    let nails = std::mem::take(&mut state.machines[state.seat].nails);
    let mut balls = std::mem::take(&mut state.balls);
    let pocket_half_w = effective_pocket_half_w(state);
    let attacker_open = matches!(state.mode, Mode::Jackpot(_));
    let seed = &mut state.rng_state;

    let mut start_hits = 0u32;
    let mut attacker_hits = 0u32;
    let mut side_hits = 0u32;

    balls.retain_mut(|ball| {
        for _ in 0..PHYSICS_SUBSTEPS {
            let prev_y = ball.y;
            ball.vy += GRAVITY;
            ball.x += ball.vx;
            ball.y += ball.vy;
            ball.vx *= HORIZONTAL_DRAG;
            bounce_walls(ball);
            bounce_nails(ball, &nails, seed);
            clamp_speed(ball);

            if crossed_downward(prev_y, ball.y, START_POCKET_Y)
                && (ball.x - START_POCKET_X).abs() < pocket_half_w
            {
                start_hits += 1;
                return false;
            }
            if crossed_downward(prev_y, ball.y, SIDE_POCKET_Y)
                && ((ball.x - SIDE_POCKET_LEFT_X).abs() < SIDE_POCKET_HALF_W
                    || (ball.x - SIDE_POCKET_RIGHT_X).abs() < SIDE_POCKET_HALF_W)
            {
                side_hits += 1;
                return false;
            }
            if attacker_open
                && crossed_downward(prev_y, ball.y, ATTACKER_Y)
                && (ball.x - ATTACKER_X).abs() < ATTACKER_HALF_W
            {
                attacker_hits += 1;
                return false;
            }
            if ball.y > BOARD_H {
                return false;
            }
        }
        true
    });

    state.balls = balls;
    state.machines[state.seat].nails = nails;

    if side_hits > 0 {
        add_balls(state, side_hits * SIDE_PAYOUT);
    }
    for _ in 0..attacker_hits {
        add_balls(state, ATTACKER_PAYOUT);
        if let Mode::Jackpot(mut j) = state.mode {
            j.count += 1;
            j.payout += ATTACKER_PAYOUT;
            state.mode = Mode::Jackpot(j);
        }
    }
    for _ in 0..start_hits {
        resolve_start_pocket(state);
    }
}

fn bounce_walls(ball: &mut Ball) {
    if ball.x < BALL_R {
        ball.x = BALL_R;
        ball.vx = -ball.vx * WALL_RESTITUTION;
    } else if ball.x > BOARD_W - BALL_R {
        ball.x = BOARD_W - BALL_R;
        ball.vx = -ball.vx * WALL_RESTITUTION;
    }
    if ball.y < BALL_R {
        // 天井。打ち出した玉はレールを駆け上がってここで折り返す。
        ball.y = BALL_R;
        ball.vy = -ball.vy * WALL_RESTITUTION;
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
            ball.vx -= (1.0 + NAIL_RESTITUTION) * vn * nx;
            ball.vy -= (1.0 + NAIL_RESTITUTION) * vn * ny;
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

/// 持ち玉を増やす。自己記録の最高持ち玉はここを通す。
fn add_balls(state: &mut PachinkoState, amount: u32) {
    state.balls_held = state.balls_held.saturating_add(amount);
    state.record.best_balls = state.record.best_balls.max(state.balls_held);
}

// ── 抽選 ───────────────────────────────────────────────────────

/// ヘソ入賞1個分の処理。賞球を払い、保留に空きがあれば当落を確定させる。
fn resolve_start_pocket(state: &mut PachinkoState) {
    add_balls(state, START_PAYOUT);
    state.start_flash = START_FLASH_TICKS;
    if state.pending.len() >= MAX_PENDING {
        // 保留満タン中の入賞は賞球だけ。抽選を受けられない玉が出ることが
        // 「打ち出しを止める」判断の材料になる。
        return;
    }
    let outcome = roll_outcome(state);
    state.pending.push(Pending::new(outcome));
    if let Some(machine) = state.seated_machine_mut() {
        machine.spins_seen += 1;
    }
}

fn seated_spec(state: &PachinkoState) -> MachineSpec {
    state
        .seated_machine()
        .map(|m| m.spec)
        .unwrap_or(MACHINE_SPECS[0].1)
}

/// ヘソ入賞時に当落を確定させる。演出 (リーチ・保留ランク・停止の型・確定
/// シグナル) もここで一緒に決める。演出は当落を決めるのではなく、決まった
/// 当落を何段階に分けて小出しにするかだけを決める — 実機の「先に当落が
/// 決まり、演出が後から説明する」構造をそのまま写している。
pub fn roll_outcome(state: &mut PachinkoState) -> SpinOutcome {
    let spec = seated_spec(state);
    let odds = match state.mode {
        Mode::Kakuhen { .. } => spec.kakuhen_odds,
        _ => spec.normal_odds,
    };
    let assisted = state.mode.is_assisted();
    let seed = &mut state.rng_state;
    let hit = rng_below(seed, odds.max(1)) == 0;
    let reach = pick_reach(hit, seed);
    let rounds = if hit { pick_rounds(spec, seed) } else { 0 };
    let kakuhen = hit && rng_below(seed, 100) < spec.kakuhen_rate;
    let rank = pick_rank(hit, seed);
    let stop = pick_stop_style(hit, reach, rank, seed);
    let confirmed = pick_confirmed(hit, rank, seed);
    let reels = pick_reels(hit, reach, seed);
    SpinOutcome {
        hit,
        rounds,
        kakuhen,
        reach,
        rank,
        stop,
        confirmed,
        assisted,
        reels,
    }
}

/// 演出の格を引く。当たり側とハズレ側で配分を変えることで「格が上がるほど
/// 当たりが近い」という関係が生まれる。プレミアはハズレ側の配分を 0 にして
/// あり、出れば当たりになる。この対応関係は UI では明かさない。
fn pick_reach(hit: bool, seed: &mut u32) -> ReachKind {
    let roll = rng_below(seed, 100);
    if hit {
        match roll {
            0..=19 => ReachKind::Normal,
            20..=84 => ReachKind::Super,
            _ => ReachKind::Premium,
        }
    } else {
        match roll {
            0..=87 => ReachKind::None,
            88..=96 => ReachKind::Normal,
            _ => ReachKind::Super,
        }
    }
}

/// 保留が最終的に到達するランクを引く。当たり側とハズレ側で別の重み表を
/// 使い、その比が信頼度を決める (`PendingRank::weight_on_hit` 参照)。
fn pick_rank(hit: bool, seed: &mut u32) -> PendingRank {
    let weight = |rank: PendingRank| {
        if hit {
            rank.weight_on_hit()
        } else {
            rank.weight_on_miss()
        }
    };
    let mut roll = rng_below(seed, RANK_WEIGHT_TOTAL);
    for rank in PendingRank::ALL {
        let w = weight(rank);
        if roll < w {
            return rank;
        }
        roll -= w;
    }
    PendingRank::White
}

/// 復活が出る当たりの割合 (%)。稀であることそのものが効き目なので、ここを
/// 上げると「復活したのに当たり前」になって演出が死ぬ。
const REVIVAL_PERCENT: u32 = 3;
/// 惜しいハズレを出す割合 (%)。条件を満たすハズレの中での割合。
const NEAR_MISS_PERCENT: u32 = 35;
/// 滑りが出る割合 (%)。当たり/ハズレどちらでも起きるので、滑った時点では
/// まだ何も分からない — この「分からなさ」が期待を延ばす。
const SLIP_PERCENT: u32 = 10;
/// 虹以外の当たりで確定シグナルが立つ割合 (%)。
const CONFIRMED_PERCENT: u32 = 6;

/// 惜しいハズレを出してよい状況か。無条件に出すと「またか」で慣れてしまい、
/// 惜しさが情報を運ばなくなる。熱い保留かスーパーリーチ以上に限ることで、
/// 「期待した上で惜しかった」という筋書きが成り立つ場面だけに絞る。
fn near_miss_ready(reach: ReachKind, rank: PendingRank) -> bool {
    rank >= PendingRank::Green || reach >= ReachKind::Super
}

/// デジタルの止まり方を引く。当落は既に決まっているので、ここは「決まった
/// 結果をどう見せるか」だけを選ぶ。
fn pick_stop_style(
    hit: bool,
    reach: ReachKind,
    rank: PendingRank,
    seed: &mut u32,
) -> StopStyle {
    if hit {
        if rng_below(seed, 100) < REVIVAL_PERCENT {
            return StopStyle::Revival;
        }
    } else if near_miss_ready(reach, rank) && rng_below(seed, 100) < NEAR_MISS_PERCENT {
        return StopStyle::NearMiss;
    }
    if rng_below(seed, 100) < SLIP_PERCENT {
        StopStyle::Slip
    } else {
        StopStyle::Plain
    }
}

/// 確定シグナルを立てるか。虹保留は信頼度 100% なので必ず立て、それ以外の
/// 当たりでは稀に立てる。「確定が存在する」ことが、確定ではない他の全ての
/// 演出に天井を与える — 期待の階段は、上り切れる場所があって初めて階段になる。
fn pick_confirmed(hit: bool, rank: PendingRank, seed: &mut u32) -> bool {
    if !hit {
        return false;
    }
    rank == PendingRank::Rainbow || rng_below(seed, 100) < CONFIRMED_PERCENT
}

fn pick_rounds(spec: MachineSpec, seed: &mut u32) -> u32 {
    let total: u32 = spec.round_table.iter().map(|&(_, w)| w).sum();
    let mut roll = rng_below(seed, total.max(1));
    for &(rounds, weight) in spec.round_table {
        if roll < weight {
            return rounds;
        }
        roll -= weight;
    }
    spec.round_table.first().map(|&(r, _)| r).unwrap_or(1)
}

/// 停止出目。当たりはゾロ目、リーチは左右が揃って中だけ違う目、リーチ無しは
/// 左右が揃わない目にする。出目と結果が矛盾すると、演出から当落を読むという
/// 学習そのものが成り立たなくなる。
fn pick_reels(hit: bool, reach: ReachKind, seed: &mut u32) -> [u8; 3] {
    let left = rng_below(seed, 10) as u8;
    if hit {
        return [left, left, left];
    }
    let other = |seed: &mut u32| -> u8 {
        let mut v = rng_below(seed, 9) as u8;
        if v >= left {
            v += 1;
        }
        v
    };
    if reach == ReachKind::None {
        let middle = rng_below(seed, 10) as u8;
        [left, middle, other(seed)]
    } else {
        [left, other(seed), left]
    }
}

/// 変動1回ごとに保留が1段昇格する確率 (%)。毎回上がると最終ランクへ数変動で
/// 着いてしまい、待つ時間が情報を運ばなくなる。
const PROMOTE_PERCENT: u32 = 50;

/// 保留を1つ消化してデジタルを回し始める。
fn start_spin_if_idle(state: &mut PachinkoState) {
    if state.digit != Digit::Idle || state.pending.is_empty() {
        return;
    }
    if matches!(state.mode, Mode::Jackpot(_)) {
        // 大当たり中は保留を溜めるだけ。消化はラウンド消化の後に回る。
        return;
    }
    let outcome = state.pending.remove(0).outcome;
    promote_pending(state);
    if outcome.reach != ReachKind::None {
        state.reach_flash = REACH_FLASH_TICKS;
        state.reach_flash_kind = outcome.reach;
    }
    state.digit = Digit::Spinning {
        ticks_left: outcome.reach.spin_ticks() + outcome.stop.extra_ticks(),
        outcome,
    };
}

/// 待っている保留のランクを最終ランクへ1段ずつ近づける。
///
/// 変動の開始ごとに引き直すことで、保留1個から複数回の情報イベントを取り出す。
/// 「今回っているのはハズレでも、2個先に赤がある」という状態が、当落と無関係な
/// 通常変動を期待の時間に変える。
///
/// 先頭 (次の変動で消化される保留) だけは抽選せず最終ランクまで押し上げる。
/// 消化されると保留は画面から消えるので、そこで昇格の余地を残すと最終ランクが
/// 一度も見られないまま終わる。1変動でも待った保留は必ず最終ランクを見せてから
/// 消化される、というのがここの不変条件になる。
fn promote_pending(state: &mut PachinkoState) {
    let seed = &mut state.rng_state;
    for (index, pending) in state.pending.iter_mut().enumerate() {
        let target = pending.outcome.rank;
        if pending.rank >= target {
            continue;
        }
        let next = if index == 0 {
            target
        } else if rng_below(seed, 100) < PROMOTE_PERCENT {
            pending.rank.promoted()
        } else {
            continue;
        };
        pending.rank = next;
        pending.promote_flash = PENDING_PROMOTE_FLASH_TICKS;
    }
}

fn advance_digit(state: &mut PachinkoState) {
    let finished = match &mut state.digit {
        Digit::Spinning { ticks_left, outcome } => {
            *ticks_left = ticks_left.saturating_sub(1);
            if *ticks_left == 0 {
                Some(*outcome)
            } else {
                None
            }
        }
        Digit::Idle => None,
    };
    if let Some(outcome) = finished {
        state.digit = Digit::Idle;
        // 停止した出目は液晶に残る。`Digit::Idle` は出目を持たないので、
        // ここで書き戻さないと当たった瞬間にゾロ目が消える。
        state.last_reels = outcome.reels;
        resolve_spin(state, outcome);
    }
}

/// デジタル停止時の処理。当たりなら大当たりへ、ハズレなら電サポの残り回転を
/// 1減らす。
fn resolve_spin(state: &mut PachinkoState, outcome: SpinOutcome) {
    // 当たった回転自体もハマり回数に数える (「42回転で当たった」という
    // 数え方に合わせる) ので、当落を見る前に加算する。
    state.spins_since_jackpot = state.spins_since_jackpot.saturating_add(1);
    if !outcome.hit {
        decay_assist(state);
        end_chain_if_back_to_normal(state);
        return;
    }
    // 連チャンの継続は消化時点のモードではなく抽選時点の状態で決める。保留は
    // 抽選から消化まで時間差があり、その間に電サポが切れることがある —
    // 消化時点で見ると、電サポ中に引いた当たりが初当たりに化ける。
    state.chain = if outcome.assisted { state.chain + 1 } else { 1 };
    state.record.best_chain = state.record.best_chain.max(state.chain);
    state.record.total_jackpots += 1;
    state.history.insert(
        0,
        HistoryEntry {
            rounds: outcome.rounds,
            kakuhen: outcome.kakuhen,
            spins_before: state.spins_since_jackpot,
        },
    );
    state.history.truncate(HISTORY_LEN);
    state.spins_since_jackpot = 0;
    state.jackpot_seq = state.jackpot_seq.wrapping_add(1);
    state.mode = Mode::Jackpot(JackpotState {
        round: 1,
        total_rounds: outcome.rounds.max(1),
        count: 0,
        ticks_left: ROUND_LIMIT_TICKS,
        kakuhen: outcome.kakuhen,
        payout: 0,
    });
    state.add_log(format!("{}Rの大当たり！", outcome.rounds));
}

/// 電サポの残り回転を1消化する。`Kakuhen { spins_left: 0 }` は次回当たりまで
/// 続く印なので減らさない。
fn decay_assist(state: &mut PachinkoState) {
    match state.mode {
        Mode::Kakuhen { spins_left } if spins_left > 0 => {
            let left = spins_left - 1;
            if left == 0 {
                end_assist(state, "確変終了");
            } else {
                state.mode = Mode::Kakuhen { spins_left: left };
            }
        }
        Mode::Jitan { spins_left } => {
            let left = spins_left.saturating_sub(1);
            if left == 0 {
                end_assist(state, "時短終了");
            } else {
                state.mode = Mode::Jitan { spins_left: left };
            }
        }
        _ => {}
    }
}

/// 電サポを終えて通常時へ戻す。
fn end_assist(state: &mut PachinkoState, reason: &str) {
    state.mode = Mode::Normal;
    state.add_log(reason);
}

/// 連チャンを数え直しに戻す。残したままだと通常時の画面が終わった連チャンを
/// 続いているものとして出し続ける。自己記録 (`record.best_chain`) は当たりの
/// たびに更新済みなので失われない。
///
/// 電サポが切れた瞬間ではなく、電サポ中に引いた抽選を消化し尽くしてから戻す。
/// 保留は抽選から消化まで時間差があり、電サポ切れの直後に残った保留で当たる
/// (引き戻す) ことがある — その当たりは電サポ中の抽選なので連チャンの一部で、
/// そこで数え直すと表示も自己記録も過少になる。
///
/// 判定は「電サポ中に引いた保留がまだ残っているか」で行う。消化した抽選が
/// 電サポ外のものかどうかで見ると、電サポ中の保留が全てハズレで尽きた後に
/// 打ち出しを止めた場合、次の抽選が来ないまま連チャン表示が残り続ける。
fn end_chain_if_back_to_normal(state: &mut PachinkoState) {
    if state.mode.is_assisted() {
        return;
    }
    let assisted_left = state.pending.iter().any(|p| p.outcome.assisted);
    if !assisted_left {
        state.chain = 0;
    }
}

/// 大当たりのラウンド進行。玉が入らないまま時間切れになったラウンドも
/// 進めることで、打ち出しを止めたまま大当たりが終わらない状態を防ぐ。
fn advance_jackpot(state: &mut PachinkoState) {
    let Mode::Jackpot(mut jackpot) = state.mode else {
        return;
    };
    jackpot.ticks_left = jackpot.ticks_left.saturating_sub(1);
    let round_over = jackpot.count >= ROUND_COUNT || jackpot.ticks_left == 0;
    if !round_over {
        state.mode = Mode::Jackpot(jackpot);
        return;
    }
    if jackpot.round >= jackpot.total_rounds {
        end_jackpot(state, jackpot);
        return;
    }
    jackpot.round += 1;
    jackpot.count = 0;
    jackpot.ticks_left = ROUND_LIMIT_TICKS;
    state.mode = Mode::Jackpot(jackpot);
}

/// 最終ラウンドを消化し終えた大当たりを閉じ、確変か時短へ送り出す。
///
/// ここで `jackpot_end_seq` を進める。ここが「アタッカーで取れる出玉が
/// 出揃った」唯一の地点で、保存の契機を待たせたくない瞬間にあたる。
fn end_jackpot(state: &mut PachinkoState, jackpot: JackpotState) {
    state.jackpot_end_seq = state.jackpot_end_seq.wrapping_add(1);
    // 出玉は `Mode::Jackpot` が抱えているので、モードを移す前に取り出す。
    // 終了後のサマリはこの値を読む。
    state.last_jackpot_payout = jackpot.payout;
    let spec = seated_spec(state);
    if jackpot.kakuhen {
        state.mode = Mode::Kakuhen { spins_left: 0 };
        state.add_log("確変突入！");
    } else {
        state.mode = Mode::Jitan {
            spins_left: spec.jitan_spins,
        };
        state.add_log(format!("時短{}回", spec.jitan_spins));
    }
}

// ── tick ───────────────────────────────────────────────────────

pub fn tick_n(state: &mut PachinkoState, n: u32) {
    for _ in 0..n {
        tick(state);
    }
}

pub fn tick(state: &mut PachinkoState) {
    if state.phase != Phase::Playing {
        return;
    }
    decay_glow(state);
    try_fire(state);
    step_balls(state);
    advance_jackpot(state);
    advance_digit(state);
    start_spin_if_idle(state);
    auto_reload(state);
    ease_jackpot_payout(state);
}

fn decay_glow(state: &mut PachinkoState) {
    for ball in &mut state.balls {
        ball.hit_glow = ball.hit_glow.saturating_sub(1);
    }
    state.start_flash = state.start_flash.saturating_sub(1);
    state.reach_flash = state.reach_flash.saturating_sub(1);
    for pending in &mut state.pending {
        pending.promote_flash = pending.promote_flash.saturating_sub(1);
    }
}

/// 出玉カウンタの表示値が実際の値へ 1 tick で詰める割合。
pub const PAYOUT_EASE_RATE: f64 = 0.20;
/// 表示値を実際の値へ一致させる差の下限。等比で寄せるだけでは端数が残り続け、
/// 数字が止まったのに桁の下が揺れる。
const PAYOUT_SNAP_DIFF: f64 = 0.5;

/// 表示用の出玉を実際の値へ追いつかせる。差の一定割合ずつ詰めるので、内部値が
/// 一度に跳ねても表示は数 tick かけて登り、数字が動いている時間が伸びる。
fn ease_jackpot_payout(state: &mut PachinkoState) {
    let target = state.jackpot_payout() as f64;
    let diff = target - state.jackpot_payout_shown;
    if diff.abs() < PAYOUT_SNAP_DIFF {
        state.jackpot_payout_shown = target;
    } else {
        state.jackpot_payout_shown += diff * PAYOUT_EASE_RATE;
    }
}

fn try_fire(state: &mut PachinkoState) {
    if state.fire_cooldown > 0 {
        state.fire_cooldown -= 1;
        return;
    }
    if !state.firing || state.balls_held == 0 || state.balls.len() >= MAX_BALLS {
        return;
    }
    let (vx, vy) = launch_velocity(state.power);
    let seed = &mut state.rng_state;
    let jitter = |seed: &mut u32| 1.0 + (rand01(seed) - 0.5) * 2.0 * LAUNCH_JITTER;
    let (vx, vy) = (vx * jitter(seed), vy * jitter(seed));
    state.balls.push(Ball {
        x: LAUNCH_X,
        y: LAUNCH_Y,
        vx,
        vy,
        hit_glow: 0,
    });
    state.balls_held -= 1;
    if let Some(machine) = state.seated_machine_mut() {
        machine.balls_spent += 1;
    }
    state.fire_cooldown = FIRE_INTERVAL_TICKS;
}

/// 玉が尽きたら現金から自動で借りる。盤面に玉が残っている間は待つので、
/// 「最後の1発が入るか」を見届けてから次の千円が減る。
fn auto_reload(state: &mut PachinkoState) {
    if !state.firing || state.balls_held > 0 || !state.balls.is_empty() {
        return;
    }
    if buy_balls(state) {
        return;
    }
    state.firing = false;
    state.add_log("軍資金が尽きた");
}

// ── プレイヤー操作 ─────────────────────────────────────────────

pub fn toggle_fire(state: &mut PachinkoState) -> bool {
    if state.phase != Phase::Playing {
        return false;
    }
    state.firing = !state.firing;
    true
}

/// ハンドル強度を動かす。両端に張り付いている方向へは動かないので false。
pub fn adjust_power(state: &mut PachinkoState, delta: i16) -> bool {
    let next = (state.power as i16 + delta).clamp(0, 100) as u8;
    if next == state.power {
        return false;
    }
    state.power = next;
    true
}

/// 現金を玉に替える。
pub fn buy_balls(state: &mut PachinkoState) -> bool {
    if state.cash < BALL_LOAN_YEN {
        return false;
    }
    state.cash -= BALL_LOAN_YEN;
    state.invested += BALL_LOAN_YEN;
    state.record.total_invested += BALL_LOAN_YEN;
    add_balls(state, BALL_LOAN_COUNT);
    true
}

/// ホールで台を選んで着席する。台ごとの遊技状態 (デジタル・保留・モード) は
/// 引き継がず、持ち玉だけを持ち込む。
pub fn sit_at(state: &mut PachinkoState, index: usize) -> bool {
    if state.phase != Phase::Hall || index >= state.machines.len() {
        return false;
    }
    state.seat = index;
    // 数字キーやタップで直接座った場合も選択を合わせる。席を立った直後の
    // ホールで、今まで打っていた台とは別の台のプレビューが出るのを防ぐ。
    state.hall_cursor = index;
    state.has_seated = true;
    state.phase = Phase::Playing;
    reset_seat(state);
    let name = state.machines[index].name;
    state.add_log(format!("{name} に座った"));
    true
}

/// 席を立ってホールへ戻る。大当たり中は出玉を捨てることになるので断る。
pub fn leave_seat(state: &mut PachinkoState) -> bool {
    if state.phase != Phase::Playing {
        return false;
    }
    if matches!(state.mode, Mode::Jackpot(_)) {
        state.add_log("大当たり中は席を立てない");
        return false;
    }
    reset_seat(state);
    state.phase = Phase::Hall;
    true
}

/// 台に紐づく遊技状態を捨てる。盤面の玉は台に残るので持ち出せない。
fn reset_seat(state: &mut PachinkoState) {
    state.balls.clear();
    state.firing = false;
    state.fire_cooldown = 0;
    state.digit = Digit::Idle;
    // 出目は台の液晶に残るものなので、別の台へ移ったら持ち込まない。
    state.last_reels = INITIAL_REELS;
    state.pending.clear();
    state.mode = Mode::Normal;
    state.chain = 0;
    // 履歴もハマり回数もその台で起きた事実なので、移った先へ持ち込むと
    // 「この台は何回転で当たっているか」という判断材料そのものが嘘になる。
    state.history.clear();
    state.spins_since_jackpot = 0;
    state.start_flash = 0;
    state.reach_flash = 0;
    // 出玉のサマリも前の台で起きた事実なので、移った先へ持ち込まない。
    state.last_jackpot_payout = 0;
    state.jackpot_payout_shown = 0.0;
}

/// 持ち玉を換金して記録へ確定させる。
pub fn cash_out(state: &mut PachinkoState) -> bool {
    if state.balls_held == 0 {
        return false;
    }
    let yen = state.balls_held * BALL_LOAN_YEN / BALL_LOAN_COUNT;
    state.record.total_returned += yen;
    state.cash += yen;
    state.balls_held = 0;
    true
}

// ── 釘 ─────────────────────────────────────────────────────────

/// 寄り釘の段数と、最上段・最下段の y。
///
/// 寄り釘はヘソより上にあり、ここの密度がそのまま回転率を決める。段数・本数・
/// 間隔のどれを動かしても最下段の千鳥の位相がずれ、ヘソの真上に釘が来るか
/// 隙間が来るかが入れ替わって回転率が倍近く動く
/// (`simulator::spin_rate_report` の対照で実測できる)。盤面から釘を減らし
/// たいときは、回転率に効かない下部釘 (`LOWER_ROWS`) の方から間引く。
const RAIL_ROWS: usize = 5;
const RAIL_TOP_Y: f64 = 20.0;
const RAIL_BOTTOM_Y: f64 = 46.0;
/// 寄り釘1段あたりの本数と x の間隔。
const RAIL_NAILS_PER_ROW: usize = 7;
const RAIL_STEP_X: f64 = 8.0;
const RAIL_BASE_X: f64 = 8.0;
/// `rail_bias` が最大のときに外側の釘を中央へ寄せる割合。中央の釘は動かず、
/// 端ほど大きく動くので、盤面では「上部の釘が中央へ傾いている」形に見える。
///
/// 寄り釘は等間隔の格子なので、ここを大きくして格子ごと縮めると、段の隙間が
/// ヘソの真上へ揃う `rail_bias` の値でだけ玉道が一本に繋がり、回転率が跳ね
/// 上がる。跳ね方は `rail_bias` に対して単調ではなく、盤面の見た目からは
/// 読めない。読める手がかり (ヘソ釘の開き) より強い当たり外れを隠し持たせ
/// ないよう、傾きは玉道を大きく変えない範囲に留める。
const RAIL_BIAS_PULL: f64 = 0.02;
/// 釘1本ごとの位置の揺らぎ。同じ `nail_spread` / `rail_bias` の台でも
/// 盤面が同一にならないようにして、台ごとの見た目の個体差を作る。
///
/// ここを大きくすると、玉道を決めるのが「見えるヘソ釘の開き」ではなく
/// 「見えない寄り釘のズレ」になり、盤面から回りやすさを読むという判断軸が
/// 成立しなくなる。`simulator::nail_spread_correlates_with_spin_rate` が
/// その退行を検知する。
const NAIL_JITTER: f64 = 0.15;

/// 下部釘の段数と本数。密度を絞る狙いは `RAIL_ROWS` と同じ。こちらはヘソ
/// より下にあり回転率に効かないので、段数からも間引ける。
const LOWER_ROWS: usize = 3;
const LOWER_TOP_Y: f64 = 58.0;
const LOWER_BOTTOM_Y: f64 = 74.0;
const LOWER_NAILS_PER_ROW: usize = 7;
const LOWER_BASE_X: f64 = 6.2;
const LOWER_STEP_X: f64 = 8.6;

/// 台の釘配置を seed から生成する。`nail_spread` / `rail_bias` を釘の座標
/// そのものへ反映させることで、プレイヤーは盤面を見て回りやすさを推し量れる
/// (数値としては UI に出さない)。
pub fn generate_nails(seed: &mut u32, nail_spread: f64, rail_bias: f64) -> Vec<Nail> {
    let mut nails = Vec::new();
    let center = BOARD_W / 2.0;

    // 寄り釘。段ごとに半ピッチずらして千鳥に組む。ヘソの真上にあたる最下段は
    // 中央を空ける並びにして、玉がヘソへ落ちる道を残す。
    for row in 0..RAIL_ROWS {
        let t = row as f64 / (RAIL_ROWS - 1) as f64;
        let y = RAIL_TOP_Y + (RAIL_BOTTOM_Y - RAIL_TOP_Y) * t;
        let stagger = if row % 2 == 0 { RAIL_STEP_X / 2.0 } else { 0.0 };
        for i in 0..RAIL_NAILS_PER_ROW {
            let base_x = RAIL_BASE_X + stagger + RAIL_STEP_X * i as f64;
            let pulled = center + (base_x - center) * (1.0 - rail_bias * RAIL_BIAS_PULL);
            nails.push(Nail {
                x: pulled + rand_range(seed, -NAIL_JITTER, NAIL_JITTER),
                y: y + rand_range(seed, -NAIL_JITTER * 0.5, NAIL_JITTER * 0.5),
            });
        }
    }

    // ヘソ釘。この2本の間隔が「開いて見える」ことが釘読みの手がかりになるので、
    // 通常時の受け口 (`pocket_half_w`) の外側へ釘半径分だけ逃がした位置に
    // 置き、見た目と当たり判定を一致させる。
    let mouth = pocket_half_w(nail_spread, false) + NAIL_R;
    for side in [-1.0, 1.0] {
        nails.push(Nail {
            x: START_POCKET_X + side * mouth,
            y: START_POCKET_Y - 2.0,
        });
    }

    // 下部釘。ヘソを外した玉を一般入賞口とアタッカーへ振り分ける。
    for row in 0..LOWER_ROWS {
        let t = row as f64 / (LOWER_ROWS - 1) as f64;
        let y = LOWER_TOP_Y + (LOWER_BOTTOM_Y - LOWER_TOP_Y) * t;
        let stagger = if row % 2 == 0 { 0.0 } else { LOWER_STEP_X / 2.0 };
        for i in 0..LOWER_NAILS_PER_ROW {
            let x = LOWER_BASE_X + stagger + LOWER_STEP_X * i as f64;
            nails.push(Nail {
                x: x + rand_range(seed, -NAIL_JITTER, NAIL_JITTER),
                y,
            });
        }
    }

    // アタッカー脇。開放中の受け口へ玉を導く。
    for side in [-1.0, 1.0] {
        nails.push(Nail {
            x: ATTACKER_X + side * (ATTACKER_HALF_W + 1.5),
            y: ATTACKER_Y - 3.0,
        });
    }

    nails
}

/// ホールに並ぶ台のヘソ釘の開きの範囲。
///
/// 下限は「全く回らない台」を並べないための足切り。上限は出玉率 (賞球総数 ÷
/// 打ち込み玉数) の天井を決める — 開くほど回り、回るほど当たるので、ここを
/// 上げすぎると打つほど玉が増える台がホールに並び、有限の軍資金という前提が
/// 崩れる。`simulator::payout_ratio_stays_below_break_even` がこの上限の台を
/// 実際に打って確かめる。
pub const NAIL_SPREAD_RANGE: (f64, f64) = (0.25, 0.80);
/// ホールに並ぶ台の寄り釘の傾きの範囲。効き方は `RAIL_BIAS_PULL` を参照。
pub const RAIL_BIAS_RANGE: (f64, f64) = (-0.6, 0.9);

/// 来店ごとのホールを作る。同じスペックの台が釘だけ違う形で並ぶことがあり、
/// それが釘読みという判断軸を成立させる。
pub fn generate_hall(state: &mut PachinkoState) {
    let mut machines = Vec::with_capacity(HALL_SIZE);
    for _ in 0..HALL_SIZE {
        let pick = rng_below(&mut state.rng_state, MACHINE_SPECS.len() as u32) as usize;
        let (name, spec) = MACHINE_SPECS[pick];
        let nail_spread =
            rand_range(&mut state.rng_state, NAIL_SPREAD_RANGE.0, NAIL_SPREAD_RANGE.1);
        let rail_bias = rand_range(&mut state.rng_state, RAIL_BIAS_RANGE.0, RAIL_BIAS_RANGE.1);
        let nails = generate_nails(&mut state.rng_state, nail_spread, rail_bias);
        machines.push(Machine {
            name,
            spec,
            nail_spread,
            rail_bias,
            nails,
            balls_spent: 0,
            spins_seen: 0,
        });
    }
    state.machines = machines;
    state.seat = 0;
}

/// 千円 (= `BALL_LOAN_COUNT` 玉) あたりの回転数。実測値なので、打ち込んだ
/// 玉が少ないうちは当てにならない。標本が足りない間は `None` を返し、
/// 「まだ分からない」ことを表示側でそのまま出せるようにする。
pub fn spin_rate(machine: &Machine) -> Option<f64> {
    if machine.balls_spent < 50 {
        return None;
    }
    Some(machine.spins_seen as f64 * BALL_LOAN_COUNT as f64 / machine.balls_spent as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seated_state() -> PachinkoState {
        let mut state = PachinkoState::new();
        generate_hall(&mut state);
        assert!(sit_at(&mut state, 0));
        state
    }

    /// 釘だけを差し替えた台に着席した状態。物理の単体検証に使う。
    fn state_with_nails(nails: Vec<Nail>) -> PachinkoState {
        let mut state = seated_state();
        state.machines[0].nails = nails;
        state
    }

    fn start_pocket_gap(nail_spread: f64) -> f64 {
        let mut seed = 0x1234_5678;
        let nails = generate_nails(&mut seed, nail_spread, 0.0);
        let mouth: Vec<f64> = nails
            .iter()
            .filter(|n| (n.y - (START_POCKET_Y - 2.0)).abs() < 1e-9)
            .map(|n| n.x)
            .collect();
        assert_eq!(mouth.len(), 2, "ヘソ釘が2本ではない: {mouth:?}");
        (mouth[0] - mouth[1]).abs()
    }

    #[test]
    fn nail_spread_widens_the_start_pocket_gap() {
        // 釘読みは「ヘソ釘の開きが見た目で分かる」ことが前提。開きが
        // `nail_spread` に連動しないと、盤面から回りやすさを読む軸が消える。
        let narrow = start_pocket_gap(0.15);
        let wide = start_pocket_gap(0.95);
        assert!(
            wide > narrow + 1.0,
            "nail_spread を上げてもヘソ釘が開いていない (狭={narrow:.2} 広={wide:.2})"
        );
    }

    #[test]
    fn rail_bias_pulls_upper_nails_toward_center() {
        // 寄り釘の傾きも釘読みの手がかり。bias を上げたら上部の釘が中央へ
        // 寄る、という対応が崩れると盤面の見た目が何も語らなくなる。
        let center = BOARD_W / 2.0;
        let spread_of = |bias: f64| {
            let mut seed = 0xABCD_1234;
            let nails = generate_nails(&mut seed, 0.5, bias);
            let upper: Vec<&Nail> = nails.iter().filter(|n| n.y < START_POCKET_Y - 4.0).collect();
            let sum: f64 = upper.iter().map(|n| (n.x - center).abs()).sum();
            sum / upper.len() as f64
        };
        assert!(
            spread_of(0.9) < spread_of(-0.6),
            "rail_bias を上げても上部釘が中央へ寄っていない (寄={:.2} 逃={:.2})",
            spread_of(0.9),
            spread_of(-0.6)
        );
    }

    #[test]
    fn a_ball_bounces_off_a_nail_instead_of_passing_through() {
        let mut state = state_with_nails(vec![Nail { x: 32.0, y: 30.0 }]);
        state.balls.push(Ball {
            x: 32.0,
            y: 28.0,
            vx: 0.0,
            vy: 0.5,
            hit_glow: 0,
        });
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
    fn balls_stay_inside_the_board() {
        let mut state = seated_state();
        state.firing = true;
        assert!(buy_balls(&mut state));
        for _ in 0..3_000 {
            tick(&mut state);
            for ball in &state.balls {
                assert!(
                    ball.x >= 0.0 && ball.x <= BOARD_W && ball.y >= 0.0 && ball.y <= BOARD_H,
                    "玉が盤面の外へ出た ({:.2}, {:.2})",
                    ball.x,
                    ball.y
                );
            }
            assert!(state.balls.len() <= MAX_BALLS);
        }
    }

    #[test]
    fn balls_eventually_reach_the_start_pocket() {
        // 釘配置や打ち出し角度が退行して1発も入らなくなると、抽選そのものが
        // 起きなくなる。ホールのどの台でも入賞が起きることを確かめる。
        for seat in 0..HALL_SIZE {
            let mut state = PachinkoState::new();
            generate_hall(&mut state);
            state.phase = Phase::Hall;
            assert!(sit_at(&mut state, seat));
            state.firing = true;
            assert!(buy_balls(&mut state));
            tick_n(&mut state, 3_000);
            assert!(
                state.machines[seat].spins_seen > 0,
                "{} 番目の台でヘソ入賞が1回も起きなかった",
                seat
            );
        }
    }

    #[test]
    fn a_fast_ball_still_registers_the_start_pocket() {
        // 1サブステップの移動量が入賞口の高さを超える速度でも拾えること。
        // 矩形の内包判定へ退行すると、速い玉だけが素通りするようになる。
        let mut state = state_with_nails(Vec::new());
        state.balls.push(Ball {
            x: START_POCKET_X,
            y: START_POCKET_Y - 0.5,
            vx: 0.0,
            vy: MAX_SPEED,
            hit_glow: 0,
        });
        let before = state.balls_held;
        step_balls(&mut state);
        assert!(state.balls.is_empty(), "入賞した玉が盤面に残っている");
        assert_eq!(state.balls_held, before + START_PAYOUT);
        assert_eq!(state.pending.len(), 1, "ヘソ入賞なのに保留が積まれていない");
    }

    #[test]
    fn kakuhen_hits_more_often_than_normal() {
        let trials = 20_000;
        let count_hits = |mode: Mode| {
            let mut state = seated_state();
            state.mode = mode;
            (0..trials)
                .filter(|_| roll_outcome(&mut state).hit)
                .count()
        };
        let normal = count_hits(Mode::Normal);
        let kakuhen = count_hits(Mode::Kakuhen { spins_left: 0 });
        assert!(
            kakuhen > normal * 2,
            "確変中の当たりが通常時と大差ない (通常={normal} 確変={kakuhen} / {trials}回)"
        );
    }

    #[test]
    fn premium_never_appears_on_a_loss_and_reels_match_the_outcome() {
        let mut state = seated_state();
        for _ in 0..20_000 {
            let outcome = roll_outcome(&mut state);
            if !outcome.hit {
                assert_ne!(
                    outcome.reach,
                    ReachKind::Premium,
                    "プレミアがハズレで出た。出れば当たりという関係が壊れる"
                );
            }
            let [l, m, r] = outcome.reels;
            if outcome.hit {
                assert!(l == m && m == r, "当たりなのにゾロ目でない: {:?}", outcome.reels);
            } else if outcome.reach == ReachKind::None {
                assert_ne!(l, r, "リーチ無しなのに左右が揃っている: {:?}", outcome.reels);
            } else {
                assert!(
                    l == r && m != l,
                    "リーチハズレの出目になっていない: {:?}",
                    outcome.reels
                );
            }
        }
    }

    #[test]
    fn adjust_power_clamps_to_the_handle_range() {
        let mut state = seated_state();
        assert!(adjust_power(&mut state, -200));
        assert_eq!(state.power, 0);
        assert!(!adjust_power(&mut state, -1), "下限で更に下げられている");
        assert!(adjust_power(&mut state, 500));
        assert_eq!(state.power, 100);
        assert!(!adjust_power(&mut state, 1), "上限で更に上げられている");
    }

    #[test]
    fn leave_seat_is_refused_during_a_jackpot() {
        let mut state = seated_state();
        state.mode = Mode::Jackpot(JackpotState {
            round: 1,
            total_rounds: 10,
            count: 0,
            ticks_left: ROUND_LIMIT_TICKS,
            kakuhen: true,
            payout: 0,
        });
        assert!(!leave_seat(&mut state), "大当たり中に席を立てている");
        assert_eq!(state.phase, Phase::Playing);
        state.mode = Mode::Normal;
        assert!(leave_seat(&mut state));
        assert_eq!(state.phase, Phase::Hall);
    }

    #[test]
    fn auto_reload_buys_balls_and_stops_firing_when_broke() {
        let mut state = seated_state();
        state.firing = true;
        state.cash = BALL_LOAN_YEN;
        auto_reload(&mut state);
        assert_eq!(state.balls_held, BALL_LOAN_COUNT);
        assert_eq!(state.cash, 0);
        assert_eq!(state.invested, BALL_LOAN_YEN);

        state.balls_held = 0;
        auto_reload(&mut state);
        assert!(!state.firing, "現金が尽きても打ち出しが止まっていない");
    }

    #[test]
    fn a_jackpot_ends_even_if_no_ball_enters_the_attacker() {
        // 打ち出しを止めたまま大当たりが終わらないと、席も立てないまま
        // 進行が止まる。
        let mut state = seated_state();
        state.mode = Mode::Jackpot(JackpotState {
            round: 1,
            total_rounds: 3,
            count: 0,
            ticks_left: ROUND_LIMIT_TICKS,
            kakuhen: false,
            payout: 0,
        });
        tick_n(&mut state, ROUND_LIMIT_TICKS * 4);
        assert!(
            !matches!(state.mode, Mode::Jackpot(_)),
            "玉が入らないラウンドで大当たりが終わらない"
        );
    }

    #[test]
    fn jackpot_end_seq_advances_at_the_end_of_a_jackpot_not_at_the_hit() {
        // 出玉が確定するのは終了時なので、保存の契機を読む側が「確定」と
        // 「終了」を取り違えないようにする。
        let mut state = seated_state();
        let before = state.jackpot_end_seq;
        resolve_spin(&mut state, jackpot(3));
        assert_eq!(
            state.jackpot_end_seq, before,
            "大当たりの確定だけで終了カウンタが進んでいる"
        );

        tick_n(&mut state, ROUND_LIMIT_TICKS * 4);
        assert!(
            !matches!(state.mode, Mode::Jackpot(_)),
            "大当たりが終わっていない"
        );
        assert_eq!(
            state.jackpot_end_seq,
            before + 1,
            "大当たりの終了で終了カウンタが進んでいない"
        );
    }

    fn miss(reach: ReachKind) -> SpinOutcome {
        SpinOutcome {
            hit: false,
            rounds: 0,
            kakuhen: false,
            reach,
            rank: PendingRank::White,
            stop: StopStyle::Plain,
            confirmed: false,
            assisted: false,
            reels: [1, 2, 3],
        }
    }

    fn jackpot(rounds: u32) -> SpinOutcome {
        SpinOutcome {
            hit: true,
            rounds,
            kakuhen: false,
            reach: ReachKind::Super,
            rank: PendingRank::Red,
            stop: StopStyle::Plain,
            confirmed: false,
            assisted: false,
            reels: [7, 7, 7],
        }
    }

    /// 電サポ中に引いた当たり。連チャンが伸びるのはこちらだけ。
    fn assisted_jackpot(rounds: u32) -> SpinOutcome {
        SpinOutcome {
            assisted: true,
            ..jackpot(rounds)
        }
    }

    /// 電サポ中に引いたハズレ。
    fn assisted_miss(reach: ReachKind) -> SpinOutcome {
        SpinOutcome {
            assisted: true,
            ..miss(reach)
        }
    }

    #[test]
    fn the_chain_ends_together_with_the_assistance_it_was_built_on() {
        let mut state = seated_state();
        // 時短つきの当たりを引き、電サポ中にもう1回当てて連チャンを伸ばす。
        resolve_spin(&mut state, jackpot(5));
        state.mode = Mode::Jitan { spins_left: 2 };
        resolve_spin(&mut state, assisted_jackpot(5));
        state.mode = Mode::Jitan { spins_left: 2 };
        assert_eq!(state.chain, 2);

        resolve_spin(&mut state, miss(ReachKind::None));
        resolve_spin(&mut state, miss(ReachKind::None));

        assert_eq!(state.mode, Mode::Normal, "時短が切れて通常時に戻っていない");
        assert_eq!(
            state.chain, 0,
            "電サポが切れても連チャン数が残り、通常時の画面が終わった連チャンを続いているものとして出し続ける"
        );
        assert_eq!(state.record.best_chain, 2, "自己記録まで巻き戻してはいけない");
    }

    #[test]
    fn spins_since_jackpot_counts_rotations_and_resets_on_a_hit() {
        let mut state = seated_state();
        for _ in 0..39 {
            resolve_spin(&mut state, miss(ReachKind::None));
        }
        resolve_spin(&mut state, jackpot(5));
        assert_eq!(
            state.history[0].spins_before, 40,
            "当たった回転を含めた回転数がハマり回数になっていない"
        );

        state.mode = Mode::Normal;
        for _ in 0..14 {
            resolve_spin(&mut state, miss(ReachKind::None));
        }
        resolve_spin(&mut state, jackpot(5));
        assert_eq!(
            state.history[0].spins_before, 15,
            "大当たり時に回転数が 0 へ戻っていない"
        );
    }

    #[test]
    fn jackpot_seq_advances_on_every_hit_even_without_new_start_pocket_entries() {
        // 保留を貯めた状態で連続して当たると、次のヘソ入賞を挟まずに大当たりが
        // 2回起きる。演出トリガは「増えたか」だけを見るので、ヘソ入賞の
        // カウンタに連動させると発火を取りこぼす。
        let mut state = seated_state();
        let before = state.jackpot_seq;
        resolve_spin(&mut state, jackpot(5));
        let after_first = state.jackpot_seq;
        state.mode = Mode::Normal;
        resolve_spin(&mut state, jackpot(10));

        assert!(after_first > before, "1回目の大当たりで演出トリガが進んでいない");
        assert!(
            state.jackpot_seq > after_first,
            "ヘソ入賞を挟まない連続大当たりで演出トリガが進んでいない"
        );
    }

    #[test]
    fn moving_to_another_machine_leaves_the_history_and_the_miss_count_behind() {
        // 履歴もハマり回数も台ごとの事実。持ち越すと、移った先の台が
        // 「もう当たっている」「既に◯◯回転ハマっている」台に見える。
        let mut state = seated_state();
        for _ in 0..29 {
            resolve_spin(&mut state, miss(ReachKind::None));
        }
        resolve_spin(&mut state, jackpot(5));
        assert_eq!(state.history.len(), 1);
        state.mode = Mode::Normal;
        for _ in 0..30 {
            resolve_spin(&mut state, miss(ReachKind::None));
        }
        assert_eq!(state.spins_since_jackpot, 30);

        assert!(leave_seat(&mut state));
        assert!(sit_at(&mut state, 1));
        assert!(
            state.history.is_empty(),
            "前の台の大当たり履歴が移った先の台に残っている"
        );
        assert_eq!(
            state.spins_since_jackpot, 0,
            "前の台の回転数が移った先の台に残っている"
        );

        for _ in 0..5 {
            resolve_spin(&mut state, miss(ReachKind::None));
        }
        resolve_spin(&mut state, jackpot(5));
        assert_eq!(
            state.history[0].spins_before, 6,
            "前の台の回転数がハマり回数へ水増しされている"
        );
    }

    #[test]
    fn sitting_down_marks_the_player_as_seated() {
        // ホールの表示はこの印で着席中の台を選ぶ。印が立たないままだと、
        // 座っている台があっても強調されない。
        let mut state = PachinkoState::new();
        generate_hall(&mut state);
        assert!(!state.has_seated);
        assert!(sit_at(&mut state, 2));
        assert!(state.has_seated);
        assert!(leave_seat(&mut state));
        assert!(state.has_seated, "席を立っても着席した事実は消えない");
        assert_eq!(state.seat, 2);
    }

    #[test]
    fn a_start_pocket_entry_lights_the_pocket() {
        let mut state = state_with_nails(Vec::new());
        state.balls.push(Ball {
            x: START_POCKET_X,
            y: START_POCKET_Y - 0.5,
            vx: 0.0,
            vy: MAX_SPEED,
            hit_glow: 0,
        });
        step_balls(&mut state);
        assert_eq!(
            state.start_flash, START_FLASH_TICKS,
            "ヘソ入賞の演出トリガが立っていない"
        );
    }

    #[test]
    fn entering_a_reach_lights_the_board_with_its_own_kind() {
        let mut state = seated_state();
        state.pending.push(Pending::new(miss(ReachKind::Super)));
        start_spin_if_idle(&mut state);
        assert_eq!(state.reach_flash, REACH_FLASH_TICKS);
        assert_eq!(state.reach_flash_kind, ReachKind::Super);

        // リーチにならない回転は光らせない。毎回転光ると、光ること自体が
        // リーチの合図でなくなる。
        state.digit = Digit::Idle;
        state.reach_flash = 0;
        state.pending.push(Pending::new(miss(ReachKind::None)));
        start_spin_if_idle(&mut state);
        assert_eq!(state.reach_flash, 0);
    }

    #[test]
    fn effect_flashes_survive_a_batched_tick() {
        // `delta_ticks` は最大5までまとめて来るので、それ未満の長さの
        // カウンタは一度も描画されないまま消える (`HIT_GLOW_TICKS` 参照)。
        const { assert!(START_FLASH_TICKS >= HIT_GLOW_TICKS) };
        const { assert!(REACH_FLASH_TICKS >= HIT_GLOW_TICKS) };
        let mut state = seated_state();
        state.start_flash = START_FLASH_TICKS;
        state.reach_flash = REACH_FLASH_TICKS;
        tick_n(&mut state, HIT_GLOW_TICKS as u32);
        assert!(state.start_flash > 0, "ヘソの光が描画される前に消えている");
        assert!(state.reach_flash > 0, "リーチの光が描画される前に消えている");
        tick_n(&mut state, REACH_FLASH_TICKS as u32);
        assert_eq!(state.start_flash, 0, "ヘソの光が消えない");
        assert_eq!(state.reach_flash, 0, "リーチの光が消えない");
    }

    /// 最終ランクだけを指定したハズレ。ランクは抽選の産物なので、昇格の
    /// 段取りだけを見たいテストでは直接置く。
    fn ranked_miss(rank: PendingRank) -> SpinOutcome {
        SpinOutcome {
            rank,
            ..miss(ReachKind::None)
        }
    }

    #[test]
    fn a_new_pending_starts_white_however_hot_it_ends_up() {
        // 入賞と同時に最終ランクを見せると、待つ間に情報が増えるという体験
        // そのものが消える。
        let mut state = seated_state();
        let mut saw_a_hot_final = false;
        for _ in 0..2_000 {
            state.pending.clear();
            resolve_start_pocket(&mut state);
            let pending = state.pending[0];
            assert_eq!(
                pending.rank,
                PendingRank::White,
                "積まれた直後の保留が最終ランク ({}) で現れた",
                pending.final_rank().label()
            );
            saw_a_hot_final |= pending.final_rank() > PendingRank::White;
        }
        assert!(
            saw_a_hot_final,
            "最終ランクが白の保留しか出ず、飛ばないことの検証になっていない"
        );
    }

    #[test]
    fn a_waiting_pending_climbs_one_rank_at_a_time() {
        let mut state = seated_state();
        // 先頭は次の変動で消化されるため最終ランクまで押し上げられる。1段ずつ
        // 上る様子は、その後ろで待つ保留で見る。
        state.pending.push(Pending::new(ranked_miss(PendingRank::White)));
        state.pending.push(Pending::new(ranked_miss(PendingRank::Rainbow)));
        let mut rank = PendingRank::White;
        let mut steps = 0;
        for _ in 0..500 {
            promote_pending(&mut state);
            let now = state.pending[1].rank;
            if now != rank {
                assert_eq!(
                    now,
                    rank.promoted(),
                    "昇格が段を飛ばした ({} → {})",
                    rank.label(),
                    now.label()
                );
                rank = now;
                steps += 1;
            }
            if rank == PendingRank::Rainbow {
                break;
            }
        }
        assert_eq!(steps, 5, "白から虹まで5段を1段ずつ上っていない");
    }

    #[test]
    fn a_pending_reaches_its_final_rank_before_it_is_consumed() {
        // 消化されると保留は画面から消える。最終ランクを一度も見せないまま
        // 消えると、昇格という情報イベントが最後まで届かない。
        let mut state = seated_state();
        for _ in 0..MAX_PENDING {
            state
                .pending
                .push(Pending::new(ranked_miss(PendingRank::Gold)));
        }
        while state.pending.len() > 1 {
            state.digit = Digit::Idle;
            start_spin_if_idle(&mut state);
            assert_eq!(
                state.pending[0].rank,
                PendingRank::Gold,
                "次の変動で消化される保留が最終ランクに届いていない"
            );
        }
    }

    #[test]
    fn a_promotion_flash_survives_a_batched_tick() {
        // `delta_ticks` は最大5までまとめて来るので、それ未満の長さの
        // カウンタは一度も描画されないまま消える (`HIT_GLOW_TICKS` 参照)。
        const { assert!(PENDING_PROMOTE_FLASH_TICKS >= HIT_GLOW_TICKS) };
        let mut state = seated_state();
        // 回転中にしておくと保留が消化されず、光の寿命だけを見られる。
        state.digit = Digit::Spinning {
            ticks_left: 1_000,
            outcome: miss(ReachKind::None),
        };
        state
            .pending
            .push(Pending::new(ranked_miss(PendingRank::Gold)));
        promote_pending(&mut state);
        assert_eq!(state.pending[0].promote_flash, PENDING_PROMOTE_FLASH_TICKS);
        tick_n(&mut state, HIT_GLOW_TICKS as u32);
        assert!(
            state.pending[0].promote_flash > 0,
            "昇格の光が描画される前に消えている"
        );
        tick_n(&mut state, PENDING_PROMOTE_FLASH_TICKS as u32);
        assert_eq!(state.pending[0].promote_flash, 0, "昇格の光が消えない");
    }

    #[test]
    fn a_rainbow_pending_never_appears_on_a_loss() {
        let mut state = seated_state();
        let mut rainbows = 0;
        for _ in 0..200_000 {
            let outcome = roll_outcome(&mut state);
            if outcome.rank == PendingRank::Rainbow {
                rainbows += 1;
                assert!(
                    outcome.hit,
                    "虹保留がハズレで出た。出れば当たりという関係が壊れる"
                );
            }
        }
        assert!(rainbows > 0, "虹保留が一度も出ず、検証になっていない");
    }

    #[test]
    fn a_near_miss_needs_a_hot_pending_or_a_super_reach() {
        // 条件無しに出すと「またか」で慣れ、惜しさが情報を運ばなくなる。
        let mut state = seated_state();
        let mut near_misses = 0;
        for _ in 0..200_000 {
            let outcome = roll_outcome(&mut state);
            if outcome.stop != StopStyle::NearMiss {
                continue;
            }
            near_misses += 1;
            assert!(!outcome.hit, "当たりが惜しいハズレとして止まった");
            assert!(
                near_miss_ready(outcome.reach, outcome.rank),
                "熱くもない回転が惜しいハズレになった (保留={} リーチ={})",
                outcome.rank.label(),
                outcome.reach.label()
            );
        }
        assert!(near_misses > 0, "惜しいハズレが一度も出ず、検証になっていない");
    }

    #[test]
    fn a_revival_never_appears_on_a_loss() {
        let mut state = seated_state();
        let mut revivals = 0;
        for _ in 0..200_000 {
            let outcome = roll_outcome(&mut state);
            if outcome.stop == StopStyle::Revival {
                revivals += 1;
                assert!(outcome.hit, "ハズレが復活して揃った");
            }
        }
        assert!(revivals > 0, "復活が一度も出ず、検証になっていない");
    }

    #[test]
    fn a_confirmed_signal_always_means_a_hit() {
        // 確定が嘘をつくと、確定ではない他の全ての演出の意味まで一緒に失われる。
        let mut state = seated_state();
        let mut confirmed = 0;
        let mut rainbows = 0;
        for _ in 0..200_000 {
            let outcome = roll_outcome(&mut state);
            if outcome.confirmed {
                confirmed += 1;
                assert!(outcome.hit, "ハズレに確定シグナルが立った");
            }
            if outcome.rank == PendingRank::Rainbow {
                rainbows += 1;
                assert!(
                    outcome.confirmed,
                    "信頼度100%の虹保留に確定シグナルが立っていない"
                );
            }
        }
        assert!(confirmed > 0, "確定シグナルが一度も立たず、検証になっていない");
        assert!(rainbows > 0, "虹保留が一度も出ず、検証になっていない");
    }

    #[test]
    fn the_stop_style_lengthens_the_spin_it_belongs_to() {
        let mut state = seated_state();
        let mut spin_ticks = |stop: StopStyle| {
            state.digit = Digit::Idle;
            state.pending.clear();
            state.pending.push(Pending::new(SpinOutcome {
                stop,
                ..miss(ReachKind::None)
            }));
            start_spin_if_idle(&mut state);
            match state.digit {
                Digit::Spinning { ticks_left, .. } => ticks_left,
                Digit::Idle => panic!("回転が始まっていない"),
            }
        };
        let plain = spin_ticks(StopStyle::Plain);
        let slip = spin_ticks(StopStyle::Slip);
        let revival = spin_ticks(StopStyle::Revival);
        assert_eq!(plain, ReachKind::None.spin_ticks());
        assert!(slip > plain, "滑りで回転が伸びていない (滑り={slip} 素={plain})");
        assert!(
            revival > slip,
            "復活が滑りより短い (復活={revival} 滑り={slip})"
        );
    }

    #[test]
    fn a_hit_drawn_during_assistance_keeps_the_chain_after_the_assistance_ends() {
        // 保留は抽選から消化まで時間差がある。電サポ切れの直後に残保留で
        // 当たると、消化時点のモードで見た場合だけ初当たりへ化ける。
        let mut state = seated_state();
        resolve_spin(&mut state, jackpot(5));
        assert_eq!(state.chain, 1);

        // 電サポ中に「ハズレ → 当たり」の順で積んだ保留を、先頭から消化する。
        // 消化中の抽選は保留から取り出された状態なので、当たりの方だけが
        // `pending` に残る。
        state.mode = Mode::Jitan { spins_left: 1 };
        state.pending.push(Pending::new(assisted_jackpot(5)));
        resolve_spin(&mut state, assisted_miss(ReachKind::None));
        assert_eq!(state.mode, Mode::Normal, "時短が切れていない");
        let queued = state.pending.remove(0);
        resolve_spin(&mut state, queued.outcome);

        assert_eq!(
            state.chain, 2,
            "電サポ中に引いた当たりが初当たり扱いになり、連チャンが数え直された"
        );
        assert_eq!(state.record.best_chain, 2);
    }

    #[test]
    fn the_chain_ends_when_the_last_assisted_pending_misses() {
        // 電サポ中に引いた保留が全てハズレで尽きた後、玉切れや打ち出しの
        // 停止で次の抽選が来ないことがある。次の抽選を待って数え直す作りだと、
        // 通常時の画面が終わった連チャンを出し続けたまま止まる。
        let mut state = seated_state();
        resolve_spin(&mut state, jackpot(5));
        state.mode = Mode::Jitan { spins_left: 1 };
        // 消化待ちの保留を1つ残したまま、電サポ中に引いたハズレを消化する。
        state.pending.push(Pending::new(assisted_miss(ReachKind::None)));
        resolve_spin(&mut state, assisted_miss(ReachKind::None));
        assert_eq!(state.mode, Mode::Normal, "時短が切れていない");
        assert_eq!(
            state.chain, 1,
            "電サポ中に引いた保留が残っている間は引き戻しの余地があるので連チャンを保つ"
        );

        // 最後の1つを消化すると、もう引き戻す余地が無い。
        let last = state.pending.remove(0);
        resolve_spin(&mut state, last.outcome);
        assert_eq!(
            state.chain, 0,
            "電サポ中の保留を消化し尽くしても連チャン表示が残り続ける"
        );
    }

    #[test]
    fn the_payout_counter_closes_in_on_the_actual_ball_count() {
        // 内部値が一度に跳ねても表示は数 tick かけて登る。差が縮み続けること
        // (＝止まらない・追い越さない) がカウンタの見え方を支える。
        let mut state = seated_state();
        state.mode = Mode::Jackpot(JackpotState {
            round: 1,
            total_rounds: 16,
            count: 0,
            ticks_left: ROUND_LIMIT_TICKS,
            kakuhen: false,
            payout: 1_200,
        });
        let target = 1_200.0;
        let mut prev = target - state.jackpot_payout_shown;
        let mut ticks = 0;
        for _ in 0..200 {
            ease_jackpot_payout(&mut state);
            let diff = target - state.jackpot_payout_shown;
            assert!(diff >= 0.0, "表示値が実際の値を追い越した ({diff})");
            assert!(diff < prev, "表示値が実際の値へ近づいていない ({prev} → {diff})");
            prev = diff;
            ticks += 1;
            if diff == 0.0 {
                break;
            }
        }
        assert_eq!(
            state.jackpot_payout_shown, target,
            "表示値が実際の値に追いつかない"
        );
        assert!(ticks > 1, "1 tick で追いついてしまい、数字が動く時間がない");
    }

    #[test]
    fn the_payout_of_a_finished_jackpot_stays_readable() {
        // 終了後のサマリはこの値を読む。モードが移った瞬間に消えると、
        // 「何発出たか」を見せる相手が居なくなる。
        let mut state = seated_state();
        state.mode = Mode::Jackpot(JackpotState {
            round: 1,
            total_rounds: 1,
            count: 0,
            ticks_left: 1,
            kakuhen: false,
            payout: 320,
        });
        tick(&mut state);
        assert!(
            !matches!(state.mode, Mode::Jackpot(_)),
            "大当たりが終わっていない"
        );
        assert_eq!(state.jackpot_payout(), 320);
    }

    #[test]
    fn attacker_entries_add_up_into_the_jackpot_payout() {
        let mut state = state_with_nails(Vec::new());
        state.mode = Mode::Jackpot(JackpotState {
            round: 1,
            total_rounds: 16,
            count: 0,
            ticks_left: ROUND_LIMIT_TICKS,
            kakuhen: false,
            payout: 0,
        });
        state.balls.push(Ball {
            x: ATTACKER_X,
            y: ATTACKER_Y - 0.5,
            vx: 0.0,
            vy: MAX_SPEED,
            hit_glow: 0,
        });
        step_balls(&mut state);
        assert_eq!(state.jackpot_payout(), ATTACKER_PAYOUT);
    }

    #[test]
    fn generate_hall_varies_with_the_rng_state() {
        // ホールの並びは `rng_state` だけが決める。ここが効かないと来店の
        // たびに同じ台・同じ釘が並び、「今日はどの台が回るか」を読む余地が
        // 消える。
        let hall_of = |seed: u32| -> Vec<String> {
            let mut state = PachinkoState::new();
            state.rng_state = seed;
            generate_hall(&mut state);
            state
                .machines
                .iter()
                .map(|m| format!("{} {:.4}", m.name, m.nail_spread))
                .collect()
        };
        assert_ne!(hall_of(0x1234_5678), hall_of(0x9E37_79B9));
    }

    #[test]
    fn a_finished_spin_leaves_its_reels_on_the_display() {
        // 停止した出目を残さないと、当たった瞬間に液晶からゾロ目が消え、
        // 何が揃ったのか見えないまま次の回転へ移る。
        let mut state = seated_state();
        let outcome = jackpot(8);
        state.digit = Digit::Spinning { ticks_left: 1, outcome };
        advance_digit(&mut state);
        assert_eq!(state.digit, Digit::Idle);
        assert_eq!(state.last_reels, outcome.reels);

        // 台を替えれば前の台の液晶は付いてこない。
        state.mode = Mode::Normal;
        assert!(leave_seat(&mut state));
        assert_eq!(state.last_reels, INITIAL_REELS);
    }
}
