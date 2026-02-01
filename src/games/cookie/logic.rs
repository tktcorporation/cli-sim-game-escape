/// Cookie Factory game logic — pure functions, fully testable.

use super::state::{CookieState, Particle, ProducerKind};

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
    if state.purchase_flash > 0 {
        state.purchase_flash = state.purchase_flash.saturating_sub(delta_ticks);
    }
    // Update particles
    for p in &mut state.particles {
        p.life = p.life.saturating_sub(delta_ticks);
    }
    state.particles.retain(|p| p.life > 0);
}

/// Simple pseudo-random number generator (xorshift32).
fn next_rng(state: &mut CookieState) -> u32 {
    let mut x = state.rng_state;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    state.rng_state = x;
    x
}

/// Manual click: add cookies_per_click to cookies + spawn particles.
pub fn click(state: &mut CookieState) {
    let amount = state.cookies_per_click;
    state.cookies += amount;
    state.cookies_all_time += amount;
    state.total_clicks += 1;
    state.click_flash = 3; // flash for 3 ticks

    // Spawn floating "+N" particle
    let col_offset = (next_rng(state) % 13) as i16 - 6; // -6..+6
    let life = 8 + (next_rng(state) % 5); // 8-12 ticks
    let text = if amount >= 10.0 {
        format!("+{}", format_number(amount))
    } else {
        format!("+{}", amount as u32)
    };
    state.particles.push(Particle {
        text,
        col_offset,
        life,
        max_life: life,
    });

    // Cap particles to avoid memory issues
    if state.particles.len() > 20 {
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

    let cost = state.producers[idx].cost();
    if state.cookies >= cost {
        state.cookies -= cost;
        state.producers[idx].count += 1;
        state.purchase_flash = 5; // flash for 5 ticks (0.5s)
        state.add_log(
            &format!(
                "🎉 {} を購入！ ({}台)",
                state.producers[idx].kind.name(),
                state.producers[idx].count
            ),
            true,
        );
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
    let cost = state.upgrades[upgrade_idx].cost;
    if state.cookies < cost {
        return false;
    }

    state.cookies -= cost;
    state.upgrades[upgrade_idx].purchased = true;
    state.purchase_flash = 8; // longer flash for upgrades (0.8s)

    let upgrade = &state.upgrades[upgrade_idx];
    let name = upgrade.name.clone();
    let target = upgrade.target.clone();
    let multiplier = upgrade.multiplier;

    // Special case: first upgrade "強化クリック" adds to click power
    if name == "強化クリック" {
        state.cookies_per_click += 1.0;
        state.add_log(&format!("✦ {} 適用！クリック+1", name), true);
    } else {
        // Apply multiplier to the target producer
        if let Some(p) = state.producers.iter_mut().find(|p| p.kind == target) {
            p.multiplier *= multiplier;
        }
        state.add_log(&format!("✦ {} 適用！", name), true);
    }

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
        state.producers[0].count = 10; // 10 cursors = 1.0 cps
        tick(&mut state, 10); // 1 second
        assert!((state.cookies - 1.0).abs() < 0.001);
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
        state.producers[0].count = 10; // 1.0 cps
        state.producers[1].count = 3;  // 3.0 cps
        tick(&mut state, 10); // 1 second → 4.0 cookies
        assert!((state.cookies - 4.0).abs() < 0.001);
    }

    #[test]
    fn tick_100_ticks_idle() {
        let mut state = CookieState::new();
        state.producers[1].count = 5; // 5 grandmas = 5.0 cps
        tick(&mut state, 100); // 10 seconds
        assert!((state.cookies - 50.0).abs() < 0.01);
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
    fn cookies_all_time_tracks_total() {
        let mut state = CookieState::new();
        state.producers[0].count = 10;
        click(&mut state); // +1
        tick(&mut state, 10); // +1
        assert!((state.cookies_all_time - 2.0).abs() < 0.001);

        // Spend cookies, all_time doesn't decrease
        state.cookies = 1000.0;
        buy_producer(&mut state, &ProducerKind::Cursor);
        assert!(state.cookies_all_time >= 2.0);
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
}
