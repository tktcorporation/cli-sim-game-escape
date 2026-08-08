//! 星環 (Star Ring) の自動プレイシミュレーター。
//!
//! 買える強化を買う bot で長期運転し、panic なし・撃破進行・星屑の長期増加を
//! 不変条件として検証する。あわせて、現行バランスの既知の歪みを数値で見る
//! ための感度レポートを提供する:
//!
//! - **鉱脈密度のトレードオフ**: 密度を上げると収入と漏洩が同時に増えるため、
//!   「強化なのに悪化する」局面が起きやすいか
//! - **公転速度の寄与**: 公転速度へ投資したランとしなかったランで、撃破効率が
//!   どれだけ違うか
//! - **進行カーブ**: レア鉱石解放・強化積み上げ・漏洩率が時間とともにどう伸びるか
//!
//! バランス調整時は
//! `cargo test starringe::simulator -- --nocapture`
//! でレポートを見ながら数値を触るとよい。
//!
//! 今後の改修方針メモ (ゲームロジック側は未着手):
//! - 中心へ一直線に迫るだけの動きを弱め、防衛コアへの圧を別の形で作る
//! - 鉱脈密度のような「収入↑と脅威↑が同一レバー」をやめる
//! - ウェーブ / 深度進行で敵強化と新攻撃手段の解放を載せる
//! - 砲台数・連射以外の攻撃手段・敵バリエーション・演出を増やす

#![cfg(test)]

use super::logic::{can_upgrade_further, purchase_upgrade, tick, upgrade_cost};
use super::state::{OreKind, StarRingState, UpgradeKind};

/// 購入方策。感度分析で「どの強化が効いているか」を切り分けるために使う。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BuyPolicy {
    /// 買える中で最安を買う (同率なら ALL 順)。
    Cheapest,
    /// 戦闘系のみ (砲台・火力・連射)。密度/収率/公転は買わない。
    CombatOnly,
    /// 戦闘系 + 収率。密度と公転は買わない。
    CombatAndYield,
    /// 密度を優先し、買えない時だけ最安。
    DensityFirst,
    /// 公転速度を優先し、買えない時だけ最安。
    OrbitFirst,
    /// 火力を優先し、買えない時だけ最安。
    DamageFirst,
    /// 指定種別だけ買わない (ablation 用)。
    Blocklist(&'static [UpgradeKind]),
}

fn is_allowed(policy: BuyPolicy, kind: UpgradeKind) -> bool {
    match policy {
        BuyPolicy::Cheapest
        | BuyPolicy::DensityFirst
        | BuyPolicy::OrbitFirst
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
        BuyPolicy::DensityFirst => Some(UpgradeKind::Density),
        BuyPolicy::OrbitFirst => Some(UpgradeKind::OrbitSpeed),
        BuyPolicy::DamageFirst => Some(UpgradeKind::Damage),
        _ => None,
    }
}

/// 方策に従って最大1回購入する。買えたら true。
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
        "ticks={} shards={:.1} earned={:.1} leaked={:.1} net={:.1} kills={} leaks={} leak_rate={:.1}% sps≈{:.2}",
        snap.ticks,
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
        "upgrades: 砲={} 速={} 火={} 連={} 密={} 収={}  unlocked_ores={}",
        1 + snap.levels[UpgradeKind::Turrets.index()],
        snap.levels[UpgradeKind::OrbitSpeed.index()],
        snap.levels[UpgradeKind::Damage.index()],
        snap.levels[UpgradeKind::FireRate.index()],
        snap.levels[UpgradeKind::Density.index()],
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

// ---------------------------------------------------------------------------
// 不変条件 / 回帰テスト
// ---------------------------------------------------------------------------

#[test]
fn long_run_never_panics_and_keeps_invariants() {
    let state = run_bot(2_500, BuyPolicy::Cheapest, 0xC0FFEE42);
    let snap = RunSnapshot::from_state(&state);
    report("2500ticks/cheapest", &snap);

    assert!(state.elapsed_ticks >= 2_500);
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
    // 漏洩を除いた純増の下限は置かない (密度強化で一時的に悪化しうるのが現行の課題)。
    // 代わりに「獲得自体は正」と「パーティクル/鉱石が爆発しない」を見る。
    assert!(
        state.particles.len() < 600,
        "particles={}",
        state.particles.len()
    );
    assert!(state.ores.len() < 80, "ores={}", state.ores.len());
    assert!(state.beams.len() < 80, "beams={}", state.beams.len());
    assert!(state.shards.is_finite());
    assert!(state.shards_earned.is_finite());
    assert!(state.shards_leaked.is_finite());
}

#[test]
fn bot_purchases_upgrades_over_time() {
    let early = run_snapshot(200, BuyPolicy::Cheapest, 1);
    let late = run_snapshot(2_000, BuyPolicy::Cheapest, 1);
    report("early200", &early);
    report("late2000", &late);

    let early_levels: u32 = early.levels.iter().sum();
    let late_levels: u32 = late.levels.iter().sum();
    assert!(
        late_levels > early_levels,
        "長期ほど強化が進むはず early={early_levels} late={late_levels}"
    );
    assert!(
        late.kills > early.kills,
        "撃破も増えるはず early={} late={}",
        early.kills,
        late.kills
    );
    assert!(
        late.earned > early.earned,
        "累計獲得も増えるはず"
    );
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
fn combat_only_bot_still_progresses() {
    let snap = run_snapshot(2_000, BuyPolicy::CombatOnly, 7);
    report("combat_only_2000", &snap);
    assert!(snap.kills >= 15, "戦闘強化だけでも撃破が進むはず kills={}", snap.kills);
    assert_eq!(
        snap.levels[UpgradeKind::Density.index()],
        0,
        "CombatOnly は密度を買わない"
    );
    assert_eq!(
        snap.levels[UpgradeKind::OrbitSpeed.index()],
        0,
        "CombatOnly は公転を買わない"
    );
}

#[test]
fn ore_unlock_progression_over_long_run() {
    let snap = run_snapshot(8_000, BuyPolicy::Cheapest, 11);
    report("ore_unlock_8000", &snap);
    assert!(
        snap.unlocked_ores >= 2,
        "長時間で少なくとも岩石までは解放されるはず unlocked={}",
        snap.unlocked_ores
    );
    // 新星核 (450撃破) は到達できてもよいが、必須にはしない。
    assert!(
        snap.kills >= OreKind::Rock.unlock_kills(),
        "岩石解放閾値まで撃破が進むはず kills={}",
        snap.kills
    );
}

// ---------------------------------------------------------------------------
// 感度レポート (バランス調整用、assert は緩め / 観測が主目的)
// ---------------------------------------------------------------------------

/// 複数シードでの中央値進行。改修前後の「体感の土台」を数値で残す。
#[test]
fn progression_balance_report() {
    const RUNS: u32 = 24;
    const TICKS: u32 = 3_000;
    let mut kills = Vec::with_capacity(RUNS as usize);
    let mut nets = Vec::with_capacity(RUNS as usize);
    let mut leak_rates = Vec::with_capacity(RUNS as usize);
    let mut dens = Vec::with_capacity(RUNS as usize);
    let mut orbits = Vec::with_capacity(RUNS as usize);

    for seed in 1..=RUNS {
        let snap = run_snapshot(TICKS, BuyPolicy::Cheapest, seed);
        kills.push(snap.kills);
        nets.push(snap.net_earned());
        leak_rates.push(snap.leak_rate());
        dens.push(snap.levels[UpgradeKind::Density.index()] as u64);
        orbits.push(snap.levels[UpgradeKind::OrbitSpeed.index()] as u64);
    }

    let med_kills = median_u64(&mut kills);
    let med_net = median_f64(&mut nets);
    let med_leak = median_f64(&mut leak_rates);
    let med_dens = median_u64(&mut dens);
    let med_orbit = median_u64(&mut orbits);

    eprintln!(
        "[starringe/progression] runs={RUNS} ticks={TICKS} median_kills={med_kills} \
         median_net={med_net:.1} median_leak_rate={:.1}% median_density_lv={med_dens} median_orbit_lv={med_orbit}",
        med_leak * 100.0
    );

    assert!(
        med_kills >= 20,
        "最安買い bot の中央撃破が低すぎる: {med_kills}"
    );
}

/// 鉱脈密度へ寄せた方策と寄せない方策を比較する。
/// 「密度を上げると収入と漏洩が同時に増える」現行の難しさを可視化する。
#[test]
fn density_tradeoff_report() {
    const RUNS: u32 = 16;
    const TICKS: u32 = 2_500;

    let mut combat_nets = Vec::new();
    let mut combat_leaks = Vec::new();
    let mut dens_nets = Vec::new();
    let mut dens_leaks = Vec::new();
    let mut dens_levels = Vec::new();

    for seed in 1..=RUNS {
        let c = run_snapshot(TICKS, BuyPolicy::CombatAndYield, seed);
        let d = run_snapshot(TICKS, BuyPolicy::DensityFirst, seed);
        combat_nets.push(c.net_earned());
        combat_leaks.push(c.leak_rate());
        dens_nets.push(d.net_earned());
        dens_leaks.push(d.leak_rate());
        dens_levels.push(d.levels[UpgradeKind::Density.index()] as u64);
    }

    let c_net = median_f64(&mut combat_nets);
    let c_leak = median_f64(&mut combat_leaks);
    let d_net = median_f64(&mut dens_nets);
    let d_leak = median_f64(&mut dens_leaks);
    let d_lv = median_u64(&mut dens_levels);

    eprintln!(
        "[starringe/density] ticks={TICKS} runs={RUNS}\n\
         combat+yield: median_net={c_net:.1} median_leak={:.1}%\n\
         density-first: median_net={d_net:.1} median_leak={:.1}% median_density_lv={d_lv}",
        c_leak * 100.0,
        d_leak * 100.0
    );

    // 密度優先は実際に密度を積んでいることだけ保証。
    // net / leak の勝敗は現行バランスの観測値なのでハード assert しない。
    assert!(d_lv >= 1, "密度優先 bot が密度を買えていない");
}

/// 公転速度への投資が撃破効率に効いているかを ablation する。
/// 効きが薄いなら「メリットが分からない」という体感と一致する。
#[test]
fn orbit_speed_ablation_report() {
    const RUNS: u32 = 16;
    const TICKS: u32 = 2_500;

    // 公転を許す最安買い vs 公転だけ禁止した最安買い。
    let without_orbit = BuyPolicy::Blocklist(&[UpgradeKind::OrbitSpeed]);

    let mut kills_with = Vec::new();
    let mut kills_without = Vec::new();
    let mut earned_with = Vec::new();
    let mut earned_without = Vec::new();
    let mut orbit_levels = Vec::new();

    for seed in 1..=RUNS {
        let w = run_snapshot(TICKS, BuyPolicy::Cheapest, seed);
        let o = run_snapshot(TICKS, without_orbit, seed);
        kills_with.push(w.kills);
        kills_without.push(o.kills);
        earned_with.push(w.earned);
        earned_without.push(o.earned);
        orbit_levels.push(w.levels[UpgradeKind::OrbitSpeed.index()] as u64);
    }

    let kw = median_u64(&mut kills_with);
    let ko = median_u64(&mut kills_without);
    let ew = median_f64(&mut earned_with);
    let eo = median_f64(&mut earned_without);
    let ol = median_u64(&mut orbit_levels);
    let kill_delta_pct = if ko == 0 {
        0.0
    } else {
        (kw as f64 - ko as f64) / ko as f64 * 100.0
    };
    let earned_delta_pct = if eo.abs() < 1e-9 {
        0.0
    } else {
        (ew - eo) / eo * 100.0
    };

    eprintln!(
        "[starringe/orbit] ticks={TICKS} runs={RUNS} median_orbit_lv={ol}\n\
         with-orbit:    median_kills={kw} median_earned={ew:.1}\n\
         without-orbit: median_kills={ko} median_earned={eo:.1}\n\
         delta: kills={kill_delta_pct:+.1}% earned={earned_delta_pct:+.1}%"
    );

    // 効果量自体は観測専用 (薄いなら「メリットが分からない」改修根拠になる)。
    // ランが空でないことだけ保証する。
    assert!(kw > 0 && ko > 0 && ew > 0.0 && eo > 0.0);
}

/// 公転速度を意図的に積ませた場合の寄与。最安買いだと公転を避ける可能性があるため、
/// OrbitFirst と CombatOnly を直接比較する。
#[test]
fn orbit_first_vs_combat_report() {
    const RUNS: u32 = 16;
    const TICKS: u32 = 2_500;

    let mut orbit_kills = Vec::new();
    let mut combat_kills = Vec::new();
    let mut orbit_lv = Vec::new();

    for seed in 1..=RUNS {
        let o = run_snapshot(TICKS, BuyPolicy::OrbitFirst, seed);
        let c = run_snapshot(TICKS, BuyPolicy::CombatOnly, seed);
        orbit_kills.push(o.kills);
        combat_kills.push(c.kills);
        orbit_lv.push(o.levels[UpgradeKind::OrbitSpeed.index()] as u64);
    }

    let ok = median_u64(&mut orbit_kills);
    let ck = median_u64(&mut combat_kills);
    let ol = median_u64(&mut orbit_lv);
    let delta_pct = if ck == 0 {
        0.0
    } else {
        (ok as f64 - ck as f64) / ck as f64 * 100.0
    };

    eprintln!(
        "[starringe/orbit-first] ticks={TICKS} runs={RUNS} median_orbit_lv={ol}\n\
         orbit-first median_kills={ok}\n\
         combat-only median_kills={ck}\n\
         delta={delta_pct:+.1}%"
    );

    assert!(ol >= 2, "公転優先 bot が公転を積めていない lv={ol}");
}

/// 方策横断の一覧。改修前ベースラインとして `--nocapture` で残す。
#[test]
fn strategy_comparison_report() {
    const TICKS: u32 = 3_000;
    const SEED: u32 = 42;

    let policies = [
        ("cheapest", BuyPolicy::Cheapest),
        ("combat", BuyPolicy::CombatOnly),
        ("combat+yield", BuyPolicy::CombatAndYield),
        ("density-first", BuyPolicy::DensityFirst),
        ("orbit-first", BuyPolicy::OrbitFirst),
        ("damage-first", BuyPolicy::DamageFirst),
        (
            "no-density",
            BuyPolicy::Blocklist(&[UpgradeKind::Density]),
        ),
        (
            "no-orbit",
            BuyPolicy::Blocklist(&[UpgradeKind::OrbitSpeed]),
        ),
    ];

    eprintln!("[starringe/strategies] ticks={TICKS} seed={SEED}");
    for (name, policy) in policies {
        let snap = run_snapshot(TICKS, policy, SEED);
        eprintln!(
            "  {name:14} kills={:>5} earned={:>8.1} leaked={:>7.1} net={:>8.1} leak={:>5.1}% \
             lv=[砲{} 速{} 火{} 連{} 密{} 収{}]",
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

/// 時系列スナップショット。ウェーブ/深度改修の前後比較用ベースライン。
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
                "  t={:>5} kills={:>5} earned={:>8.1} leaked={:>7.1} leak={:>5.1}% ores={} \
                 lv=[砲{} 速{} 火{} 連{} 密{} 収{}]",
                snap.ticks,
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
        state.total_kills >= OreKind::Rock.unlock_kills(),
        "8000tick で岩石解放に届くはず"
    );
}
