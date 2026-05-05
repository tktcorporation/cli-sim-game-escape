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
        let mut city = City::with_seed(seed);
        city.ai_tier = tier;
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

    // ─────────────────────────────────────────────────────────────────
    // TODO (player input): is_game_progressing
    //
    // We need a *single* function that answers "given this snapshot, is
    // the game still moving forward?".  Use this in any future test that
    // asks "did we stall out?".
    //
    // Define it in a way that makes sense for a 1-hour Tier-1 run:
    //   - If you set the bar too high, even Tier-1 fails (game unwinnable).
    //   - If you set it too low, real stalls slip through.
    //
    // See `tier1_random_makes_progress_in_one_hour` above for the kind of
    // numbers a 1-hour Tier-1 run actually produces.
    // ─────────────────────────────────────────────────────────────────
    #[allow(dead_code)]
    fn is_game_progressing(_s: &Snapshot) -> bool {
        // TODO: implement.  Suggested shape:
        //   s.buildings_built >= ??? && s.income_per_sec > 0 && s.cash > ???
        unimplemented!("define progression criteria — see TODO above");
    }
}
