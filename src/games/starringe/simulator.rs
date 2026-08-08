//! 星環 (Star Ring) の自動プレイシミュレーター。
//!
//! 解放済み武装の強化と環強化を買い続ける bot で長期運転し、panic なし・
//! 撃破進行・層開放・星屑の長期増加を不変条件として検証する。
//!
//! 感度レポート:
//!
//! - **収率の寄与**: 収率強化の有無で獲得がどう変わるか
//! - **核脈動の寄与**: 環武装 (コア AOE) の有無で撃破がどう変わるか
//! - **武装ステの寄与**: 弾数 / 連射 / 威力の優先比較
//! - **層進行カーブ**: 撃破＋星屑開放で武装・鉱石種が解放されるペース
//! - **逸失率**: 中心到達で報酬を逃す割合
//!
//! `cargo test starringe::simulator -- --nocapture` でレポートを確認できる。

#![cfg(test)]

use super::logic::{
    can_unlock_next_layer, can_upgrade_ring, can_upgrade_weapon_stat, purchase_ring_upgrade,
    purchase_weapon_stat, ring_upgrade_cost, tick, unlock_next_layer, weapon_stat_cost,
};
use super::state::{
    Layer, OreKind, RingUpgrade, StarRingState, WeaponKind, WeaponStat, RING_UPGRADE_COUNT,
};

/// 購入方策。感度分析で「どの強化が効いているか」を切り分ける。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BuyPolicy {
    Cheapest,
    WeaponsOnly,
    WeaponsAndYield,
    YieldFirst,
    PulseFirst,
    PowerFirst,
    RateFirst,
    CountFirst,
    BlockRing(&'static [RingUpgrade]),
}

#[derive(Clone, Copy, Debug)]
enum Purchase {
    Weapon(WeaponKind, WeaponStat),
    Ring(RingUpgrade),
}

fn ring_allowed(policy: BuyPolicy, kind: RingUpgrade) -> bool {
    match policy {
        BuyPolicy::WeaponsOnly => false,
        BuyPolicy::WeaponsAndYield => matches!(kind, RingUpgrade::Yield),
        BuyPolicy::BlockRing(list) => !list.contains(&kind),
        BuyPolicy::Cheapest
        | BuyPolicy::YieldFirst
        | BuyPolicy::PulseFirst
        | BuyPolicy::PowerFirst
        | BuyPolicy::RateFirst
        | BuyPolicy::CountFirst => true,
    }
}

fn preferred_ring(policy: BuyPolicy) -> Option<RingUpgrade> {
    match policy {
        BuyPolicy::YieldFirst => Some(RingUpgrade::Yield),
        BuyPolicy::PulseFirst => Some(RingUpgrade::CorePulse),
        _ => None,
    }
}

fn preferred_stat(policy: BuyPolicy) -> Option<WeaponStat> {
    match policy {
        BuyPolicy::PowerFirst => Some(WeaponStat::Power),
        BuyPolicy::RateFirst => Some(WeaponStat::Rate),
        BuyPolicy::CountFirst => Some(WeaponStat::Count),
        _ => None,
    }
}

fn try_buy_ring(state: &mut StarRingState, kind: RingUpgrade) -> bool {
    if !can_upgrade_ring(state, kind) {
        return false;
    }
    let cost = ring_upgrade_cost(state, kind);
    if state.shards + 1e-9 < cost {
        return false;
    }
    purchase_ring_upgrade(state, kind)
}

fn bot_buy_once(state: &mut StarRingState, policy: BuyPolicy) -> bool {
    // 層開放は進行の閘門。撃破条件を満たしたら費用を貯めて開く（強化で食いつぶさない）。
    if state.kills_ready_for_next_layer() {
        if can_unlock_next_layer(state) {
            return unlock_next_layer(state);
        }
        return false;
    }
    if let Some(pref) = preferred_ring(policy) {
        if ring_allowed(policy, pref) && try_buy_ring(state, pref) {
            return true;
        }
    }
    if let Some(pref) = preferred_stat(policy) {
        let mut best: Option<(WeaponKind, f64)> = None;
        for w in state.unlocked_weapons() {
            if !can_upgrade_weapon_stat(state, w, pref) {
                continue;
            }
            let cost = weapon_stat_cost(state, w, pref);
            if state.shards + 1e-9 < cost {
                continue;
            }
            if best.map(|(_, c)| cost < c).unwrap_or(true) {
                best = Some((w, cost));
            }
        }
        if let Some((w, _)) = best {
            return purchase_weapon_stat(state, w, pref);
        }
    }

    let mut best: Option<(Purchase, f64)> = None;
    for w in state.unlocked_weapons() {
        for stat in WeaponStat::ALL {
            if !can_upgrade_weapon_stat(state, w, stat) {
                continue;
            }
            let cost = weapon_stat_cost(state, w, stat);
            if state.shards + 1e-9 < cost {
                continue;
            }
            if best.map(|(_, c)| cost < c).unwrap_or(true) {
                best = Some((Purchase::Weapon(w, stat), cost));
            }
        }
    }
    for kind in RingUpgrade::ALL {
        if !ring_allowed(policy, kind) || !can_upgrade_ring(state, kind) {
            continue;
        }
        let cost = ring_upgrade_cost(state, kind);
        if state.shards + 1e-9 < cost {
            continue;
        }
        if best.map(|(_, c)| cost < c).unwrap_or(true) {
            best = Some((Purchase::Ring(kind), cost));
        }
    }

    match best {
        Some((Purchase::Weapon(w, s), _)) => purchase_weapon_stat(state, w, s),
        Some((Purchase::Ring(r), _)) => purchase_ring_upgrade(state, r),
        None => false,
    }
}

fn bot_spend(state: &mut StarRingState, policy: BuyPolicy, max_buys: u32) {
    for _ in 0..max_buys {
        if !bot_buy_once(state, policy) {
            break;
        }
    }
}

#[derive(Clone, Debug)]
struct RunSnapshot {
    ticks: u64,
    shards: f64,
    earned: f64,
    kills: u64,
    missed: u64,
    layer: u32,
    unlocked_weapons: usize,
    unlocked_ores: usize,
    shards_per_sec: f64,
    weapon_levels: [[u32; 3]; 5],
    ring_levels: [u32; RING_UPGRADE_COUNT],
}

impl RunSnapshot {
    fn from_state(state: &StarRingState) -> Self {
        Self {
            ticks: state.elapsed_ticks,
            shards: state.shards,
            earned: state.shards_earned,
            kills: state.total_kills,
            missed: state.missed_count,
            layer: state.layer(),
            unlocked_weapons: state.unlocked_weapons().len(),
            unlocked_ores: state.unlocked_ore_kinds().len(),
            shards_per_sec: state.shards_per_sec(),
            weapon_levels: state.weapon_levels,
            ring_levels: state.ring_levels,
        }
    }

    fn miss_rate(&self) -> f64 {
        let total = self.kills + self.missed;
        if total == 0 {
            0.0
        } else {
            self.missed as f64 / total as f64
        }
    }

    fn total_weapon_levels(&self) -> u32 {
        self.weapon_levels.iter().flatten().sum()
    }

    fn total_levels(&self) -> u32 {
        self.total_weapon_levels() + self.ring_levels.iter().sum::<u32>()
    }

    fn yield_lv(&self) -> u32 {
        self.ring_levels[RingUpgrade::Yield.index()]
    }

    fn pulse_lv(&self) -> u32 {
        self.ring_levels[RingUpgrade::CorePulse.index()]
    }
}

fn run_bot(ticks: u32, policy: BuyPolicy, seed: u32) -> StarRingState {
    let mut state = StarRingState::new();
    state.rng_state = seed;
    for _ in 0..ticks {
        bot_spend(&mut state, policy, 4);
        tick(&mut state, 1);
    }
    state
}

fn run_snapshot(ticks: u32, policy: BuyPolicy, seed: u32) -> RunSnapshot {
    RunSnapshot::from_state(&run_bot(ticks, policy, seed))
}

fn report(label: &str, snap: &RunSnapshot) {
    eprintln!("=== 星環 sim: {label} ===");
    eprintln!(
        "ticks={} shards={:.1} earned={:.1} kills={} missed={} miss_rate={:.1}% layer={} sps≈{:.2}",
        snap.ticks,
        snap.shards,
        snap.earned,
        snap.kills,
        snap.missed,
        snap.miss_rate() * 100.0,
        snap.layer,
        snap.shards_per_sec
    );
    eprint!("weapons:");
    for w in WeaponKind::ALL {
        let lv = snap.weapon_levels[w.index()];
        if lv.iter().any(|&x| x > 0) || w.unlock_layer() <= snap.layer {
            eprint!(
                " {}[弾{}連{}威{}]",
                w.label(),
                lv[WeaponStat::Count.index()],
                lv[WeaponStat::Rate.index()],
                lv[WeaponStat::Power.index()],
            );
        }
    }
    eprintln!();
    eprintln!(
        "ring: 収={} 脈={}  unlocked_w={} unlocked_ore={}",
        snap.yield_lv(),
        snap.pulse_lv(),
        snap.unlocked_weapons,
        snap.unlocked_ores
    );
}

fn median_f64(values: &mut [f64]) -> f64 {
    assert!(!values.is_empty());
    values.sort_by(|a, b| a.partial_cmp(b).unwrap());
    values[values.len() / 2]
}

fn median_u64(values: &mut [u64]) -> u64 {
    assert!(!values.is_empty());
    values.sort_unstable();
    values[values.len() / 2]
}

fn median_u32(values: &mut [u32]) -> u32 {
    assert!(!values.is_empty());
    values.sort_unstable();
    values[values.len() / 2]
}

// ---------------------------------------------------------------------------
// 不変条件 / 回帰テスト
// ---------------------------------------------------------------------------

#[test]
fn long_run_never_panics_and_keeps_invariants() {
    let state = run_bot(3_000, BuyPolicy::Cheapest, 0xC0FFEE42);
    let snap = RunSnapshot::from_state(&state);
    report("3000ticks/cheapest", &snap);

    assert!(state.elapsed_ticks >= 3_000);
    assert!(
        state.total_kills >= 25,
        "長時間プレイで撃破が進むはず kills={}",
        state.total_kills
    );
    assert!(
        state.shards_earned > 30.0,
        "撃破による累計獲得が伸びるはず earned={}",
        state.shards_earned
    );
    assert!(state.shards_earned >= 0.0);
    assert!(
        state.particles.len() < 600,
        "particles={}",
        state.particles.len()
    );
    assert!(state.ores.len() < 80, "ores={}", state.ores.len());
    assert!(
        state.projectiles.len() < 250,
        "projectiles={}",
        state.projectiles.len()
    );
    assert!(state.pulse_rings.len() < 20);
    assert!(state.shards.is_finite());
}

#[test]
fn bot_purchases_upgrades_and_advances_layers() {
    let early = run_snapshot(300, BuyPolicy::Cheapest, 1);
    let late = run_snapshot(4_500, BuyPolicy::Cheapest, 1);
    report("early300", &early);
    report("late4500", &late);

    assert!(
        late.total_levels() > early.total_levels(),
        "長期ほど強化が進むはず early={} late={}",
        early.total_levels(),
        late.total_levels()
    );
    assert!(late.kills > early.kills);
    assert!(late.layer >= early.layer);
    assert!(
        late.layer >= 2,
        "4500tick で少なくとも第2層を開放しているはず layer={}",
        late.layer
    );
    assert!(
        late.unlocked_weapons >= 2,
        "層進行で武装が増えるはず n={}",
        late.unlocked_weapons
    );
}

#[test]
fn shards_earned_is_monotone_nondecreasing() {
    let mut state = StarRingState::new();
    let mut prev = state.shards_earned;
    for t in 0..2_000 {
        bot_buy_once(&mut state, BuyPolicy::Cheapest);
        tick(&mut state, 1);
        assert!(
            state.shards_earned + 1e-9 >= prev,
            "tick {t}: shards_earned が減った {prev} -> {}",
            state.shards_earned
        );
        prev = state.shards_earned;
    }
    assert!(prev > 0.0);
}

#[test]
fn layer_milestones_change_spawn_pressure() {
    assert!(Layer::spawn_batch(4) >= 3);
    assert!(Layer::hp_mult(4) > Layer::hp_mult(1) + 0.5);
    assert!(Layer::value_mult(4) > Layer::value_mult(1) + 0.5);
    assert!(Layer::THRESHOLDS[1] >= 60);
}

#[test]
fn arrival_never_reduces_shards_over_long_run() {
    let mut state = StarRingState::new();
    let mut min_shards = state.shards;
    for _ in 0..800 {
        tick(&mut state, 1);
        min_shards = min_shards.min(state.shards);
    }
    assert!(min_shards + 1e-9 >= 0.0);
    assert!(
        state.shards + 1e-9 >= 12.0,
        "購入なしなら初期星屑を下回らない shards={}",
        state.shards
    );
}

#[test]
fn weapons_only_bot_still_progresses() {
    let snap = run_snapshot(2_500, BuyPolicy::WeaponsOnly, 7);
    report("weapons_only_2500", &snap);
    assert!(
        snap.kills >= 15,
        "武装強化だけでも撃破が進むはず kills={}",
        snap.kills
    );
    assert_eq!(snap.yield_lv(), 0);
    assert_eq!(snap.pulse_lv(), 0);
}

// ---------------------------------------------------------------------------
// 感度レポート
// ---------------------------------------------------------------------------

#[test]
fn progression_balance_report() {
    const RUNS: u32 = 24;
    const TICKS: u32 = 3_500;

    let mut kills = Vec::with_capacity(RUNS as usize);
    let mut earned = Vec::with_capacity(RUNS as usize);
    let mut miss_rates = Vec::with_capacity(RUNS as usize);
    let mut layers = Vec::with_capacity(RUNS as usize);
    let mut yields = Vec::with_capacity(RUNS as usize);
    let mut pulses = Vec::with_capacity(RUNS as usize);

    for seed in 1..=RUNS {
        let snap = run_snapshot(TICKS, BuyPolicy::Cheapest, seed);
        kills.push(snap.kills);
        earned.push(snap.earned);
        miss_rates.push(snap.miss_rate());
        layers.push(snap.layer);
        yields.push(snap.yield_lv() as u64);
        pulses.push(snap.pulse_lv() as u64);
    }

    let med_kills = median_u64(&mut kills);
    let med_earned = median_f64(&mut earned);
    let med_miss = median_f64(&mut miss_rates);
    let med_layer = median_u32(&mut layers);
    let med_yield = median_u64(&mut yields);
    let med_pulse = median_u64(&mut pulses);

    eprintln!(
        "[starringe/progression] runs={RUNS} ticks={TICKS} median_kills={med_kills} \
         median_earned={med_earned:.1} median_miss_rate={:.1}% median_layer={med_layer} \
         median_yield_lv={med_yield} median_pulse_lv={med_pulse}",
        med_miss * 100.0
    );

    assert!(med_kills >= 20, "中央撃破が低すぎる: {med_kills}");
    assert!(med_layer >= 2, "中央到達層が浅い: {med_layer}");
    assert!(med_layer <= 8, "中央到達層が深すぎる: {med_layer}");
}

/// 最安買い (収率を含む) vs 収率なし。
#[test]
fn yield_ablation_report() {
    const RUNS: u32 = 18;
    const TICKS: u32 = 4_000;

    let no_yield = BuyPolicy::BlockRing(&[RingUpgrade::Yield]);

    let mut earned_with = Vec::new();
    let mut earned_no = Vec::new();
    let mut kills_with = Vec::new();
    let mut kills_no = Vec::new();
    let mut yield_lv = Vec::new();

    for seed in 1..=RUNS {
        let with = run_snapshot(TICKS, BuyPolicy::Cheapest, seed);
        let without = run_snapshot(TICKS, no_yield, seed);
        earned_with.push(with.earned);
        earned_no.push(without.earned);
        kills_with.push(with.kills);
        kills_no.push(without.kills);
        yield_lv.push(with.yield_lv() as u64);
    }

    let ew = median_f64(&mut earned_with);
    let en = median_f64(&mut earned_no);
    let kw = median_u64(&mut kills_with);
    let kn = median_u64(&mut kills_no);
    let yl = median_u64(&mut yield_lv);
    let earned_delta = if en == 0.0 {
        0.0
    } else {
        (ew - en) / en * 100.0
    };

    eprintln!(
        "[starringe/yield-ablation] ticks={TICKS} runs={RUNS} median_yield_lv={yl}
\
         cheapest:  median_kills={kw} median_earned={ew:.1}
\
         no-yield:  median_kills={kn} median_earned={en:.1}
\
         delta earned={earned_delta:+.1}%"
    );

    assert!(yl >= 1, "最安買い bot が収率を積めていない");
    assert!(
        earned_delta > 5.0,
        "収率込みの獲得が伸びていない: delta={earned_delta:.1}%"
    );
}

/// 最安買い (核脈動を含む) vs 核脈動なし。
#[test]
fn core_pulse_ablation_report() {
    const RUNS: u32 = 14;
    const TICKS: u32 = 5_000;

    let no_pulse = BuyPolicy::BlockRing(&[RingUpgrade::CorePulse]);

    let mut kills_with = Vec::new();
    let mut kills_without = Vec::new();
    let mut earned_with = Vec::new();
    let mut earned_without = Vec::new();
    let mut pulse_lv = Vec::new();

    for seed in 1..=RUNS {
        let w = run_snapshot(TICKS, BuyPolicy::Cheapest, seed);
        let o = run_snapshot(TICKS, no_pulse, seed);
        kills_with.push(w.kills);
        kills_without.push(o.kills);
        earned_with.push(w.earned);
        earned_without.push(o.earned);
        pulse_lv.push(w.pulse_lv() as u64);
    }

    let kw = median_u64(&mut kills_with);
    let ko = median_u64(&mut kills_without);
    let ew = median_f64(&mut earned_with);
    let eo = median_f64(&mut earned_without);
    let pl = median_u64(&mut pulse_lv);
    let kill_delta = if ko == 0 {
        0.0
    } else {
        (kw as f64 - ko as f64) / ko as f64 * 100.0
    };

    eprintln!(
        "[starringe/pulse-ablation] ticks={TICKS} runs={RUNS} median_pulse_lv={pl}\n\
         cheapest:  median_kills={kw} median_earned={ew:.1}\n\
         no-pulse:  median_kills={ko} median_earned={eo:.1}\n\
         delta kills={kill_delta:+.1}%"
    );

    assert!(pl >= 1, "最安買い bot が核脈動を積めていない");
    assert!(
        kill_delta > -15.0,
        "核脈動込みが壊滅的に弱い: delta={kill_delta:.1}%"
    );
}

#[test]
fn weapon_stat_priority_report() {
    const RUNS: u32 = 14;
    const TICKS: u32 = 3_500;

    let policies = [
        ("count-first", BuyPolicy::CountFirst),
        ("rate-first", BuyPolicy::RateFirst),
        ("power-first", BuyPolicy::PowerFirst),
        ("weapons-only", BuyPolicy::WeaponsOnly),
    ];

    eprintln!("[starringe/weapon-stats] ticks={TICKS} runs={RUNS}");
    for (name, policy) in policies {
        let mut kills = Vec::new();
        let mut earned = Vec::new();
        let mut miss = Vec::new();
        for seed in 1..=RUNS {
            let snap = run_snapshot(TICKS, policy, seed);
            kills.push(snap.kills);
            earned.push(snap.earned);
            miss.push(snap.miss_rate());
        }
        eprintln!(
            "  {name:13} median_kills={} median_earned={:.1} median_miss={:.1}%",
            median_u64(&mut kills),
            median_f64(&mut earned),
            median_f64(&mut miss) * 100.0
        );
    }
}

#[test]
fn timeline_progression_report() {
    let checkpoints = [250u32, 500, 1_000, 2_000, 4_000, 8_000];
    let mut state = StarRingState::new();
    state.rng_state = 99;
    let mut next_i = 0;
    let mut t = 0u32;

    eprintln!("[starringe/timeline] policy=cheapest");
    while next_i < checkpoints.len() {
        bot_spend(&mut state, BuyPolicy::Cheapest, 4);
        tick(&mut state, 1);
        t += 1;
        if t == checkpoints[next_i] {
            let snap = RunSnapshot::from_state(&state);
            eprintln!(
                "  t={:>5} layer={:>2} kills={:>5} earned={:>8.1} miss={:>5.1}% \
                 w={} ores={} yield={} pulse={}",
                snap.ticks,
                snap.layer,
                snap.kills,
                snap.earned,
                snap.miss_rate() * 100.0,
                snap.unlocked_weapons,
                snap.unlocked_ores,
                snap.yield_lv(),
                snap.pulse_lv(),
            );
            next_i += 1;
        }
    }

    assert!(
        state.layer() >= OreKind::Rock.unlock_layer(),
        "8000tick で岩石層には届くはず layer={}",
        state.layer()
    );
    assert!(
        state.unlocked_weapons().len() >= 2,
        "8000tick で武装が2種以上解放されるはず n={}",
        state.unlocked_weapons().len()
    );
}

#[test]
fn strategy_comparison_report() {
    const TICKS: u32 = 4_500;
    const SEED: u32 = 42;

    let policies = [
        ("cheapest", BuyPolicy::Cheapest),
        ("weapons", BuyPolicy::WeaponsOnly),
        ("w+yield", BuyPolicy::WeaponsAndYield),
        ("yield-first", BuyPolicy::YieldFirst),
        ("pulse-first", BuyPolicy::PulseFirst),
        ("power-first", BuyPolicy::PowerFirst),
        ("rate-first", BuyPolicy::RateFirst),
        ("count-first", BuyPolicy::CountFirst),
        (
            "no-pulse",
            BuyPolicy::BlockRing(&[RingUpgrade::CorePulse]),
        ),
        ("no-yield", BuyPolicy::BlockRing(&[RingUpgrade::Yield])),
    ];

    eprintln!("[starringe/strategies] ticks={TICKS} seed={SEED}");
    for (name, policy) in policies {
        let snap = run_snapshot(TICKS, policy, SEED);
        eprintln!(
            "  {name:12} layer={:>2} kills={:>5} earned={:>8.1} miss={:>5.1}% \
             yield={} pulse={} wlv={} w={} ores={}",
            snap.layer,
            snap.kills,
            snap.earned,
            snap.miss_rate() * 100.0,
            snap.yield_lv(),
            snap.pulse_lv(),
            snap.total_weapon_levels(),
            snap.unlocked_weapons,
            snap.unlocked_ores,
        );
    }
}

#[test]
fn miss_rate_stays_bounded_under_cheapest_bot() {
    const RUNS: u32 = 16;
    const TICKS: u32 = 4_000;
    let mut rates = Vec::new();
    for seed in 1..=RUNS {
        let snap = run_snapshot(TICKS, BuyPolicy::Cheapest, seed);
        rates.push(snap.miss_rate());
    }
    let med = median_f64(&mut rates);
    eprintln!(
        "[starringe/miss-rate] ticks={TICKS} runs={RUNS} median_miss_rate={:.1}%",
        med * 100.0
    );
    assert!(
        med < 0.85,
        "逸失率が高すぎて刈り取りが成立していない: {:.1}%",
        med * 100.0
    );
}

#[test]
fn new_ore_kinds_appear_over_long_run() {
    let snap = run_snapshot(12_000, BuyPolicy::Cheapest, 17);
    report("ore_variety_12000", &snap);
    assert!(
        snap.unlocked_ores >= 4,
        "長時間で鉱石種が増えるはず unlocked={}",
        snap.unlocked_ores
    );
    // 浮遊片は第3層
    assert!(
        snap.layer >= OreKind::Wisp.unlock_layer(),
        "浮遊片層に届くはず layer={}",
        snap.layer
    );
}
