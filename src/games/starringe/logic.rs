//! 星環の純粋ロジック。描画・I/O に依存しない。

use super::state::{
    BeamFlash, BeamKind, Ore, OreKind, OreMotion, Particle, ParticleKind, PulseRing, StarRingState,
    UpgradeKind, BOOST_DURATION, CX, CY, INNER_RADIUS, ORBIT_Y_SQUASH, SPAWN_RADIUS, WORLD_H,
    WORLD_W,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DamageSource {
    Laser,
    Lance,
    Pulse,
    Strike,
}

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

/// 強化がまだ上限に達しておらず、層で解放済みか。
pub fn can_upgrade_further(state: &StarRingState, kind: UpgradeKind) -> bool {
    if !state.upgrade_unlocked(kind) {
        return false;
    }
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
/// 脈動を解放済みなら小さな核波も添える。
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
    apply_damage(state, best, dmg, DamageSource::Strike);

    if state.level(UpgradeKind::Pulse) > 0 {
        let r = state.pulse_radius() * 0.55;
        let dmg = state.pulse_damage() * 0.6;
        pulse_damage_area(state, r, dmg);
        state.pulse_rings.push(PulseRing {
            radius: r,
            life: 8,
            max_life: 8,
        });
    }
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
        if state.depth_flash_ticks > 0 {
            state.depth_flash_ticks -= 1;
        }

        step_particles(state);
        step_beams(state);
        step_pulse_rings(state);
        step_ores(state);
        resolve_leaks(state);
        spawn_ores(state);
        fire_turrets(state);
        fire_lance(state);
        fire_pulse(state);

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

fn step_pulse_rings(state: &mut StarRingState) {
    for r in &mut state.pulse_rings {
        if r.life > 0 {
            r.life -= 1;
            // 波が外へ広がる見た目
            r.radius += 0.55;
        }
    }
    state.pulse_rings.retain(|r| r.life > 0);
    if state.pulse_rings.len() > 12 {
        let drop = state.pulse_rings.len() - 8;
        state.pulse_rings.drain(0..drop);
    }
}

fn step_ores(state: &mut StarRingState) {
    for ore in &mut state.ores {
        let prev_x = ore.x;
        let prev_y = ore.y;
        let dx = ore.x - CX;
        let dy = ore.y - CY;
        let r = dx.hypot(dy).max(0.2);
        let ang = dy.atan2(dx);
        ore.age = ore.age.wrapping_add(1);

        let radial = match ore.motion {
            OreMotion::Spiral => ore.kind.radial_speed(),
            OreMotion::Orbit => ore.kind.radial_speed(),
            OreMotion::Zigzag => {
                let breath = (ore.age as f64 * 0.18).sin() * 0.08;
                ore.kind.radial_speed() + breath
            }
            OreMotion::Heavy => ore.kind.radial_speed() * 0.85,
        };
        let ang_delta = match ore.motion {
            OreMotion::Zigzag => ore.ang_vel + (ore.age as f64 * 0.11).sin() * 0.012,
            _ => ore.ang_vel,
        };

        let new_ang = ang + ang_delta;
        let new_r = (r + radial).max(0.15);
        ore.x = CX + new_ang.cos() * new_r;
        ore.y = CY + new_ang.sin() * new_r * ORBIT_Y_SQUASH.max(0.55);
        ore.vx = ore.x - prev_x;
        ore.vy = ore.y - prev_y;
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
    let weights: Vec<(OreKind, u32)> = unlocked
        .into_iter()
        .map(|k| {
            let w = match k {
                OreKind::Dust => 46,
                OreKind::Rock => 26,
                OreKind::Crystal => 12,
                OreKind::Wisp => 10,
                OreKind::Prism => 6,
                OreKind::Shell => 5,
                OreKind::Splitter => 5,
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

fn spawn_one(state: &mut StarRingState, kind: OreKind, angle: f64, radius: f64) {
    let x = CX + angle.cos() * radius;
    let y = CY + angle.sin() * radius * ORBIT_Y_SQUASH.max(0.55);
    let sign = if rng_next(state).is_multiple_of(2) {
        1.0
    } else {
        -1.0
    };
    let ang_vel = kind.ang_speed() * sign * rand_range(state, 0.85, 1.15);
    let hp = kind.base_hp() * state.depth_hp_mult();
    state.ores.push(Ore {
        x,
        y,
        vx: 0.0,
        vy: 0.0,
        hp,
        kind,
        radius: kind.radius(),
        motion: kind.default_motion(),
        ang_vel,
        age: 0,
    });
}

fn spawn_ores(state: &mut StarRingState) {
    let interval = state.spawn_interval();
    if !state.elapsed_ticks.is_multiple_of(interval) {
        return;
    }
    if state.ores.len() >= 48 {
        return;
    }
    let batch = state.spawn_batch().min(48 - state.ores.len());
    for _ in 0..batch {
        let kind = pick_ore_kind(state);
        let angle = rand_range(state, 0.0, std::f64::consts::TAU);
        spawn_one(state, kind, angle, SPAWN_RADIUS);
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
            let depth = a.sin();
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
            kind: BeamKind::Laser,
        });
        state.particles.push(Particle {
            x: (gx + tx) * 0.5,
            y: (gy + ty) * 0.5,
            vx: 0.0,
            vy: 0.0,
            life: 2,
            kind: ParticleKind::Spark,
        });
        apply_damage(state, idx, dmg, DamageSource::Laser);
    }
}

fn fire_lance(state: &mut StarRingState) {
    let Some(interval) = state.lance_interval() else {
        return;
    };
    if !state.elapsed_ticks.is_multiple_of(interval) {
        return;
    }
    if state.ores.is_empty() {
        return;
    }
    // 最も外側の鉱石を起点に、中心方向へ貫通させる
    let mut best = 0usize;
    let mut best_r = -1.0;
    for (i, ore) in state.ores.iter().enumerate() {
        let r = (ore.x - CX).hypot(ore.y - CY);
        if r > best_r {
            best_r = r;
            best = i;
        }
    }
    let ox = state.ores[best].x;
    let oy = state.ores[best].y;
    let dx = CX - ox;
    let dy = CY - oy;
    let len = dx.hypot(dy).max(0.001);
    let ux = dx / len;
    let uy = dy / len;

    // 砲台リング外からコア手前まで
    let x0 = ox - ux * 4.0;
    let y0 = oy - uy * 4.0;
    let x1 = CX + ux * INNER_RADIUS;
    let y1 = CY + uy * INNER_RADIUS;
    state.beams.push(BeamFlash {
        x0,
        y0,
        x1,
        y1,
        life: 5,
        kind: BeamKind::Lance,
    });

    let dmg = state.lance_damage();
    // 線分に近い鉱石を外側から順に削る
    let mut hits: Vec<(usize, f64)> = state
        .ores
        .iter()
        .enumerate()
        .filter_map(|(i, ore)| {
            let dist = point_line_distance(ore.x, ore.y, x0, y0, x1, y1);
            if dist <= ore.radius + 1.2 {
                let along = (ore.x - x0) * ux + (ore.y - y0) * uy;
                Some((i, along))
            } else {
                None
            }
        })
        .collect();
    hits.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

    // 後ろから消すとインデックスが壊れないよう、インデックス降順でダメージ
    let mut indices: Vec<usize> = hits.into_iter().map(|(i, _)| i).collect();
    indices.sort_unstable_by(|a, b| b.cmp(a));
    for idx in indices {
        apply_damage(state, idx, dmg, DamageSource::Lance);
    }
    burst(state, ox, oy, 4, 1.6, ParticleKind::Spark, 10);
}

fn point_line_distance(px: f64, py: f64, x0: f64, y0: f64, x1: f64, y1: f64) -> f64 {
    let dx = x1 - x0;
    let dy = y1 - y0;
    let len2 = dx * dx + dy * dy;
    if len2 < 1e-9 {
        return (px - x0).hypot(py - y0);
    }
    let t = ((px - x0) * dx + (py - y0) * dy) / len2;
    let t = t.clamp(0.0, 1.0);
    let qx = x0 + dx * t;
    let qy = y0 + dy * t;
    (px - qx).hypot(py - qy)
}

fn fire_pulse(state: &mut StarRingState) {
    let Some(interval) = state.pulse_interval() else {
        return;
    };
    if !state.elapsed_ticks.is_multiple_of(interval) {
        return;
    }
    let r = state.pulse_radius();
    let dmg = state.pulse_damage();
    pulse_damage_area(state, r, dmg);
    state.pulse_rings.push(PulseRing {
        radius: r * 0.35,
        life: 10,
        max_life: 10,
    });
    state.core_flash_ticks = state.core_flash_ticks.max(4);
    burst(state, CX, CY, 5, 1.0, ParticleKind::Spark, 12);
}

fn pulse_damage_area(state: &mut StarRingState, radius: f64, dmg: f64) {
    let mut i = state.ores.len();
    while i > 0 {
        i -= 1;
        let dist = (state.ores[i].x - CX).hypot(state.ores[i].y - CY);
        if dist <= radius + state.ores[i].radius {
            apply_damage(state, i, dmg, DamageSource::Pulse);
        }
    }
}

fn apply_damage(state: &mut StarRingState, idx: usize, dmg: f64, source: DamageSource) {
    if idx >= state.ores.len() {
        return;
    }
    let armored = state.ores[idx].kind.armored();
    let dealt = if armored {
        match source {
            DamageSource::Lance | DamageSource::Pulse => dmg,
            DamageSource::Laser | DamageSource::Strike => dmg * 0.45,
        }
    } else {
        dmg
    };
    state.ores[idx].hp -= dealt;
    if state.ores[idx].hp > 0.0 {
        return;
    }
    let ore = state.ores.remove(idx);
    let gain = ore.kind.base_value() * state.yield_mult();
    state.shards += gain;
    state.shards_earned += gain;
    state.tick_gain += gain;
    state.total_kills += 1;
    state.depth_kills += 1;
    burst(
        state,
        ore.x,
        ore.y,
        6 + (ore.kind as usize).min(6),
        1.4 + ore.radius * 0.25,
        ParticleKind::Shard,
        18,
    );
    burst(state, ore.x, ore.y, 4, 2.0, ParticleKind::Spark, 12);

    if ore.kind.splits_on_death() && state.ores.len() < 48 {
        let base_ang = (ore.y - CY).atan2(ore.x - CX);
        let child_hp = OreKind::Dust.base_hp() * state.depth_hp_mult() * 0.7;
        for k in 0..2 {
            let ang = base_ang + (k as f64 - 0.5) * 0.55;
            let r = (ore.x - CX).hypot(ore.y - CY).max(INNER_RADIUS + 3.0);
            spawn_one(state, OreKind::Dust, ang, r);
            if let Some(child) = state.ores.last_mut() {
                child.hp = child_hp;
                child.radius = 1.1;
            }
        }
    }

    maybe_advance_depth(state);
}

fn maybe_advance_depth(state: &mut StarRingState) {
    let need = state.kills_to_next_depth();
    if state.depth_kills < need {
        return;
    }
    state.depth_kills = 0;
    state.depth = state.depth.saturating_add(1);
    if state.depth > state.best_depth {
        state.best_depth = state.depth;
    }
    state.depth_flash_ticks = 24;
    state.shake_ticks = state.shake_ticks.max(8);
    state.core_flash_ticks = state.core_flash_ticks.max(10);
    burst(state, CX, CY, 14, 2.2, ParticleKind::Shard, 22);
    burst(state, CX, CY, 10, 1.6, ParticleKind::Spark, 16);
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

    fn push_ore(state: &mut StarRingState, kind: OreKind, x: f64, y: f64, hp: f64) {
        state.ores.push(Ore {
            x,
            y,
            vx: 0.0,
            vy: 0.0,
            hp,
            kind,
            radius: kind.radius(),
            motion: kind.default_motion(),
            ang_vel: kind.ang_speed(),
            age: 0,
        });
    }

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
        push_ore(&mut state, OreKind::Dust, CX + 10.0, CY, 0.5);
        state.upgrade_levels[UpgradeKind::Damage.index()] = 5;
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
    fn pulse_and_lance_locked_until_depth() {
        let mut state = StarRingState::new();
        state.shards = 1e9;
        assert!(!state.upgrade_unlocked(UpgradeKind::Pulse));
        assert!(!purchase_upgrade(&mut state, UpgradeKind::Pulse));
        assert!(!state.upgrade_unlocked(UpgradeKind::Lance));
        assert!(!purchase_upgrade(&mut state, UpgradeKind::Lance));

        state.depth = 2;
        assert!(purchase_upgrade(&mut state, UpgradeKind::Pulse));
        state.depth = 4;
        assert!(purchase_upgrade(&mut state, UpgradeKind::Lance));
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
        push_ore(&mut state, OreKind::Rock, CX + 1.0, CY, 10.0);
        tick(&mut state, 1);
        assert!(state.leak_count >= 1);
        assert!(state.shards < 50.0);
        assert!(state.shards_leaked > 0.0);
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
    fn ore_kinds_unlock_with_depth() {
        let mut state = StarRingState::new();
        assert_eq!(
            state.unlocked_ore_kinds(),
            vec![OreKind::Dust, OreKind::Rock]
        );
        state.depth = OreKind::Wisp.unlock_depth();
        assert!(state.unlocked_ore_kinds().contains(&OreKind::Wisp));
        state.depth = OreKind::Nova.unlock_depth();
        assert_eq!(state.unlocked_ore_kinds().len(), 8);
    }

    #[test]
    fn depth_advances_after_enough_kills() {
        let mut state = StarRingState::new();
        let need = state.kills_to_next_depth();
        for _ in 0..need {
            push_ore(&mut state, OreKind::Dust, CX + 12.0, CY, 0.1);
            apply_damage(&mut state, 0, 99.0, DamageSource::Laser);
        }
        assert_eq!(state.depth, 2);
        assert_eq!(state.best_depth, 2);
        assert!(state.depth_flash_ticks > 0);
    }

    #[test]
    fn ores_drift_tangentially_not_pure_radial() {
        let mut state = StarRingState::new();
        push_ore(&mut state, OreKind::Dust, CX + 20.0, CY, 10.0);
        let x0 = state.ores[0].x;
        let y0 = state.ores[0].y;
        for _ in 0..8 {
            step_ores(&mut state);
        }
        let dy = state.ores[0].y - y0;
        // 純粋な中心方向 (負の x) だけではない — 接線成分が出る
        assert!(
            dy.abs() > 0.05,
            "螺旋漂流なので Y 方向にも動くはず dy={dy}"
        );
        let r0 = (x0 - CX).hypot(y0 - CY);
        let r1 = (state.ores[0].x - CX).hypot(state.ores[0].y - CY);
        assert!(r1 < r0, "ゆっくり内側へ沈むはず r0={r0} r1={r1}");
    }

    #[test]
    fn pulse_damages_nearby_ores() {
        let mut state = StarRingState::new();
        state.depth = 2;
        state.shards = 1e9;
        purchase_upgrade(&mut state, UpgradeKind::Pulse);
        push_ore(&mut state, OreKind::Dust, CX + 6.0, CY, 5.0);
        let interval = state.pulse_interval().unwrap();
        state.elapsed_ticks = interval;
        fire_pulse(&mut state);
        assert!(
            state.ores.is_empty() || state.ores[0].hp < 5.0,
            "脈動で近傍が削られるはず"
        );
        assert!(!state.pulse_rings.is_empty());
    }

    #[test]
    fn lance_hits_multiple_ores_in_line() {
        let mut state = StarRingState::new();
        state.depth = 4;
        state.shards = 1e9;
        purchase_upgrade(&mut state, UpgradeKind::Lance);
        // 同じ角度上に2体
        push_ore(&mut state, OreKind::Dust, CX + 24.0, CY, 3.0);
        push_ore(&mut state, OreKind::Dust, CX + 14.0, CY, 3.0);
        let interval = state.lance_interval().unwrap();
        state.elapsed_ticks = interval;
        fire_lance(&mut state);
        let total_hp: f64 = state.ores.iter().map(|o| o.hp).sum();
        assert!(
            total_hp < 6.0 || state.total_kills > 0,
            "穿光が並びを削るはず hp_sum={total_hp} kills={}",
            state.total_kills
        );
        assert!(state.beams.iter().any(|b| b.kind == BeamKind::Lance));
    }

    #[test]
    fn shell_resists_laser_but_not_lance() {
        let mut state = StarRingState::new();
        push_ore(&mut state, OreKind::Shell, CX + 15.0, CY, 10.0);
        apply_damage(&mut state, 0, 4.0, DamageSource::Laser);
        let after_laser = state.ores[0].hp;
        assert!((after_laser - (10.0 - 4.0 * 0.45)).abs() < 1e-6);

        apply_damage(&mut state, 0, 4.0, DamageSource::Lance);
        let after_lance = state.ores[0].hp;
        assert!((after_lance - (after_laser - 4.0)).abs() < 1e-6);
    }

    #[test]
    fn splitter_spawns_children_on_death() {
        let mut state = StarRingState::new();
        push_ore(&mut state, OreKind::Splitter, CX + 16.0, CY, 0.5);
        apply_damage(&mut state, 0, 99.0, DamageSource::Laser);
        assert_eq!(state.total_kills, 1);
        assert!(
            state.ores.len() >= 2,
            "裂片撃破で子が生まれるはず ores={}",
            state.ores.len()
        );
        assert!(state.ores.iter().all(|o| o.kind == OreKind::Dust));
    }

    #[test]
    fn deeper_depth_spawns_faster_without_player_density_upgrade() {
        let shallow = StarRingState::new();
        let mut deep = StarRingState::new();
        deep.depth = 9;
        assert!(
            deep.spawn_interval() < shallow.spawn_interval(),
            "深い層ほど出現が速い"
        );
        assert!(deep.spawn_batch() >= shallow.spawn_batch());
        // プレイヤー側に密度強化スロットは存在しない
        assert!(!UpgradeKind::ALL.iter().any(|k| k.label() == "鉱脈密度"));
    }
}
