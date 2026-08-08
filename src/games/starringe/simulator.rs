//! 星環 (Star Ring) の自動プレイシミュレーター。
//!
//! 解放済み武装の強化と環強化を買い続ける bot で長期運転し、panic なし・
//! 撃破進行・層進行・星屑の長期増加を不変条件として検証する。
//!
//! あわせて、今後のバランス改修のベースライン計測用に感度レポートを提供する:
//!
//! - **公転速度の寄与**: 公転速度へ投資したランとしなかったランで、撃破効率が
//!   どれだけ違うか (「メリットがよく分からない」問題の数値化)
//! - **武装ステの寄与**: 弾数 / 連射 / 威力をそれぞれ優先した時の差
//! - **層進行カーブ**: 撃破閾値で武装・鉱石種が解放されるペース
//! - **逸失率**: 中心到達で報酬を逃す割合 (防衛ペナルティではないが、刈り取り
//!   効率の指標として見る)
//!
//! バランス調整時は
//! `cargo test starringe::simulator -- --nocapture`
//! でレポートを見ながら数値を触るとよい。
//!
//! 今後の改修方針メモ (ゲームロジック側は未着手 / 一部は PR #153 で着手済み):
//! - 中心へ一直線に迫る動きを弱め、防衛コアへの圧を別の形で作る
//! - 鉱脈密度のような「収入↑と脅威↑が同一レバー」は復活させない
//! - より深い層で敵強化と新攻撃手段の解放を厚くする
//! - 砲台数・連射以外の攻撃手段・敵バリエーション・演出を増やす

#![cfg(test)]

use super::logic::{
    can_upgrade_weapon_stat, purchase_ring_upgrade, purchase_weapon_stat, ring_upgrade_cost, tick,
    weapon_stat_cost,
};
use super::state::{
    Layer, OreKind, RingUpgrade, StarRingState, WeaponKind, WeaponStat,
};

/// 購入方策。感度分析で「どの強化が効いているか」を切り分ける。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BuyPolicy {
    /// 買える中で最安を買う。
    Cheapest,
    /// 武装ステのみ (環強化は買わない)。
    WeaponsOnly,
    /// 武装ステ + 収率。公転は買わない。
    WeaponsAndYield,
    /// 公転速度を優先し、買えない時だけ最安。
    OrbitFirst,
    /// 収率を優先し、買えない時だけ最安。
    YieldFirst,
    /// 威力を優先し、買えない時だけ最安。
    PowerFirst,
    /// 連射を優先し、買えない時だけ最安。
    RateFirst,
    /// 弾数を優先し、買えない時だけ最安。
    CountFirst,
    /// 指定の環強化を買わない (ablation 用)。
    BlockRing(&'static [RingUpgrade]),
    /// 指定の武装ステを買わない (ablation 用)。
    BlockStat(&'static [WeaponStat]),
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
        BuyPolicy::BlockStat(_)
        | BuyPolicy::Cheapest
        | BuyPolicy::OrbitFirst
        | BuyPolicy::YieldFirst
        | BuyPolicy::PowerFirst
        | BuyPolicy::RateFirst
        | BuyPolicy::CountFirst => true,
    }
}

fn stat_allowed(policy: BuyPolicy, stat: WeaponStat) -> bool {
    match policy {
        BuyPolicy::BlockStat(list) => !list.contains(&stat),
        _ => true,
    }
}

fn preferred_ring(policy: BuyPolicy) -> Option<RingUpgrade> {
    match policy {
        BuyPolicy::OrbitFirst => Some(RingUpgrade::OrbitSpeed),
        BuyPolicy::YieldFirst => Some(RingUpgrade::Yield),
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
    let cost = ring_upgrade_cost(state, kind);
    if state.shards + 1e-9 < cost {
        return false;
    }
    purchase_ring_upgrade(state, kind)
}

/// 方策に従って最大1回購入する。買えたら true。
fn bot_buy_once(state: &mut StarRingState, policy: BuyPolicy) -> bool {
    if let Some(pref) = preferred_ring(policy) {
        if ring_allowed(policy, pref) && try_buy_ring(state, pref) {
            return true;
        }
    }
    if let Some(pref) = preferred_stat(policy) {
        // 解放済み武装のうち最安の pref ステを買う
        let mut best: Option<(WeaponKind, f64)> = None;
        for w in state.unlocked_weapons() {
            if !stat_allowed(policy, pref) || !can_upgrade_weapon_stat(state, w, pref) {
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
            if !stat_allowed(policy, stat) || !can_upgrade_weapon_stat(state, w, stat) {
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
        if !ring_allowed(policy, kind) {
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
    ring_levels: [u32; 2],
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

    fn orbit_lv(&self) -> u32 {
        self.ring_levels[RingUpgrade::OrbitSpeed.index()]
    }

    fn yield_lv(&self) -> u32 {
        self.ring_levels[RingUpgrade::Yield.index()]
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
        "ring: 速={} 収={}  unlocked_w={} unlocked_ore={}",
        snap.orbit_lv(),
        snap.yield_lv(),
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
        state.total_kills >= 30,
        "長時間プレイで撃破が進むはず kills={}",
        state.total_kills
    );
    assert!(
        state.shards_earned > 40.0,
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
    assert!(state.shards.is_finite());
    assert!(state.shards_earned.is_finite());
}

#[test]
fn bot_purchases_upgrades_and_advances_layers() {
    let early = run_snapshot(300, BuyPolicy::Cheapest, 1);
    let late = run_snapshot(4_000, BuyPolicy::Cheapest, 1);
    report("early300", &early);
    report("late4000", &late);

    assert!(
        late.total_levels() > early.total_levels(),
        "長期ほど強化が進むはず early={} late={}",
        early.total_levels(),
        late.total_levels()
    );
    assert!(
        late.kills > early.kills,
        "撃破も増えるはず early={} late={}",
        early.kills,
        late.kills
    );
    assert!(
        late.layer >= early.layer,
        "層は後退しない early={} late={}",
        early.layer,
        late.layer
    );
    assert!(
        late.layer >= 2,
        "4000tick で少なくとも第2層に届くはず layer={}",
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
    assert!(Layer::spawn_batch(4) >= 2);
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
    assert!(
        min_shards + 1e-9 >= 0.0,
        "星屑が負にならない min={min_shards}"
    );
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
        snap.kills >= 20,
        "武装強化だけでも撃破が進むはず kills={}",
        snap.kills
    );
    assert_eq!(snap.orbit_lv(), 0, "WeaponsOnly は公転を買わない");
    assert_eq!(snap.yield_lv(), 0, "WeaponsOnly は収率を買わない");
}

// ---------------------------------------------------------------------------
// 感度レポート (バランス調整用 — 観測が主目的、assert は緩め)
// ---------------------------------------------------------------------------

/// 複数シードでの中央値進行。改修前後の「体感の土台」を数値で残す。
#[test]
fn progression_balance_report() {
    const RUNS: u32 = 24;
    const TICKS: u32 = 3_500;

    let mut kills = Vec::with_capacity(RUNS as usize);
    let mut earned = Vec::with_capacity(RUNS as usize);
    let mut miss_rates = Vec::with_capacity(RUNS as usize);
    let mut layers = Vec::with_capacity(RUNS as usize);
    let mut orbits = Vec::with_capacity(RUNS as usize);

    for seed in 1..=RUNS {
        let snap = run_snapshot(TICKS, BuyPolicy::Cheapest, seed);
        kills.push(snap.kills);
        earned.push(snap.earned);
        miss_rates.push(snap.miss_rate());
        layers.push(snap.layer);
        orbits.push(snap.orbit_lv() as u64);
    }

    let med_kills = median_u64(&mut kills);
    let med_earned = median_f64(&mut earned);
    let med_miss = median_f64(&mut miss_rates);
    let med_layer = median_u32(&mut layers);
    let med_orbit = median_u64(&mut orbits);

    eprintln!(
        "[starringe/progression] runs={RUNS} ticks={TICKS} median_kills={med_kills} \
         median_earned={med_earned:.1} median_miss_rate={:.1}% median_layer={med_layer} \
         median_orbit_lv={med_orbit}",
        med_miss * 100.0
    );

    assert!(
        med_kills >= 25,
        "最安買い bot の中央撃破が低すぎる: {med_kills}"
    );
    assert!(
        med_layer >= 2,
        "3500tick の中央到達層が浅い: {med_layer}"
    );
    // 進行が速すぎると層の「区切り」感が薄れる
    assert!(
        med_layer <= 8,
        "3500tick の中央到達層が深すぎる: {med_layer}"
    );
}

/// 公転速度へ寄せた方策 / 公転を買わない方策を比較する。
/// 「公転速度を上げるメリットがよく分からない」問題を数値化する。
#[test]
fn orbit_speed_ablation_report() {
    const RUNS: u32 = 18;
    const TICKS: u32 = 3_500;

    let no_orbit = BuyPolicy::BlockRing(&[RingUpgrade::OrbitSpeed]);

    let mut kills_orbit = Vec::new();
    let mut kills_no = Vec::new();
    let mut earned_orbit = Vec::new();
    let mut earned_no = Vec::new();
    let mut miss_orbit = Vec::new();
    let mut miss_no = Vec::new();
    let mut orbit_lv = Vec::new();

    for seed in 1..=RUNS {
        let with = run_snapshot(TICKS, BuyPolicy::OrbitFirst, seed);
        let without = run_snapshot(TICKS, no_orbit, seed);
        kills_orbit.push(with.kills);
        kills_no.push(without.kills);
        earned_orbit.push(with.earned);
        earned_no.push(without.earned);
        miss_orbit.push(with.miss_rate());
        miss_no.push(without.miss_rate());
        orbit_lv.push(with.orbit_lv() as u64);
    }

    let ko = median_u64(&mut kills_orbit);
    let kn = median_u64(&mut kills_no);
    let eo = median_f64(&mut earned_orbit);
    let en = median_f64(&mut earned_no);
    let mo = median_f64(&mut miss_orbit);
    let mn = median_f64(&mut miss_no);
    let ol = median_u64(&mut orbit_lv);
    let kill_delta = if kn == 0 {
        0.0
    } else {
        (ko as f64 - kn as f64) / kn as f64 * 100.0
    };
    let earned_delta = if en == 0.0 {
        0.0
    } else {
        (eo - en) / en * 100.0
    };

    eprintln!(
        "[starringe/orbit-ablation] ticks={TICKS} runs={RUNS} median_orbit_lv={ol}\n\
         orbit-first:  median_kills={ko} median_earned={eo:.1} median_miss={:.1}%\n\
         no-orbit:     median_kills={kn} median_earned={en:.1} median_miss={:.1}%\n\
         delta kills={kill_delta:+.1}% earned={earned_delta:+.1}%",
        mo * 100.0,
        mn * 100.0
    );

    assert!(ol >= 1, "公転優先 bot が公転を積めていない");
    // 現行バランスでは公転の寄与が薄い想定。回帰として「致命的に悪化しない」だけ置く。
    // 改修で公転の役割をはっきりさせる時は、ここを「明確にプラス」へ引き上げる。
    assert!(
        kill_delta > -40.0,
        "公転優先が壊滅的に弱い: delta={kill_delta:.1}%"
    );
}

/// 武装ステ (弾数/連射/威力) の優先方策比較。
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

/// 層進行と武装・鉱石解放のペースを時系列で見る。
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
                 w={} ores={} orbit={} yield={}",
                snap.ticks,
                snap.layer,
                snap.kills,
                snap.earned,
                snap.miss_rate() * 100.0,
                snap.unlocked_weapons,
                snap.unlocked_ores,
                snap.orbit_lv(),
                snap.yield_lv(),
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

/// 複数方策の横断比較。改修前後のスナップショット用。
#[test]
fn strategy_comparison_report() {
    const TICKS: u32 = 4_000;
    const SEED: u32 = 42;

    let policies = [
        ("cheapest", BuyPolicy::Cheapest),
        ("weapons", BuyPolicy::WeaponsOnly),
        ("w+yield", BuyPolicy::WeaponsAndYield),
        ("orbit-first", BuyPolicy::OrbitFirst),
        ("yield-first", BuyPolicy::YieldFirst),
        ("power-first", BuyPolicy::PowerFirst),
        ("rate-first", BuyPolicy::RateFirst),
        ("count-first", BuyPolicy::CountFirst),
        (
            "no-orbit",
            BuyPolicy::BlockRing(&[RingUpgrade::OrbitSpeed]),
        ),
        ("no-yield", BuyPolicy::BlockRing(&[RingUpgrade::Yield])),
        (
            "no-power",
            BuyPolicy::BlockStat(&[WeaponStat::Power]),
        ),
    ];

    eprintln!("[starringe/strategies] ticks={TICKS} seed={SEED}");
    for (name, policy) in policies {
        let snap = run_snapshot(TICKS, policy, SEED);
        eprintln!(
            "  {name:12} layer={:>2} kills={:>5} earned={:>8.1} miss={:>5.1}% \
             orbit={} yield={} wlv={} w={} ores={}",
            snap.layer,
            snap.kills,
            snap.earned,
            snap.miss_rate() * 100.0,
            snap.orbit_lv(),
            snap.yield_lv(),
            snap.total_weapon_levels(),
            snap.unlocked_weapons,
            snap.unlocked_ores,
        );
    }
}

/// 逸失 (中心到達) が星屑を減らさないこと、かつ逸失率が暴走しないこと。
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
    // 防衛失敗ではないので厳密な閾値は置かないが、大半が逸失する状態は刈り取りが破綻
    assert!(
        med < 0.85,
        "逸失率が高すぎて刈り取りが成立していない: {:.1}%",
        med * 100.0
    );
}
