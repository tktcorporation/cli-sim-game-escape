//! 玉響 — 自動プレイシミュレーター。
//!
//! 席に着いて打ちっぱなしにする bot でホールを回し、長時間運転で panic や
//! 不変条件の破れが起きないこと、および「釘読み」「軍資金のやりくり」という
//! 判断軸が数値として成立していることを検証する。
//!
//! パチンコは1回の試行の分散が極端に大きく、目視のプレイテストでは
//! 「たまたま当たった / たまたま溶けた」としか分からない。回転率・出玉率・
//! 破産までの時間は、いずれも多数の試行の分布として初めて読める。
//!
//! バランスの要は **出玉率 (賞球総数 ÷ 打ち込み玉数)** で、これが 1.0 を
//! 超えると打つほど玉が増えて軍資金が尽きず、「有限の資金でどこまで粘るか」
//! という判断軸そのものが消える。`payout_ratio_stays_below_break_even` が
//! その一線を守る。
//!
//! `cargo test pachinko::simulator -- --nocapture` でレポートを確認できる。

#![cfg(test)]

use super::logic::{self, NAIL_SPREAD_RANGE, RAIL_BIAS_RANGE};
use super::state::{
    Digit, Machine, Mode, PachinkoState, PendingRank, ReachKind, StopStyle, BALL_LOAN_COUNT,
    BALL_LOAN_YEN, BALL_R, BOARD_H, BOARD_W, FIRE_INTERVAL_TICKS, HALL_SIZE, HIT_GLOW_TICKS,
    MACHINE_SPECS, MAX_BALLS, MAX_PENDING,
};

// ── 自動プレイ ─────────────────────────────────────────────────

/// 軍資金と持ち玉を使い切るまでに許す tick の上限。出玉率が 1.0 に届いた台は
/// 打ち止めにならないので、ここで打ち切って「終わらない」ことを検知する。
const BUST_TICK_LIMIT: u32 = 400_000;
/// レポート用に打ち止めまで回すときの上限。不変条件の検証より短く取り、
/// 試行数を稼ぐ側へ時間を割く。
const REPORT_TICK_LIMIT: u32 = 250_000;

/// 指定のスペックと釘で台を1台作る。`logic::generate_hall` はスペックも釘も
/// 乱数で振るため、比較したい軸だけを動かす測定には使えない。
fn machine_with(spec_index: usize, nail_spread: f64, rail_bias: f64, seed: &mut u32) -> Machine {
    let (name, spec) = MACHINE_SPECS[spec_index];
    let nails = logic::generate_nails(seed, nail_spread, rail_bias);
    Machine {
        name,
        spec,
        nail_spread,
        rail_bias,
        nails,
        balls_spent: 0,
        spins_seen: 0,
    }
}

/// 指定の台に着席した状態を作る。
fn seated_state(seed: u32, machine: Machine) -> PachinkoState {
    let mut state = PachinkoState::new();
    state.rng_state = seed;
    state.machines = vec![machine];
    assert!(logic::sit_at(&mut state, 0), "着席できなかった");
    state
}

/// ホールを生成して指定の席に着いた状態を作る。
fn hall_state(seed: u32, seat: usize) -> PachinkoState {
    let mut state = PachinkoState::new();
    state.rng_state = seed;
    logic::generate_hall(&mut state);
    assert!(logic::sit_at(&mut state, seat), "着席できなかった");
    state
}

/// 自動プレイ1回分の結果。
#[derive(Clone, Debug)]
struct RunResult {
    name: &'static str,
    nail_spread: f64,
    /// 打ち止めまでに進んだ tick。上限に達した場合は上限値。
    ticks: u32,
    /// 上限に達しても打ち止めにならなかったか。
    censored: bool,
    /// 軍資金を全て玉に替え終えた tick。替え終わる前に打ち止めなら None。
    ticks_to_spend_budget: Option<u32>,
    jackpots: u32,
    best_chain: u32,
    spins: u32,
    balls_fired: u32,
    /// 賞球で得た玉の総数。
    balls_won: u32,
    peak_balls: u32,
}

impl RunResult {
    /// 遊技時間 (秒)。10 ticks/sec。
    fn play_secs(&self) -> f64 {
        self.ticks as f64 / 10.0
    }

    /// 軍資金を使い切るまでの秒数。使い切る前に打ち止めなら全体の遊技時間。
    fn budget_secs(&self) -> f64 {
        self.ticks_to_spend_budget.unwrap_or(self.ticks) as f64 / 10.0
    }

    /// 出玉率。1.0 で収支トントン。
    fn payout_ratio(&self) -> f64 {
        if self.balls_fired == 0 {
            0.0
        } else {
            self.balls_won as f64 / self.balls_fired as f64
        }
    }

    /// 実測回転率 (千円あたりの回転数)。
    fn spin_rate(&self) -> f64 {
        if self.balls_fired == 0 {
            0.0
        } else {
            self.spins as f64 * BALL_LOAN_COUNT as f64 / self.balls_fired as f64
        }
    }
}

/// 打ちっぱなしにして、軍資金と持ち玉を使い切るまで回す。玉の補充は
/// `logic::tick` 内の自動補充に任せ、bot はハンドルを握るだけ。
fn play_until_broke(state: &mut PachinkoState, limit: u32) -> RunResult {
    state.firing = true;
    let mut ticks_to_spend_budget = None;
    let mut peak_balls = 0u32;
    let mut t = 0u32;
    while state.firing && t < limit {
        logic::tick(state);
        t += 1;
        peak_balls = peak_balls.max(state.balls_held);
        if ticks_to_spend_budget.is_none() && state.cash < BALL_LOAN_YEN {
            ticks_to_spend_budget = Some(t);
        }
    }
    let machine = state.seated_machine().expect("着席していない");
    let bought = state.invested / BALL_LOAN_YEN * BALL_LOAN_COUNT;
    // 持ち玉 = 借りた玉 + 賞球 - 打ち込んだ玉、を賞球について解く。
    let won = state.balls_held as i64 + machine.balls_spent as i64 - bought as i64;
    RunResult {
        name: machine.name,
        nail_spread: machine.nail_spread,
        ticks: t,
        censored: state.firing,
        ticks_to_spend_budget,
        jackpots: state.record.total_jackpots,
        best_chain: state.record.best_chain,
        spins: machine.spins_seen,
        balls_fired: machine.balls_spent,
        balls_won: won.max(0) as u32,
        peak_balls,
    }
}

/// ヘソへの入りやすさだけを測る。毎 tick 保留とデジタルを捨てるのは、
/// 電サポ中はヘソの受け口が広がる (`logic::effective_pocket_half_w`) ため、
/// 当たりを引くと「釘の開き」ではなく「引きの強さ」を測ってしまうから。
fn measure_raw_spin_rate(nail_spread: f64, rail_bias: f64, seed: u32, ticks: u32) -> f64 {
    let mut nail_seed = seed ^ 0x5EED_1234;
    let machine = machine_with(0, nail_spread, rail_bias, &mut nail_seed);
    let mut state = seated_state(seed, machine);
    // 玉切れで測定が途切れないよう直接持たせる。現金は 0 のままなので
    // 自動補充は走らない。
    state.balls_held = 1_000_000;
    state.cash = 0;
    state.firing = true;
    for _ in 0..ticks {
        logic::tick(&mut state);
        state.pending.clear();
        state.digit = Digit::Idle;
    }
    let machine = state.seated_machine().expect("着席していない");
    machine.spins_seen as f64 * BALL_LOAN_COUNT as f64 / machine.balls_spent.max(1) as f64
}

/// 長期の出玉率 (賞球総数 ÷ 打ち込み玉数)。軍資金という区切りを外して
/// 打ち続けたときの比なので、1回の試行の当たり外れではなく台そのものの
/// 性質を測れる。
fn measure_payout_ratio(
    spec_index: usize,
    nail_spread: f64,
    rail_bias: f64,
    layout_seed: u32,
    play_seed: u32,
    ticks: u32,
) -> f64 {
    let mut nail_seed = layout_seed;
    let machine = machine_with(spec_index, nail_spread, rail_bias, &mut nail_seed);
    let mut state = seated_state(play_seed, machine);
    state.balls_held = 100_000_000;
    state.cash = 0;
    state.firing = true;
    let held_before = state.balls_held;
    for _ in 0..ticks {
        logic::tick(&mut state);
    }
    let fired = state.seated_machine().expect("着席していない").balls_spent;
    let won = state.balls_held as i64 - held_before as i64 + fired as i64;
    won as f64 / fired.max(1) as f64
}

// ── 玉の見え方 ─────────────────────────────────────────────────

/// 玉1個が打ち出されてから盤面を去るまでの軌跡。
struct Flight {
    /// 盤面に居た tick 数。10 ticks/sec なのでそのまま滞空時間になる。
    ticks: u32,
    /// 釘に触れた tick 数。1 tick は `logic::PHYSICS_SUBSTEPS` 回の判定を
    /// 含むため、同じ tick に複数本の釘へ当たっても 1 と数える。
    contact_ticks: u32,
    /// tick ごとの移動距離。描画は tick 単位なので、この値がそのまま
    /// 「1コマで玉がどれだけ飛ぶか」になる。
    steps: Vec<f64>,
}

/// 玉を1個だけ打ち出し、盤面を去るまで tick 単位で追う。
///
/// 玉同士は衝突しないので、1個だけ流した軌跡は盤面が混んでいるときの軌跡と
/// 変わらない。混雑した盤面から特定の1個を追い続けるより、こちらの方が
/// 取り違えなく測れる。
fn measure_flight(state: &mut PachinkoState) -> Flight {
    state.balls_held = state.balls_held.max(1);
    state.firing = true;
    state.fire_cooldown = 0;
    // 打ち出しは `fire_cooldown` を挟むので、玉が出るまで数 tick かかる。
    for _ in 0..(FIRE_INTERVAL_TICKS + 2) {
        logic::tick(state);
        if !state.balls.is_empty() {
            break;
        }
    }
    state.firing = false;
    assert_eq!(state.balls.len(), 1, "測定用の玉が打ち出されていない");

    let mut flight = Flight {
        ticks: 1,
        contact_ticks: 0,
        steps: Vec::new(),
    };
    // 盤面の上端から下端まで落ちても足りる長さ。無限ループの保険。
    const FLIGHT_TICK_LIMIT: u32 = 2_000;
    while flight.ticks < FLIGHT_TICK_LIMIT {
        let before = state.balls[0];
        logic::tick(state);
        flight.ticks += 1;
        let Some(after) = state.balls.first() else {
            break;
        };
        let (dx, dy) = (after.x - before.x, after.y - before.y);
        flight.steps.push((dx * dx + dy * dy).sqrt());
        // `decay_glow` が tick の頭で 1 減らした後に `step_balls` が焼き直すので、
        // tick 終わりに満タンなら この tick で釘に触れている。
        if after.hit_glow == HIT_GLOW_TICKS {
            flight.contact_ticks += 1;
        }
    }
    // 次の測定へ玉と保留を持ち越さない。
    state.balls.clear();
    state.pending.clear();
    state.digit = Digit::Idle;
    flight
}

/// 打ちっぱなしにしたときの盤面上の玉数 (平均, 最大)。滞空時間が伸びると
/// ここが `MAX_BALLS` へ張り付き、打ち出しそのものが止まる。
fn measure_board_crowding(nail_spread: f64, seed: u32, ticks: u32) -> (f64, usize) {
    let mut nail_seed = seed ^ 0x5EED_1234;
    let machine = machine_with(0, nail_spread, 0.0, &mut nail_seed);
    let mut state = seated_state(seed, machine);
    state.balls_held = 1_000_000;
    state.cash = 0;
    state.firing = true;
    let mut total = 0u64;
    let mut peak = 0usize;
    for _ in 0..ticks {
        logic::tick(&mut state);
        total += state.balls.len() as u64;
        peak = peak.max(state.balls.len());
    }
    (total as f64 / ticks as f64, peak)
}

// ── 統計ヘルパ ─────────────────────────────────────────────────

fn percentile(sorted: &[f64], q: f64) -> f64 {
    assert!(!sorted.is_empty());
    let idx = ((sorted.len() - 1) as f64 * q).round() as usize;
    sorted[idx]
}

fn sorted(mut values: Vec<f64>) -> Vec<f64> {
    values.sort_by(|a, b| a.partial_cmp(b).unwrap());
    values
}

fn mean(values: &[f64]) -> f64 {
    if values.is_empty() {
        0.0
    } else {
        values.iter().sum::<f64>() / values.len() as f64
    }
}

fn stddev(values: &[f64]) -> f64 {
    if values.len() < 2 {
        return 0.0;
    }
    let m = mean(values);
    (values.iter().map(|v| (v - m) * (v - m)).sum::<f64>() / (values.len() - 1) as f64).sqrt()
}

fn reach_index(kind: ReachKind) -> usize {
    match kind {
        ReachKind::None => 0,
        ReachKind::Normal => 1,
        ReachKind::Super => 2,
        ReachKind::Premium => 3,
    }
}

/// 1台につき `TRIALS` 回抽選を引き、リーチの格ごとの (出現数, 当たり数) を返す。
fn reach_tally(spec_index: usize, trials: u32) -> ([u32; 4], [u32; 4]) {
    let mut nail_seed = 0x0F0F_0F0F;
    let machine = machine_with(spec_index, 0.5, 0.0, &mut nail_seed);
    let mut state = seated_state(0x1234_ABCD, machine);
    let mut seen = [0u32; 4];
    let mut hits = [0u32; 4];
    for _ in 0..trials {
        let outcome = logic::roll_outcome(&mut state);
        let slot = reach_index(outcome.reach);
        seen[slot] += 1;
        if outcome.hit {
            hits[slot] += 1;
        }
    }
    (seen, hits)
}

fn rank_index(rank: PendingRank) -> usize {
    PendingRank::ALL
        .iter()
        .position(|&r| r == rank)
        .expect("PendingRank::ALL に無いランク")
}

fn stop_index(stop: StopStyle) -> usize {
    StopStyle::ALL
        .iter()
        .position(|&s| s == stop)
        .expect("StopStyle::ALL に無い停止型")
}

/// 1台につき `trials` 回抽選を引いた集計。演出の設計は「決まった結果を
/// どう小出しにするか」なので、確かめたいのは出現率そのものではなく、
/// 出現率と当たり率の対応関係になる。
struct EffectTally {
    /// ランクごとの (出現数, 当たり数)。
    rank_seen: [u32; 6],
    rank_hits: [u32; 6],
    /// 停止の型ごとの出現数。
    stop_seen: [u32; 4],
    /// 停止の型による回転時間の上乗せ (tick) の合計。
    extra_ticks: u64,
    /// 素の回転時間 (`ReachKind::spin_ticks`) の合計。
    base_ticks: u64,
    hits: u32,
    confirmed: u32,
}

fn effect_tally(spec_index: usize, trials: u32) -> EffectTally {
    let mut nail_seed = 0x0F0F_0F0F;
    let machine = machine_with(spec_index, 0.5, 0.0, &mut nail_seed);
    let mut state = seated_state(0x1234_ABCD, machine);
    let mut tally = EffectTally {
        rank_seen: [0; 6],
        rank_hits: [0; 6],
        stop_seen: [0; 4],
        extra_ticks: 0,
        base_ticks: 0,
        hits: 0,
        confirmed: 0,
    };
    for _ in 0..trials {
        let outcome = logic::roll_outcome(&mut state);
        let slot = rank_index(outcome.rank);
        tally.rank_seen[slot] += 1;
        tally.stop_seen[stop_index(outcome.stop)] += 1;
        tally.extra_ticks += outcome.stop.extra_ticks() as u64;
        tally.base_ticks += outcome.reach.spin_ticks() as u64;
        if outcome.hit {
            tally.hits += 1;
            tally.rank_hits[slot] += 1;
        }
        if outcome.confirmed {
            tally.confirmed += 1;
        }
    }
    tally
}

// ── レポート ───────────────────────────────────────────────────

/// 玉の動きが目で追える速さかを測る。10 ticks/sec で描画するので、1 tick の
/// 移動距離が玉の直径 (`BALL_R * 2`) の数倍を超えると、玉は毎コマ離れた位置へ
/// 飛んで現れ、釘に弾かれる瞬間そのものが見えなくなる。
#[test]
fn ball_motion_report() {
    const LAYOUTS: u32 = 4;
    const BALLS_PER_LAYOUT: u32 = 24;
    let diameter = BALL_R * 2.0;

    let mut flight_ticks = Vec::new();
    let mut contacts = Vec::new();
    let mut steps = Vec::new();
    let mut nail_counts = Vec::new();

    for layout in 1..=LAYOUTS {
        let mut nail_seed = layout.wrapping_mul(2_654_435_761);
        let machine = machine_with(0, 0.55, 0.0, &mut nail_seed);
        nail_counts.push(machine.nails.len() as f64);
        let mut state = seated_state(layout.wrapping_mul(40_503), machine);
        state.cash = 0;
        for _ in 0..BALLS_PER_LAYOUT {
            let flight = measure_flight(&mut state);
            flight_ticks.push(flight.ticks as f64);
            contacts.push(flight.contact_ticks as f64);
            steps.extend(flight.steps);
        }
    }

    let f = sorted(flight_ticks);
    let c = sorted(contacts);
    let s = sorted(steps);

    eprintln!(
        "[pachinko/motion] 釘{:.0}本 玉{}個の軌跡 (釘{LAYOUTS}通り × {BALLS_PER_LAYOUT}個)",
        mean(&nail_counts),
        f.len()
    );
    eprintln!(
        "  滞空時間:       平均={:.1}tick ({:.1}s) 中央={:.0}tick p90={:.0}tick 最長={:.0}tick",
        mean(&f),
        mean(&f) / 10.0,
        percentile(&f, 0.5),
        percentile(&f, 0.9),
        f[f.len() - 1],
    );
    eprintln!(
        "  釘に触れたtick: 平均={:.1}回 中央={:.0}回 p90={:.0}回 (滞空の{:.0}%)",
        mean(&c),
        percentile(&c, 0.5),
        percentile(&c, 0.9),
        mean(&c) / mean(&f) * 100.0,
    );
    eprintln!(
        "  1tickの移動:    平均={:.2} (玉の直径の{:.1}倍 / 盤面高の{:.1}%) \
         中央={:.2} p90={:.2} 最大={:.2} (直径の{:.1}倍)",
        mean(&s),
        mean(&s) / diameter,
        mean(&s) / BOARD_H * 100.0,
        percentile(&s, 0.5),
        percentile(&s, 0.9),
        s[s.len() - 1],
        s[s.len() - 1] / diameter,
    );

    for spread in [NAIL_SPREAD_RANGE.0, 0.55, NAIL_SPREAD_RANGE.1] {
        let (avg, peak) = measure_board_crowding(spread, 0x3333_4444, 20_000);
        eprintln!(
            "  盤面の玉数:     開き={spread:.2} 平均={avg:.1}個 最大={peak}個 / 上限{MAX_BALLS}個",
        );
    }
}

/// 各台の実測回転率を `nail_spread` と並べて出す。釘の開きが回転率として
/// 現れているか (＝盤面を見て台を選ぶ意味があるか) を人間が読むためのもの。
#[test]
fn spin_rate_report() {
    eprintln!("[pachinko/spin-rate] ホールの台を打ち止めまで打った実測");
    for seed in 1..=3u32 {
        for seat in 0..HALL_SIZE {
            let mut state = hall_state(seed.wrapping_mul(7919), seat);
            let run = play_until_broke(&mut state, REPORT_TICK_LIMIT);
            eprintln!(
                "  seed={seed} seat={seat} {:8} 釘の開き={:.2} 回転率={:5.1}/千円 \
                 総回転={:4} 大当={:2} 遊技={:.0}s",
                run.name,
                run.nail_spread,
                run.spin_rate(),
                run.spins,
                run.jackpots,
                run.play_secs(),
            );
        }
    }

    // 実プレイの回転率は電サポの有無で揺れるので、「釘の開き→回転率」の
    // 対応はこちらの対照で読む。
    eprintln!("[pachinko/spin-rate] 釘の開きだけを動かした対照 (電サポ無し)");
    for spread in [0.25, 0.39, 0.53, 0.66, 0.80] {
        let rates = sorted(
            (1..=12u32)
                .map(|s| measure_raw_spin_rate(spread, 0.0, s.wrapping_mul(104_729), 4_000))
                .collect(),
        );
        eprintln!(
            "  開き={spread:.2} 平均={:5.2}/千円 (最低={:.2} 最高={:.2})",
            mean(&rates),
            rates[0],
            rates[rates.len() - 1],
        );
    }
}

/// 1万円あたりの遊技時間・出玉・大当たり回数・連チャンの分布。
#[test]
fn payout_report() {
    const RUNS: u32 = 32;
    let mut budget_secs = Vec::new();
    let mut play_secs = Vec::new();
    let mut won = Vec::new();
    let mut jackpots = Vec::new();
    let mut ratios = Vec::new();
    let mut peaks = Vec::new();
    let mut chains = Vec::new();
    let mut censored = 0u32;

    for seed in 1..=RUNS {
        let mut state = hall_state(seed.wrapping_mul(2_654_435_761), 0);
        let run = play_until_broke(&mut state, REPORT_TICK_LIMIT);
        budget_secs.push(run.budget_secs());
        play_secs.push(run.play_secs());
        won.push(run.balls_won as f64);
        jackpots.push(run.jackpots as f64);
        ratios.push(run.payout_ratio());
        peaks.push(run.peak_balls as f64);
        if run.jackpots > 0 {
            chains.push(run.best_chain as f64);
        }
        if run.censored {
            censored += 1;
        }
    }

    let b = sorted(budget_secs);
    let p = sorted(play_secs);
    let w = sorted(won);
    let j = sorted(jackpots);
    let r = sorted(ratios);
    let k = sorted(peaks);
    let c = sorted(chains);

    eprintln!("[pachinko/payout] runs={RUNS} 軍資金1万円 (250玉×10) 打ち止めまで");
    eprintln!(
        "  軍資金を使い切るまで: 中央={:.0}s p10={:.0}s p90={:.0}s",
        percentile(&b, 0.5),
        percentile(&b, 0.1),
        percentile(&b, 0.9)
    );
    eprintln!(
        "  打ち止めまで:         中央={:.0}s p10={:.0}s p90={:.0}s (打ち切り={censored}件)",
        percentile(&p, 0.5),
        percentile(&p, 0.1),
        percentile(&p, 0.9)
    );
    eprintln!(
        "  出玉 (賞球総数):      平均={:.0} 中央={:.0} p10={:.0} p90={:.0}",
        mean(&w),
        percentile(&w, 0.5),
        percentile(&w, 0.1),
        percentile(&w, 0.9)
    );
    eprintln!(
        "  最高持ち玉:           中央={:.0} p90={:.0}",
        percentile(&k, 0.5),
        percentile(&k, 0.9)
    );
    eprintln!(
        "  大当たり回数:         平均={:.2} 中央={:.0} p90={:.0} 0回の試行={:.0}%",
        mean(&j),
        percentile(&j, 0.5),
        percentile(&j, 0.9),
        j.iter().filter(|&&x| x == 0.0).count() as f64 / RUNS as f64 * 100.0
    );
    eprintln!(
        "  最高連チャン:         平均={:.2} 中央={:.0} 最大={:.0} (当たりを引いた{}試行)",
        mean(&c),
        percentile(&c, 0.5),
        c.last().copied().unwrap_or(0.0),
        c.len()
    );
    eprintln!(
        "  出玉率 (出玉/打込):   平均={:.2} 中央={:.2} p90={:.2}",
        mean(&r),
        percentile(&r, 0.5),
        percentile(&r, 0.9)
    );
}

/// リーチの格ごとの実際の当たり率。格を上げるほど当たりが近い、という
/// 関係が壊れると「長く回った＝熱い」という学習が嘘になる。
#[test]
fn reach_reliability_report() {
    const TRIALS: u32 = 200_000;
    eprintln!("[pachinko/reach] 格ごとの当たり率 (試行={TRIALS}/機種)");
    for (index, (name, _)) in MACHINE_SPECS.iter().enumerate() {
        let (seen, hits) = reach_tally(index, TRIALS);
        eprint!("  {name:8}");
        for kind in ReachKind::ALL {
            let slot = reach_index(kind);
            let rate = if seen[slot] == 0 {
                0.0
            } else {
                hits[slot] as f64 / seen[slot] as f64 * 100.0
            };
            eprint!(" {}={rate:5.1}%({}回)", kind.label(), seen[slot]);
        }
        eprintln!();
    }
}

/// 保留のランクごとの実際の当たり率 (＝信頼度) と出現率。
///
/// 信頼度は当たり時とハズレ時の出現率の比が決めるので、狙った値になっている
/// かは実測でしか読めない (`PendingRank::weight_on_hit` 参照)。狙いは 1/80 の
/// 花火繚乱で 白<1% / 青5% / 緑18% / 赤45% / 金80% / 虹100%。
#[test]
fn pending_rank_reliability_report() {
    const TRIALS: u32 = 400_000;
    eprintln!("[pachinko/rank] 保留ランクごとの当たり率と出現率 (試行={TRIALS}/機種)");
    for (index, (name, spec)) in MACHINE_SPECS.iter().enumerate() {
        let tally = effect_tally(index, TRIALS);
        eprintln!("  {name:8} 1/{}", spec.normal_odds);
        for rank in PendingRank::ALL {
            let slot = rank_index(rank);
            let seen = tally.rank_seen[slot];
            let rate = if seen == 0 {
                0.0
            } else {
                tally.rank_hits[slot] as f64 / seen as f64 * 100.0
            };
            eprintln!(
                "    {} 信頼度={rate:6.2}%  出現={:6.3}% ({seen}回 / 当たり{}回)",
                rank.label(),
                seen as f64 / TRIALS as f64 * 100.0,
                tally.rank_hits[slot],
            );
        }
    }
}

/// 停止の型ごとの発生率と、それが遊技時間へ与える影響。
///
/// 上乗せ tick は1回転を長くする。回転が長いほど保留が詰まり、抽選を受け
/// られないヘソ入賞が増える (＝実効回転率が落ちる) ので、演出の見応えと
/// 遊技のテンポはここでトレードオフになる。
#[test]
fn stop_style_report() {
    const TRIALS: u32 = 400_000;
    eprintln!("[pachinko/stop] 停止の型の発生率と回転時間への影響 (試行={TRIALS}/機種)");
    for (index, (name, _)) in MACHINE_SPECS.iter().enumerate() {
        let tally = effect_tally(index, TRIALS);
        eprint!("  {name:8}");
        for stop in StopStyle::ALL {
            eprint!(
                " {}={:.3}%",
                stop.label(),
                tally.stop_seen[stop_index(stop)] as f64 / TRIALS as f64 * 100.0
            );
        }
        eprintln!(
            " | 平均変動={:.2}tick (素={:.2} 上乗せ={:.3} = {:+.2}%) 確定={:.4}%",
            (tally.base_ticks + tally.extra_ticks) as f64 / TRIALS as f64,
            tally.base_ticks as f64 / TRIALS as f64,
            tally.extra_ticks as f64 / TRIALS as f64,
            tally.extra_ticks as f64 / tally.base_ticks as f64 * 100.0,
            tally.confirmed as f64 / TRIALS as f64 * 100.0,
        );
    }
}

/// 3機種の性格の差。甘い台は当たりが軽いが出玉が少なく、荒い台はその逆、
/// という設計意図が収支の荒さ (σ) の差として出ているかを読む。
#[test]
fn machine_spec_report() {
    const RUNS: u32 = 20;
    eprintln!("[pachinko/spec] runs={RUNS} 釘は開き0.55 / 傾き0.0 に固定");
    for (index, (name, spec)) in MACHINE_SPECS.iter().enumerate() {
        let mut won = Vec::new();
        let mut jackpots = Vec::new();
        let mut secs = Vec::new();
        let mut ratios = Vec::new();
        let mut chains = Vec::new();
        let mut peaks = Vec::new();
        for seed in 1..=RUNS {
            let mut nail_seed = seed.wrapping_mul(2_246_822_519);
            let machine = machine_with(index, 0.55, 0.0, &mut nail_seed);
            let mut state = seated_state(seed.wrapping_mul(3_266_489_917), machine);
            let run = play_until_broke(&mut state, REPORT_TICK_LIMIT);
            won.push(run.balls_won as f64);
            jackpots.push(run.jackpots as f64);
            secs.push(run.play_secs());
            ratios.push(run.payout_ratio());
            chains.push(run.best_chain as f64);
            peaks.push(run.peak_balls as f64);
        }
        let s = sorted(secs);
        eprintln!(
            "  {name:8} 1/{:<3} 確変{:>2}%/{:>2}回 出玉:平均={:6.0} σ={:6.0} \
             最高持ち玉:中央={:5.0} 最大={:6.0} 大当:平均={:.2} 連チャン最高={:.0} \
             遊技:中央={:.0}s 出玉率={:.2}",
            spec.normal_odds,
            spec.kakuhen_rate,
            spec.jitan_spins,
            mean(&won),
            stddev(&won),
            percentile(&sorted(peaks.clone()), 0.5),
            sorted(peaks).last().copied().unwrap_or(0.0),
            mean(&jackpots),
            sorted(chains).last().copied().unwrap_or(0.0),
            percentile(&s, 0.5),
            mean(&ratios),
        );
    }
}

/// 台ごとの長期出玉率。釘の開きが収支へどう効くか (＝良い台を選ぶ見返り)
/// と、どの台も 1.0 を割っていることを同時に読む。
///
/// 釘の組み合わせごとに長期の試行を回すため単体で 25 秒かかる。assert を
/// 持たない観測専用なので、`#[ignore]` で routine な `cargo test` から外す
/// (metropolis の長尺ベンチと同じ運用)。バランスを触るときに
/// `cargo test pachinko::simulator::payout_ratio_report -- --ignored --nocapture`
/// で読む。出玉率が損益分岐を割ることの保証は
/// `payout_ratio_stays_below_break_even` が短い試行で担う。
#[test]
#[ignore]
fn payout_ratio_report() {
    const LAYOUTS: u32 = 8;
    const TICKS: u32 = 120_000;
    eprintln!("[pachinko/ratio] 長期出玉率 (釘{LAYOUTS}通り × {TICKS}tick)");
    for (index, (name, _)) in MACHINE_SPECS.iter().enumerate() {
        for spread in [NAIL_SPREAD_RANGE.0, 0.5, NAIL_SPREAD_RANGE.1] {
            let ratios = sorted(
                (1..=LAYOUTS)
                    .map(|s| {
                        measure_payout_ratio(
                            index,
                            spread,
                            0.0,
                            s.wrapping_mul(2_654_435_761),
                            s.wrapping_mul(40_503),
                            TICKS,
                        )
                    })
                    .collect(),
            );
            eprintln!(
                "  {name:8} 開き={spread:.2} 平均={:.3} 中央={:.3} 最大={:.3}",
                mean(&ratios),
                percentile(&ratios, 0.5),
                ratios[ratios.len() - 1]
            );
        }
    }
}

// ── 不変条件 ───────────────────────────────────────────────────

#[test]
fn long_run_never_panics_and_keeps_invariants() {
    let mut state = hall_state(0x0BAD_F00D, 0);
    state.firing = true;
    for t in 0..20_000u32 {
        logic::tick(&mut state);

        assert!(
            state.balls.len() <= MAX_BALLS,
            "盤面の玉が上限を超えた — 打ち出しが上限を見ずに撃っている疑い \
             (tick={t}, {}個)",
            state.balls.len()
        );
        assert!(
            state.pending.len() <= MAX_PENDING,
            "保留が上限を超えた — 保留満タン時のヘソ入賞が積まれている疑い \
             (tick={t}, {}個)",
            state.pending.len()
        );
        for ball in &state.balls {
            assert!(
                ball.x.is_finite()
                    && ball.y.is_finite()
                    && ball.vx.is_finite()
                    && ball.vy.is_finite(),
                "玉の座標か速度が NaN/無限になった — 釘の法線計算がゼロ除算した疑い \
                 (tick={t}, pos=({}, {}), vel=({}, {}))",
                ball.x,
                ball.y,
                ball.vx,
                ball.vy
            );
            assert!(
                (0.0..=BOARD_W).contains(&ball.x) && ball.y <= BOARD_H,
                "玉が盤面の外へ出た — 壁の反射か釘の押し出しが盤外へ飛ばした疑い \
                 (tick={t}, pos=({:.2}, {:.2}))",
                ball.x,
                ball.y
            );
        }
        if let Mode::Jackpot(jackpot) = state.mode {
            assert!(
                (1..=jackpot.total_rounds).contains(&jackpot.round),
                "ラウンド番号が総ラウンド数の範囲外 — ラウンド進行の終了判定の抜け \
                 (tick={t}, {}R/{}R)",
                jackpot.round,
                jackpot.total_rounds
            );
        }
        assert!(
            state.balls_held < u32::MAX,
            "持ち玉が飽和した — 賞球の加算が止まらなくなった疑い (tick={t})"
        );
        assert!(
            state.cash <= 10_000,
            "現金が初期軍資金を超えた — 打っているだけで金が増えている \
             (tick={t}, {}円)",
            state.cash
        );
        assert!(
            state.invested <= 10_000,
            "投資額が軍資金を超えた — 借りられないはずの千円を借りている \
             (tick={t}, {}円)",
            state.invested
        );
    }
}

#[test]
fn balls_eventually_reach_the_start_pocket() {
    // 釘配置や打ち出し角度が退行して1発も入らなくなると、抽選そのものが
    // 起きなくなる。複数の来店 (seed) × ホールの全台で確かめる。
    for seed in [1u32, 12_345, 0xBEEF, 0x5A5A_5A5A] {
        for seat in 0..HALL_SIZE {
            let mut state = hall_state(seed, seat);
            state.firing = true;
            logic::tick_n(&mut state, 4_000);
            let machine = &state.machines[seat];
            assert!(
                machine.spins_seen > 0,
                "seed={seed} の {seat} 番台でヘソ入賞が4000tickの間1回も起きなかった — \
                 釘配置か打ち出し角度が退行してヘソへの道が塞がった疑い \
                 (台={}, 開き={:.2}, 傾き={:.2}, 打込={}玉)",
                machine.name,
                machine.nail_spread,
                machine.rail_bias,
                machine.balls_spent
            );
        }
    }
}

#[test]
fn jackpot_is_reachable_within_a_reasonable_budget() {
    const SEEDS: u32 = 20;
    let mut reached = 0u32;
    let mut detail = Vec::new();
    for seed in 1..=SEEDS {
        let mut state = hall_state(seed.wrapping_mul(2_654_435_761), 0);
        state.firing = true;
        let mut t = 0u32;
        // 軍資金 (1万円) を全て玉に替え終えるまで。ここまでに一度も当たらない
        // なら「1万円で当たらなかった」試行。
        while state.firing && state.cash >= BALL_LOAN_YEN && t < BUST_TICK_LIMIT {
            logic::tick(&mut state);
            t += 1;
        }
        if state.record.total_jackpots > 0 {
            reached += 1;
        }
        detail.push((state.machines[0].name, state.record.total_jackpots));
    }
    assert!(
        reached * 2 >= SEEDS,
        "1万円以内に大当たりへ届いた試行が半数未満 — 初当たりが遠すぎてテンポが悪い \
         ({reached}/{SEEDS}) 内訳={detail:?}"
    );
}

#[test]
fn nail_spread_correlates_with_spin_rate() {
    // 釘読みはこのゲームの判断軸そのもの。ヘソ釘の開きが実測回転率に現れ
    // なくなると、盤面を見て台を選ぶ意味が消える。単一 seed では玉道の
    // 揺らぎで簡単に逆転するため、十分な試行数の平均で比べる。
    const SEEDS: u32 = 24;
    const TICKS: u32 = 4_000;
    let average = |spread: f64| -> f64 {
        let rates: Vec<f64> = (1..=SEEDS)
            .map(|s| measure_raw_spin_rate(spread, 0.0, s.wrapping_mul(104_729), TICKS))
            .collect();
        mean(&rates)
    };
    let narrow = average(NAIL_SPREAD_RANGE.0);
    let wide = average(NAIL_SPREAD_RANGE.1);
    assert!(
        wide > narrow,
        "ヘソ釘を開いた台の方が回らない — 釘読みが判断軸として機能しない \
         (開き{:.2}: {wide:.2}回転/千円, 開き{:.2}: {narrow:.2}回転/千円)",
        NAIL_SPREAD_RANGE.1,
        NAIL_SPREAD_RANGE.0
    );
    assert!(
        wide > narrow * 1.3,
        "ヘソ釘を開いても回転率の差がわずかで、盤面を見て台を選ぶ意味が薄い — \
         釘の揺らぎ (NAIL_JITTER) が開きの差を埋めている疑い \
         (開き{:.2}: {wide:.2}回転/千円, 開き{:.2}: {narrow:.2}回転/千円)",
        NAIL_SPREAD_RANGE.1,
        NAIL_SPREAD_RANGE.0
    );
}

#[test]
fn higher_reach_kind_has_higher_hit_rate() {
    const TRIALS: u32 = 300_000;
    for (index, (name, _)) in MACHINE_SPECS.iter().enumerate() {
        let (seen, hits) = reach_tally(index, TRIALS);
        let rate = |slot: usize| {
            if seen[slot] == 0 {
                0.0
            } else {
                hits[slot] as f64 / seen[slot] as f64
            }
        };
        let (plain, normal, super_, premium) = (rate(0), rate(1), rate(2), rate(3));
        assert!(
            seen[1] > 100 && seen[2] > 100 && seen[3] > 100,
            "{name}: リーチの格が十分な回数出ていないので信頼度を測れない \
             (通常={} リーチ={} スーパー={} プレミア={})",
            seen[0],
            seen[1],
            seen[2],
            seen[3]
        );
        assert!(
            premium > super_,
            "{name}: プレミアがスーパーリーチより当たらない — 格と信頼度の対応が逆転している \
             (プレミア={:.1}% スーパー={:.1}%)",
            premium * 100.0,
            super_ * 100.0
        );
        assert!(
            super_ > normal,
            "{name}: スーパーリーチがノーマルリーチより当たらない — 格と信頼度の対応が逆転している \
             (スーパー={:.1}% ノーマル={:.1}%)",
            super_ * 100.0,
            normal * 100.0
        );
        assert!(
            normal > plain,
            "{name}: リーチにならない回転の方が当たる — 演出が当落を説明していない \
             (ノーマル={:.1}% 非リーチ={:.1}%)",
            normal * 100.0,
            plain * 100.0
        );
    }
}

/// 保留のランクが上がるほど当たりが近いこと。ランクは「当たる確率を変える
/// もの」ではなく「既に決まった当落を小出しにするもの」なので、この対応が
/// 崩れると赤や金を見ても何も期待できなくなり、昇格という情報イベントが
/// ただの色の変化に落ちる。
#[test]
fn a_hotter_pending_rank_hits_more_often() {
    // 虹は当たりの2%にしか現れず、その当たり自体が最も重い台で1/105。
    // 全ランクが順序を測れる本数に届くまで試行を積む。
    const TRIALS: u32 = 800_000;
    for (index, (name, _)) in MACHINE_SPECS.iter().enumerate() {
        let tally = effect_tally(index, TRIALS);
        let rate = |rank: PendingRank| {
            let slot = rank_index(rank);
            if tally.rank_seen[slot] == 0 {
                0.0
            } else {
                tally.rank_hits[slot] as f64 / tally.rank_seen[slot] as f64
            }
        };
        // 金は当たり8%対ハズレ0.025%と細いので、標本が薄いまま順序を見ると
        // 揺らぎで簡単に逆転する。まず本数を確かめる。
        for rank in PendingRank::ALL {
            let slot = rank_index(rank);
            assert!(
                tally.rank_seen[slot] > 100,
                "{name}: {} 保留が {}回しか出ておらず信頼度を測れない",
                rank.label(),
                tally.rank_seen[slot]
            );
        }
        for pair in PendingRank::ALL.windows(2) {
            assert!(
                rate(pair[1]) > rate(pair[0]),
                "{name}: {} 保留が {} 保留より当たらない — ランクと信頼度の対応が逆転している \
                 ({}={:.2}% {}={:.2}%)",
                pair[1].label(),
                pair[0].label(),
                pair[1].label(),
                rate(pair[1]) * 100.0,
                pair[0].label(),
                rate(pair[0]) * 100.0,
            );
        }
        assert_eq!(
            rate(PendingRank::Rainbow),
            1.0,
            "{name}: 虹保留がハズレでも出ている"
        );
    }
}

/// 出玉率が 1.0 を割っていること。ここを超えると打つほど玉が増え、軍資金と
/// いう制約が消えて「やめどき」という判断軸そのものが無くなる。
///
/// ホールに並びうる最良の釘 (`NAIL_SPREAD_RANGE` の上限) を、寄り釘の傾きの
/// 両端と組み合わせて確かめる。回転率は開きと傾きの両方で動くので、開きだけを
/// 最大にしても最良の台にはならない。
///
/// 判定は釘1通りごとの最大値ではなく平均で行う。`TICKS` の間に引ける大当たり
/// は荒い台で10回に満たず、1通りの実測値は台の性質より引きの強さで決まる —
/// 素の分布でも3%前後の釘が 1.0 を超えるので、最大値で判定すると乱数列が
/// ずれるだけで落ちる。平均なら標本のばらつきが `LAYOUTS` 分の1に縮み、
/// 台そのものの出玉率を見られる。
#[test]
fn payout_ratio_stays_below_break_even() {
    const LAYOUTS: u32 = 6;
    const TICKS: u32 = 120_000;
    let spread = NAIL_SPREAD_RANGE.1;
    for (index, (name, _)) in MACHINE_SPECS.iter().enumerate() {
        for bias in [RAIL_BIAS_RANGE.0, 0.0, RAIL_BIAS_RANGE.1] {
            let ratios: Vec<f64> = (1..=LAYOUTS)
                .map(|s| {
                    measure_payout_ratio(
                        index,
                        spread,
                        bias,
                        s.wrapping_mul(2_654_435_761),
                        s.wrapping_mul(40_503),
                        TICKS,
                    )
                })
                .collect();
            let average = mean(&ratios);
            let worst = sorted(ratios).last().copied().unwrap_or(0.0);
            assert!(
                average < 1.0,
                "{name}: ホールに並びうる最良の釘の長期出玉率が 1.0 を超えた — \
                 打つほど玉が増えて軍資金が尽きなくなる \
                 (開き={spread:.2} 傾き={bias:+.2} 平均={average:.3} \
                 最大={worst:.3} 釘{LAYOUTS}通り)"
            );
            // 平均だけを見ていると、引きの強さでは説明できない跳ね上がりを
            // 取りこぼす。実測の最大は 1.05 前後に収まるので、そこから離れた
            // 上限を置いて「分散ではなく仕組みが壊れた」場合だけを捕まえる。
            assert!(
                worst < 1.35,
                "{name}: 釘1通りの長期出玉率が引きのばらつきで説明できない水準に達した — \
                 平均が 1.0 を割っていても、賞球か入賞判定のどこかが壊れている疑いがある \
                 (開き={spread:.2} 傾き={bias:+.2} 平均={average:.3} \
                 最大={worst:.3} 釘{LAYOUTS}通り)"
            );
        }
    }
}

/// 釘の開きが収支へ効いていること。回るだけで収支が変わらないなら、
/// 良い台を選ぶ意味が無くなり釘読みが徒労になる。
#[test]
fn opening_the_nails_pays_off() {
    const LAYOUTS: u32 = 6;
    const TICKS: u32 = 120_000;
    let average = |spread: f64| -> f64 {
        let ratios: Vec<f64> = (1..=LAYOUTS)
            .map(|s| {
                measure_payout_ratio(
                    1,
                    spread,
                    0.0,
                    s.wrapping_mul(2_654_435_761),
                    s.wrapping_mul(40_503),
                    TICKS,
                )
            })
            .collect();
        mean(&ratios)
    };
    let narrow = average(NAIL_SPREAD_RANGE.0);
    let wide = average(NAIL_SPREAD_RANGE.1);
    assert!(
        wide > narrow * 1.25,
        "釘を開けた台と締めた台で収支がほとんど変わらない — 台を選ぶ見返りが無い \
         (開き{:.2}: 出玉率{wide:.3}, 開き{:.2}: 出玉率{narrow:.3})",
        NAIL_SPREAD_RANGE.1,
        NAIL_SPREAD_RANGE.0
    );
}

#[test]
fn money_runs_out_in_finite_time() {
    // 打ち続ければ必ず終わる、が「やめどき」という判断軸の前提。出玉率が
    // 1.0 を超えると打つほど増えて永遠に終わらない。
    for seed in [7u32, 31, 1_009, 0xC0FF_EE00] {
        for seat in 0..HALL_SIZE {
            let mut state = hall_state(seed, seat);
            let run = play_until_broke(&mut state, BUST_TICK_LIMIT);
            assert!(
                !run.censored,
                "seed={seed} seat={seat} ({}) で {BUST_TICK_LIMIT} tick 打っても軍資金が尽きない — \
                 出玉率が 1.0 に届いて打つほど増えている疑い \
                 (出玉率={:.2}, 釘の開き={:.2}, 大当={}回)",
                run.name,
                run.payout_ratio(),
                run.nail_spread,
                run.jackpots
            );
        }
    }
}

/// 打ち出した玉が必ず盤面から出ること。釘の反発 (`NAIL_RESTITUTION`) を
/// 上げると、玉は釘と釘の間で跳ね続けて落ちてこなくなる。1個でも居座ると
/// その分だけ玉数上限 (`MAX_BALLS`) の枠が埋まり続け、打ち出しが細っていく。
#[test]
fn no_ball_gets_stuck_bouncing_on_the_nails() {
    // 中央値の10倍を超える滞空は「跳ね続けている」と見なす。上限そのものは
    // `measure_flight` の `FLIGHT_TICK_LIMIT` が持つので、ここはそれより
    // 十分手前で切って、詰まりかけている段階で気付けるようにする。
    const STALL_TICKS: u32 = 1_000;
    for layout in 1..=6u32 {
        let mut nail_seed = layout.wrapping_mul(2_654_435_761);
        let machine = machine_with(0, 0.55, 0.0, &mut nail_seed);
        let mut state = seated_state(layout.wrapping_mul(40_503), machine);
        state.cash = 0;
        for ball in 0..40u32 {
            let flight = measure_flight(&mut state);
            assert!(
                flight.ticks < STALL_TICKS,
                "釘の間で跳ね続けて落ちてこない玉がある — 反発が強すぎて玉が \
                 盤面に居座り、玉数上限の枠を食い潰す \
                 (釘{layout}通り目の{ball}個目, {}tick 滞空, 釘に触れた回数={})",
                flight.ticks,
                flight.contact_ticks
            );
        }
    }
}

/// 打ち出し間隔と玉数上限の噛み合わせ。間隔を詰めすぎると盤面の玉が上限へ
/// 張り付き、`FIRE_INTERVAL_TICKS` を縮めても発射数が増えなくなる (＝定数の
/// 意味が失われる)。
#[test]
fn the_ball_cap_does_not_throttle_the_firing_rate() {
    let mut nail_seed = 0x1111_2222;
    let machine = machine_with(0, 0.55, 0.0, &mut nail_seed);
    let mut state = seated_state(0x3333_4444, machine);
    state.balls_held = 1_000_000;
    state.cash = 0;
    state.firing = true;
    const TICKS: u32 = 20_000;
    let mut peak_on_board = 0usize;
    for _ in 0..TICKS {
        logic::tick(&mut state);
        peak_on_board = peak_on_board.max(state.balls.len());
    }
    let fired = state.seated_machine().expect("着席していない").balls_spent;
    // `try_fire` は残り tick を減らす tick では撃たないので、上限に当たって
    // いなければ `FIRE_INTERVAL_TICKS + 1` tick に1発になる。
    let ideal = TICKS / (FIRE_INTERVAL_TICKS + 1);
    assert!(
        fired * 100 >= ideal * 97,
        "打ち出しが玉数上限で頭打ちになっている — 打ち出し間隔を詰めても発射数が増えない \
         (実測={fired}発 / 理想={ideal}発, 盤面の玉の最大={peak_on_board}/{MAX_BALLS})"
    );
    assert!(
        peak_on_board < MAX_BALLS,
        "盤面の玉が上限に達した — これ以上打ち出し間隔を詰めると発射が止まる \
         (最大={peak_on_board}/{MAX_BALLS})"
    );
}

