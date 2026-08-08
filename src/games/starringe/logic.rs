//! 星環の純粋ロジック。描画・I/O に依存しない。

use super::state::{
    BeamFlash, Ore, OreKind, Particle, ParticleKind, StarRingState, UpgradeKind, BOOST_DURATION,
    CX, CY, INNER_RADIUS, ORBIT_Y_SQUASH, SPAWN_RADIUS, WORLD_H, WORLD_W,
};

fn rng_next(state: &mut StarRingState) -> u32 {
    let mut x = state.rng_state;
    if x == 0 {
        x = 0xA5A5_5A5A;
    }
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    state.rng_state = x;
    x
}

fn rand01(state: &mut StarRingState) -> f64 {
    (rng_next(state) as f64) / (u32::MAX as f64)
}

fn rand_range(state: &mut StarRingState, lo: f64, hi: f64) -> f64 {
    lo + (hi - lo) * rand01(state)
}

/// 強化の現在コスト。
pub fn upgrade_cost(state: &StarRingState, kind: UpgradeKind) -> f64 {
    let lv = state.level(kind) as f64;
    kind.base_cost() * kind.growth().powf(lv)
}

/// 強化がまだ上限に達していないか。
pub fn can_upgrade_further(state: &StarRingState, kind: UpgradeKind) -> bool {
    match kind.max_level() {
        Some(max) => state.level(kind) < max,
        None => true,
    }
}

/// 強化を購入する。成功で true。
pub fn purchase_upgrade(state: &mut StarRingState, kind: UpgradeKind) -> bool {
    if !can_upgrade_further(state, kind) {
        return false;
    }
    let cost = upgrade_cost(state, kind);
    if state.shards + 1e-9 < cost {
        return false;
    }
    state.shards -= cost;
    state.upgrade_levels[kind.index()] += 1;
    state.shake_ticks = state.shake_ticks.max(4);
    true
}

/// 手動タップ: 近傍1体にダメージ + 一時火力ブースト。
pub fn manual_strike(state: &mut StarRingState) {
    state.boost_ticks = BOOST_DURATION;
    if state.ores.is_empty() {
        burst(state, CX, CY, 6, 1.2, ParticleKind::Spark, 14);
        return;
    }
    let mut best = 0usize;
    let mut best_d = f64::MAX;
    for (i, ore) in state.ores.iter().enumerate() {
        let d = (ore.x - CX).hypot(ore.y - CY);
        if d < best_d {
            best_d = d;
            best = i;
        }
    }
    let dmg = state.damage() * 1.5;
    apply_damage(state, best, dmg);
}

pub fn tick(state: &mut StarRingState, delta_ticks: u32) {
    for _ in 0..delta_ticks {
        state.elapsed_ticks = state.elapsed_ticks.wrapping_add(1);
        state.tick_gain = 0.0;
        if state.shake_ticks > 0 {
            state.shake_ticks -= 1;
        }
        if state.core_flash_ticks > 0 {
            state.core_flash_ticks -= 1;
        }
        if state.boost_ticks > 0 {
            state.boost_ticks -= 1;
        }

        step_particles(state);
        step_beams(state);
        step_ores(state);
        resolve_leaks(state);
        spawn_ores(state);
        fire_turrets(state);

        // 星屑/秒用リングバッファ
        state.recent_gain[state.recent_gain_idx] = state.tick_gain;
        state.recent_gain_idx = (state.recent_gain_idx + 1) % state.recent_gain.len();
    }
}

fn step_particles(state: &mut StarRingState) {
    for p in &mut state.particles {
        if p.life == 0 {
            continue;
        }
        p.x += p.vx;
        p.y += p.vy;
        match p.kind {
            ParticleKind::Dust => {
                p.vx *= 0.96;
                p.vy *= 0.96;
            }
            ParticleKind::Spark | ParticleKind::Ember => {
                p.vx *= 0.98;
                p.vy *= 0.98;
            }
            ParticleKind::Shard => {
                p.vx *= 0.99;
                p.vy *= 0.99;
            }
        }
        p.life -= 1;
    }
    state.particles.retain(|p| {
        p.life > 0 && p.x > -8.0 && p.x < WORLD_W + 8.0 && p.y > -8.0 && p.y < WORLD_H + 8.0
    });
    // パーティクル爆発しすぎ防止
    if state.particles.len() > 500 {
        let drop = state.particles.len() - 400;
        state.particles.drain(0..drop);
    }
}

fn step_beams(state: &mut StarRingState) {
    for b in &mut state.beams {
        if b.life > 0 {
            b.life -= 1;
        }
    }
    state.beams.retain(|b| b.life > 0);
}

fn step_ores(state: &mut StarRingState) {
    for ore in &mut state.ores {
        ore.x += ore.vx;
        ore.y += ore.vy;
    }
}

fn resolve_leaks(state: &mut StarRingState) {
    let mut i = 0;
    while i < state.ores.len() {
        let ore = &state.ores[i];
        let dist = (ore.x - CX).hypot(ore.y - CY);
        if dist <= INNER_RADIUS {
            let ore = state.ores.remove(i);
            let loss = (ore.kind.base_value() * 0.35 * state.yield_mult()).min(state.shards);
            state.shards -= loss;
            state.shards_leaked += loss;
            state.leak_count += 1;
            state.core_flash_ticks = state.core_flash_ticks.max(8);
            state.shake_ticks = state.shake_ticks.max(5);
            burst(state, ore.x, ore.y, 8, 1.0, ParticleKind::Ember, 16);
            burst(state, CX, CY, 5, 0.8, ParticleKind::Dust, 12);
        } else {
            i += 1;
        }
    }
}

fn pick_ore_kind(state: &mut StarRingState) -> OreKind {
    let unlocked = state.unlocked_ore_kinds();
    // 高レアほど出にくく重み付け
    let weights: Vec<(OreKind, u32)> = unlocked
        .into_iter()
        .map(|k| {
            let w = match k {
                OreKind::Dust => 50,
                OreKind::Rock => 28,
                OreKind::Crystal => 14,
                OreKind::Prism => 6,
                OreKind::Nova => 2,
            };
            (k, w)
        })
        .collect();
    let total: u32 = weights.iter().map(|(_, w)| *w).sum();
    if total == 0 {
        return OreKind::Dust;
    }
    let mut roll = rng_next(state) % total;
    for (k, w) in weights {
        if roll < w {
            return k;
        }
        roll -= w;
    }
    OreKind::Dust
}

fn spawn_ores(state: &mut StarRingState) {
    let interval = state.spawn_interval();
    if !state.elapsed_ticks.is_multiple_of(interval) {
        return;
    }
    // 同時出現上限 (描画負荷とゲームテンポのバランス)
    if state.ores.len() >= 40 {
        return;
    }
    let batch = state.spawn_batch().min(40 - state.ores.len());
    for _ in 0..batch {
        let kind = pick_ore_kind(state);
        let angle = rand_range(state, 0.0, std::f64::consts::TAU);
        let x = CX + angle.cos() * SPAWN_RADIUS;
        let y = CY + angle.sin() * SPAWN_RADIUS * ORBIT_Y_SQUASH.max(0.55);
        // 楕円外周から中心へ直線接近
        let dx = CX - x;
        let dy = CY - y;
        let dist = dx.hypot(dy).max(0.001);
        let speed = kind.speed() * (1.0 + state.level(UpgradeKind::Density) as f64 * 0.03);
        state.ores.push(Ore {
            x,
            y,
            vx: dx / dist * speed,
            vy: dy / dist * speed,
            hp: kind.base_hp(),
            kind,
            radius: kind.radius(),
        });
    }
}

/// 砲台のワールド座標一覧 (立体感用に depth = sin も返す)。
pub fn turret_positions(state: &StarRingState) -> Vec<(f64, f64, f64)> {
    let n = state.turret_count().max(1);
    let r = state.ring_radius();
    let base = state.elapsed_ticks as f64 * state.orbit_speed();
    (0..n)
        .map(|i| {
            let a = base + i as f64 * std::f64::consts::TAU / n as f64;
            let x = CX + a.cos() * r;
            let y = CY + a.sin() * r * ORBIT_Y_SQUASH;
            let depth = a.sin(); // -1=奥, +1=手前
            (x, y, depth)
        })
        .collect()
}

fn fire_turrets(state: &mut StarRingState) {
    let interval = state.fire_interval();
    if !state.elapsed_ticks.is_multiple_of(interval) {
        return;
    }
    if state.ores.is_empty() {
        return;
    }
    let guns = turret_positions(state);
    let dmg = state.damage();

    // 砲ごとにその時点の最近傍を狙い直す。撃破でインデックスが縮んでも安全。
    for &(gx, gy, _) in &guns {
        if state.ores.is_empty() {
            break;
        }
        let mut best: Option<(usize, f64)> = None;
        for (i, ore) in state.ores.iter().enumerate() {
            let d = (ore.x - gx).hypot(ore.y - gy);
            if best.map(|(_, bd)| d < bd).unwrap_or(true) {
                best = Some((i, d));
            }
        }
        let Some((idx, _)) = best else {
            continue;
        };
        let (tx, ty) = (state.ores[idx].x, state.ores[idx].y);
        state.beams.push(BeamFlash {
            x0: gx,
            y0: gy,
            x1: tx,
            y1: ty,
            life: 3,
        });
        state.particles.push(Particle {
            x: (gx + tx) * 0.5,
            y: (gy + ty) * 0.5,
            vx: 0.0,
            vy: 0.0,
            life: 2,
            kind: ParticleKind::Spark,
        });
        apply_damage(state, idx, dmg);
    }
}

fn apply_damage(state: &mut StarRingState, idx: usize, dmg: f64) {
    if idx >= state.ores.len() {
        return;
    }
    state.ores[idx].hp -= dmg;
    if state.ores[idx].hp > 0.0 {
        return;
    }
    let ore = state.ores.remove(idx);
    let gain = ore.kind.base_value() * state.yield_mult();
    state.shards += gain;
    state.shards_earned += gain;
    state.tick_gain += gain;
    state.total_kills += 1;
    burst(
        state,
        ore.x,
        ore.y,
        6 + ore.kind as usize * 2,
        1.4 + ore.radius * 0.25,
        ParticleKind::Shard,
        18,
    );
    burst(state, ore.x, ore.y, 4, 2.0, ParticleKind::Spark, 12);
}

fn burst(
    state: &mut StarRingState,
    x: f64,
    y: f64,
    count: usize,
    speed: f64,
    kind: ParticleKind,
    life: u32,
) {
    for _ in 0..count {
        let a = rand_range(state, 0.0, std::f64::consts::TAU);
        let s = speed * rand_range(state, 0.5, 1.2);
        state.particles.push(Particle {
            x,
            y,
            vx: a.cos() * s,
            vy: a.sin() * s,
            life,
            kind,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawn_ores_appear_after_interval() {
        let mut state = StarRingState::new();
        let interval = state.spawn_interval();
        for _ in 0..interval {
            tick(&mut state, 1);
        }
        assert!(
            !state.ores.is_empty(),
            "スポーン間隔経過後に鉱石が出現するはず"
        );
    }

    #[test]
    fn killing_ore_increases_shards() {
        let mut state = StarRingState::new();
        let before = state.shards;
        state.ores.push(Ore {
            x: CX + 10.0,
            y: CY,
            vx: 0.0,
            vy: 0.0,
            hp: 0.5,
            kind: OreKind::Dust,
            radius: 1.4,
        });
        // 火力を上げて確実に撃破
        state.upgrade_levels[UpgradeKind::Damage.index()] = 5;
        // 発射タイミングまで進める
        let interval = state.fire_interval();
        for _ in 0..interval + 2 {
            tick(&mut state, 1);
        }
        assert!(
            state.shards > before || state.total_kills > 0,
            "撃破で星屑が増えるはず shards={}->{} kills={}",
            before,
            state.shards,
            state.total_kills
        );
        assert!(state.shards_earned > 0.0);
    }

    #[test]
    fn purchase_upgrade_spends_shards_and_raises_level() {
        let mut state = StarRingState::new();
        state.shards = 1000.0;
        let cost = upgrade_cost(&state, UpgradeKind::Damage);
        assert!(purchase_upgrade(&mut state, UpgradeKind::Damage));
        assert_eq!(state.level(UpgradeKind::Damage), 1);
        assert!((state.shards - (1000.0 - cost)).abs() < 1e-6);
    }

    #[test]
    fn purchase_fails_when_broke() {
        let mut state = StarRingState::new();
        state.shards = 0.0;
        assert!(!purchase_upgrade(&mut state, UpgradeKind::Turrets));
        assert_eq!(state.level(UpgradeKind::Turrets), 0);
    }

    #[test]
    fn turret_count_caps_at_max() {
        let mut state = StarRingState::new();
        state.shards = 1e12;
        for _ in 0..20 {
            purchase_upgrade(&mut state, UpgradeKind::Turrets);
        }
        assert_eq!(state.turret_count(), super::super::state::MAX_TURRETS);
        assert!(!can_upgrade_further(&state, UpgradeKind::Turrets));
    }

    #[test]
    fn leak_reduces_shards_without_game_over() {
        let mut state = StarRingState::new();
        state.shards = 50.0;
        state.ores.push(Ore {
            x: CX + 1.0,
            y: CY,
            vx: 0.0,
            vy: 0.0,
            hp: 10.0,
            kind: OreKind::Rock,
            radius: 1.9,
        });
        tick(&mut state, 1);
        assert!(state.leak_count >= 1);
        assert!(state.shards < 50.0);
        assert!(state.shards_leaked > 0.0);
        // ゲーム継続
        tick(&mut state, 5);
        assert!(state.elapsed_ticks >= 6);
    }

    #[test]
    fn manual_strike_applies_boost() {
        let mut state = StarRingState::new();
        manual_strike(&mut state);
        assert_eq!(state.boost_ticks, BOOST_DURATION);
        assert!(state.damage() > 1.0);
    }

    #[test]
    fn ore_kinds_unlock_with_kills() {
        let mut state = StarRingState::new();
        assert_eq!(state.unlocked_ore_kinds(), vec![OreKind::Dust]);
        state.total_kills = OreKind::Rock.unlock_kills();
        assert!(state.unlocked_ore_kinds().contains(&OreKind::Rock));
        state.total_kills = OreKind::Nova.unlock_kills();
        assert_eq!(state.unlocked_ore_kinds().len(), 5);
    }
}
