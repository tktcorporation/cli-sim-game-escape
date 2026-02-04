//! Cookie Factory game logic — pure functions, fully testable.

use super::state::{
    ActiveBuff, CookieState, DragonAura, GoldenCookieEvent, GoldenEffect, MarketPhase,
    MilestoneCondition, MilestoneStatus, MiniEventKind, Particle, ParticleStyle, ProducerKind,
    ResearchPath, UpgradeEffect,
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
    state.total_ticks += delta_ticks as u64;

    // Update statistics
    let current_cps = state.total_cps();
    if current_cps > state.best_cps {
        state.best_cps = current_cps;
    }

    // Track cookies earned in this window
    state.cookies_earned_window += production;

    // Sample CPS history every 10 ticks (1 second)
    state.cps_sample_counter += delta_ticks;
    if state.cps_sample_counter >= 10 {
        state.cps_sample_counter = 0;
        state.cps_delta = current_cps - state.prev_cps;
        state.prev_cps = current_cps;
        state.cps_history.push(current_cps);
        if state.cps_history.len() > 40 {
            state.cps_history.remove(0);
        }
        // Track peak per-second
        if state.cookies_earned_window > state.peak_cookies_per_sec {
            state.peak_cookies_per_sec = state.cookies_earned_window;
        }
        state.cookies_earned_window = 0.0;
    }

    if state.click_flash > 0 {
        state.click_flash = state.click_flash.saturating_sub(delta_ticks);
    }
    if state.purchase_flash > 0 {
        state.purchase_flash = state.purchase_flash.saturating_sub(delta_ticks);
    }

    // Combo cooldown: if no click within 15 ticks (1.5s), reset combo
    state.click_cooldown += delta_ticks;
    if state.click_cooldown > 15 {
        state.combo_count = 0;
    }

    // Update particles
    for p in &mut state.particles {
        p.life = p.life.saturating_sub(delta_ticks);
    }
    state.particles.retain(|p| p.life > 0);

    // Ambient sparkle particles when producing
    if current_cps > 1.0 {
        spawn_ambient_particles(state, delta_ticks);
    }

    // Tick active buffs
    tick_buffs(state, delta_ticks);

    // Tick golden cookie spawning
    tick_golden(state, delta_ticks);

    // Tick mini-events
    tick_mini_event(state, delta_ticks);

    // Tick market phase
    tick_market(state, delta_ticks);

    // Check milestones
    check_milestones(state);

    // Tick milestone flash
    if state.milestone_flash > 0 {
        state.milestone_flash = state.milestone_flash.saturating_sub(delta_ticks);
    }

    // Tick prestige flash
    if state.prestige_flash > 0 {
        state.prestige_flash = state.prestige_flash.saturating_sub(delta_ticks);
    }
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
/// Prestige upgrades (GoldenCookieSpeed) can reduce this.
fn random_spawn_delay(state: &mut CookieState) -> u32 {
    let r = state.next_random();
    let base = 300 + (r % 600); // 300..900 ticks = 30..90 seconds
    let speed_factor: f64 = state
        .prestige_upgrades
        .iter()
        .filter(|u| u.purchased)
        .filter_map(|u| {
            if let super::state::PrestigeEffect::GoldenCookieSpeed(f) = &u.effect {
                Some(*f)
            } else {
                None
            }
        })
        .product();
    let speed_factor = if speed_factor > 0.0 { speed_factor } else { 1.0 };
    // Dragon aura can also speed up golden cookie spawning
    let dragon_speed = state.dragon_golden_speed();
    (base as f64 * speed_factor * dragon_speed).max(100.0) as u32 // minimum 10 seconds
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

    // Apply the effect (research can extend buff duration)
    let buff_dur_mult = state.research_buff_duration();
    match &effect {
        GoldenEffect::ProductionFrenzy { .. } => {
            let ticks = (70.0 * buff_dur_mult) as u32;
            state.active_buffs.push(ActiveBuff {
                effect: effect.clone(),
                ticks_left: ticks,
            });
            state.add_log(&format!("🍪 {} ({:.0}秒)", effect.detail(), ticks as f64 / 10.0), true);
        }
        GoldenEffect::ClickFrenzy { .. } => {
            let ticks = (100.0 * buff_dur_mult) as u32;
            state.active_buffs.push(ActiveBuff {
                effect: effect.clone(),
                ticks_left: ticks,
            });
            state.add_log(&format!("🍪 {} ({:.0}秒)", effect.detail(), ticks as f64 / 10.0), true);
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
    spawn_celebration(state, 5); // golden cookie celebration

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

/// Click particle accents — golden theme only.
const CLICK_ACCENTS: &[&str] = &["✦", "✧", "·"];

/// Celebration accents — reserved for big moments.
const CELEBRATION_ACCENTS: &[&str] = &["✦", "✧", "🍪", "·"];

/// Sparkle characters for ambient particles — subtle, small.
const SPARKLE_CHARS: &[&str] = &["·", "✧", "˚", "°"];

/// Manual click: add cookies_per_click to cookies (with buffs) + spawn particles.
pub fn click(state: &mut CookieState) {
    let power = state.effective_click_power();
    state.cookies += power;
    state.cookies_all_time += power;
    state.total_clicks += 1;
    state.click_flash = 3; // flash for 3 ticks

    // Update combo
    state.click_cooldown = 0;
    state.combo_count += 1;
    if state.combo_count > state.best_combo {
        state.best_combo = state.combo_count;
    }

    let combo = state.combo_count;

    // Main "+N" particle
    let col_offset = (state.next_random() % 13) as i16 - 6;
    let life = 8 + (state.next_random() % 5);
    let text = if power >= 10.0 {
        format!("+{}", format_number(power))
    } else {
        format!("+{}", power as u32)
    };
    state.particles.push(Particle {
        text,
        col_offset,
        life,
        max_life: life,
        style: ParticleStyle::Click,
        row_offset: 0,
    });

    // Subtle accent particle on higher combos (1 at most)
    if combo >= 5 {
        let idx = state.next_random() as usize % CLICK_ACCENTS.len();
        let col = (state.next_random() % 11) as i16 - 5;
        let life = 6 + (state.next_random() % 4);
        state.particles.push(Particle {
            text: CLICK_ACCENTS[idx].to_string(),
            col_offset: col,
            life,
            max_life: life,
            style: ParticleStyle::Emoji,
            row_offset: 0,
        });
    }

    // Combo milestone indicator — concise, elegant
    if combo >= 10 && combo.is_multiple_of(10) {
        let combo_text = format!("── {}連打 ──", combo);
        state.particles.push(Particle {
            text: combo_text,
            col_offset: 0,
            life: 12,
            max_life: 12,
            style: ParticleStyle::Combo,
            row_offset: -2,
        });
    }

    // Cap particles to avoid memory issues
    while state.particles.len() > 30 {
        state.particles.remove(0);
    }
}

/// Spawn ambient sparkle particles during production.
fn spawn_ambient_particles(state: &mut CookieState, _delta_ticks: u32) {
    // Spawn rate: subtle, scales with CPS but never overwhelming
    let cps = state.total_cps();
    let spawn_chance = if cps > 10000.0 {
        8
    } else if cps > 1000.0 {
        12
    } else if cps > 100.0 {
        18
    } else {
        30
    };

    if state.anim_frame.is_multiple_of(spawn_chance) {
        let idx = state.next_random() as usize % SPARKLE_CHARS.len();
        let col = (state.next_random() % 25) as i16 - 12;
        let row = (state.next_random() % 7) as i16 - 3;
        let life = 3 + (state.next_random() % 4);
        state.particles.push(Particle {
            text: SPARKLE_CHARS[idx].to_string(),
            col_offset: col,
            life,
            max_life: life,
            style: ParticleStyle::Sparkle,
            row_offset: row,
        });
    }

    // Cap particles
    while state.particles.len() > 30 {
        state.particles.remove(0);
    }
}

/// Spawn a burst of celebration particles (golden cookies, upgrades, milestones).
pub fn spawn_celebration(state: &mut CookieState, count: u32) {
    for _ in 0..count {
        let idx = state.next_random() as usize % CELEBRATION_ACCENTS.len();
        let col = (state.next_random() % 15) as i16 - 7;
        let row = (state.next_random() % 5) as i16 - 2;
        let life = 8 + (state.next_random() % 6);
        state.particles.push(Particle {
            text: CELEBRATION_ACCENTS[idx].to_string(),
            col_offset: col,
            life,
            max_life: life,
            style: ParticleStyle::Celebration,
            row_offset: row,
        });
    }
    while state.particles.len() > 30 {
        state.particles.remove(0);
    }
}

/// Try to buy a producer by kind. Returns true if successful.
pub fn buy_producer(state: &mut CookieState, kind: &ProducerKind) -> bool {
    let idx = state.producers.iter().position(|p| p.kind == *kind);
    let idx = match idx {
        Some(i) => i,
        None => return false,
    };

    let base_cost = state.producers[idx].cost();
    let cost = base_cost * state.total_cost_modifier();
    if state.cookies >= cost {
        state.cookies -= cost;
        state.producers[idx].count += 1;
        state.purchase_flash = 5; // flash for 5 ticks (0.5s)
        let modifier = state.total_cost_modifier();
        if modifier < 0.99 {
            state.add_log(
                &format!(
                    "{} を購入！ ({}台) 💰割引適用！",
                    state.producers[idx].kind.name(),
                    state.producers[idx].count
                ),
                false,
            );
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
        // Consume active discount after purchase
        if state.active_discount > 0.0 {
            state.active_discount = 0.0;
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
    let cost = base_cost * state.total_cost_modifier();
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
    state.purchase_flash = 8; // longer flash for upgrades (0.8s)
    spawn_celebration(state, 3); // subtle celebration

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
        UpgradeEffect::KittenBoost { multiplier } => {
            // Recalculate kitten multiplier with the newly purchased upgrade
            recalculate_kitten_multiplier(state);
            state.add_log(
                &format!(
                    "🐱 {} 適用！ミルク{:.0}%×{:.0}%=CPS+{:.1}%",
                    name,
                    state.milk * 100.0,
                    multiplier * 100.0,
                    state.milk * multiplier * 100.0,
                ),
                true,
            );
        }
    }
}

/// Check all milestones and award newly achieved ones.
/// Check all milestones and mark newly met conditions as Ready (claimable).
pub fn check_milestones(state: &mut CookieState) {
    let cps = state.total_cps();
    let mut newly_ready = false;

    for milestone in &mut state.milestones {
        if milestone.status != MilestoneStatus::Locked {
            continue;
        }
        let met = match &milestone.condition {
            MilestoneCondition::TotalCookies(threshold) => state.cookies_all_time >= *threshold,
            MilestoneCondition::ProducerCount(kind, count) => {
                state.producers[kind.index()].count >= *count
            }
            MilestoneCondition::CpsReached(threshold) => cps >= *threshold,
            MilestoneCondition::TotalClicks(threshold) => state.total_clicks >= *threshold,
            MilestoneCondition::GoldenClaimed(threshold) => {
                state.golden_cookies_claimed >= *threshold
            }
        };
        if met {
            milestone.status = MilestoneStatus::Ready;
            newly_ready = true;
        }
    }

    if newly_ready {
        state.milestone_flash = 15; // 1.5 seconds flash to draw attention
    }
}

/// Player claims a specific ready milestone by index. Returns true if successful.
pub fn claim_milestone(state: &mut CookieState, index: usize) -> bool {
    if index >= state.milestones.len() {
        return false;
    }
    if state.milestones[index].status != MilestoneStatus::Ready {
        return false;
    }

    state.milestones[index].status = MilestoneStatus::Claimed;

    // Recalculate milk
    let achieved = state.achieved_milestone_count() as f64;
    state.milk = achieved * 0.04; // 4% milk per achievement

    // Recalculate kitten multiplier
    recalculate_kitten_multiplier(state);

    let name = state.milestones[index].name.clone();
    state.add_log(
        &format!("🏆 解放！「{}」 (ミルク: {:.0}%)", name, state.milk * 100.0),
        true,
    );
    state.milestone_flash = 15;
    spawn_celebration(state, 4);
    true
}

/// Claim all ready milestones at once. Returns count of claimed milestones.
pub fn claim_all_milestones(state: &mut CookieState) -> usize {
    let ready_indices: Vec<usize> = state.milestones.iter().enumerate()
        .filter(|(_, m)| m.status == MilestoneStatus::Ready)
        .map(|(i, _)| i)
        .collect();
    let count = ready_indices.len();
    if count == 0 {
        return 0;
    }
    for idx in &ready_indices {
        state.milestones[*idx].status = MilestoneStatus::Claimed;
    }

    let achieved = state.achieved_milestone_count() as f64;
    state.milk = achieved * 0.04;
    recalculate_kitten_multiplier(state);

    let names: Vec<String> = ready_indices.iter()
        .map(|i| state.milestones[*i].name.clone())
        .collect();
    state.add_log(
        &format!("🏆 {}個解放！「{}」 (ミルク: {:.0}%)", count, names.join("」「"), state.milk * 100.0),
        true,
    );
    state.milestone_flash = 15;
    count
}

/// Recalculate kitten_multiplier from milk and purchased kitten upgrades.
pub fn recalculate_kitten_multiplier(state: &mut CookieState) {
    let mut multiplier = 1.0;
    for upgrade in &state.upgrades {
        if upgrade.purchased {
            if let UpgradeEffect::KittenBoost { multiplier: m } = &upgrade.effect {
                // Each kitten upgrade multiplies CPS by (1 + milk * m)
                multiplier *= 1.0 + state.milk * m;
            }
        }
    }
    state.kitten_multiplier = multiplier;
}

/// Perform a prestige reset. Returns the number of new heavenly chips earned.
pub fn perform_prestige(state: &mut CookieState) -> u64 {
    let new_chips = state.pending_heavenly_chips();
    if new_chips == 0 {
        state.add_log("⚠ 転生に必要なクッキーが足りません (1兆枚以上)", true);
        return 0;
    }

    // Record statistics
    state.cookies_all_runs += state.cookies_all_time;
    state.heavenly_chips += new_chips;
    state.prestige_count += 1;
    if state.cookies_all_time > state.best_cookies_single_run {
        state.best_cookies_single_run = state.cookies_all_time;
    }

    // Calculate milk retention from prestige upgrades
    let milk_retention: f64 = state
        .prestige_upgrades
        .iter()
        .filter(|u| u.purchased)
        .filter_map(|u| {
            if let super::state::PrestigeEffect::MilkRetention(pct) = &u.effect {
                Some(*pct)
            } else {
                None
            }
        })
        .sum();
    let retained_milk = state.milk * milk_retention.min(1.0);

    // Calculate starting cookies from prestige upgrades
    let starting_cookies: f64 = state
        .prestige_upgrades
        .iter()
        .filter(|u| u.purchased)
        .filter_map(|u| {
            if let super::state::PrestigeEffect::StartingCookies(amount) = &u.effect {
                Some(*amount)
            } else {
                None
            }
        })
        .sum();

    // Recalculate prestige multiplier from chips + prestige upgrades
    let chip_bonus = 1.0 + state.heavenly_chips as f64 * 0.01;
    let upgrade_cps_mult: f64 = state
        .prestige_upgrades
        .iter()
        .filter(|u| u.purchased)
        .filter_map(|u| {
            if let super::state::PrestigeEffect::CpsMultiplier(m) = &u.effect {
                Some(*m)
            } else {
                None
            }
        })
        .product();
    state.prestige_multiplier = chip_bonus * upgrade_cps_mult;

    // Calculate click multiplier from prestige upgrades
    let click_mult: f64 = state
        .prestige_upgrades
        .iter()
        .filter(|u| u.purchased)
        .filter_map(|u| {
            if let super::state::PrestigeEffect::ClickMultiplier(m) = &u.effect {
                Some(*m)
            } else {
                None
            }
        })
        .product();

    // Reset game state (keep prestige fields)
    state.cookies = starting_cookies;
    state.cookies_all_time = starting_cookies;
    state.total_clicks = 0;
    state.cookies_per_click = 1.0 * click_mult;
    state.producers = super::state::ProducerKind::all()
        .iter()
        .map(|k| super::state::Producer::new(k.clone()))
        .collect();
    state.upgrades = CookieState::create_upgrades();
    state.log.clear();
    state.show_upgrades = false;
    state.show_milestones = false;
    state.show_prestige = false;
    state.anim_frame = 0;
    state.click_flash = 0;
    state.purchase_flash = 0;
    state.particles.clear();
    state.synergy_multiplier = 1.0;
    state.cross_synergies.clear();
    state.golden_next_spawn = 300;
    state.golden_event = None;
    state.active_buffs.clear();
    state.golden_cookies_claimed = 0;
    state.count_scalings.clear();
    state.cps_percent_bonuses.clear();
    state.mini_event_next = 150;
    state.active_discount = 0.0;
    state.milestones = CookieState::create_milestones();
    state.milk = retained_milk;
    state.milestone_flash = 0;
    state.kitten_multiplier = 1.0;
    state.prestige_flash = 30; // 3 second celebration
    state.click_cooldown = 0;
    state.combo_count = 0;
    state.cps_history.clear();
    state.cps_sample_counter = 0;
    state.cps_delta = 0.0;
    state.prev_cps = 0.0;
    state.cookies_earned_window = 0.0;
    state.peak_cookies_per_sec = 0.0;

    // Reset research (player can choose a different path next run)
    state.research_path = ResearchPath::None;
    state.research_nodes = CookieState::create_research_nodes();

    // Market continues (not reset)
    // Dragon persists (not reset)

    state.add_log(
        &format!(
            "🌟 転生！ 天国チップ+{} (合計{}) CPS×{:.2}",
            new_chips, state.heavenly_chips, state.prestige_multiplier
        ),
        true,
    );
    state.add_log("新たな旅が始まる…", true);
    spawn_celebration(state, 6); // prestige celebration

    new_chips
}

/// Buy a prestige upgrade by index. Returns true if successful.
pub fn buy_prestige_upgrade(state: &mut CookieState, index: usize) -> bool {
    if index >= state.prestige_upgrades.len() {
        return false;
    }
    if state.prestige_upgrades[index].purchased {
        return false;
    }
    let cost = state.prestige_upgrades[index].cost;
    if state.available_chips() < cost {
        return false;
    }

    state.heavenly_chips_spent += cost;
    state.prestige_upgrades[index].purchased = true;

    let name = state.prestige_upgrades[index].name.clone();
    let desc = state.prestige_upgrades[index].description.clone();
    state.add_log(
        &format!("👼 {} 購入！({})", name, desc),
        true,
    );
    state.purchase_flash = 10;

    true
}

// ═══════════════════════════════════════════════════════
// Market phase cycling
// ═══════════════════════════════════════════════════════

/// Tick the market phase timer and transition when expired.
fn tick_market(state: &mut CookieState, delta_ticks: u32) {
    state.market_ticks_left = state.market_ticks_left.saturating_sub(delta_ticks);
    if state.market_ticks_left == 0 {
        // Pick next phase (never same as current)
        let r = state.next_random() % 3;
        let next = match (&state.market_phase, r) {
            (MarketPhase::Normal, 0) => MarketPhase::Bull,
            (MarketPhase::Normal, _) => MarketPhase::Bear,
            (MarketPhase::Bull, 0) => MarketPhase::Normal,
            (MarketPhase::Bull, _) => MarketPhase::Bear,
            (MarketPhase::Bear, 0) => MarketPhase::Normal,
            (MarketPhase::Bear, _) => MarketPhase::Bull,
        };

        // Duration: 45-90 seconds (450-900 ticks)
        let duration = 450 + (state.next_random() % 450);
        state.market_phase = next.clone();
        state.market_ticks_left = duration;

        let msg = match &next {
            MarketPhase::Bull => "📈 好景気到来！CPS↑ コスト↑",
            MarketPhase::Bear => "📉 不景気…CPS↓ コスト↓",
            MarketPhase::Normal => "📊 市場安定",
        };
        state.add_log(msg, true);
    }
}

// ═══════════════════════════════════════════════════════
// Research Tree
// ═══════════════════════════════════════════════════════

/// Buy a research node by index. Returns true if successful.
/// The first purchase locks the player into that research path.
pub fn buy_research(state: &mut CookieState, index: usize) -> bool {
    if index >= state.research_nodes.len() {
        return false;
    }
    if state.research_nodes[index].purchased {
        return false;
    }

    let node_path = state.research_nodes[index].path.clone();
    let node_tier = state.research_nodes[index].tier;

    // Check if path is compatible
    if state.research_path != ResearchPath::None && state.research_path != node_path {
        return false; // locked into a different path
    }

    // Check tier prerequisite: must have purchased previous tier
    if node_tier > 1 {
        let has_prev = state
            .research_nodes
            .iter()
            .any(|n| n.path == node_path && n.tier == node_tier - 1 && n.purchased);
        if !has_prev {
            return false;
        }
    }

    let cost = state.research_nodes[index].cost;
    if state.cookies < cost {
        return false;
    }

    state.cookies -= cost;
    state.research_nodes[index].purchased = true;

    // Lock into this path on first purchase
    if state.research_path == ResearchPath::None {
        state.research_path = node_path.clone();
        let path_name = match &node_path {
            ResearchPath::MassProduction => "量産路線",
            ResearchPath::Quality => "品質路線",
            ResearchPath::None => "なし",
        };
        state.add_log(
            &format!("🔬 研究パス選択: {}！", path_name),
            true,
        );
    }

    let name = state.research_nodes[index].name.clone();
    state.add_log(
        &format!("🔬 研究完了！「{}」", name),
        true,
    );
    state.purchase_flash = 8;
    spawn_celebration(state, 4);

    true
}

// ═══════════════════════════════════════════════════════
// Dragon
// ═══════════════════════════════════════════════════════

/// Feed producers to the dragon. Sacrifices `count` units of the given producer kind.
/// Returns true if the dragon leveled up.
pub fn feed_dragon(state: &mut CookieState, kind: &ProducerKind, count: u32) -> bool {
    if state.dragon_level >= 7 {
        return false; // max level
    }

    let idx = kind.index();
    if state.producers[idx].count < count {
        return false; // not enough producers
    }

    let feed_cost = state.dragon_feed_cost();
    if feed_cost == 0 {
        return false;
    }

    state.producers[idx].count -= count;
    state.dragon_fed_total += count;

    state.add_log(
        &format!(
            "🐉 {} {}台をドラゴンに捧げた！({}/{})",
            kind.name(),
            count,
            state.dragon_fed_toward_next(),
            feed_cost,
        ),
        true,
    );

    // Check if dragon leveled up
    if state.dragon_fed_toward_next() >= feed_cost {
        state.dragon_level += 1;
        state.add_log(
            &format!("🐉 ドラゴンLv.{}に成長！", state.dragon_level),
            true,
        );
        spawn_celebration(state, 6);
        true
    } else {
        false
    }
}

/// Set the dragon's active aura. Requires dragon_level >= 1.
pub fn set_dragon_aura(state: &mut CookieState, aura: DragonAura) -> bool {
    if state.dragon_level == 0 {
        return false;
    }
    state.dragon_aura = aura.clone();
    state.add_log(
        &format!("🐉 オーラ変更: {}", aura.name()),
        true,
    );
    true
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

    #[test]
    fn milestone_becomes_ready_on_cookie_threshold() {
        let mut state = CookieState::new();
        assert_eq!(state.achieved_milestone_count(), 0);
        state.cookies_all_time = 100.0;
        check_milestones(&mut state);
        // "はじめの一歩" should be ready (not yet claimed)
        assert_eq!(state.milestones[0].status, MilestoneStatus::Ready);
        assert_eq!(state.achieved_milestone_count(), 0); // not claimed yet
        assert!((state.milk - 0.0).abs() < 0.001); // milk unchanged until claimed
    }

    #[test]
    fn milestone_claim_applies_milk() {
        let mut state = CookieState::new();
        state.cookies_all_time = 100.0;
        check_milestones(&mut state);
        assert!(claim_milestone(&mut state, 0));
        assert_eq!(state.milestones[0].status, MilestoneStatus::Claimed);
        assert!(state.milk > 0.0);
    }

    #[test]
    fn milestone_claim_all_works() {
        let mut state = CookieState::new();
        state.total_clicks = 100;
        state.cookies_all_time = 100.0;
        check_milestones(&mut state);
        let ready = state.ready_milestone_count();
        assert!(ready >= 2); // at least "はじめの一歩" + "クリッカー"
        let claimed = claim_all_milestones(&mut state);
        assert_eq!(claimed, ready);
        assert_eq!(state.ready_milestone_count(), 0);
        assert_eq!(state.achieved_milestone_count(), ready);
    }

    #[test]
    fn milestone_achieved_on_clicks() {
        let mut state = CookieState::new();
        state.total_clicks = 100;
        check_milestones(&mut state);
        let click_milestone = state.milestones.iter().find(|m| m.name == "クリッカー").unwrap();
        assert_eq!(click_milestone.status, MilestoneStatus::Ready);
    }

    #[test]
    fn milk_increases_with_claimed_milestones() {
        let mut state = CookieState::new();
        state.cookies_all_time = 1_000_000.0;
        state.total_clicks = 10_000;
        state.producers[0].count = 100;
        state.producers[1].count = 50;
        check_milestones(&mut state);
        claim_all_milestones(&mut state);
        let count = state.achieved_milestone_count();
        assert!(count > 5);
        // milk = claimed * 0.04
        assert!((state.milk - count as f64 * 0.04).abs() < 0.001);
    }

    #[test]
    fn kitten_upgrade_multiplies_cps() {
        let mut state = CookieState::new();
        state.producers[1].count = 10; // 10 CPS base
        state.milk = 0.60; // 60% milk
        state.cookies = 100_000.0;
        let base_cps = state.total_cps();
        // Find and buy first kitten upgrade
        let kitten_idx = state.upgrades.iter().position(|u| u.name == "子猫の手伝い").unwrap();
        buy_upgrade(&mut state, kitten_idx);
        let new_cps = state.total_cps();
        // Expected: CPS * (1 + 0.60 * 0.05) = CPS * 1.03
        assert!(new_cps > base_cps);
        assert!((new_cps / base_cps - 1.03).abs() < 0.01);
    }

    #[test]
    fn multiple_kitten_upgrades_stack_multiplicatively() {
        let mut state = CookieState::new();
        state.producers[1].count = 10;
        state.milk = 1.0; // 100% milk
        state.cookies = 10_000_000_000.0;
        // Buy first kitten (5%)
        let idx1 = state.upgrades.iter().position(|u| u.name == "子猫の手伝い").unwrap();
        buy_upgrade(&mut state, idx1);
        // Buy second kitten (10%)
        let idx2 = state.upgrades.iter().position(|u| u.name == "子猫の労働者").unwrap();
        buy_upgrade(&mut state, idx2);
        // Expected: (1 + 1.0*0.05) * (1 + 1.0*0.10) = 1.05 * 1.10 = 1.155
        assert!((state.kitten_multiplier - 1.155).abs() < 0.01);
    }

    #[test]
    fn milestone_flash_decreases_over_ticks() {
        let mut state = CookieState::new();
        state.cookies_all_time = 100.0;
        tick(&mut state, 1); // triggers milestone check
        assert!(state.milestone_flash > 0);
        let flash = state.milestone_flash;
        tick(&mut state, 5);
        assert!(state.milestone_flash < flash);
    }

    #[test]
    fn prestige_requires_trillion_cookies() {
        let mut state = CookieState::new();
        state.cookies_all_time = 1e11; // 100 billion — not enough
        let chips = perform_prestige(&mut state);
        assert_eq!(chips, 0);
        assert_eq!(state.prestige_count, 0);
    }

    #[test]
    fn prestige_earns_chips_from_trillion() {
        let mut state = CookieState::new();
        state.cookies_all_time = 1e12; // 1 trillion → sqrt(1) = 1 chip
        let chips = perform_prestige(&mut state);
        assert_eq!(chips, 1);
        assert_eq!(state.heavenly_chips, 1);
        assert_eq!(state.prestige_count, 1);
    }

    #[test]
    fn prestige_resets_cookies_and_producers() {
        let mut state = CookieState::new();
        state.cookies = 5e12;
        state.cookies_all_time = 5e12;
        state.producers[0].count = 100;
        state.producers[4].count = 50;
        perform_prestige(&mut state);
        // Producers should be reset
        assert_eq!(state.producers[0].count, 0);
        assert_eq!(state.producers[4].count, 0);
        // cookies_all_runs should track total
        assert!(state.cookies_all_runs > 0.0);
    }

    #[test]
    fn prestige_multiplier_scales_with_chips() {
        let mut state = CookieState::new();
        state.cookies_all_time = 100e12; // sqrt(100) = 10 chips
        perform_prestige(&mut state);
        assert_eq!(state.heavenly_chips, 10);
        // prestige_multiplier = 1.0 + 10 * 0.01 = 1.10
        assert!((state.prestige_multiplier - 1.10).abs() < 0.001);
    }

    #[test]
    fn prestige_chips_accumulate_across_runs() {
        let mut state = CookieState::new();
        state.cookies_all_time = 1e12;
        perform_prestige(&mut state); // +1 chip
        assert_eq!(state.heavenly_chips, 1);
        state.cookies_all_time = 3e12; // total across runs: 4e12, sqrt(4) = 2 chips, already have 1
        perform_prestige(&mut state); // +1 chip
        assert_eq!(state.heavenly_chips, 2);
        assert_eq!(state.prestige_count, 2);
    }

    #[test]
    fn buy_prestige_upgrade_success() {
        let mut state = CookieState::new();
        state.heavenly_chips = 10;
        assert!(buy_prestige_upgrade(&mut state, 0)); // cost: 1 chip
        assert!(state.prestige_upgrades[0].purchased);
        assert_eq!(state.heavenly_chips_spent, 1);
        assert_eq!(state.available_chips(), 9);
    }

    #[test]
    fn buy_prestige_upgrade_insufficient_chips() {
        let mut state = CookieState::new();
        state.heavenly_chips = 0;
        assert!(!buy_prestige_upgrade(&mut state, 0));
        assert!(!state.prestige_upgrades[0].purchased);
    }

    #[test]
    fn prestige_starting_cookies_from_upgrade() {
        let mut state = CookieState::new();
        // Give enough chips directly, then buy upgrade
        state.heavenly_chips = 10;
        buy_prestige_upgrade(&mut state, 0); // "天使の贈り物": start with 1000
        assert!(state.prestige_upgrades[0].purchased);
        // Now set up cookies for prestige (need pending > 0)
        // cookies_all_runs=0, cookies_all_time=4e12 → total 4e12 → sqrt(4)=2 chips
        // Already have 10, so pending = max(0, 2-10) = 0. Need more cookies.
        state.cookies_all_time = 200e12; // sqrt(200) ≈ 14 > 10
        let pending = state.pending_heavenly_chips();
        assert!(pending > 0, "pending should be > 0, got {}", pending);
        perform_prestige(&mut state);
        // Starting cookies = 1000 from upgrade
        assert!((state.cookies - 1000.0).abs() < 0.01,
            "expected 1000 cookies, got {}", state.cookies);
    }

    #[test]
    fn statistics_track_best_cps() {
        let mut state = CookieState::new();
        state.producers[1].count = 10; // 10 CPS
        tick(&mut state, 1);
        assert!(state.best_cps >= 10.0);
    }

    #[test]
    fn total_ticks_accumulates() {
        let mut state = CookieState::new();
        tick(&mut state, 50);
        tick(&mut state, 30);
        assert_eq!(state.total_ticks, 80);
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use crate::games::cookie::state::Producer;
    use proptest::prelude::*;

    // ── Strategy helpers ──────────────────────────────────

    fn arb_producer_kind() -> impl Strategy<Value = ProducerKind> {
        prop_oneof![
            Just(ProducerKind::Cursor),
            Just(ProducerKind::Grandma),
            Just(ProducerKind::Farm),
            Just(ProducerKind::Mine),
            Just(ProducerKind::Factory),
        ]
    }

    // ── format_number properties ──────────────────────────

    proptest! {
        #[test]
        fn prop_format_number_no_panic(n in -1e12f64..1e12) {
            let _ = format_number(n);
        }

        #[test]
        fn prop_format_number_nonneg_no_leading_minus(n in 0.0f64..1e12) {
            let s = format_number(n);
            prop_assert!(!s.starts_with('-'), "got: {}", s);
        }

        #[test]
        fn prop_format_number_negative_has_minus(n in -1e12f64..-0.1) {
            let s = format_number(n);
            prop_assert!(s.starts_with('-'), "got: {}", s);
        }

        #[test]
        fn prop_format_number_integer_no_dot(int_val in 0u64..1_000_000_000) {
            let s = format_number(int_val as f64);
            prop_assert!(!s.contains('.'), "integer {} formatted as: {}", int_val, s);
        }

        #[test]
        fn prop_format_number_commas_at_correct_positions(int_val in 0u64..1_000_000_000) {
            let s = format_number(int_val as f64);
            let stripped: String = s.chars().filter(|c| *c != ',').collect();
            prop_assert_eq!(stripped, int_val.to_string());
        }

        #[test]
        fn prop_format_number_small_values_no_comma(n in 0.0f64..1000.0) {
            let s = format_number(n);
            // Integer part < 1000 should never have a comma
            let int_part: String = s.split('.').next().unwrap().to_string();
            prop_assert!(!int_part.contains(','), "got: {}", s);
        }
    }

    // ── Producer cost properties ──────────────────────────

    proptest! {
        #[test]
        fn prop_producer_cost_always_positive(
            kind in arb_producer_kind(),
            count in 0u32..200,
        ) {
            let mut p = Producer::new(kind);
            p.count = count;
            prop_assert!(p.cost() > 0.0, "cost was {}", p.cost());
        }

        #[test]
        fn prop_producer_cost_strictly_increases(
            kind in arb_producer_kind(),
            count in 0u32..199,
        ) {
            let mut p = Producer::new(kind.clone());
            p.count = count;
            let cost_before = p.cost();
            p.count = count + 1;
            let cost_after = p.cost();
            prop_assert!(cost_after > cost_before,
                "cost did not increase: {} -> {}", cost_before, cost_after);
        }

        #[test]
        fn prop_producer_cost_ratio_is_1_15(
            kind in arb_producer_kind(),
            count in 0u32..150,
        ) {
            let mut p = Producer::new(kind.clone());
            p.count = count;
            let cost_a = p.cost();
            p.count = count + 1;
            let cost_b = p.cost();
            let ratio = cost_b / cost_a;
            prop_assert!((ratio - 1.15).abs() < 0.0001,
                "expected ratio 1.15, got {} (count={})", ratio, count);
        }
    }

    // ── Producer CPS properties ───────────────────────────

    proptest! {
        #[test]
        fn prop_producer_cps_nonnegative(
            kind in arb_producer_kind(),
            count in 0u32..500,
            multiplier in 1.0f64..100.0,
        ) {
            let mut p = Producer::new(kind);
            p.count = count;
            p.multiplier = multiplier;
            prop_assert!(p.base_cps() >= 0.0);
        }

        #[test]
        fn prop_producer_cps_zero_when_zero_count(
            kind in arb_producer_kind(),
            multiplier in 1.0f64..100.0,
        ) {
            let mut p = Producer::new(kind);
            p.count = 0;
            p.multiplier = multiplier;
            prop_assert!((p.base_cps() - 0.0).abs() < f64::EPSILON);
        }

        #[test]
        fn prop_producer_cps_linear_in_count(
            kind in arb_producer_kind(),
            count in 1u32..100,
            multiplier in 1.0f64..50.0,
        ) {
            let mut p = Producer::new(kind.clone());
            p.multiplier = multiplier;
            p.count = count;
            let cps_a = p.base_cps();
            p.count = count * 2;
            let cps_b = p.base_cps();
            prop_assert!((cps_b / cps_a - 2.0).abs() < 0.0001,
                "CPS should double when count doubles: {} vs {}", cps_a, cps_b);
        }

        #[test]
        fn prop_synergy_bonus_increases_cps(
            kind in arb_producer_kind(),
            count in 1u32..100,
            multiplier in 1.0f64..50.0,
            synergy in 0.01f64..5.0,
        ) {
            let mut p = Producer::new(kind);
            p.count = count;
            p.multiplier = multiplier;
            prop_assert!(p.cps_with_synergy(synergy) > p.base_cps());
        }

        #[test]
        fn prop_payback_positive_when_has_production(
            kind in arb_producer_kind(),
            count in 0u32..100,
            multiplier in 1.0f64..50.0,
        ) {
            let mut p = Producer::new(kind);
            p.count = count;
            p.multiplier = multiplier;
            if let Some(pb) = p.payback_seconds_with_synergy(0.0) {
                prop_assert!(pb > 0.0, "payback should be positive: {}", pb);
            }
        }
    }

    // ── buy_producer properties ───────────────────────────

    proptest! {
        #[test]
        fn prop_buy_producer_fails_without_funds(
            kind in arb_producer_kind(),
        ) {
            let mut state = CookieState::new();
            state.cookies = 0.0;
            prop_assert!(!buy_producer(&mut state, &kind));
        }

        #[test]
        fn prop_buy_producer_deducts_exact_cost(
            kind in arb_producer_kind(),
            extra in 0.0f64..1000.0,
        ) {
            let mut state = CookieState::new();
            let idx = kind.index();
            let cost = state.producers[idx].cost();
            state.cookies = cost + extra;
            let before = state.cookies;
            let success = buy_producer(&mut state, &kind);
            prop_assert!(success);
            let expected = before - cost;
            prop_assert!((state.cookies - expected).abs() < 0.001,
                "expected {} cookies left, got {}", expected, state.cookies);
        }

        #[test]
        fn prop_buy_producer_increments_count(
            kind in arb_producer_kind(),
        ) {
            let mut state = CookieState::new();
            let idx = kind.index();
            state.cookies = 1e12;
            let count_before = state.producers[idx].count;
            buy_producer(&mut state, &kind);
            prop_assert_eq!(state.producers[idx].count, count_before + 1);
        }

        #[test]
        fn prop_buy_producer_preserves_cookies_all_time(
            kind in arb_producer_kind(),
        ) {
            let mut state = CookieState::new();
            state.cookies = 1e12;
            state.cookies_all_time = 1e12;
            let all_time_before = state.cookies_all_time;
            buy_producer(&mut state, &kind);
            prop_assert_eq!(state.cookies_all_time, all_time_before,
                "cookies_all_time should not change on purchase");
        }

        #[test]
        fn prop_buy_producer_with_discount_cheaper(
            kind in arb_producer_kind(),
            discount in 0.01f64..0.99,
        ) {
            let idx = kind.index();
            let state_full = CookieState::new();
            let full_cost = state_full.producers[idx].cost();

            let mut state_disc = CookieState::new();
            state_disc.active_discount = discount;
            let disc_cost = state_disc.producers[idx].cost() * (1.0 - discount);

            prop_assert!(disc_cost < full_cost);
        }
    }

    // ── tick properties ───────────────────────────────────

    proptest! {
        #[test]
        fn prop_tick_zero_is_noop(cookies in 0.0f64..1e12) {
            let mut state = CookieState::new();
            state.cookies = cookies;
            tick(&mut state, 0);
            prop_assert!((state.cookies - cookies).abs() < f64::EPSILON);
        }

        #[test]
        fn prop_tick_never_reduces_cookies(
            delta in 1u32..100,
        ) {
            let mut state = CookieState::new();
            state.cookies = 100.0;
            state.producers[0].count = 5; // some production
            let before = state.cookies;
            tick(&mut state, delta);
            prop_assert!(state.cookies >= before,
                "cookies decreased from {} to {}", before, state.cookies);
        }

        #[test]
        fn prop_tick_production_proportional_to_delta(
            delta in 1u32..50,
        ) {
            // With no buffs/golden, production is delta * cps / 10
            let mut s1 = CookieState::new();
            s1.producers[0].count = 10;
            s1.golden_next_spawn = 99999;
            s1.mini_event_next = 99999;

            let mut s2 = CookieState::new();
            s2.producers[0].count = 10;
            s2.golden_next_spawn = 99999;
            s2.mini_event_next = 99999;

            tick(&mut s1, delta);
            tick(&mut s2, delta * 2);

            let prod1 = s1.cookies;
            let prod2 = s2.cookies;
            prop_assert!((prod2 / prod1 - 2.0).abs() < 0.01,
                "expected 2x production, got {} / {} = {}", prod2, prod1, prod2 / prod1);
        }
    }
}
