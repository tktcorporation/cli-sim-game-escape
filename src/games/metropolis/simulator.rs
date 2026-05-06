//! Balance simulator for Idle Metropolis.
//!
//! The whole point of having this *first* is that the lowest-tier AI is
//! deliberately dumb.  Without sim coverage, "Tier 1 is dumb but the game
//! still moves forward" is just a hope.  These tests turn it into a
//! checkable invariant.
//!
//! Run with:
//!   cargo test -p cli-sim-game-escape metropolis -- --nocapture

#[cfg(test)]
mod tests {
    use crate::games::metropolis::logic;
    use crate::games::metropolis::state::*;

    /// One row of the simulation log.
    #[derive(Debug, Clone)]
    struct Snapshot {
        sec: u32,
        cash: i64,
        population: u32,
        buildings_built: u64,
        constructions_started: u64,
        income_per_sec: i64,
        roads: u32,
        houses: u32,
        shops: u32,
    }

    fn snap(city: &City, sec: u32) -> Snapshot {
        Snapshot {
            sec,
            cash: city.cash,
            population: city.population(),
            buildings_built: city.buildings_finished,
            constructions_started: city.buildings_started,
            income_per_sec: logic::compute_income_per_sec(city),
            roads: city.count_built(Building::Road),
            houses: city.count_built(Building::House),
            shops: city.count_built(Building::Shop),
        }
    }

    fn print_snapshot(s: &Snapshot) {
        eprintln!(
            "│ t={:>4}s  cash=${:>7}  pop={:>3}  built={:>3} (R{} H{} S{})  inc=${}/s  started={}",
            s.sec,
            s.cash,
            s.population,
            s.buildings_built,
            s.roads,
            s.houses,
            s.shops,
            s.income_per_sec,
            s.constructions_started,
        );
    }

    /// Run a city forward `total_seconds` seconds, snapshotting at
    /// the supplied checkpoints.  Returns the full snapshot list.
    fn run(
        seed: u64,
        tier: AiTier,
        workers: u32,
        total_seconds: u32,
        report_at: &[u32],
    ) -> Vec<Snapshot> {
        // 旧仕様の Balanced は廃止。Tier 階差は cash で評価したいので、
        // shop を最も多く建てる Income をベンチ基準にする。Growth だと
        // 70/20/10 (Tier 4) で店舗が少なくなり、tier 階差が薄まる。
        run_with_strategy(seed, tier, Strategy::Income, workers, total_seconds, report_at)
    }

    fn run_with_strategy(
        seed: u64,
        tier: AiTier,
        strategy: Strategy,
        workers: u32,
        total_seconds: u32,
        report_at: &[u32],
    ) -> Vec<Snapshot> {
        let mut city = City::with_seed(seed);
        city.ai_tier = tier;
        city.strategy = strategy;
        city.workers = workers;

        let mut snaps: Vec<Snapshot> = Vec::new();
        let mut report_idx = 0;

        for sec in 1..=total_seconds {
            logic::tick(&mut city, TICKS_PER_SEC);
            if report_idx < report_at.len() && sec == report_at[report_idx] {
                let s = snap(&city, sec);
                snaps.push(s.clone());
                print_snapshot(&s);
                report_idx += 1;
            }
        }

        // Always include final.
        if snaps.last().map(|s| s.sec) != Some(total_seconds) {
            let s = snap(&city, total_seconds);
            snaps.push(s.clone());
            print_snapshot(&s);
        }
        snaps
    }

    /// Sanity check: the *dumbest* AI must still make the city grow.
    /// If this test ever breaks, the lowest tier has become unwinnable
    /// and the player would be stuck forever.
    #[test]
    fn tier1_random_makes_progress_in_one_hour() {
        eprintln!("\n=== Tier 1 (Random Bot)  workers=1  1 hour ===");
        let snaps = run(
            0xC1A5_5EED,
            AiTier::Random,
            1,
            3600,
            &[60, 300, 600, 1200, 1800, 2700, 3600],
        );
        let final_snap = snaps.last().unwrap();

        // Required progression invariants.  Tune these in concert with
        // `is_game_progressing` in this file.  See the TODO below.
        assert!(
            final_snap.buildings_built >= 5,
            "Tier 1 should finish at least 5 buildings in an hour, got {}",
            final_snap.buildings_built
        );
        assert!(
            final_snap.population > 0,
            "Tier 1 should have *some* houses standing after 1 hour"
        );
        // The interesting one: did income ever start flowing?
        let any_income = snaps.iter().any(|s| s.income_per_sec > 0);
        assert!(
            any_income,
            "Tier 1 never started earning income — the game is stalled"
        );
    }

    /// Tier 4 strategies should specialize:
    ///   - Income → 最も現金が稼げる
    ///   - Growth → 最も人口が伸びる
    ///   - Tech   → 道路網が広く、建設総数が多い (展開重視)
    ///
    /// 戦略ボタンの「意味」を担保する回帰テスト。
    #[test]
    fn tier4_strategies_specialize() {
        let seed = 0xC1A5_5EED;
        let span = 1800;
        let cps = [1800];
        let inc = run_with_strategy(seed, AiTier::DemandAware, Strategy::Income, 1, span, &cps);
        let tec = run_with_strategy(seed, AiTier::DemandAware, Strategy::Tech, 1, span, &cps);
        let gro = run_with_strategy(seed, AiTier::DemandAware, Strategy::Growth, 1, span, &cps);

        let inc_final = inc.last().unwrap();
        let tec_final = tec.last().unwrap();
        let gro_final = gro.last().unwrap();
        eprintln!(
            "[T4 strategy 30min] Income: cash=${} pop={}  Tech: cash=${} pop={} roads={}  Growth: cash=${} pop={}",
            inc_final.cash, inc_final.population,
            tec_final.cash, tec_final.population, tec_final.roads,
            gro_final.cash, gro_final.population,
        );

        // Income は現金 1 位。
        assert!(
            inc_final.cash >= tec_final.cash,
            "Income should beat Tech in cash"
        );
        // Growth は人口 1 位。
        assert!(
            gro_final.population >= tec_final.population,
            "Growth should beat Tech in population"
        );
        // Tech は道路網が太い (Income/Growth より roads が多い)。
        assert!(
            tec_final.roads >= inc_final.roads && tec_final.roads >= gro_final.roads,
            "Tech should have the largest road network: Tech={} Income={} Growth={}",
            tec_final.roads,
            inc_final.roads,
            gro_final.roads,
        );
    }

    /// Each higher Tier should produce more cash than the one below it
    /// at the same wall-clock time.  This is the *headline* reason a
    /// player would spend money on CPU upgrades.
    #[test]
    fn tier_ordering_holds_at_30min() {
        let seed = 0xC1A5_5EED;
        let span = 1800; // 30 minutes
        let cps = [60, 1800];
        eprintln!("\n=== Tier 1 ===");
        let s1 = run(seed, AiTier::Random, 1, span, &cps);
        eprintln!("\n=== Tier 2 ===");
        let s2 = run(seed, AiTier::Greedy, 1, span, &cps);
        eprintln!("\n=== Tier 3 ===");
        let s3 = run(seed, AiTier::RoadPlanner, 1, span, &cps);
        eprintln!("\n=== Tier 4 (Growth baseline) ===");
        let s4 = run(seed, AiTier::DemandAware, 1, span, &cps);

        let c1 = s1.last().unwrap().cash;
        let c2 = s2.last().unwrap().cash;
        let c3 = s3.last().unwrap().cash;
        let c4 = s4.last().unwrap().cash;
        eprintln!(
            "\n[30min cash] T1=${} T2=${} T3=${} T4=${}",
            c1, c2, c3, c4
        );

        assert!(c2 > c1, "T2 should beat T1 ({} vs {})", c2, c1);
        assert!(
            c3 >= c2,
            "T3 should be >= T2 (road-aware placement) ({} vs {})",
            c3,
            c2
        );
        assert!(
            c4 >= c3,
            "T4 should be >= T3 (demand-aware on top of roads) ({} vs {})",
            c4,
            c3
        );
    }

    /// Tier 2 should outperform Tier 1 — adjacency placement means roads
    /// and shops cluster, so income kicks in earlier.
    #[test]
    fn tier2_outperforms_tier1() {
        eprintln!("\n=== Tier 1 baseline (30 min) ===");
        let s1 = run(0xDEAD_BEEF, AiTier::Random, 1, 1800, &[600, 1200, 1800]);
        eprintln!("\n=== Tier 2 challenger (30 min) ===");
        let s2 = run(0xDEAD_BEEF, AiTier::Greedy, 1, 1800, &[600, 1200, 1800]);

        let t1_final = s1.last().unwrap();
        let t2_final = s2.last().unwrap();
        assert!(
            t2_final.cash >= t1_final.cash,
            "Tier 2 should beat Tier 1 in cash: T1=${} T2=${}",
            t1_final.cash,
            t2_final.cash
        );
    }

    /// More workers ⇒ more parallel construction ⇒ faster growth.
    /// Uses Tier 2 (Greedy) because Tier 1's pure-random rolls vary too
    /// much by seed for a single-run worker comparison to be stable —
    /// our first attempt failed at seed 42 because Tier-1's RNG happened
    /// to roll expensive Houses on the 4-worker run and cheap Roads on
    /// the 1-worker run.  Tier 2 clusters predictably so the worker
    /// mechanic shows up cleanly.
    #[test]
    fn more_workers_build_more() {
        let s_solo = run(42, AiTier::Greedy, 1, 600, &[600]);
        let s_team = run(42, AiTier::Greedy, 4, 600, &[600]);
        let solo = s_solo.last().unwrap();
        let team = s_team.last().unwrap();
        assert!(
            team.constructions_started > solo.constructions_started,
            "4 workers should start more constructions than 1: solo={} team={}",
            solo.constructions_started,
            team.constructions_started,
        );
    }

    /// Is the game *still progressing* at this snapshot?
    ///
    /// Time-gated thresholds so we can call this on any snapshot from
    /// 60s onward.  Each rule reflects an observation from the post-fix
    /// Tier 1 run:
    ///   - 60s   : at least one building has finished (anything!)
    ///   - 5min  : income has started flowing (≥ $1/s)
    ///   - 30min : the city is actually a city (≥ 10 buildings, ≥ $5/s)
    ///   - 60min : the player can afford the Tier 2 upgrade ($500),
    ///     counting future income from the next minute too.
    ///
    /// Tighter bars would make Tier 1 unwinnable; looser bars would let
    /// "5 houses forever" stalls slip through undetected.
    fn is_game_progressing(s: &Snapshot) -> bool {
        let mins = s.sec / 60;
        if mins >= 1 && s.buildings_built < 1 {
            return false;
        }
        if mins >= 5 && s.income_per_sec < 1 {
            return false;
        }
        if mins >= 30 && (s.buildings_built < 10 || s.income_per_sec < 5) {
            return false;
        }
        if mins >= 60 && s.cash + s.income_per_sec * 60 < 500 {
            return false;
        }
        true
    }

    /// Run Tier 1 across many seeds and assert *every* snapshot keeps
    /// the game progressing.  This is the headline guarantee: no matter
    /// the dice rolls, the dumbest AI never traps the player.
    #[test]
    fn tier1_never_stalls_across_seeds() {
        let seeds: [u64; 8] = [
            0xC1A5_5EED,
            0xDEAD_BEEF,
            42,
            1,
            0xFEED_FACE,
            0x1234_5678,
            0xBEEF_CAFE,
            0xAAAA_BBBB,
        ];
        let checkpoints = [60, 300, 1800, 3600];
        for seed in seeds {
            let snaps = run(seed, AiTier::Random, 1, 3600, &checkpoints);
            for s in &snaps {
                assert!(
                    is_game_progressing(s),
                    "Tier 1 stalled at seed=0x{:X} t={}s: {:?}",
                    seed,
                    s.sec,
                    s
                );
            }
        }
    }
}
