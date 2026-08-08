//! 星環の自動プレイシミュレーター。
//!
//! 解放済み武装の最安強化と環強化を買い続ける bot で長期運転し、
//! panic なし・撃破進行・層進行・星屑の長期増加を不変条件として検証する。

#[cfg(test)]
mod tests {
    use crate::games::starringe::logic::{
        can_upgrade_weapon_stat, purchase_ring_upgrade, purchase_weapon_stat, ring_upgrade_cost,
        tick, weapon_stat_cost,
    };
    use crate::games::starringe::state::{
        Layer, RingUpgrade, StarRingState, WeaponKind, WeaponStat,
    };

    /// 買える強化のうち最安を1つ買う。
    fn bot_buy_cheapest(state: &mut StarRingState) -> bool {
        let mut best_weapon: Option<(WeaponKind, WeaponStat, f64)> = None;
        for w in state.unlocked_weapons() {
            for stat in WeaponStat::ALL {
                if !can_upgrade_weapon_stat(state, w, stat) {
                    continue;
                }
                let cost = weapon_stat_cost(state, w, stat);
                if state.shards + 1e-9 < cost {
                    continue;
                }
                if best_weapon.map(|(_, _, c)| cost < c).unwrap_or(true) {
                    best_weapon = Some((w, stat, cost));
                }
            }
        }
        let mut best_ring: Option<(RingUpgrade, f64)> = None;
        for kind in RingUpgrade::ALL {
            let cost = ring_upgrade_cost(state, kind);
            if state.shards + 1e-9 < cost {
                continue;
            }
            if best_ring.map(|(_, c)| cost < c).unwrap_or(true) {
                best_ring = Some((kind, cost));
            }
        }

        match (best_weapon, best_ring) {
            (Some((w, s, wc)), Some((r, rc))) => {
                if wc <= rc {
                    purchase_weapon_stat(state, w, s)
                } else {
                    purchase_ring_upgrade(state, r)
                }
            }
            (Some((w, s, _)), None) => purchase_weapon_stat(state, w, s),
            (None, Some((r, _))) => purchase_ring_upgrade(state, r),
            (None, None) => false,
        }
    }

    fn run_bot(ticks: u32) -> StarRingState {
        let mut state = StarRingState::new();
        for _ in 0..ticks {
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
            "ticks={} shards={:.1} earned={:.1} kills={} missed={} layer={}",
            state.elapsed_ticks,
            state.shards,
            state.shards_earned,
            state.total_kills,
            state.missed_count,
            state.layer()
        );
        eprint!("weapons:");
        for w in WeaponKind::ALL {
            if state.is_weapon_unlocked(w) {
                eprint!(
                    " {}[弾{}連{}威{}]",
                    w.label(),
                    state.weapon_stat(w, WeaponStat::Count),
                    state.weapon_stat(w, WeaponStat::Rate),
                    state.weapon_stat(w, WeaponStat::Power),
                );
            }
        }
        eprintln!();
        eprintln!(
            "ring: 速={} 収={}  shards/sec≈{:.2}  projs={} ores={}",
            state.ring_level(RingUpgrade::OrbitSpeed),
            state.ring_level(RingUpgrade::Yield),
            state.shards_per_sec(),
            state.projectiles.len(),
            state.ores.len()
        );
    }

    #[test]
    fn long_run_never_panics_and_keeps_invariants() {
        let state = run_bot(3_000);
        report("3000ticks", &state);

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
        // 漏洩ペナルティは廃止済み — 所持星屑は購入で減るので earned で見る
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
    }

    #[test]
    fn bot_purchases_upgrades_and_advances_layers() {
        let early = run_bot(300);
        let late = run_bot(4_000);
        report("early300", &early);
        report("late4000", &late);

        let early_levels: u32 = early
            .weapon_levels
            .iter()
            .flatten()
            .sum::<u32>()
            + early.ring_levels.iter().sum::<u32>();
        let late_levels: u32 = late
            .weapon_levels
            .iter()
            .flatten()
            .sum::<u32>()
            + late.ring_levels.iter().sum::<u32>();
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
            late.layer() >= early.layer(),
            "層は後退しない early={} late={}",
            early.layer(),
            late.layer()
        );
        assert!(
            late.layer() >= 2,
            "4000tick で少なくとも第2層に届くはず layer={}",
            late.layer()
        );
        assert!(
            late.unlocked_weapons().len() >= 2,
            "層進行で武装が増えるはず n={}",
            late.unlocked_weapons().len()
        );
    }

    #[test]
    fn shards_earned_is_monotone_nondecreasing() {
        let mut state = StarRingState::new();
        let mut prev = state.shards_earned;
        for t in 0..2_000 {
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

    #[test]
    fn layer_milestones_change_spawn_pressure() {
        assert!(Layer::spawn_batch(4) >= 2);
        assert!(Layer::hp_mult(4) > Layer::hp_mult(1) + 0.5);
        assert!(Layer::value_mult(4) > Layer::value_mult(1) + 0.5);
        // 第1層から第2層までの撃破要求が「ぬるくない」
        assert!(Layer::THRESHOLDS[1] >= 60);
    }

    #[test]
    fn arrival_never_reduces_shards_over_long_run() {
        // 強化を一切買わず逸失が起きやすい状況でも星屑が負のペナルティを受けない
        let mut state = StarRingState::new();
        let mut min_shards = state.shards;
        for _ in 0..800 {
            tick(&mut state, 1);
            // 購入なし。撃破で増えるか現状維持のみ
            min_shards = min_shards.min(state.shards);
        }
        assert!(
            min_shards + 1e-9 >= 0.0,
            "星屑が負にならない min={min_shards}"
        );
        // 初期所持を下回るのは「漏洩で減る」旧仕様。新仕様では撃破以外で減らない
        // (購入していないので初期以上)
        assert!(
            state.shards + 1e-9 >= 12.0,
            "購入なしなら初期星屑を下回らない shards={}",
            state.shards
        );
    }
}
