//! 星環の自動プレイシミュレーター。
//!
//! 買える強化を買い続ける bot で長期運転し、panic なし・撃破進行・
//! 星屑の長期増加を不変条件として検証する。

#[cfg(test)]
mod tests {
    use crate::games::starringe::logic::{
        can_upgrade_further, purchase_upgrade, tick, upgrade_cost,
    };
    use crate::games::starringe::state::{StarRingState, UpgradeKind};

    /// 買える強化のうち、コストが最安のものを買う (同率なら ALL 順)。
    fn bot_buy_cheapest(state: &mut StarRingState) -> bool {
        let mut best: Option<(UpgradeKind, f64)> = None;
        for kind in UpgradeKind::ALL {
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

    fn run_bot(ticks: u32) -> StarRingState {
        let mut state = StarRingState::new();
        for _ in 0..ticks {
            // 毎 tick 買えるだけ買う (最大数回)
            for _ in 0..4 {
                if !bot_buy_cheapest(&mut state) {
                    break;
                }
            }
            tick(&mut state, 1);
        }
        state
    }

    fn report(label: &str, state: &StarRingState) {
        eprintln!("=== 星環 sim: {label} ===");
        eprintln!(
            "ticks={} shards={:.1} earned={:.1} leaked={:.1} kills={} leaks={}",
            state.elapsed_ticks,
            state.shards,
            state.shards_earned,
            state.shards_leaked,
            state.total_kills,
            state.leak_count
        );
        eprintln!(
            "upgrades: 砲={} 速={} 火={} 連={} 密={} 収={}",
            state.turret_count(),
            state.level(UpgradeKind::OrbitSpeed),
            state.level(UpgradeKind::Damage),
            state.level(UpgradeKind::FireRate),
            state.level(UpgradeKind::Density),
            state.level(UpgradeKind::Yield),
        );
        eprintln!("shards/sec≈{:.2}", state.shards_per_sec());
    }

    #[test]
    fn long_run_never_panics_and_keeps_invariants() {
        let state = run_bot(2_500);
        report("2500ticks", &state);

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
        // 漏洩を除いた純増は正。所持星屑は購入で減るので earned で見る。
        assert!(
            state.shards_earned + 1e-6 >= state.shards_leaked,
            "獲得が漏洩を下回らないはず earned={} leaked={}",
            state.shards_earned,
            state.shards_leaked
        );
        // パーティクル爆発しすぎていない
        assert!(
            state.particles.len() < 600,
            "particles={}",
            state.particles.len()
        );
        assert!(state.ores.len() < 80, "ores={}", state.ores.len());
    }

    #[test]
    fn bot_purchases_upgrades_over_time() {
        let early = run_bot(200);
        let late = run_bot(2_000);
        report("early200", &early);
        report("late2000", &late);

        let early_levels: u32 = early.upgrade_levels.iter().sum();
        let late_levels: u32 = late.upgrade_levels.iter().sum();
        assert!(
            late_levels > early_levels,
            "長期ほど強化が進むはず early={early_levels} late={late_levels}"
        );
        assert!(
            late.total_kills > early.total_kills,
            "撃破も増えるはず early={} late={}",
            early.total_kills,
            late.total_kills
        );
        assert!(
            late.shards_earned > early.shards_earned,
            "累計獲得も増えるはず"
        );
    }

    #[test]
    fn shards_earned_is_monotone_nondecreasing() {
        let mut state = StarRingState::new();
        let mut prev = state.shards_earned;
        for t in 0..1_500 {
            bot_buy_cheapest(&mut state);
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
}
