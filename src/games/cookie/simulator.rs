//! Balance simulator for Cookie Factory.
//! Run with: cargo test -p cli-sim-game-escape simulate_optimal -- --nocapture

#[cfg(test)]
mod tests {
    use crate::games::cookie::logic;
    use crate::games::cookie::state::*;

    /// What to purchase next.
    enum Purchase {
        Producer(ProducerKind),
        Upgrade(usize),
    }

    /// Find the purchase with the best ROI (lowest payback time).
    /// For upgrades, estimate the CPS gain and compute payback.
    fn find_best_purchase(state: &CookieState) -> Option<Purchase> {
        let mut best: Option<(f64, Purchase)> = None; // (payback_seconds, purchase)

        // Check producers
        for p in &state.producers {
            if state.cookies < p.cost() {
                continue;
            }
            let syn = state.synergy_bonus(&p.kind);
            if let Some(payback) = p.payback_seconds_with_synergy(syn) {
                let dominated = best.as_ref().map_or(false, |(bp, _)| *bp <= payback);
                if !dominated {
                    best = Some((payback, Purchase::Producer(p.kind.clone())));
                }
            }
        }

        // Check upgrades
        for (idx, upgrade) in state.upgrades.iter().enumerate() {
            if upgrade.purchased || state.cookies < upgrade.cost {
                continue;
            }
            if !state.is_upgrade_unlocked(upgrade) {
                continue;
            }
            // Estimate CPS gain from the upgrade
            let current_cps = state.total_cps();
            let cps_gain = estimate_upgrade_cps_gain(state, &upgrade.effect);
            if cps_gain > 0.0 {
                let payback = upgrade.cost / cps_gain;
                let dominated = best.as_ref().map_or(false, |(bp, _)| *bp <= payback);
                if !dominated {
                    best = Some((payback, Purchase::Upgrade(idx)));
                }
            } else {
                // For click upgrades, use a rough estimate: 5 clicks/sec
                if let UpgradeEffect::ClickPower(amount) = &upgrade.effect {
                    let cps_gain = amount * 5.0; // assume 5 clicks/sec
                    let payback = upgrade.cost / cps_gain;
                    let dominated = best.as_ref().map_or(false, |(bp, _)| *bp <= payback);
                    if !dominated {
                        best = Some((payback, Purchase::Upgrade(idx)));
                    }
                }
                // For upgrades with 0 estimated gain but affordable, buy them with low priority
                if current_cps > 0.0 {
                    let payback = upgrade.cost / current_cps * 100.0; // Very low priority
                    if best.is_none() {
                        best = Some((payback, Purchase::Upgrade(idx)));
                    }
                }
            }
        }

        best.map(|(_, p)| p)
    }

    /// Estimate CPS gain from an upgrade effect.
    fn estimate_upgrade_cps_gain(state: &CookieState, effect: &UpgradeEffect) -> f64 {
        match effect {
            UpgradeEffect::ProducerMultiplier { target, multiplier } => {
                let p = &state.producers[target.index()];
                let syn = state.synergy_bonus(target);
                let current = p.cps_with_synergy(syn);
                // New CPS = count * base_rate * (multiplier * old_mult) * (1 + syn)
                // Gain = current * (new_mult/old_mult - 1)
                current * (multiplier - 1.0)
            }
            UpgradeEffect::SynergyBoost { .. } => {
                // Doubling synergy_multiplier: compute difference
                let current_cps = state.total_cps();
                // Rough estimate: synergy currently adds some %, doubling it adds that again
                let base_no_synergy: f64 = state.producers.iter().map(|p| p.base_cps()).sum();
                current_cps - base_no_synergy // synergy contribution ≈ gain
            }
            UpgradeEffect::CrossSynergy {
                source,
                target,
                bonus_per_unit,
            } => {
                let source_count = state.producers[source.index()].count as f64;
                let target_base = state.producers[target.index()].base_cps();
                target_base * source_count * bonus_per_unit * state.synergy_multiplier
            }
            UpgradeEffect::ClickPower(_) => 0.0, // Handled separately
            UpgradeEffect::CountScaling { target, bonus_per_unit } => {
                let p = &state.producers[target.index()];
                let count = p.count as f64;
                // Each unit gives bonus_per_unit to all units → total bonus = count * bonus_per_unit
                // CPS gain ≈ base_cps * count * bonus_per_unit
                p.base_cps() * count * bonus_per_unit
            }
            UpgradeEffect::CpsPercentBonus { target, percentage } => {
                let p = &state.producers[target.index()];
                let count = p.count as f64;
                state.total_cps() * count * percentage
            }
            UpgradeEffect::KittenBoost { multiplier } => {
                // CPS gain = current_cps * milk * multiplier
                state.total_cps() * state.milk * multiplier
            }
        }
    }

    /// Report game stats at a given time.
    fn report_stats(state: &CookieState, seconds: u32, purchases_made: u32) {
        let minutes = seconds / 60;
        let secs = seconds % 60;

        eprintln!("┌─── {}分{}秒 ─────────────────────────", minutes, secs);
        eprintln!(
            "│ Cookies: {}  CPS: {}  Clicks: {}",
            logic::format_number(state.cookies),
            logic::format_number(state.total_cps()),
            state.total_clicks
        );
        eprintln!(
            "│ All-time: {}  Purchases: {}",
            logic::format_number(state.cookies_all_time),
            purchases_made
        );

        // Producer counts
        let counts: Vec<String> = state
            .producers
            .iter()
            .map(|p| format!("{}:{}", p.kind.name(), p.count))
            .collect();
        eprintln!("│ Producers: {}", counts.join("  "));

        // CPS breakdown
        let cps_parts: Vec<String> = state
            .producers
            .iter()
            .filter(|p| p.count > 0)
            .map(|p| {
                let syn = state.synergy_bonus(&p.kind);
                let cps = p.cps_with_synergy(syn);
                format!(
                    "{}:{}/s(x{:.1},syn+{:.0}%)",
                    p.kind.name(),
                    logic::format_number(cps),
                    p.multiplier,
                    syn * 100.0
                )
            })
            .collect();
        eprintln!("│ CPS詳細: {}", cps_parts.join("  "));

        // Purchased upgrades
        let purchased: Vec<&str> = state
            .upgrades
            .iter()
            .filter(|u| u.purchased)
            .map(|u| u.name.as_str())
            .collect();
        eprintln!("│ 購入済UP: {:?}", purchased);

        // Milestone & milk stats
        let achieved = state.achieved_milestone_count();
        let total_milestones = state.milestones.len();
        eprintln!(
            "│ マイルストーン: {}/{} ミルク: {:.0}% 子猫倍率: x{:.3}",
            achieved, total_milestones, state.milk * 100.0, state.kitten_multiplier
        );
        let recent: Vec<&str> = state
            .milestones
            .iter()
            .filter(|m| m.status == MilestoneStatus::Claimed)
            .map(|m| m.name.as_str())
            .collect();
        if !recent.is_empty() {
            eprintln!("│ 達成済: {:?}", recent);
        }

        // Prestige info
        let pending = state.pending_heavenly_chips();
        let total_all = state.cookies_all_runs + state.cookies_all_time;
        eprintln!(
            "│ 転生: {}回  チップ: {} (使用済{})  待機: {}  倍率: x{:.2}  全時間クッキー: {}",
            state.prestige_count,
            state.heavenly_chips,
            state.heavenly_chips_spent,
            pending,
            state.prestige_multiplier,
            logic::format_number(total_all)
        );

        // Savings bonus
        eprintln!(
            "│ 貯蓄ボーナス: x{:.3}  砂糖: {}",
            state.savings_bonus(),
            state.sugar,
        );

        // Next affordable purchase
        if let Some(purchase) = find_best_purchase(state) {
            match purchase {
                Purchase::Producer(kind) => {
                    let p = &state.producers[kind.index()];
                    eprintln!("│ 次の購入候補: {} ({})", kind.name(), logic::format_number(p.cost()));
                }
                Purchase::Upgrade(idx) => {
                    let u = &state.upgrades[idx];
                    eprintln!(
                        "│ 次の購入候補: {} ({})",
                        u.name,
                        logic::format_number(u.cost)
                    );
                }
            }
        }

        // Time until next upgrade unlock
        let next_unlock: Vec<String> = state
            .upgrades
            .iter()
            .filter(|u| !u.purchased && !state.is_upgrade_unlocked(u))
            .filter_map(|u| {
                u.unlock_condition.as_ref().map(|(kind, count)| {
                    let current = state.producers[kind.index()].count;
                    format!("{}({}→{}台)", u.name, current, count)
                })
            })
            .collect();
        if !next_unlock.is_empty() {
            eprintln!("│ 未解放UP: {}", next_unlock.join(", "));
        }

        eprintln!("└────────────────────────────────────");
    }

    /// Simulate optimal play for `total_seconds` (single run, no prestige).
    fn simulate(total_seconds: u32) {
        let mut state = CookieState::new();
        let ticks_per_second: u32 = 10;
        let clicks_per_second: u32 = 5; // Reasonable click rate

        let mut total_purchases: u32 = 0;
        let mut last_purchase_time: u32 = 0;
        let mut max_idle_gap: u32 = 0;
        let mut idle_gaps: Vec<u32> = Vec::new();

        // Report at these times (seconds)
        let report_times: Vec<u32> = vec![30, 60, 120, 300, 600, 900, 1200, 1800, 2700, 3600];
        let mut next_report_idx = 0;

        eprintln!("\n========================================");
        eprintln!("  Cookie Factory バランスシミュレーター");
        eprintln!("  プレイ時間: {}分", total_seconds / 60);
        eprintln!("  クリック速度: {}/秒", clicks_per_second);
        eprintln!("========================================\n");

        for second in 1..=total_seconds {
            // Clicks
            for _ in 0..clicks_per_second {
                logic::click(&mut state);
            }

            // Tick 1 second (10 ticks)
            logic::tick(&mut state, ticks_per_second);

            // Claim golden cookies
            logic::claim_golden(&mut state);

            // Claim all ready milestones (optimal play: claim immediately)
            logic::claim_all_milestones(&mut state);

            // Try to buy things (greedy: buy best ROI until can't afford anything)
            let mut bought_this_second = false;
            for _ in 0..20 {
                // Safety limit
                match find_best_purchase(&state) {
                    Some(Purchase::Producer(kind)) => {
                        if logic::buy_producer(&mut state, &kind) {
                            bought_this_second = true;
                            total_purchases += 1;
                        } else {
                            break;
                        }
                    }
                    Some(Purchase::Upgrade(idx)) => {
                        if logic::buy_upgrade(&mut state, idx) {
                            bought_this_second = true;
                            total_purchases += 1;
                        } else {
                            break;
                        }
                    }
                    None => break,
                }
            }

            if bought_this_second {
                let gap = second - last_purchase_time;
                if gap > 1 {
                    idle_gaps.push(gap);
                    if gap > max_idle_gap {
                        max_idle_gap = gap;
                    }
                }
                last_purchase_time = second;
            }

            // Report at intervals
            if next_report_idx < report_times.len() && second >= report_times[next_report_idx] {
                report_stats(&state, second, total_purchases);
                next_report_idx += 1;
            }
        }

        // Final report
        eprintln!("\n======== 最終サマリー ========");
        report_stats(&state, total_seconds, total_purchases);

        // Idle gap analysis
        eprintln!("\n--- 購入間隔分析 ---");
        eprintln!("総購入回数: {}", total_purchases);
        eprintln!("最大待ち時間: {}秒", max_idle_gap);
        let long_gaps: Vec<&u32> = idle_gaps.iter().filter(|g| **g >= 10).collect();
        eprintln!("10秒以上の待ち: {}回", long_gaps.len());
        let very_long_gaps: Vec<&u32> = idle_gaps.iter().filter(|g| **g >= 30).collect();
        eprintln!("30秒以上の待ち: {}回", very_long_gaps.len());

        if !idle_gaps.is_empty() {
            let avg_gap: f64 = idle_gaps.iter().map(|g| *g as f64).sum::<f64>() / idle_gaps.len() as f64;
            eprintln!("平均待ち時間: {:.1}秒", avg_gap);
        }

        // Remaining upgrades
        let unpurchased: Vec<&str> = state
            .upgrades
            .iter()
            .filter(|u| !u.purchased)
            .map(|u| u.name.as_str())
            .collect();
        eprintln!("未購入UP: {:?}", unpurchased);
        eprintln!("==============================\n");
    }

    /// Simulate optimal play with automatic prestige decisions.
    /// Prestiges when pending chips > 50% of current chips (meaningful boost).
    fn simulate_with_prestige(total_seconds: u32) {
        let mut state = CookieState::new();
        let ticks_per_second: u32 = 10;
        let clicks_per_second: u32 = 5;

        let mut total_purchases: u32 = 0;
        let mut prestige_log: Vec<(u32, u64, f64, f64)> = Vec::new(); // (second, chips, multiplier, all_time)
        let mut run_start_second: u32 = 0;

        // Report at these times
        let report_times: Vec<u32> = vec![300, 600, 900, 1200, 1800, 2700, 3600, 5400, 7200];
        let mut next_report_idx = 0;

        eprintln!("\n========================================");
        eprintln!("  Cookie Factory 転生シミュレーター");
        eprintln!("  プレイ時間: {}分", total_seconds / 60);
        eprintln!("  クリック速度: {}/秒", clicks_per_second);
        eprintln!("  転生条件: 待機チップ > 現チップ×50%");
        eprintln!("========================================\n");

        for second in 1..=total_seconds {
            // Clicks
            for _ in 0..clicks_per_second {
                logic::click(&mut state);
            }

            // Tick
            logic::tick(&mut state, ticks_per_second);

            // Claim golden cookies
            logic::claim_golden(&mut state);

            // Claim milestones
            logic::claim_all_milestones(&mut state);

            // Buy research (prefer mass production path for simulation)
            for idx in 0..state.research_nodes.len() {
                if !state.research_nodes[idx].purchased
                    && state.research_nodes[idx].path == ResearchPath::MassProduction
                {
                    logic::buy_research(&mut state, idx);
                }
            }

            // Try to buy things
            for _ in 0..20 {
                match find_best_purchase(&state) {
                    Some(Purchase::Producer(kind)) => {
                        if logic::buy_producer(&mut state, &kind) {
                            total_purchases += 1;
                        } else {
                            break;
                        }
                    }
                    Some(Purchase::Upgrade(idx)) => {
                        if logic::buy_upgrade(&mut state, idx) {
                            total_purchases += 1;
                        } else {
                            break;
                        }
                    }
                    None => break,
                }
            }

            // Check if we should prestige
            let pending = state.pending_heavenly_chips();
            let current = state.heavenly_chips;
            let run_duration = second - run_start_second;
            // Prestige when:
            // - At least 1 pending chip
            // - Run has lasted at least 120 seconds (don't prestige too fast)
            // - Pending chips > 50% of current (meaningful boost), OR first prestige with > 0
            let should_prestige = pending > 0
                && run_duration >= 120
                && (current == 0 || pending as f64 > current as f64 * 0.5);

            if should_prestige {
                let total_all = state.cookies_all_runs + state.cookies_all_time;
                eprintln!(
                    "🌟 [{}分{}秒] 転生実行！ 待機チップ: {} → 合計: {} 全時間クッキー: {}",
                    second / 60,
                    second % 60,
                    pending,
                    current + pending,
                    logic::format_number(total_all),
                );

                let new_chips = logic::perform_prestige(&mut state);

                // Buy prestige upgrades (prioritize production path)
                let upgrade_order = [
                    "angels_gift",
                    "heavenly_power",
                    "angels_aura",
                    "factory_memory",
                    "efficiency_peak",
                    "heavenly_wealth",
                    "angels_click",
                    "gods_click",
                    "golden_rush",
                    "golden_intuition",
                    "sugar_alchemy",
                    "luck_extension",
                    "combo_mastery",
                    "click_sovereign",
                    "milk_memory",
                    "luck_sovereign",
                ];
                for id in &upgrade_order {
                    if let Some(idx) = state.prestige_upgrades.iter().position(|u| u.id == *id) {
                        logic::buy_prestige_upgrade(&mut state, idx);
                    }
                }

                prestige_log.push((second, state.heavenly_chips, state.prestige_multiplier, total_all));
                run_start_second = second;

                eprintln!(
                    "   → チップ+{} (合計{}) 倍率: x{:.2}  購入済転生UP: {:?}",
                    new_chips,
                    state.heavenly_chips,
                    state.prestige_multiplier,
                    state.prestige_upgrades.iter().filter(|u| u.purchased).map(|u| u.name.as_str()).collect::<Vec<_>>(),
                );
            }

            // Report at intervals
            if next_report_idx < report_times.len() && second >= report_times[next_report_idx] {
                report_stats(&state, second, total_purchases);
                next_report_idx += 1;
            }
        }

        // Final report
        eprintln!("\n======== 転生シミュレーション最終結果 ========");
        report_stats(&state, total_seconds, total_purchases);

        eprintln!("\n--- 転生履歴 ---");
        for (i, (sec, chips, mult, all_time)) in prestige_log.iter().enumerate() {
            eprintln!(
                "  転生{}: {}分{}秒  チップ合計: {}  倍率: x{:.2}  全時間: {}",
                i + 1,
                sec / 60,
                sec % 60,
                chips,
                mult,
                logic::format_number(*all_time),
            );
        }
        eprintln!("  最終転生回数: {}", state.prestige_count);
        eprintln!("  最終チップ: {} (使用: {}  残: {})", state.heavenly_chips, state.heavenly_chips_spent, state.available_chips());
        eprintln!("  最終倍率: x{:.2}", state.prestige_multiplier);
        eprintln!("  砂糖: {} (全時間: {})", state.sugar, state.sugar_all_time);
        eprintln!("  購入済転生UP: {:?}", state.prestige_upgrades.iter().filter(|u| u.purchased).map(|u| u.name.as_str()).collect::<Vec<_>>());
        eprintln!("=============================================\n");
    }

    #[test]
    fn simulate_optimal_1hour() {
        simulate(3600);
    }

    #[test]
    fn simulate_optimal_30min() {
        simulate(1800);
    }

    #[test]
    fn simulate_prestige_2hours() {
        simulate_with_prestige(7200);
    }
}
