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
    Phase, ReachKind, SpinOutcome, ATTACKER_HALF_W, ATTACKER_PAYOUT, ATTACKER_X, ATTACKER_Y,
    BALL_LOAN_COUNT, BALL_LOAN_YEN, BALL_R, BOARD_H, BOARD_W, FIRE_INTERVAL_TICKS, HALL_SIZE,
    HISTORY_LEN, HIT_GLOW_TICKS, INITIAL_REELS, LAUNCH_X, LAUNCH_Y, MACHINE_SPECS, MAX_BALLS,
    MAX_PENDING, NAIL_R, REACH_FLASH_TICKS, ROUND_COUNT, ROUND_LIMIT_TICKS, SIDE_PAYOUT,
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
/// 1サブステップあたりの重力加速度。
const GRAVITY: f64 = 0.055;
/// 釘との衝突の反発係数。
const NAIL_RESTITUTION: f64 = 0.55;
/// 壁・天井との衝突の反発係数。
const WALL_RESTITUTION: f64 = 0.45;
/// 速度の上限。加速が積み上がると1サブステップの移動量が釘の直径を超え、
/// 釘をすり抜けるようになる。
const MAX_SPEED: f64 = 2.6;
/// 釘に当たった時に横方向へ乗るばらつきの最大幅。同じ軌道で入っても結果が
/// 割れる「パチンコらしさ」の源で、0 にすると釘配置だけで結果が決まる
/// 決定論的な機械になってしまう。
const NAIL_SCATTER: f64 = 0.22;
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
const HORIZONTAL_DRAG: f64 = 0.94;
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
    (-(0.55 + p * 1.30), -0.75 - p * 0.55)
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
    state.pending.push(outcome);
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

/// ヘソ入賞時に当落を確定させる。演出 (リーチ) もここで一緒に決める
/// (実機と同じく「先に当落が決まり、演出が後から説明する」構造)。
pub fn roll_outcome(state: &mut PachinkoState) -> SpinOutcome {
    let spec = seated_spec(state);
    let odds = match state.mode {
        Mode::Kakuhen { .. } => spec.kakuhen_odds,
        _ => spec.normal_odds,
    };
    let seed = &mut state.rng_state;
    let hit = rng_below(seed, odds.max(1)) == 0;
    let reach = pick_reach(hit, seed);
    let rounds = if hit { pick_rounds(spec, seed) } else { 0 };
    let kakuhen = hit && rng_below(seed, 100) < spec.kakuhen_rate;
    let reels = pick_reels(hit, reach, seed);
    SpinOutcome {
        hit,
        rounds,
        kakuhen,
        reach,
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

/// 保留を1つ消化してデジタルを回し始める。
fn start_spin_if_idle(state: &mut PachinkoState) {
    if state.digit != Digit::Idle || state.pending.is_empty() {
        return;
    }
    if matches!(state.mode, Mode::Jackpot(_)) {
        // 大当たり中は保留を溜めるだけ。消化はラウンド消化の後に回る。
        return;
    }
    let outcome = state.pending.remove(0);
    if outcome.reach != ReachKind::None {
        state.reach_flash = REACH_FLASH_TICKS;
        state.reach_flash_kind = outcome.reach;
    }
    state.digit = Digit::Spinning {
        ticks_left: outcome.reach.spin_ticks(),
        outcome,
    };
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
        return;
    }
    let assisted = state.mode.is_assisted();
    state.chain = if assisted { state.chain + 1 } else { 1 };
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
                state.mode = Mode::Normal;
                state.add_log("確変終了");
            } else {
                state.mode = Mode::Kakuhen { spins_left: left };
            }
        }
        Mode::Jitan { spins_left } => {
            let left = spins_left.saturating_sub(1);
            if left == 0 {
                state.mode = Mode::Normal;
                state.add_log("時短終了");
            } else {
                state.mode = Mode::Jitan { spins_left: left };
            }
        }
        _ => {}
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
}

fn decay_glow(state: &mut PachinkoState) {
    for ball in &mut state.balls {
        ball.hit_glow = ball.hit_glow.saturating_sub(1);
    }
    state.start_flash = state.start_flash.saturating_sub(1);
    state.reach_flash = state.reach_flash.saturating_sub(1);
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

/// 下部釘の段数と本数。
const LOWER_ROWS: usize = 4;
const LOWER_TOP_Y: f64 = 58.0;
const LOWER_BOTTOM_Y: f64 = 74.0;
const LOWER_NAILS_PER_ROW: usize = 8;
const LOWER_BASE_X: f64 = 6.0;
const LOWER_STEP_X: f64 = 7.4;

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
        SpinOutcome { hit: false, rounds: 0, kakuhen: false, reach, reels: [1, 2, 3] }
    }

    fn jackpot(rounds: u32) -> SpinOutcome {
        SpinOutcome { hit: true, rounds, kakuhen: false, reach: ReachKind::Super, reels: [7, 7, 7] }
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
        state.pending.push(miss(ReachKind::Super));
        start_spin_if_idle(&mut state);
        assert_eq!(state.reach_flash, REACH_FLASH_TICKS);
        assert_eq!(state.reach_flash_kind, ReachKind::Super);

        // リーチにならない回転は光らせない。毎回転光ると、光ること自体が
        // リーチの合図でなくなる。
        state.digit = Digit::Idle;
        state.reach_flash = 0;
        state.pending.push(miss(ReachKind::None));
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
