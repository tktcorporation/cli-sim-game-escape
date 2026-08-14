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
//! - **逸失率**: コア到達で報酬を逃す割合
//! - **迎撃圧の時間推移**: 逸失率が序盤から終盤にかけてどう下がるか
//!
//! `cargo test starringe::simulator -- --nocapture` でレポートを確認できる。

#![cfg(test)]

use super::logic::{
    can_unlock_next_layer, can_upgrade_ring, can_upgrade_weapon_stat, manual_strike,
    purchase_ring_upgrade, purchase_weapon_stat, ring_upgrade_cost, tick, unlock_next_layer,
    weapon_stat_cost, MAX_ORES, MAX_PULSE_RINGS,
};
use super::state::{
    Layer, OreKind, RingUpgrade, StarRingState, WeaponKind, WeaponStat, FIELD_MARGIN,
    RING_UPGRADE_COUNT, SPAWN_X_MARGIN, SPAWN_Y, VISIBLE_X_HI, VISIBLE_X_LO, VISIBLE_Y_HI,
    VISIBLE_Y_LO, WORLD_W,
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

/// 湧きが刈り取りを上回る飽和状態を作り、核脈動のレベルだけを変えて撃破数を測る。
///
/// 武装を Lv1 に固定した第5層は湧き (`Layer::spawn_batch`) が砲台の火力を上回るので、
/// 撃破数が湧き量へ張り付かない。核脈動が上空の降下レーンをどこまで舐められて
/// いるかが、そのまま撃破数の差として出る。
fn saturated_kills(pulse_lv: u32, seed: u32) -> u64 {
    const TICKS: u32 = 3_000;
    let mut state = StarRingState::new();
    state.rng_state = seed;
    state.current_layer = 5;
    state.ring_levels[RingUpgrade::CorePulse.index()] = pulse_lv;
    for w in state.unlocked_weapons() {
        state.weapon_levels[w.index()] = [1, 1, 1];
    }
    for _ in 0..TICKS {
        tick(&mut state, 1);
    }
    state.total_kills
}

/// 最安買い (核脈動を含む) vs 核脈動なし + 飽和状態での核脈動の寄与。
///
/// 通常進行の撃破数は湧き量に張り付く (`interception_pressure_over_time_report` の
/// とおり t≈1200 以降の逸失はほぼ 0) ため、削る力を上げても最安買いの撃破数は
/// ほとんど動かない。核脈動が実際に鉱石を砕けているかは、湧きが刈り取りを上回る
/// 飽和状態を別に作らないと測れない。
#[test]
fn core_pulse_ablation_report() {
    const RUNS: u32 = 14;
    const TICKS: u32 = 5_000;
    const SAT_RUNS: u32 = 10;
    /// 飽和側で比較する核脈動レベル。最安買い bot が 5000tick で積む水準に合わせる。
    const SAT_PULSE_LV: u32 = 6;

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

    let mut sat_with = Vec::with_capacity(SAT_RUNS as usize);
    let mut sat_without = Vec::with_capacity(SAT_RUNS as usize);
    for seed in 1..=SAT_RUNS {
        sat_with.push(saturated_kills(SAT_PULSE_LV, seed));
        sat_without.push(saturated_kills(0, seed));
    }
    let sw = median_u64(&mut sat_with);
    let so = median_u64(&mut sat_without);
    let sat_delta = (sw as f64 - so as f64) / so.max(1) as f64 * 100.0;

    eprintln!(
        "[starringe/pulse-ablation] ticks={TICKS} runs={RUNS} median_pulse_lv={pl}\n\
         cheapest:  median_kills={kw} median_earned={ew:.1}\n\
         no-pulse:  median_kills={ko} median_earned={eo:.1}\n\
         delta kills={kill_delta:+.1}%\n\
         saturated (第5層/武装Lv1固定, runs={SAT_RUNS}): 脈Lv{SAT_PULSE_LV} median_kills={sw} \
         vs 脈なし {so}  delta kills={sat_delta:+.1}%"
    );

    assert!(pl >= 1, "最安買い bot が核脈動を積めていない");
    assert!(
        kill_delta > -15.0,
        "核脈動込みが壊滅的に弱い: delta={kill_delta:.1}%"
    );
    // 飽和状態での寄与は実測 +250% 前後。波が届く高さを縮めると、上空で降下する
    // 鉱石を舐められなくなってここが落ちる — 核脈動が環強化として仕事をして
    // いることの下限として置く。
    assert!(
        sat_delta > 150.0,
        "核脈動が飽和状態でも鉱石を砕けていない: delta={sat_delta:.1}%"
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

/// 連打で波を積み上げても、同時に持つ波の本数が上限のまわりに収まること。
///
/// タップは入力イベントごとに波を立てる (`logic::manual_strike`) ので、
/// 10 ticks/sec の歩みに縛られずに積み上がる。`MAX_PULSE_RINGS` は描画コストの
/// 上限で、切り詰めた波を描かせるために 1tick 残す猶予 (`logic::step_pulse_rings`)
/// はその上限を一時的に越える——越え幅が青天井だと、切り詰めそのものが意味を
/// 失う。脈動レベルが上がるほど波の寿命は伸びるので、伸びきった側でも測る。
#[test]
fn rapid_tapping_keeps_the_wave_count_bounded() {
    const TICKS: u32 = 900;
    const TAP_RATES: [usize; 3] = [1, 4, 12];

    eprintln!("[starringe/waves] ticks={TICKS} 上限={MAX_PULSE_RINGS}");
    for pulse_lv in [1u32, 6, 12, 20] {
        for taps in TAP_RATES {
            let mut state = StarRingState::new();
            state.rng_state = 0x51DE_0001 + pulse_lv;
            state.current_layer = 4;
            state.ring_levels[RingUpgrade::CorePulse.index()] = pulse_lv;
            let mut peak = 0usize;
            for _ in 0..TICKS {
                for _ in 0..taps {
                    manual_strike(&mut state);
                }
                tick(&mut state, 1);
                peak = peak.max(state.pulse_rings.len());
            }
            eprintln!("  脈Lv{pulse_lv:>2} {taps:>2}連打/tick peak_rings={peak}");
            // 猶予は 1tick なので、上乗せはその tick に立てた本数までで頭打ちに
            // なる。猶予が複数 tick へ伸びると、波の寿命ぶん積み上がってこの幅を
            // 越える。
            assert!(
                peak <= MAX_PULSE_RINGS + taps + 2,
                "脈Lv{pulse_lv} {taps}連打/tick で波の本数が上限から離れすぎている \
                 peak={peak} / 上限{MAX_PULSE_RINGS}"
            );
        }
    }
}

/// 裂片が湧く層で、分裂が盤面を溢れさせず迎撃圧も殺さないこと。
///
/// 裂片は撃破のたびに星塵を残り枠のぶんだけ (最大 2 体) 足すので、湧きの総量は
/// 他の層より上振れする。子は通常の星塵と同じ寸法・HP で湧く
/// (`logic::apply_damage`) ため、分裂の重さは裂片を割った回数と盤面の空きで
/// 決まる——ここが崩れると、盤面が上限へ張り付く側か、割っても何も増えない側の
/// どちらかへ倒れる。
///
/// 武装を Lv1 に固定するのは `saturated_kills` と同じ理由で、強化が積み上がって
/// 逸失が 0 に落ちた状態では分裂の重さが撃破数へ出ないため。
#[test]
fn splitting_layer_keeps_the_board_playable() {
    const RUNS: u32 = 12;
    const TICKS: u32 = 3_000;

    let mut miss_rates = Vec::with_capacity(RUNS as usize);
    let mut kills = Vec::with_capacity(RUNS as usize);
    let mut peaks = Vec::with_capacity(RUNS as usize);

    for seed in 1..=RUNS {
        let mut state = StarRingState::new();
        state.rng_state = seed;
        state.current_layer = OreKind::Splitter.unlock_layer();
        for w in state.unlocked_weapons() {
            state.weapon_levels[w.index()] = [1, 1, 1];
        }
        let mut peak = 0usize;
        for _ in 0..TICKS {
            tick(&mut state, 1);
            peak = peak.max(state.ores.len());
        }
        let total = state.total_kills + state.missed_count;
        miss_rates.push(state.missed_count as f64 / total.max(1) as f64);
        kills.push(state.total_kills);
        peaks.push(peak as u64);
    }

    let miss = median_f64(&mut miss_rates);
    let med_kills = median_u64(&mut kills);
    let peak = median_u64(&mut peaks);
    eprintln!(
        "[starringe/splitting] runs={RUNS} ticks={TICKS} layer={} \
         median_kills={med_kills} median_miss_rate={:.1}% median_peak_ores={peak}",
        OreKind::Splitter.unlock_layer(),
        miss * 100.0
    );

    assert!(
        peak <= MAX_ORES as u64,
        "分裂で同時存在数が上限を超えた peak={peak} / 上限{MAX_ORES}"
    );
    // 逸失率の中央値は 62% 前後。武装 Lv1 固定の飽和状態なので取りこぼしは多く
    // 出るが、両端へ振れれば「割っても増えない」か「割ったら手に負えない」の
    // どちらかへ倒れている。
    assert!(
        (0.45..0.78).contains(&miss),
        "裂片層の逸失率が想定帯から外れている: {:.1}%",
        miss * 100.0
    );
    assert!(
        med_kills > 0,
        "裂片層で一体も撃破できていない kills={med_kills}"
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

// ---------------------------------------------------------------------------
// フィールドモデルの不変条件
// ---------------------------------------------------------------------------

/// 鉱石はワールドの内側に留まる。
///
/// 横は左右の反射壁 (`FIELD_MARGIN`)、縦はコア到達 / 場外落下の判定
/// (`logic::resolve_arrivals`) で回収されるので、tick の切れ目では常に
/// Canvas の内側かつ壁の内側にいる。画面外へ流れる鉱石があると
/// 「どこから何が降ってきているか」を目で追えなくなる。
///
/// 見るのは中心ではなく円の全体。中心が内側にあっても半径ぶんが Canvas の
/// bounds (`0..WORLD_W` × `0..WORLD_H`) を越えていれば、その鉱石は端で欠けて
/// 描かれる。画面シェイクで振れた tick も欠けないよう、境界は縦横それぞれの
/// 振れ幅を見込んだ `VISIBLE_X_LO`/`VISIBLE_X_HI`・`VISIBLE_Y_LO`/`VISIBLE_Y_HI`
/// に取る。
#[test]
fn ores_stay_inside_the_field_over_a_long_run() {
    const TICKS: u32 = 6_000;
    const EPS: f64 = 1e-6;
    let mut state = StarRingState::new();
    state.rng_state = 0x5EED_1234;
    let mut checked = 0u64;
    for t in 0..TICKS {
        bot_spend(&mut state, BuyPolicy::Cheapest, 4);
        tick(&mut state, 1);
        for ore in &state.ores {
            assert!(
                ore.x - ore.radius() >= FIELD_MARGIN - EPS
                    && ore.x + ore.radius() <= WORLD_W - FIELD_MARGIN + EPS,
                "tick {t}: 鉱石が左右の壁を越えた x={} r={} kind={:?}",
                ore.x,
                ore.radius(),
                ore.kind
            );
            assert!(
                ore.x - ore.radius() >= VISIBLE_X_LO - EPS
                    && ore.x + ore.radius() <= VISIBLE_X_HI + EPS,
                "tick {t}: 鉱石が描画範囲の横幅からはみ出した x={} r={} kind={:?}",
                ore.x,
                ore.radius(),
                ore.kind
            );
            assert!(
                ore.y - ore.radius() >= VISIBLE_Y_LO - EPS
                    && ore.y + ore.radius() <= VISIBLE_Y_HI + EPS,
                "tick {t}: 鉱石が描画範囲の上下からはみ出した y={} r={} kind={:?}",
                ore.y,
                ore.radius(),
                ore.kind
            );
            assert!(ore.x.is_finite() && ore.y.is_finite());
            checked += 1;
        }
    }
    assert!(
        checked > 10_000,
        "検査対象が少なすぎてフィールド外判定が効いていない n={checked}"
    );
}

/// 出現 x は横幅全体へ散る。
///
/// 湧きが一箇所へ寄ると、迎撃が「その一点を撃つだけ」に退化する。
/// 上空から降り始めた鉱石だけを数え、幅を5区画に割って偏りを見る
/// (裂片の分裂で生まれる子は親の位置に依存するので対象外)。
#[test]
fn spawn_x_spreads_across_the_whole_width() {
    const TICKS: u32 = 6_000;
    const BUCKETS: usize = 5;
    let mut hist = [0u64; BUCKETS];
    let lo = FIELD_MARGIN;
    let span = WORLD_W - FIELD_MARGIN * 2.0;
    for seed in 1..=4u32 {
        let mut state = StarRingState::new();
        state.rng_state = seed;
        for _ in 0..TICKS {
            bot_spend(&mut state, BuyPolicy::Cheapest, 4);
            tick(&mut state, 1);
            for ore in state.ores.iter().filter(|o| o.age == 0 && o.y > SPAWN_Y - 8.0) {
                let i = (((ore.x - lo) / span * BUCKETS as f64) as usize).min(BUCKETS - 1);
                hist[i] += 1;
            }
        }
    }
    let total: u64 = hist.iter().sum();
    eprintln!("[starringe/spawn-x] total={total} buckets={hist:?}");
    assert!(total > 2_000, "湧きの標本が足りない total={total}");

    // 出現 x は鉱石ごとに [SPAWN_X_MARGIN + 半径, WORLD_W - SPAWN_X_MARGIN - 半径]
    // の一様分布なので、端の区画は大きい鉱石ほど狭くなる。最も大きい鉱石が端の
    // 区画へ湧く割合を下限の基準に取る。
    let r_max = OreKind::ALL
        .iter()
        .map(|k| k.radius())
        .fold(0.0f64, f64::max);
    let edge_margin = SPAWN_X_MARGIN + r_max;
    let edge_share =
        (span / BUCKETS as f64 - (edge_margin - FIELD_MARGIN)) / (WORLD_W - edge_margin * 2.0);
    let floor = edge_share * 0.55;
    for (i, &n) in hist.iter().enumerate() {
        let share = n as f64 / total as f64;
        assert!(
            share > floor,
            "区画 {i} への湧きが少なすぎる share={:.1}% floor={:.1}%",
            share * 100.0,
            floor * 100.0
        );
        assert!(
            share < 0.35,
            "区画 {i} へ湧きが偏っている share={:.1}%",
            share * 100.0
        );
    }
}

/// 迎撃圧の時間推移。
///
/// 逸失率は序盤に高く、強化が積み上がるほど下がる——「守る」ではなく
/// 「刈り取る」ゲームなので、投資が実った終盤に取りこぼしが消えるのは設計どおり。
/// 検証したいのは 3 点: 序盤に迎撃の駆け引きが成立していること (下限)、序盤でも
/// 刈り取りが立ち上がること (上限)、そして終盤には取りこぼしが消えていること。
#[test]
fn interception_pressure_over_time_report() {
    const RUNS: u32 = 32;
    const EDGES: [u32; 6] = [0, 500, 1_000, 2_000, 4_000, 8_000];

    let mut window = vec![(0u64, 0u64); EDGES.len() - 1];
    let mut opening_rates = Vec::with_capacity(RUNS as usize);
    let mut peak_ores = Vec::with_capacity(RUNS as usize);

    for seed in 1..=RUNS {
        let mut state = StarRingState::new();
        state.rng_state = seed;
        let mut prev = (0u64, 0u64);
        let mut wi = 0usize;
        let mut peak = 0usize;
        for t in 1..=EDGES[EDGES.len() - 1] {
            bot_spend(&mut state, BuyPolicy::Cheapest, 4);
            tick(&mut state, 1);
            peak = peak.max(state.ores.len());
            if t == EDGES[wi + 1] {
                window[wi].0 += state.total_kills - prev.0;
                window[wi].1 += state.missed_count - prev.1;
                prev = (state.total_kills, state.missed_count);
                if EDGES[wi + 1] == 1_000 {
                    let total = state.total_kills + state.missed_count;
                    opening_rates.push(state.missed_count as f64 / total.max(1) as f64);
                }
                wi += 1;
            }
        }
        peak_ores.push(peak as u64);
    }

    eprintln!("[starringe/pressure] runs={RUNS} policy=cheapest");
    for (wi, &(k, m)) in window.iter().enumerate() {
        eprintln!(
            "  t={:>5}-{:<5} kills={:>6} missed={:>5} miss={:>5.1}%",
            EDGES[wi],
            EDGES[wi + 1],
            k / RUNS as u64,
            m / RUNS as u64,
            m as f64 / (k + m).max(1) as f64 * 100.0
        );
    }
    let opening = median_f64(&mut opening_rates);
    let peak = median_u64(&mut peak_ores);
    eprintln!(
        "  opening(t<1000) median_miss_rate={:.1}% median_peak_ores={peak}",
        opening * 100.0
    );

    // 序盤の逸失率の中央値は 12.5% 前後。上下 2 倍弱の幅に収め、迎撃圧が体感で
    // 消える側 (数%) へ緩んでも、逆に序盤が刈り取れない側へ振れても検知する。
    // シードは 1..=RUNS 固定なので、閾値に触れるのはバランスを動かした時だけ。
    assert!(
        opening > 0.06,
        "序盤の取りこぼしが減りすぎて迎撃の駆け引きが薄い: {:.1}%",
        opening * 100.0
    );
    assert!(
        opening < 0.19,
        "序盤の取りこぼしが多すぎて刈り取りが立ち上がらない: {:.1}%",
        opening * 100.0
    );
    // 終盤 (最後の窓) の逸失は実測 0%。刈り取りが実る終盤に取りこぼしが消えるのが
    // このゲームの狙いなので、そこが崩れて「守る」ゲームへ寄り始めたら検知する。
    let (late_k, late_m) = window[window.len() - 1];
    let late_rate = late_m as f64 / (late_k + late_m).max(1) as f64;
    assert!(
        late_rate < 0.02,
        "終盤に取りこぼしが残っている: {:.1}%",
        late_rate * 100.0
    );
    // 同時存在数の中央値は 20 前後。上限 (`MAX_ORES`) へ張り付くのは湧きが
    // 刈り取りに勝っている状態なので、上限に届く手前で検知する。
    assert!(
        peak < (MAX_ORES * 3 / 4) as u64,
        "同時存在数が上限に迫っている peak={peak} / 上限{MAX_ORES}"
    );
}
