//! 玉響 — 遊技ルール (純粋関数)。
//!
//! `tick()` が1tick分の全処理 (演出タイマー減衰→打ち出し→物理と入賞判定→
//! ラウンド進行→デジタル→保留消化→玉の補充) を順に進める。render.rs は
//! ここで更新された `PachinkoState` を読むだけで、書き込まない。
//!
//! 空間は `board`、釘は `nails`、運動は `physics` が担う。ここは当落・保留・
//! 席・軍資金だけを進め、座標の式を持たない。台の個性は `nails::generate_nails`
//! が座標へ落とし、物理だけが結果を決める。数値を当落へ直接掛けないことで、
//! 盤面の見た目と実測回転率が食い違わない。

use super::nails::{generate_nails, NAIL_SPREAD_RANGE, RAIL_BIAS_RANGE};
use super::physics::{self, jitter_launch, launch_velocity};
use super::rng::{rand_range, rng_below};
use super::state::{
    Ball, BallTint, Digit, HistoryEntry, JackpotState, Machine, MachineSpec, Mode, PachinkoState,
    Pending, PendingRank, Phase, ReachKind, SpinOutcome, StopStyle, ATTACKER_PAYOUT,
    BALL_LOAN_COUNT, BALL_LOAN_YEN, FIRE_INTERVAL_TICKS, HALL_SIZE, HISTORY_LEN, INITIAL_REELS,
    LAUNCH_X, LAUNCH_Y, MACHINE_SPECS, MAX_BALLS, MAX_PENDING, PENDING_PROMOTE_FLASH_TICKS,
    RANK_WEIGHT_TOTAL, REACH_FLASH_TICKS, ROUND_COUNT, ROUND_LIMIT_TICKS, SIDE_PAYOUT,
    START_FLASH_TICKS, START_PAYOUT,
};

/// 1 tick の運動結果を賞球と抽選へ渡す。口を跨いだ事実は physics が返し、
/// 払い出しの規則はここに閉じる。
fn step_balls(state: &mut PachinkoState) {
    let hits = physics::step_balls(state);
    if hits.side > 0 {
        add_balls(state, hits.side * SIDE_PAYOUT);
    }
    for _ in 0..hits.attacker {
        add_balls(state, ATTACKER_PAYOUT);
        if let Mode::Jackpot(mut j) = state.mode {
            j.count += 1;
            j.payout += ATTACKER_PAYOUT;
            state.mode = Mode::Jackpot(j);
        }
    }
    for fired_in_normal in hits.start {
        resolve_start_pocket(state, fired_in_normal);
    }
}

/// 持ち玉を増やす。自己記録の最高持ち玉はここを通す。
fn add_balls(state: &mut PachinkoState, amount: u32) {
    state.balls_held = state.balls_held.saturating_add(amount);
    state.record.best_balls = state.record.best_balls.max(state.balls_held);
}

// ── 抽選 ───────────────────────────────────────────────────────

/// ヘソ入賞1個分の処理。賞球を払い、保留に空きがあれば当落を確定させる。
fn resolve_start_pocket(state: &mut PachinkoState, fired_in_normal: bool) {
    add_balls(state, START_PAYOUT);
    state.start_flash = START_FLASH_TICKS;
    if state.pending.len() >= MAX_PENDING {
        // 保留満タン中の入賞は賞球だけ。抽選を受けられない玉が出ることが
        // 「打ち出しを止める」判断の材料になる。
        return;
    }
    let outcome = roll_outcome(state);
    state.pending.push(Pending::new(outcome));
    // 回転率は通常時の分だけで測る (`Machine::normal_spins_seen` 参照)。
    // 今のモードではなく玉が打たれた時のモードで数えるのは、打ち出しから
    // 入賞までの数秒の間にモードが変わりうるため。
    if let Some(machine) = state.seated_machine_mut() {
        machine.spins_seen += 1;
        if fired_in_normal {
            machine.normal_spins_seen += 1;
        }
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
    // 大当たり中に貯まった保留は、ラウンドが終わってから消化される。抽選を
    // 引く時点のモードで決めると、確変が約束されている大当たり中に貯めた
    // 保留だけが通常確率になり、当たっても連チャンが数え直される — 実際に
    // 消化されるのは電サポへ入った直後なのに、確変の恩恵を受けられない。
    let (odds, assisted) = match state.mode {
        Mode::Kakuhen { .. } => (spec.kakuhen_odds, true),
        Mode::Jitan { .. } => (spec.normal_odds, true),
        Mode::Jackpot(j) if j.kakuhen => (spec.kakuhen_odds, true),
        Mode::Jackpot(_) => (spec.normal_odds, true),
        Mode::Normal => (spec.normal_odds, false),
    };
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
fn pick_stop_style(hit: bool, reach: ReachKind, rank: PendingRank, seed: &mut u32) -> StopStyle {
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
        Digit::Spinning {
            ticks_left,
            outcome,
        } => {
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
    // 表示用の出玉も 0 から数え直す。連チャンで前回の額が残っていると、
    // カウンタは新しい大当たりの獲得を積み上げる前に前回の額から下がって
    // いくことになり、増えていく数字を見せるという狙いと逆の動きになる。
    state.jackpot_payout_shown = 0.0;
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
    state.last_jackpot_chain = state.chain;
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
    let seed = &mut state.rng_state;
    let (vx, vy) = jitter_launch(launch_velocity(state.power), seed);
    let fired_in_normal = state.mode == Mode::Normal;
    let tint = BallTint::ALL[rng_below(seed, BallTint::ALL.len() as u32) as usize];
    state.balls.push(Ball::falling(
        LAUNCH_X,
        LAUNCH_Y,
        vx,
        vy,
        fired_in_normal,
        tint,
    ));
    state.balls_held -= 1;
    if let Some(machine) = state.seated_machine_mut() {
        machine.balls_spent += 1;
        if fired_in_normal {
            machine.normal_balls_spent += 1;
        }
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
    state.last_jackpot_chain = 0;
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

/// 来店ごとのホールを作る。同じスペックの台が釘だけ違う形で並ぶことがあり、
/// それが釘読みという判断軸を成立させる。
pub fn generate_hall(state: &mut PachinkoState) {
    let mut machines = Vec::with_capacity(HALL_SIZE);
    for _ in 0..HALL_SIZE {
        let pick = rng_below(&mut state.rng_state, MACHINE_SPECS.len() as u32) as usize;
        let (name, spec) = MACHINE_SPECS[pick];
        let nail_spread = rand_range(
            &mut state.rng_state,
            NAIL_SPREAD_RANGE.0,
            NAIL_SPREAD_RANGE.1,
        );
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
            normal_balls_spent: 0,
            normal_spins_seen: 0,
        });
    }
    state.machines = machines;
    state.seat = 0;
}

/// 千円 (= `BALL_LOAN_COUNT` 玉) あたりの回転数。実測値なので、打ち込んだ
/// 玉が少ないうちは当てにならない。標本が足りない間は `None` を返し、
/// 「まだ分からない」ことを表示側でそのまま出せるようにする。
///
/// 通常時の分だけを数える (`Machine::normal_balls_spent` 参照)。この値は
/// 台選びの根拠になるので、釘以外の要因で動くと判断そのものを誤らせる。
pub fn spin_rate(machine: &Machine) -> Option<f64> {
    if machine.normal_balls_spent < 50 {
        return None;
    }
    Some(
        machine.normal_spins_seen as f64 * BALL_LOAN_COUNT as f64
            / machine.normal_balls_spent as f64,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::games::pachinko::board::Playfield;
    use crate::games::pachinko::nails::RAIL_TOP_Y;
    use crate::games::pachinko::physics::{
        MAX_SPEED, POCKET_MOUTH_BELOW, TEETER_TICKS_MAX, TEETER_TICKS_MIN,
    };
    use crate::games::pachinko::state::{
        Nail, ATTACKER_X, ATTACKER_Y, HIT_GLOW_TICKS, START_POCKET_X, START_POCKET_Y,
    };

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

    fn test_ball(x: f64, y: f64, vx: f64, vy: f64) -> Ball {
        Ball::falling(x, y, vx, vy, true, BallTint::Gold)
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
                    Playfield::TABLE.contains(ball.x, ball.y, 0.0),
                    "玉が逆U字の盤面の外へ出た ({:.2}, {:.2})",
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
        state.balls.push(test_ball(
            START_POCKET_X,
            START_POCKET_Y - 0.5,
            0.0,
            MAX_SPEED,
        ));
        let before = state.balls_held;
        step_balls(&mut state);
        // 速い玉も縁に乗ってから入る。即消えさせると、サブステップを跨いだ
        // 素通りと区別が付かなくなる。
        if !state.balls.is_empty() {
            assert!(
                state.balls[0].teeter > 0,
                "速い玉がヘソを素通りして盤面に残っている"
            );
            for _ in 0..(u32::from(TEETER_TICKS_MAX) + 2) {
                step_balls(&mut state);
                if state.balls.is_empty() {
                    break;
                }
            }
        }
        assert!(state.balls.is_empty(), "入賞した玉が盤面に残っている");
        assert_eq!(state.balls_held, before + START_PAYOUT);
        assert_eq!(state.pending.len(), 1, "ヘソ入賞なのに保留が積まれていない");
    }

    #[test]
    fn same_power_launches_fan_out_across_the_board() {
        // 同じハンドル強度でも釘帯へ入る列が分かれる。速さだけを振ると角度が
        // 固定されたまま、釘の間に一本の溝ができる。測定はアーチ折り返し直後
        // ではなく釘帯の上端。折り返し地点は横移動が短く、ばらけが見えない。
        let mut xs = Vec::new();
        for i in 0..36u32 {
            let mut state = state_with_nails(Vec::new());
            state.rng_state = 0xA11C_E5ED ^ i.wrapping_mul(0x9E37);
            state.power = 62;
            state.balls_held = 8;
            state.firing = true;
            state.fire_cooldown = 0;
            try_fire(&mut state);
            assert_eq!(state.balls.len(), 1, "打ち出されていない");
            for _ in 0..800 {
                step_balls(&mut state);
                let Some(ball) = state.balls.first() else {
                    break;
                };
                if ball.vy > 0.0 && ball.y >= RAIL_TOP_Y {
                    xs.push(ball.x);
                    break;
                }
            }
        }
        assert!(
            xs.len() >= 30,
            "釘帯まで届いた玉が少なすぎる ({})",
            xs.len()
        );
        let mean = xs.iter().sum::<f64>() / xs.len() as f64;
        let var = xs.iter().map(|x| (x - mean) * (x - mean)).sum::<f64>() / (xs.len() - 1) as f64;
        let std = var.sqrt();
        assert!(
            std > 1.2,
            "同じ強度の打ち出しが同じ列へ落ちている (std={std:.2}, n={})",
            xs.len()
        );
    }

    #[test]
    fn a_slow_ball_teeters_on_the_start_pocket_before_entering() {
        // ヘソへ届いた玉が即消えすると「入りそう」が無い。遅い玉は縁に乗って
        // から揺れ、そのあいだ盤面に残る。
        let mut state = state_with_nails(Vec::new());
        state.machines[0].nail_spread = 0.8;
        state
            .balls
            .push(test_ball(START_POCKET_X, START_POCKET_Y - 0.4, 0.0, 0.25));
        step_balls(&mut state);
        let ball = state.balls.first().expect("縁に乗った玉が消えている");
        assert!(
            ball.teeter >= TEETER_TICKS_MIN,
            "遅い玉がヘソの縁で揺れていない (teeter={})",
            ball.teeter
        );
        assert_eq!(state.pending.len(), 0, "揺れている最中に入賞している");

        for _ in 0..(u32::from(TEETER_TICKS_MAX) + 2) {
            step_balls(&mut state);
            if state.balls.is_empty() {
                break;
            }
        }
        assert!(
            state.balls.is_empty(),
            "縁揺れが尽きた後も玉が盤面に残っている"
        );
        assert_eq!(state.pending.len(), 1, "揺れの末にヘソ入賞していない");
    }

    #[test]
    fn a_ball_that_walks_off_the_lip_falls_instead_of_entering() {
        // 揺れの末に口の外へ出た玉は入らずに落下する。「入りそう…アウト」が
        // 入賞の確定になってしまうと、縁に乗る意味が無くなる。
        let mut state = state_with_nails(Vec::new());
        let mut ball = test_ball(START_POCKET_X + 8.0, START_POCKET_Y - 0.35, 0.0, 0.0);
        ball.teeter = 3;
        ball.teeter_x = START_POCKET_X + 8.0;
        state.balls.push(ball);
        let before = state.pending.len();
        step_balls(&mut state);
        let ball = state.balls.first().expect("口の外へ出た玉が消えている");
        assert_eq!(ball.teeter, 0, "口の外なのに縁揺れが続いている");
        assert!(
            ball.y > START_POCKET_Y + POCKET_MOUTH_BELOW,
            "口の外へ出た玉が判定矩形の中に残っている (y={:.2})",
            ball.y
        );
        assert_eq!(state.pending.len(), before, "口の外の玉が入賞している");
        step_balls(&mut state);
        let ball = state.balls.first().expect("落下中の玉が消えている");
        assert_eq!(ball.teeter, 0, "滑り落ちた玉が再び縁に乗っている");
    }

    #[test]
    fn teetering_survives_a_batched_tick() {
        // `delta_ticks` は最大5までまとめて来るので、それ未満の長さの揺れは
        // 一度も描画されないまま入賞してしまう。
        const { assert!(TEETER_TICKS_MIN as u32 > 5) };
        let mut state = state_with_nails(Vec::new());
        state
            .balls
            .push(test_ball(START_POCKET_X, START_POCKET_Y - 0.4, 0.0, 0.2));
        step_balls(&mut state);
        assert!(
            state.balls.first().is_some_and(|b| b.teeter > 0),
            "縁揺れが始まっていない"
        );
        tick_n(&mut state, 5);
        assert!(
            state.balls.first().is_some_and(|b| b.teeter > 0),
            "まとめて進めた tick のあいだに縁揺れが消えている"
        );
    }

    #[test]
    fn kakuhen_hits_more_often_than_normal() {
        let trials = 20_000;
        let count_hits = |mode: Mode| {
            let mut state = seated_state();
            state.mode = mode;
            (0..trials).filter(|_| roll_outcome(&mut state).hit).count()
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
                assert!(
                    l == m && m == r,
                    "当たりなのにゾロ目でない: {:?}",
                    outcome.reels
                );
            } else if outcome.reach == ReachKind::None {
                assert_ne!(
                    l, r,
                    "リーチ無しなのに左右が揃っている: {:?}",
                    outcome.reels
                );
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
        assert_eq!(
            state.record.best_chain, 2,
            "自己記録まで巻き戻してはいけない"
        );
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

        assert!(
            after_first > before,
            "1回目の大当たりで演出トリガが進んでいない"
        );
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
        state.balls.push(test_ball(
            START_POCKET_X,
            START_POCKET_Y - 0.5,
            0.0,
            MAX_SPEED,
        ));
        for _ in 0..(u32::from(TEETER_TICKS_MAX) + 2) {
            step_balls(&mut state);
            if state.balls.is_empty() {
                break;
            }
        }
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
        assert!(
            state.reach_flash > 0,
            "リーチの光が描画される前に消えている"
        );
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
            resolve_start_pocket(&mut state, true);
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
        state
            .pending
            .push(Pending::new(ranked_miss(PendingRank::White)));
        state
            .pending
            .push(Pending::new(ranked_miss(PendingRank::Rainbow)));
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
    fn the_displayed_spin_rate_ignores_assisted_play() {
        // 電サポ中はヘソが広がるので、全区間を混ぜた比は「釘がどれだけ開いて
        // いるか」ではなく「どれだけ当たったか」を映す。当たった台ほど回ると
        // 表示されると、ホールへ戻ったときの台選びが引きの強さに引きずられる。
        let mut state = seated_state();
        state.balls_held = 10_000;
        state.cash = 0;

        // 通常時に十分な標本を作る。盤面の玉数の上限で打ち出しが止まらない
        // よう、飛ばした玉はその場で片付ける。
        state.firing = true;
        state.mode = Mode::Normal;
        for _ in 0..400 {
            state.fire_cooldown = 0;
            try_fire(&mut state);
            state.balls.clear();
        }
        let normal_only =
            spin_rate(state.seated_machine().expect("着席していない")).expect("標本が足りない");

        // 同じ台で電サポ中に打ち込んでも、表示される回転率は動かない。
        state.mode = Mode::Kakuhen { spins_left: 0 };
        for _ in 0..400 {
            state.fire_cooldown = 0;
            try_fire(&mut state);
            state.balls.clear();
            // 電サポ中に打たれた玉なので、入賞も通常時の分子には入らない。
            resolve_start_pocket(&mut state, false);
            state.pending.clear();
        }
        let after_assist =
            spin_rate(state.seated_machine().expect("着席していない")).expect("標本が足りない");

        assert!(
            (after_assist - normal_only).abs() < 1e-9,
            "電サポ中の打ち込みと回転が表示回転率へ混ざっている \
             (通常のみ {normal_only:.2} → 電サポ後 {after_assist:.2})"
        );
        let machine = state.seated_machine().expect("着席していない");
        assert!(
            machine.balls_spent > machine.normal_balls_spent,
            "総打ち込み数は電サポ中の分も数える"
        );
    }

    #[test]
    fn a_ball_counts_toward_the_mode_it_was_fired_in() {
        // 打ち出しから入賞までは数秒あり、その間に電サポが切れることがある。
        // 入賞時のモードで数えると、分母を増やさなかった玉が分子だけ増やす。
        let mut state = seated_state();
        state.balls_held = 10;
        state.firing = true;
        state.mode = Mode::Kakuhen { spins_left: 0 };
        state.fire_cooldown = 0;
        try_fire(&mut state);
        let before = state
            .seated_machine()
            .expect("着席していない")
            .normal_balls_spent;
        assert_eq!(before, 0, "電サポ中に打った玉が通常時の分母に入っている");

        // 玉が落ちている間に電サポが切れ、その後ヘソへ入る。
        state.mode = Mode::Normal;
        let fired_in_normal = state.balls[0].fired_in_normal;
        resolve_start_pocket(&mut state, fired_in_normal);

        let machine = state.seated_machine().expect("着席していない");
        assert_eq!(
            machine.normal_spins_seen, 0,
            "電サポ中に打った玉の入賞が通常時の分子に入り、分母と区間が食い違う"
        );
        assert_eq!(machine.spins_seen, 1, "総回転数はモードによらず数える");
    }

    #[test]
    fn a_spin_carries_the_rank_of_the_pending_it_came_from() {
        // 保留が空の状態で入賞した玉は、待つ間もなく消化される。保留列から
        // 消えた後もランクを読めるよう、回転中の抽選が自分のランクを持ち回る
        // (描画はこれを使って消化中の1つを描く)。
        let mut state = seated_state();
        state
            .pending
            .push(Pending::new(ranked_miss(PendingRank::Gold)));
        state.digit = Digit::Idle;
        start_spin_if_idle(&mut state);

        let Digit::Spinning { outcome, .. } = &state.digit else {
            panic!("回転が始まっていない");
        };
        assert_eq!(
            outcome.rank,
            PendingRank::Gold,
            "回転中の抽選から元の保留のランクを読めない"
        );
    }

    #[test]
    fn a_new_jackpot_counts_its_payout_up_from_zero() {
        // 連チャンで前回の額が残っていると、カウンタは新しい大当たりの獲得を
        // 積み上げる前に前回の額から下がっていく。
        let mut state = seated_state();
        state.jackpot_payout_shown = 1_800.0;
        resolve_spin(&mut state, jackpot(10));
        assert_eq!(
            state.jackpot_payout_shown, 0.0,
            "前の大当たりの出玉が表示に残り、増える前に減っていく"
        );
    }

    #[test]
    fn the_summary_keeps_the_chain_the_jackpot_ended_on() {
        // 決算は電サポが切れた後も残る。進行中の連チャン数を使うと、
        // 数え直された時点で決算から連チャンの長さだけが消える。
        let mut state = seated_state();
        resolve_spin(&mut state, jackpot(5));
        state.mode = Mode::Jitan { spins_left: 1 };
        resolve_spin(&mut state, assisted_jackpot(5));
        assert_eq!(state.chain, 2);

        let Mode::Jackpot(jackpot_state) = state.mode else {
            panic!("大当たり中ではない");
        };
        end_jackpot(&mut state, jackpot_state);
        assert_eq!(
            state.last_jackpot_chain, 2,
            "終了時点の連チャン数が残っていない"
        );

        // 電サポを切らして連チャンを数え直しても、決算の値は動かない。
        state.mode = Mode::Normal;
        state.chain = 0;
        assert_eq!(state.last_jackpot_chain, 2);
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
        assert!(
            near_misses > 0,
            "惜しいハズレが一度も出ず、検証になっていない"
        );
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
        assert!(
            confirmed > 0,
            "確定シグナルが一度も立たず、検証になっていない"
        );
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
        assert!(
            slip > plain,
            "滑りで回転が伸びていない (滑り={slip} 素={plain})"
        );
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
    fn a_pending_drawn_during_a_jackpot_belongs_to_the_mode_that_follows_it() {
        // 大当たり中に貯まった保留はラウンドが終わってから消化される。抽選を
        // 引いた時点のモードで見ると、確変が約束されている最中に貯めた保留が
        // 通常確率になり、当たっても連チャンが数え直される。
        let mut state = seated_state();
        let spec = state.seated_machine().expect("着席していない").spec;
        state.mode = Mode::Jackpot(JackpotState {
            round: 1,
            total_rounds: 16,
            count: 0,
            ticks_left: ROUND_LIMIT_TICKS,
            kakuhen: true,
            payout: 0,
        });

        let mut hits = 0u32;
        const TRIALS: u32 = 20_000;
        for _ in 0..TRIALS {
            let outcome = roll_outcome(&mut state);
            assert!(
                outcome.assisted,
                "大当たり中に引いた保留が電サポ外の抽選として記録され、消化時に連チャンが切れる"
            );
            if outcome.hit {
                hits += 1;
            }
        }

        // 確変の分母で引けているかを、通常の分母との中間に閾値を置いて見る。
        // 乱数の揺れで判定が裏返らないよう、両者の間隔を使って余裕を取る。
        let observed = TRIALS as f64 / hits.max(1) as f64;
        let midpoint = (spec.kakuhen_odds + spec.normal_odds) as f64 / 2.0;
        assert!(
            observed < midpoint,
            "確変が約束された大当たり中の保留が通常確率で抽選されている \
             (実測 1/{observed:.1} 確変 1/{} 通常 1/{})",
            spec.kakuhen_odds,
            spec.normal_odds
        );
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
        state
            .pending
            .push(Pending::new(assisted_miss(ReachKind::None)));
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
            assert!(
                diff < prev,
                "表示値が実際の値へ近づいていない ({prev} → {diff})"
            );
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
        state
            .balls
            .push(test_ball(ATTACKER_X, ATTACKER_Y - 0.5, 0.0, MAX_SPEED));
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
        state.digit = Digit::Spinning {
            ticks_left: 1,
            outcome,
        };
        advance_digit(&mut state);
        assert_eq!(state.digit, Digit::Idle);
        assert_eq!(state.last_reels, outcome.reels);

        // 台を替えれば前の台の液晶は付いてこない。
        state.mode = Mode::Normal;
        assert!(leave_seat(&mut state));
        assert_eq!(state.last_reels, INITIAL_REELS);
    }
}
