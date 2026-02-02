//! Cookie Factory game logic — pure functions, fully testable.

use super::state::{
    ActiveBuff, CookieState, GoldenCookieEvent, GoldenEffect, MiniEventKind, ProducerKind,
    UpgradeEffect,
};

/// Advance the game by `delta_ticks` ticks (at 10 ticks/sec).
pub fn tick(state: &mut CookieState, delta_ticks: u32) {
    if delta_ticks == 0 {
        return;
    }
    let seconds = delta_ticks as f64 / 10.0;
    let production = state.total_cps() * seconds;
    state.cookies += production;
    state.cookies_all_time += production;
    state.anim_frame = state.anim_frame.wrapping_add(delta_ticks);
    if state.click_flash > 0 {
        state.click_flash = state.click_flash.saturating_sub(delta_ticks);
    }

    // Tick active buffs
    tick_buffs(state, delta_ticks);

    // Tick golden cookie spawning
    tick_golden(state, delta_ticks);

    // Tick mini-events
    tick_mini_event(state, delta_ticks);
}

/// Tick down active buffs and remove expired ones.
fn tick_buffs(state: &mut CookieState, delta_ticks: u32) {
    for buff in &mut state.active_buffs {
        buff.ticks_left = buff.ticks_left.saturating_sub(delta_ticks);
    }
    // Log when a buff expires
    let expired: Vec<String> = state
        .active_buffs
        .iter()
        .filter(|b| b.ticks_left == 0)
        .map(|b| b.effect.description().to_string())
        .collect();
    state.active_buffs.retain(|b| b.ticks_left > 0);
    for name in expired {
        state.add_log(&format!("  {} 終了", name), false);
    }
}

/// Tick golden cookie spawning and expiration.
fn tick_golden(state: &mut CookieState, delta_ticks: u32) {
    // Only spawn golden cookies once the player has any CPS
    if state.total_cps() <= 0.0 {
        return;
    }

    // Handle existing golden cookie expiration
    if let Some(ref mut event) = state.golden_event {
        event.appear_ticks_left = event.appear_ticks_left.saturating_sub(delta_ticks);
        if event.appear_ticks_left == 0 && !event.claimed {
            state.golden_event = None;
            state.add_log("  ゴールデンクッキーが消えた…", false);
            // Schedule next spawn
            let delay = random_spawn_delay(state);
            state.golden_next_spawn = delay;
        }
        return;
    }

    // Count down to next spawn
    state.golden_next_spawn = state.golden_next_spawn.saturating_sub(delta_ticks);
    if state.golden_next_spawn == 0 {
        // Spawn a golden cookie! Visible for 10 seconds (100 ticks).
        state.golden_event = Some(GoldenCookieEvent {
            appear_ticks_left: 100,
            claimed: false,
        });
        state.add_log("✦ ゴールデンクッキー出現！[G]で取得！", true);
    }
}

/// Generate a random spawn delay between 30-90 seconds (300-900 ticks).
fn random_spawn_delay(state: &mut CookieState) -> u32 {
    let r = state.next_random();
    300 + (r % 600) // 300..900 ticks = 30..90 seconds
}

/// Claim a golden cookie event. Returns true if successful.
pub fn claim_golden(state: &mut CookieState) -> bool {
    let event = match &state.golden_event {
        Some(e) if !e.claimed => e.clone(),
        _ => return false,
    };
    let _ = event; // just needed for the check

    // Pick a random effect
    let effect = pick_golden_effect(state);

    // Apply the effect
    match &effect {
        GoldenEffect::ProductionFrenzy { .. } => {
            state.active_buffs.push(ActiveBuff {
                effect: effect.clone(),
                ticks_left: 70, // 7 seconds
            });
            state.add_log(&format!("🍪 {} (7秒)", effect.detail()), true);
        }
        GoldenEffect::ClickFrenzy { .. } => {
            state.active_buffs.push(ActiveBuff {
                effect: effect.clone(),
                ticks_left: 100, // 10 seconds
            });
            state.add_log(&format!("🍪 {} (10秒)", effect.detail()), true);
        }
        GoldenEffect::InstantBonus { cps_seconds } => {
            let bonus = state.total_cps() * cps_seconds;
            state.cookies += bonus;
            state.cookies_all_time += bonus;
            state.add_log(
                &format!("🍪 {} (+{})", effect.detail(), format_number(bonus)),
                true,
            );
        }
    }

    state.golden_event = None;
    state.golden_cookies_claimed += 1;

    // Schedule next golden cookie
    let delay = random_spawn_delay(state);
    state.golden_next_spawn = delay;

    true
}

/// Pick a random golden effect.
fn pick_golden_effect(state: &mut CookieState) -> GoldenEffect {
    let r = state.next_random() % 100;
    if r < 40 {
        GoldenEffect::ProductionFrenzy { multiplier: 7.0 }
    } else if r < 70 {
        GoldenEffect::ClickFrenzy { multiplier: 10.0 }
    } else {
        GoldenEffect::InstantBonus { cps_seconds: 10.0 }
    }
}

/// Tick mini-event timer and auto-fire events.
fn tick_mini_event(state: &mut CookieState, delta_ticks: u32) {
    // Only fire mini-events once the player has some CPS
    if state.total_cps() <= 0.0 {
        return;
    }

    state.mini_event_next = state.mini_event_next.saturating_sub(delta_ticks);
    if state.mini_event_next == 0 {
        let event = pick_mini_event(state);
        apply_mini_event(state, &event);

        // Schedule next: 15-30 seconds (150-300 ticks)
        let delay = 150 + (state.next_random() % 150);
        state.mini_event_next = delay;
    }
}

/// Pick a random mini-event based on game state.
fn pick_mini_event(state: &mut CookieState) -> MiniEventKind {
    let r = state.next_random() % 100;
    if r < 30 {
        MiniEventKind::LuckyDrop { cps_seconds: 3.0 }
    } else if r < 50 {
        MiniEventKind::SugarRush { multiplier: 5.0 }
    } else if r < 80 {
        // Pick a random active producer for the surge
        let active_producers: Vec<ProducerKind> = state
            .producers
            .iter()
            .filter(|p| p.count > 0)
            .map(|p| p.kind.clone())
            .collect();
        if active_producers.is_empty() {
            MiniEventKind::LuckyDrop { cps_seconds: 3.0 }
        } else {
            let idx = state.next_random() as usize % active_producers.len();
            MiniEventKind::ProductionSurge {
                target: active_producers[idx].clone(),
                multiplier: 3.0,
            }
        }
    } else {
        MiniEventKind::DiscountWave { discount: 0.25 }
    }
}

/// Apply a mini-event's effect to the game state.
fn apply_mini_event(state: &mut CookieState, event: &MiniEventKind) {
    let desc = event.description();
    match event {
        MiniEventKind::LuckyDrop { cps_seconds } => {
            let bonus = state.total_cps() * cps_seconds;
            state.cookies += bonus;
            state.cookies_all_time += bonus;
            state.add_log(
                &format!("{} (+{})", desc, format_number(bonus)),
                true,
            );
        }
        MiniEventKind::SugarRush { multiplier } => {
            state.active_buffs.push(ActiveBuff {
                effect: GoldenEffect::ClickFrenzy {
                    multiplier: *multiplier,
                },
                ticks_left: 50, // 5 seconds
            });
            state.add_log(&desc, true);
        }
        MiniEventKind::ProductionSurge { multiplier, .. } => {
            state.active_buffs.push(ActiveBuff {
                effect: GoldenEffect::ProductionFrenzy {
                    multiplier: *multiplier,
                },
                ticks_left: 100, // 10 seconds
            });
            state.add_log(&desc, true);
        }
        MiniEventKind::DiscountWave { discount } => {
            state.active_discount = *discount;
            state.add_log(&desc, true);
        }
    }
}

/// Manual click: add cookies_per_click to cookies (with buffs).
pub fn click(state: &mut CookieState) {
    let power = state.effective_click_power();
    state.cookies += power;
    state.cookies_all_time += power;
    state.total_clicks += 1;
    state.click_flash = 3; // flash for 3 ticks
}

/// Try to buy a producer by kind. Returns true if successful.
pub fn buy_producer(state: &mut CookieState, kind: &ProducerKind) -> bool {
    let idx = state.producers.iter().position(|p| p.kind == *kind);
    let idx = match idx {
        Some(i) => i,
        None => return false,
    };

    let base_cost = state.producers[idx].cost();
    let cost = base_cost * (1.0 - state.active_discount);
    if state.cookies >= cost {
        state.cookies -= cost;
        state.producers[idx].count += 1;
        let had_discount = state.active_discount > 0.0;
        if had_discount {
            state.add_log(
                &format!(
                    "{} を購入！ ({}台) 💰割引適用！",
                    state.producers[idx].kind.name(),
                    state.producers[idx].count
                ),
                false,
            );
            state.active_discount = 0.0;
        } else {
            state.add_log(
                &format!(
                    "{} を購入！ ({}台)",
                    state.producers[idx].kind.name(),
                    state.producers[idx].count
                ),
                false,
            );
        }
        true
    } else {
        false
    }
}

/// Try to buy an upgrade by index. Returns true if successful.
pub fn buy_upgrade(state: &mut CookieState, upgrade_idx: usize) -> bool {
    if upgrade_idx >= state.upgrades.len() {
        return false;
    }
    if state.upgrades[upgrade_idx].purchased {
        return false;
    }
    let base_cost = state.upgrades[upgrade_idx].cost;
    let cost = base_cost * (1.0 - state.active_discount);
    if state.cookies < cost {
        return false;
    }

    // Check unlock condition
    let unlocked = state.is_upgrade_unlocked(&state.upgrades[upgrade_idx]);
    if !unlocked {
        return false;
    }

    state.cookies -= cost;
    if state.active_discount > 0.0 {
        state.active_discount = 0.0;
    }
    state.upgrades[upgrade_idx].purchased = true;

    let effect = state.upgrades[upgrade_idx].effect.clone();
    let name = state.upgrades[upgrade_idx].name.clone();

    apply_upgrade_effect(state, &effect, &name);

    true
}

/// Apply an upgrade's effect to the game state.
fn apply_upgrade_effect(state: &mut CookieState, effect: &UpgradeEffect, name: &str) {
    match effect {
        UpgradeEffect::ClickPower(amount) => {
            state.cookies_per_click += amount;
            state.add_log(
                &format!("✦ {} 適用！クリック+{}", name, amount),
                true,
            );
        }
        UpgradeEffect::ProducerMultiplier { target, multiplier } => {
            if let Some(p) = state.producers.iter_mut().find(|p| p.kind == *target) {
                p.multiplier *= multiplier;
            }
            state.add_log(&format!("✦ {} 適用！", name), true);
        }
        UpgradeEffect::SynergyBoost { .. } => {
            state.synergy_multiplier *= 2.0;
            state.add_log(&format!("✦ {} 適用！全シナジー2倍！", name), true);
        }
        UpgradeEffect::CrossSynergy {
            source,
            target,
            bonus_per_unit,
        } => {
            state.cross_synergies.push((
                source.clone(),
                target.clone(),
                *bonus_per_unit,
            ));
            state.add_log(&format!("✦ {} 適用！新シナジー追加！", name), true);
        }
        UpgradeEffect::CountScaling { target, bonus_per_unit } => {
            state.count_scalings.push((target.clone(), *bonus_per_unit));
            state.add_log(
                &format!("✦ {} 適用！台数ボーナス追加！", name),
                true,
            );
        }
        UpgradeEffect::CpsPercentBonus { target, percentage } => {
            state.cps_percent_bonuses.push((target.clone(), *percentage));
            state.add_log(
                &format!("✦ {} 適用！CPS連動ボーナス！", name),
                true,
            );
        }
    }
}

/// Format a number with commas (e.g. 1234567 → "1,234,567").
pub fn format_number(n: f64) -> String {
    if n < 0.0 {
        return format!("-{}", format_number(-n));
    }
    let int_part = n.floor() as u64;
    let frac = n - int_part as f64;

    let s = int_part.to_string();
    let mut result = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }
    let result: String = result.chars().rev().collect();

    if frac > 0.05 {
        format!("{}.{}", result, ((frac * 10.0).round() as u8))
    } else {
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tick_produces_cookies() {
        let mut state = CookieState::new();
        state.producers[0].count = 10; // 10 cursors = 1.0 cps base
        tick(&mut state, 10); // 1 second
        // With synergies (0 grandmas → 0 bonus), should be ~1.0
        assert!((state.cookies - 1.0).abs() < 0.01);
    }

    #[test]
    fn tick_zero_does_nothing() {
        let mut state = CookieState::new();
        state.producers[0].count = 10;
        tick(&mut state, 0);
        assert!((state.cookies - 0.0).abs() < 0.001);
    }

    #[test]
    fn tick_multiple_producers() {
        let mut state = CookieState::new();
        state.producers[0].count = 10; // 1.0 cps base
        state.producers[1].count = 3;  // 3.0 cps base
        tick(&mut state, 10); // 1 second
        // With synergies: cursor gets +3% from 3 grandmas = 1.0*1.03 = 1.03
        // Grandma gets 0% (0 farms) = 3.0
        let expected = 1.03 + 3.0;
        assert!((state.cookies - expected).abs() < 0.01);
    }

    #[test]
    fn tick_100_ticks_idle() {
        let mut state = CookieState::new();
        state.producers[1].count = 5; // 5 grandmas = 5.0 cps base
        tick(&mut state, 100); // 10 seconds
        assert!((state.cookies - 50.0).abs() < 0.1);
    }

    #[test]
    fn click_adds_cookies() {
        let mut state = CookieState::new();
        click(&mut state);
        assert!((state.cookies - 1.0).abs() < 0.001);
        assert_eq!(state.total_clicks, 1);
    }

    #[test]
    fn click_respects_per_click() {
        let mut state = CookieState::new();
        state.cookies_per_click = 5.0;
        click(&mut state);
        assert!((state.cookies - 5.0).abs() < 0.001);
    }

    #[test]
    fn click_with_buff() {
        let mut state = CookieState::new();
        state.cookies_per_click = 2.0;
        state.active_buffs.push(ActiveBuff {
            effect: GoldenEffect::ClickFrenzy { multiplier: 10.0 },
            ticks_left: 100,
        });
        click(&mut state);
        assert!((state.cookies - 20.0).abs() < 0.001);
    }

    #[test]
    fn buy_producer_success() {
        let mut state = CookieState::new();
        state.cookies = 100.0;
        assert!(buy_producer(&mut state, &ProducerKind::Cursor));
        assert_eq!(state.producers[0].count, 1);
        assert!((state.cookies - (100.0 - 15.0)).abs() < 0.01);
    }

    #[test]
    fn buy_producer_insufficient_funds() {
        let mut state = CookieState::new();
        state.cookies = 10.0;
        assert!(!buy_producer(&mut state, &ProducerKind::Cursor));
        assert_eq!(state.producers[0].count, 0);
        assert!((state.cookies - 10.0).abs() < 0.001);
    }

    #[test]
    fn buy_producer_cost_increases() {
        let mut state = CookieState::new();
        state.cookies = 1000.0;
        buy_producer(&mut state, &ProducerKind::Cursor);
        let cost_after_1 = state.producers[0].cost();
        let expected = 15.0 * 1.15;
        assert!((cost_after_1 - expected).abs() < 0.01);
    }

    #[test]
    fn buy_upgrade_success() {
        let mut state = CookieState::new();
        state.cookies = 200.0;
        // Upgrade index 0 is "強化クリック" (cost 100)
        assert!(buy_upgrade(&mut state, 0));
        assert!(state.upgrades[0].purchased);
        assert!((state.cookies_per_click - 2.0).abs() < 0.001);
    }

    #[test]
    fn buy_upgrade_multiplier() {
        let mut state = CookieState::new();
        state.cookies = 300.0;
        state.producers[0].count = 5;
        // Upgrade index 1 is "Cursor x2" (cost 200)
        assert!(buy_upgrade(&mut state, 1));
        assert!((state.producers[0].multiplier - 2.0).abs() < 0.001);
        // CPS should double
        assert!((state.producers[0].cps() - 1.0).abs() < 0.001); // 5 * 0.1 * 2.0
    }

    #[test]
    fn buy_upgrade_already_purchased() {
        let mut state = CookieState::new();
        state.cookies = 500.0;
        buy_upgrade(&mut state, 0);
        assert!(!buy_upgrade(&mut state, 0));
    }

    #[test]
    fn buy_upgrade_insufficient_funds() {
        let mut state = CookieState::new();
        state.cookies = 50.0;
        assert!(!buy_upgrade(&mut state, 0));
    }

    #[test]
    fn buy_upgrade_locked() {
        let mut state = CookieState::new();
        state.cookies = 100_000.0;
        // Index 6 = "おばあちゃんの知恵", needs Grandma >= 5
        assert!(!buy_upgrade(&mut state, 6));
        state.producers[1].count = 5;
        assert!(buy_upgrade(&mut state, 6));
    }

    #[test]
    fn buy_cross_synergy_upgrade() {
        let mut state = CookieState::new();
        state.cookies = 100_000.0;
        state.producers[1].count = 10; // 10 grandmas for unlock + synergy
        buy_upgrade(&mut state, 6); // "おばあちゃんの知恵"
        assert_eq!(state.cross_synergies.len(), 1);
        // Now cursor should get additional +1% per grandma = +10% extra
        // Base synergy: 10% + cross synergy: 10% = 20%
        let bonus = state.synergy_bonus(&ProducerKind::Cursor);
        assert!((bonus - 0.20).abs() < 0.001);
    }

    #[test]
    fn synergy_boost_upgrade() {
        let mut state = CookieState::new();
        state.cookies = 10_000_000.0;
        state.producers[4].count = 10; // 10 factories for unlock
        // Index 15 = "シナジー倍化"
        let synergy_idx = state
            .upgrades
            .iter()
            .position(|u| u.name == "シナジー倍化")
            .unwrap();
        buy_upgrade(&mut state, synergy_idx);
        assert!((state.synergy_multiplier - 2.0).abs() < 0.001);
    }

    #[test]
    fn cookies_all_time_tracks_total() {
        let mut state = CookieState::new();
        state.producers[0].count = 10;
        click(&mut state); // +1
        tick(&mut state, 10); // +~1
        assert!(state.cookies_all_time >= 1.9);

        // Spend cookies, all_time doesn't decrease
        state.cookies = 1000.0;
        buy_producer(&mut state, &ProducerKind::Cursor);
        assert!(state.cookies_all_time >= 1.9);
    }

    #[test]
    fn golden_cookie_spawns_after_delay() {
        let mut state = CookieState::new();
        state.producers[1].count = 1; // need CPS > 0
        state.golden_next_spawn = 5;
        tick(&mut state, 5);
        assert!(state.golden_event.is_some());
    }

    #[test]
    fn golden_cookie_no_spawn_without_cps() {
        let mut state = CookieState::new();
        state.golden_next_spawn = 1;
        tick(&mut state, 5);
        assert!(state.golden_event.is_none());
    }

    #[test]
    fn claim_golden_cookie() {
        let mut state = CookieState::new();
        state.producers[1].count = 5;
        state.golden_event = Some(super::super::state::GoldenCookieEvent {
            appear_ticks_left: 50,
            claimed: false,
        });
        assert!(claim_golden(&mut state));
        assert!(state.golden_event.is_none());
        assert_eq!(state.golden_cookies_claimed, 1);
    }

    #[test]
    fn claim_golden_no_event() {
        let mut state = CookieState::new();
        assert!(!claim_golden(&mut state));
    }

    #[test]
    fn buff_expires() {
        let mut state = CookieState::new();
        state.active_buffs.push(ActiveBuff {
            effect: GoldenEffect::ProductionFrenzy { multiplier: 7.0 },
            ticks_left: 10,
        });
        tick(&mut state, 10);
        assert!(state.active_buffs.is_empty());
    }

    #[test]
    fn format_number_basic() {
        assert_eq!(format_number(0.0), "0");
        assert_eq!(format_number(123.0), "123");
        assert_eq!(format_number(1234.0), "1,234");
        assert_eq!(format_number(1234567.0), "1,234,567");
    }

    #[test]
    fn format_number_with_fraction() {
        assert_eq!(format_number(12.5), "12.5");
    }

    #[test]
    fn count_scaling_upgrade_effect() {
        let mut state = CookieState::new();
        state.cookies = 10_000_000.0;
        state.producers[0].count = 50; // 50 cursors
        // Manually apply CountScaling
        state.count_scalings.push((ProducerKind::Cursor, 0.005));
        // Each cursor gives +0.5% → 50 * 0.5% = 25% bonus
        let cs_bonus = state.count_scaling_bonus(&ProducerKind::Cursor);
        assert!((cs_bonus - 0.25).abs() < 0.001);
    }

    #[test]
    fn cps_percent_bonus_effect() {
        let mut state = CookieState::new();
        state.producers[1].count = 10; // 10 grandmas = 10 CPS base
        let base_cps = state.total_cps();
        assert!((base_cps - 10.0).abs() < 0.01);
        // Add CPS percent bonus: each grandma adds 0.01% of total CPS
        state.cps_percent_bonuses.push((ProducerKind::Grandma, 0.0001));
        let new_cps = state.total_cps();
        // Extra = 10.0 (base) * 10 (grandmas) * 0.0001 = 0.01
        assert!(new_cps > base_cps);
        assert!((new_cps - 10.01).abs() < 0.01);
    }

    #[test]
    fn mini_event_fires_after_countdown() {
        let mut state = CookieState::new();
        state.producers[1].count = 5; // Need CPS > 0
        state.mini_event_next = 5;
        let log_len_before = state.log.len();
        tick(&mut state, 5);
        // A mini-event should have fired and added a log entry
        assert!(state.log.len() > log_len_before);
    }

    #[test]
    fn mini_event_no_fire_without_cps() {
        let mut state = CookieState::new();
        state.mini_event_next = 1;
        let log_len_before = state.log.len();
        tick(&mut state, 5);
        // No mini-event without CPS (only the golden check logs nothing either)
        assert_eq!(state.log.len(), log_len_before);
    }

    #[test]
    fn discount_applies_to_producer_purchase() {
        let mut state = CookieState::new();
        state.cookies = 12.0; // Less than Cursor cost (15), but 25% off = 11.25
        state.active_discount = 0.25;
        assert!(buy_producer(&mut state, &ProducerKind::Cursor));
        assert_eq!(state.producers[0].count, 1);
        assert!((state.cookies - (12.0 - 11.25)).abs() < 0.01);
        // Discount should be consumed
        assert!((state.active_discount - 0.0).abs() < 0.001);
    }

    #[test]
    fn discount_applies_to_upgrade_purchase() {
        let mut state = CookieState::new();
        state.cookies = 80.0; // Less than 100 (強化クリック cost), but 25% off = 75
        state.active_discount = 0.25;
        assert!(buy_upgrade(&mut state, 0));
        assert!(state.upgrades[0].purchased);
        assert!((state.cookies - 5.0).abs() < 0.01); // 80 - 75 = 5
        assert!((state.active_discount - 0.0).abs() < 0.001);
    }
}
