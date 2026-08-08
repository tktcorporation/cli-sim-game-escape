//! 星環 (Star Ring) の自動プレイシミュレーター。
//!
//! 買える強化を買う bot で長期運転し、panic なし・撃破進行・層進行・
//! 星屑の長期増加を不変条件として検証する。あわせて改修後バランスの
//! 感度レポートを提供する:
//!
//! - **層進行**: 深い層へ到達し、新鉱石・新武装が開くか
//! - **脈動 / 穿光**: 解放後に実際に積まれ、撃破効率に寄与するか
//! - **脅威は層が担う**: プレイヤー強化を積んでも出現圧は直接増えない
//!
//! バランス調整時は
//! `cargo test starringe::simulator -- --nocapture`
//! でレポートを見ながら数値を触るとよい。

#![cfg(test)]

use super::logic::{can_upgrade_further, purchase_upgrade, tick, upgrade_cost};
use super::state::{OreKind, StarRingState, UpgradeKind};

/// 購入方策。感度分析で「どの強化が効いているか」を切り分けるために使う。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BuyPolicy {
    /// 買える中で最安を買う (同率なら ALL 順)。
    Cheapest,
    /// 戦闘系のみ (砲台・火力・連射)。武装/収率は買わない。
    CombatOnly,
    /// 戦闘 + 収率。
    CombatAndYield,
    /// 脈動を優先。
    PulseFirst,
    /// 穿光を優先。
    LanceFirst,
    /// 火力を優先。
    DamageFirst,
    /// 指定種別だけ買わない。
    Blocklist(&'static [UpgradeKind]),
}

fn is_allowed(policy: BuyPolicy, kind: UpgradeKind) -> bool {
    match policy {
        BuyPolicy::Cheapest
        | BuyPolicy::PulseFirst
        | BuyPolicy::LanceFirst
        | BuyPolicy::DamageFirst => true,
        BuyPolicy::CombatOnly => matches!(
            kind,
            UpgradeKind::Turrets | UpgradeKind::Damage | UpgradeKind::FireRate
        ),
        BuyPolicy::CombatAndYield => matches!(
            kind,
            UpgradeKind::Turrets
                | UpgradeKind::Damage
                | UpgradeKind::FireRate
                | UpgradeKind::Yield
        ),
        BuyPolicy::Blocklist(list) => !list.contains(&kind),
    }
}

fn preferred_kind(policy: BuyPolicy) -> Option<UpgradeKind> {
    match policy {
        BuyPolicy::PulseFirst => Some(UpgradeKind::Pulse),
        BuyPolicy::LanceFirst => Some(UpgradeKind::Lance),
        BuyPolicy::DamageFirst => Some(UpgradeKind::Damage),
        _ => None,
    }
}

fn bot_buy_once(state: &mut StarRingState, policy: BuyPolicy) -> bool {
    if let Some(pref) = preferred_kind(policy) {
        if try_buy(state, pref) {
            return true;
        }
    }

    let mut best: Option<(UpgradeKind, f64)> = None;
    for kind in UpgradeKind::ALL {
        if !is_allowed(policy, kind) {
            continue;
        }
        if !can_upgrade_further(state, kind) {
            continue;
        }
        let cost = upgrade_cost(state, kind);
        if state.shards + 1e-9 < cost {
            continue;
        }
        if best.map(|(_, c)| cost < c).unwrap_or(true) {
            best = Some((kind, cost));
        }
    }
    if let Some((kind, _)) = best {
        purchase_upgrade(state, kind)
    } else {
        false
    }
}

fn try_buy(state: &mut StarRingState, kind: UpgradeKind) -> bool {
    if !can_upgrade_further(state, kind) {
        return false;
    }
    let cost = upgrade_cost(state, kind);
    if state.shards + 1e-9 < cost {
        return false;
    }
    purchase_upgrade(state, kind)
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
    leaked: f64,
    kills: u64,
    leaks: u64,
    depth: u32,
    best_depth: u32,
    levels: [u32; 6],
    unlocked_ores: usize,
    shards_per_sec: f64,
}

impl RunSnapshot {
    fn from_state(state: &StarRingState) -> Self {
        Self {
            ticks: state.elapsed_ticks,
            shards: state.shards,
            earned: state.shards_earned,
            leaked: state.shards_leaked,
            kills: state.total_kills,
            leaks: state.leak_count,
            depth: state.depth,
            best_depth: state.best_depth,
            levels: state.upgrade_levels,
            unlocked_ores: state.unlocked_ore_kinds().len(),
            shards_per_sec: state.shards_per_sec(),
        }
    }

    fn leak_rate(&self) -> f64 {
        if self.kills + self.leaks == 0 {
            0.0
        } else {
            self.leaks as f64 / (self.kills + self.leaks) as f64
        }
    }

    fn net_earned(&self) -> f64 {
        self.earned - self.leaked
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
        "ticks={} depth={} shards={:.1} earned={:.1} leaked={:.1} net={:.1} kills={} leaks={} leak_rate={:.1}% sps≈{:.2}",
        snap.ticks,
        snap.depth,
        snap.shards,
        snap.earned,
        snap.leaked,
        snap.net_earned(),
        snap.kills,
        snap.leaks,
        snap.leak_rate() * 100.0,
        snap.shards_per_sec
    );
    eprintln!(
        "upgrades: 砲={} 火={} 連={} 脈={} 穿={} 収={}  unlocked_ores={}",
        1 + snap.levels[UpgradeKind::Turrets.index()],
        snap.levels[UpgradeKind::Damage.index()],
        snap.levels[UpgradeKind::FireRate.index()],
        snap.levels[UpgradeKind::Pulse.index()],
        snap.levels[UpgradeKind::Lance.index()],
        snap.levels[UpgradeKind::Yield.index()],
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
        state.total_kills >= 20,
        "長時間プレイで撃破が進むはず kills={}",
        state.total_kills
    );
    assert!(
        state.shards_earned > 30.0,
        "撃破による累計獲得が伸びるはず earned={}",
        state.shards_earned
    );
    assert!(state.depth >= 1);
    assert!(
        state.particles.len() < 600,
        "particles={}",
        state.particles.len()
    );
    assert!(state.ores.len() < 80, "ores={}", state.ores.len());
    assert!(state.beams.len() < 80, "beams={}", state.beams.len());
    assert!(state.pulse_rings.len() < 20);
    assert!(state.shards.is_finite());
    assert!(state.shards_earned.is_finite());
    assert!(state.shards_leaked.is_finite());
}

#[test]
fn bot_purchases_upgrades_over_time() {
    let early = run_snapshot(200, BuyPolicy::Cheapest, 1);
    let late = run_snapshot(2_500, BuyPolicy::Cheapest, 1);
    report("early200", &early);
    report("late2500", &late);

    let early_levels: u32 = early.levels.iter().sum();
    let late_levels: u32 = late.levels.iter().sum();
    assert!(
        late_levels > early_levels,
        "長期ほど強化が進むはず early={early_levels} late={late_levels}"
    );
    assert!(late.kills > early.kills);
    assert!(late.earned > early.earned);
}

#[test]
fn shards_earned_is_monotone_nondecreasing() {
    let mut state = StarRingState::new();
    let mut prev = state.shards_earned;
    for t in 0..1_500 {
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
fn depth_advances_over_long_run() {
    let snap = run_snapshot(6_000, BuyPolicy::Cheapest, 11);
    report("depth_6000", &snap);
    assert!(
        snap.depth >= 3,
        "6000tick で層3には届くはず depth={}",
        snap.depth
    );
    assert!(
        snap.best_depth >= snap.depth,
        "best_depth は depth 以上"
    );
    assert!(
        snap.unlocked_ores >= 3,
        "層進行で鉱石種が増えるはず unlocked={}",
        snap.unlocked_ores
    );
}

#[test]
fn pulse_and_lance_are_purchased_after_unlock() {
    let snap = run_snapshot(8_000, BuyPolicy::Cheapest, 13);
    report("weapons_8000", &snap);
    assert!(
        snap.depth >= UpgradeKind::Pulse.unlock_depth(),
        "脈動解放層に到達するはず"
    );
    assert!(
        snap.levels[UpgradeKind::Pulse.index()] >= 1,
        "解放後に脈動を買うはず lv={}",
        snap.levels[UpgradeKind::Pulse.index()]
    );
    if snap.depth >= UpgradeKind::Lance.unlock_depth() {
        assert!(
            snap.levels[UpgradeKind::Lance.index()] >= 1,
            "穿光解放後に買うはず"
        );
    }
}

#[test]
fn combat_only_bot_still_progresses() {
    let snap = run_snapshot(2_000, BuyPolicy::CombatOnly, 7);
    report("combat_only_2000", &snap);
    assert!(snap.kills >= 15, "戦闘強化だけでも撃破が進むはず");
    assert_eq!(snap.levels[UpgradeKind::Pulse.index()], 0);
    assert_eq!(snap.levels[UpgradeKind::Lance.index()], 0);
}

#[test]
fn player_upgrades_do_not_increase_spawn_pressure() {
    let mut plain = StarRingState::new();
    plain.depth = 4;
    let base_interval = plain.spawn_interval();
    let base_batch = plain.spawn_batch();

    plain.shards = 1e12;
    for kind in UpgradeKind::ALL {
        if plain.upgrade_unlocked(kind) {
            for _ in 0..5 {
                let _ = purchase_upgrade(&mut plain, kind);
            }
        }
    }
    assert_eq!(
        plain.spawn_interval(),
        base_interval,
        "強化を積んでも出現間隔は層だけで決まる"
    );
    assert_eq!(plain.spawn_batch(), base_batch);

    plain.depth = 10;
    assert!(
        plain.spawn_interval() < base_interval || plain.spawn_batch() > base_batch,
        "層を深くすると出現圧が上がる"
    );
}

// ---------------------------------------------------------------------------
// 感度レポート
// ---------------------------------------------------------------------------

#[test]
fn progression_balance_report() {
    const RUNS: u32 = 20;
    const TICKS: u32 = 4_000;
    let mut kills = Vec::with_capacity(RUNS as usize);
    let mut nets = Vec::with_capacity(RUNS as usize);
    let mut leak_rates = Vec::with_capacity(RUNS as usize);
    let mut depths = Vec::with_capacity(RUNS as usize);
    let mut pulses = Vec::with_capacity(RUNS as usize);
    let mut lances = Vec::with_capacity(RUNS as usize);

    for seed in 1..=RUNS {
        let snap = run_snapshot(TICKS, BuyPolicy::Cheapest, seed);
        kills.push(snap.kills);
        nets.push(snap.net_earned());
        leak_rates.push(snap.leak_rate());
        depths.push(snap.depth);
        pulses.push(snap.levels[UpgradeKind::Pulse.index()] as u64);
        lances.push(snap.levels[UpgradeKind::Lance.index()] as u64);
    }

    let med_kills = median_u64(&mut kills);
    let med_net = median_f64(&mut nets);
    let med_leak = median_f64(&mut leak_rates);
    let med_depth = median_u32(&mut depths);
    let med_pulse = median_u64(&mut pulses);
    let med_lance = median_u64(&mut lances);

    eprintln!(
        "[starringe/progression] runs={RUNS} ticks={TICKS} median_kills={med_kills} \
         median_net={med_net:.1} median_leak_rate={:.1}% median_depth={med_depth} \
         median_pulse_lv={med_pulse} median_lance_lv={med_lance}",
        med_leak * 100.0
    );

    assert!(med_kills >= 30, "中央撃破が低すぎる: {med_kills}");
    assert!(med_depth >= 2, "中央到達層が浅い: {med_depth}");
    assert!(
        med_depth <= 12,
        "4000tick の中央到達層が深すぎる (進行が速すぎる): {med_depth}"
    );
}

#[test]
fn weapon_ablation_report() {
    const RUNS: u32 = 14;
    const TICKS: u32 = 5_000;

    let no_weapons = BuyPolicy::Blocklist(&[UpgradeKind::Pulse, UpgradeKind::Lance]);

    let mut kills_with = Vec::new();
    let mut kills_without = Vec::new();
    let mut earned_with = Vec::new();
    let mut earned_without = Vec::new();

    for seed in 1..=RUNS {
        let w = run_snapshot(TICKS, BuyPolicy::Cheapest, seed);
        let o = run_snapshot(TICKS, no_weapons, seed);
        kills_with.push(w.kills);
        kills_without.push(o.kills);
        earned_with.push(w.earned);
        earned_without.push(o.earned);
    }

    let kw = median_u64(&mut kills_with);
    let ko = median_u64(&mut kills_without);
    let ew = median_f64(&mut earned_with);
    let eo = median_f64(&mut earned_without);
    let kill_delta = if ko == 0 {
        0.0
    } else {
        (kw as f64 - ko as f64) / ko as f64 * 100.0
    };

    eprintln!(
        "[starringe/weapons] ticks={TICKS} runs={RUNS}\n\
         with-weapons:    median_kills={kw} median_earned={ew:.1}\n\
         without-weapons: median_kills={ko} median_earned={eo:.1}\n\
         delta kills={kill_delta:+.1}%"
    );

    assert!(kw > 0 && ko > 0);
    // 武装は機会費用を払っても撃破で負けすぎないこと。
    // (深い層の硬い敵に対する役割を持つ前提)
    assert!(
        kill_delta > -15.0,
        "武装ありの方が大幅に弱い: delta={kill_delta:.1}%"
    );
}

#[test]
fn pulse_first_vs_combat_report() {
    const RUNS: u32 = 12;
    const TICKS: u32 = 5_000;

    let mut pulse_kills = Vec::new();
    let mut combat_kills = Vec::new();
    let mut pulse_lv = Vec::new();

    for seed in 1..=RUNS {
        let p = run_snapshot(TICKS, BuyPolicy::PulseFirst, seed);
        let c = run_snapshot(TICKS, BuyPolicy::CombatOnly, seed);
        pulse_kills.push(p.kills);
        combat_kills.push(c.kills);
        pulse_lv.push(p.levels[UpgradeKind::Pulse.index()] as u64);
    }

    let pk = median_u64(&mut pulse_kills);
    let ck = median_u64(&mut combat_kills);
    let pl = median_u64(&mut pulse_lv);
    let delta = if ck == 0 {
        0.0
    } else {
        (pk as f64 - ck as f64) / ck as f64 * 100.0
    };

    eprintln!(
        "[starringe/pulse-first] ticks={TICKS} runs={RUNS} median_pulse_lv={pl}\n\
         pulse-first median_kills={pk}\n\
         combat-only median_kills={ck}\n\
         delta={delta:+.1}%"
    );

    assert!(pl >= 1, "脈動優先 bot が脈動を積めていない");
}

#[test]
fn strategy_comparison_report() {
    const TICKS: u32 = 4_000;
    const SEED: u32 = 42;

    let policies = [
        ("cheapest", BuyPolicy::Cheapest),
        ("combat", BuyPolicy::CombatOnly),
        ("combat+yield", BuyPolicy::CombatAndYield),
        ("pulse-first", BuyPolicy::PulseFirst),
        ("lance-first", BuyPolicy::LanceFirst),
        ("damage-first", BuyPolicy::DamageFirst),
        (
            "no-pulse",
            BuyPolicy::Blocklist(&[UpgradeKind::Pulse]),
        ),
        (
            "no-lance",
            BuyPolicy::Blocklist(&[UpgradeKind::Lance]),
        ),
    ];

    eprintln!("[starringe/strategies] ticks={TICKS} seed={SEED}");
    for (name, policy) in policies {
        let snap = run_snapshot(TICKS, policy, SEED);
        eprintln!(
            "  {name:14} depth={:>2} kills={:>5} earned={:>8.1} leaked={:>7.1} net={:>8.1} leak={:>5.1}% \
             lv=[砲{} 火{} 連{} 脈{} 穿{} 収{}]",
            snap.depth,
            snap.kills,
            snap.earned,
            snap.leaked,
            snap.net_earned(),
            snap.leak_rate() * 100.0,
            1 + snap.levels[0],
            snap.levels[1],
            snap.levels[2],
            snap.levels[3],
            snap.levels[4],
            snap.levels[5],
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
                "  t={:>5} depth={:>2} kills={:>5} earned={:>8.1} leaked={:>7.1} leak={:>5.1}% ores={} \
                 lv=[砲{} 火{} 連{} 脈{} 穿{} 収{}]",
                snap.ticks,
                snap.depth,
                snap.kills,
                snap.earned,
                snap.leaked,
                snap.leak_rate() * 100.0,
                snap.unlocked_ores,
                1 + snap.levels[0],
                snap.levels[1],
                snap.levels[2],
                snap.levels[3],
                snap.levels[4],
                snap.levels[5],
            );
            next_i += 1;
        }
    }

    assert!(
        state.depth >= OreKind::Crystal.unlock_depth(),
        "8000tick で結晶層には届くはず depth={}",
        state.depth
    );
}
