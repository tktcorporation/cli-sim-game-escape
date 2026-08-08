//! 星環の純粋ロジック。描画・I/O に依存しない。

use super::state::{
    Layer, Ore, OreKind, Particle, ParticleKind, Projectile, RingUpgrade, StarRingState,
    WeaponKind, WeaponStat, BOOST_DURATION, CX, CY, INNER_RADIUS, LAYER_FLASH_TICKS, ORBIT_Y_SQUASH,
    SPAWN_RADIUS, WORLD_H, WORLD_W,
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

/// 武器ステ強化の現在コスト。
pub fn weapon_stat_cost(state: &StarRingState, weapon: WeaponKind, stat: WeaponStat) -> f64 {
    let lv = state.weapon_stat(weapon, stat) as f64;
    // 後発武器ほど少し高めに
    let tier = 1.0 + weapon.index() as f64 * 0.35;
    stat.base_cost() * tier * stat.growth().powf(lv)
}

pub fn can_upgrade_weapon_stat(state: &StarRingState, weapon: WeaponKind, stat: WeaponStat) -> bool {
    if !state.is_weapon_unlocked(weapon) {
        return false;
    }
    match stat.max_level() {
        Some(max) => state.weapon_stat(weapon, stat) < max,
        None => true,
    }
}

pub fn purchase_weapon_stat(
    state: &mut StarRingState,
    weapon: WeaponKind,
    stat: WeaponStat,
) -> bool {
    if !can_upgrade_weapon_stat(state, weapon, stat) {
        return false;
    }
    let cost = weapon_stat_cost(state, weapon, stat);
    if state.shards + 1e-9 < cost {
        return false;
    }
    state.shards -= cost;
    state.weapon_levels[weapon.index()][stat.index()] += 1;
    state.shake_ticks = state.shake_ticks.max(4);
    true
}

pub fn ring_upgrade_cost(state: &StarRingState, kind: RingUpgrade) -> f64 {
    let lv = state.ring_level(kind) as f64;
    kind.base_cost() * kind.growth().powf(lv)
}

pub fn purchase_ring_upgrade(state: &mut StarRingState, kind: RingUpgrade) -> bool {
    let cost = ring_upgrade_cost(state, kind);
    if state.shards + 1e-9 < cost {
        return false;
    }
    state.shards -= cost;
    state.ring_levels[kind.index()] += 1;
    state.shake_ticks = state.shake_ticks.max(4);
    true
}

/// 武装タブで前後の解放済み武器へ送る。
pub fn cycle_selected_weapon(state: &mut StarRingState, delta: i32) {
    let unlocked = state.unlocked_weapons();
    if unlocked.is_empty() {
        return;
    }
    let cur = unlocked
        .iter()
        .position(|&w| w == state.selected_weapon)
        .unwrap_or(0);
    let n = unlocked.len() as i32;
    let next = ((cur as i32 + delta) % n + n) % n;
    state.selected_weapon = unlocked[next as usize];
}

pub fn select_weapon(state: &mut StarRingState, weapon: WeaponKind) -> bool {
    if !state.is_weapon_unlocked(weapon) {
        return false;
    }
    state.selected_weapon = weapon;
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
    let dmg = state.weapon_damage(WeaponKind::Pulse) * 2.2;
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
        if state.layer_flash_ticks > 0 {
            state.layer_flash_ticks -= 1;
        }

        step_particles(state);
        step_projectiles(state);
        step_ores(state);
        resolve_arrivals(state);
        spawn_ores(state);
        fire_weapons(state);
        check_layer_advance(state);

        state.recent_gain[state.recent_gain_idx] = state.tick_gain;
        state.recent_gain_idx = (state.recent_gain_idx + 1) % state.recent_gain.len();
    }
}

fn check_layer_advance(state: &mut StarRingState) {
    let layer = state.layer();
    if layer > state.last_layer {
        state.layer_flash_ticks = LAYER_FLASH_TICKS;
        state.shake_ticks = state.shake_ticks.max(10);
        state.core_flash_ticks = state.core_flash_ticks.max(14);
        burst(state, CX, CY, 16, 2.2, ParticleKind::Spark, 22);
        burst(state, CX, CY, 10, 1.6, ParticleKind::Shard, 18);
        // 新武装が解放されたら選択をそちらへ寄せる
        for w in WeaponKind::ALL {
            if w.unlock_layer() == layer {
                state.selected_weapon = w;
                break;
            }
        }
        state.last_layer = layer;
    } else if layer < state.last_layer {
        // セーブ改変などで層が下がった場合の同期
        state.last_layer = layer;
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

fn step_projectiles(state: &mut StarRingState) {
    // 移動
    for p in &mut state.projectiles {
        if p.life == 0 {
            continue;
        }
        if p.spin != 0.0 {
            // 環弾: 速度ベクトルを回転させつつ外向き成分を保つ
            let speed = p.vx.hypot(p.vy).max(0.01);
            let ang = p.vy.atan2(p.vx) + p.spin;
            p.vx = ang.cos() * speed;
            p.vy = ang.sin() * speed;
        }
        p.x += p.vx;
        p.y += p.vy;
        p.life -= 1;
    }

    // 衝突 (弾→鉱石)。インデックスが動くので後ろから。
    let mut i = 0;
    while i < state.projectiles.len() {
        if state.projectiles[i].life == 0 {
            state.projectiles.swap_remove(i);
            continue;
        }
        let (px, py, pr, dmg, splash, pierce) = {
            let p = &state.projectiles[i];
            (p.x, p.y, p.radius, p.damage, p.splash, p.pierce)
        };
        let mut hit: Option<usize> = None;
        for (oi, ore) in state.ores.iter().enumerate() {
            let d = (ore.x - px).hypot(ore.y - py);
            if d <= ore.radius + pr {
                hit = Some(oi);
                break;
            }
        }
        if let Some(oi) = hit {
            let (ox, oy) = (state.ores[oi].x, state.ores[oi].y);
            apply_damage(state, oi, dmg);
            if splash > 0.0 {
                // 直撃対象以外へ着弾スプラッシュ (後ろからで index 安全)
                let splash_dmg = dmg * 0.45;
                for j in (0..state.ores.len()).rev() {
                    let d = (state.ores[j].x - ox).hypot(state.ores[j].y - oy);
                    if d > 0.01 && d <= splash {
                        apply_damage(state, j, splash_dmg);
                    }
                }
                burst(state, ox, oy, 8, 1.8, ParticleKind::Ember, 14);
            } else {
                burst(state, px, py, 2, 0.8, ParticleKind::Spark, 8);
            }
            if pierce == 0 {
                state.projectiles.swap_remove(i);
                continue;
            }
            state.projectiles[i].pierce -= 1;
        }
        i += 1;
    }

    state.projectiles.retain(|p| {
        p.life > 0 && p.x > -10.0 && p.x < WORLD_W + 10.0 && p.y > -10.0 && p.y < WORLD_H + 10.0
    });
    if state.projectiles.len() > 220 {
        let drop = state.projectiles.len() - 180;
        state.projectiles.drain(0..drop);
    }
}

fn step_ores(state: &mut StarRingState) {
    for ore in &mut state.ores {
        ore.x += ore.vx;
        ore.y += ore.vy;
    }
}

/// 中心到達: 報酬なしで消える (逸失)。星屑は減らない——防衛失敗ではない。
fn resolve_arrivals(state: &mut StarRingState) {
    let mut i = 0;
    while i < state.ores.len() {
        let ore = &state.ores[i];
        let dist = (ore.x - CX).hypot(ore.y - CY);
        if dist <= INNER_RADIUS {
            let ore = state.ores.remove(i);
            state.missed_count += 1;
            // 淡い取り込み演出のみ (警報にしない)
            burst(state, ore.x, ore.y, 4, 0.6, ParticleKind::Dust, 10);
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
    let layer = state.layer();
    let interval = Layer::spawn_interval_ticks(layer);
    if !state.elapsed_ticks.is_multiple_of(interval) {
        return;
    }
    if state.ores.len() >= 48 {
        return;
    }
    let batch = Layer::spawn_batch(layer).min(48 - state.ores.len());
    let hp_m = Layer::hp_mult(layer);
    let spd_m = Layer::speed_mult(layer);
    for _ in 0..batch {
        let kind = pick_ore_kind(state);
        let angle = rand_range(state, 0.0, std::f64::consts::TAU);
        let x = CX + angle.cos() * SPAWN_RADIUS;
        let y = CY + angle.sin() * SPAWN_RADIUS * ORBIT_Y_SQUASH.max(0.55);
        let dx = CX - x;
        let dy = CY - y;
        let dist = dx.hypot(dy).max(0.001);
        let speed = kind.speed() * spd_m;
        state.ores.push(Ore {
            x,
            y,
            vx: dx / dist * speed,
            vy: dy / dist * speed,
            hp: kind.base_hp() * hp_m,
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
            let depth = a.sin();
            (x, y, depth)
        })
        .collect()
}

fn fire_weapons(state: &mut StarRingState) {
    if state.ores.is_empty() {
        return;
    }
    let guns = turret_positions(state);
    if guns.is_empty() {
        return;
    }
    let unlocked: Vec<WeaponKind> = state.unlocked_weapons();
    for weapon in unlocked {
        let interval = state.fire_interval(weapon);
        // 武器ごとに位相をずらして同時斉射を避ける
        let phase = weapon.index() as u64 * 3;
        if !state.elapsed_ticks.wrapping_add(phase).is_multiple_of(interval) {
            continue;
        }
        let volley = state.volley_count(weapon);
        let dmg = state.weapon_damage(weapon);
        match weapon {
            WeaponKind::Pulse => fire_pulse(state, &guns, volley, dmg),
            WeaponKind::Ray => fire_ray(state, &guns, volley, dmg),
            WeaponKind::Scatter => fire_scatter(state, &guns, volley, dmg),
            WeaponKind::Arc => fire_arc(state, &guns, volley, dmg),
            WeaponKind::Nova => fire_nova(state, &guns, volley, dmg),
        }
    }
}

fn nearest_ore(state: &StarRingState, gx: f64, gy: f64) -> Option<usize> {
    let mut best: Option<(usize, f64)> = None;
    for (i, ore) in state.ores.iter().enumerate() {
        let d = (ore.x - gx).hypot(ore.y - gy);
        if best.map(|(_, bd)| d < bd).unwrap_or(true) {
            best = Some((i, d));
        }
    }
    best.map(|(i, _)| i)
}

fn aim_dir(state: &StarRingState, gx: f64, gy: f64, idx: usize) -> (f64, f64) {
    let ore = &state.ores[idx];
    let dx = ore.x - gx;
    let dy = ore.y - gy;
    let dist = dx.hypot(dy).max(0.001);
    (dx / dist, dy / dist)
}

fn fire_pulse(state: &mut StarRingState, guns: &[(f64, f64, f64)], volley: usize, dmg: f64) {
    let n = guns.len().max(1);
    for k in 0..volley {
        let (gx, gy, _) = guns[k % n];
        let Some(idx) = nearest_ore(state, gx, gy) else {
            return;
        };
        let (ux, uy) = aim_dir(state, gx, gy, idx);
        // わずかな拡散で連射感を出す
        let jitter = rand_range(state, -0.12, 0.12);
        let ang = uy.atan2(ux) + jitter;
        let speed = 2.4;
        state.projectiles.push(Projectile {
            x: gx,
            y: gy,
            vx: ang.cos() * speed,
            vy: ang.sin() * speed,
            damage: dmg,
            life: 22,
            radius: 0.55,
            pierce: 0,
            splash: 0.0,
            kind: WeaponKind::Pulse,
            spin: 0.0,
        });
    }
}

fn fire_ray(state: &mut StarRingState, guns: &[(f64, f64, f64)], volley: usize, dmg: f64) {
    let n = guns.len().max(1);
    for k in 0..volley {
        let (gx, gy, _) = guns[(k * 2) % n];
        let Some(idx) = nearest_ore(state, gx, gy) else {
            return;
        };
        let (ux, uy) = aim_dir(state, gx, gy, idx);
        let speed = 3.6;
        state.projectiles.push(Projectile {
            x: gx,
            y: gy,
            vx: ux * speed,
            vy: uy * speed,
            damage: dmg,
            life: 28,
            radius: 0.7,
            pierce: 2,
            splash: 0.0,
            kind: WeaponKind::Ray,
            spin: 0.0,
        });
    }
}

fn fire_scatter(state: &mut StarRingState, guns: &[(f64, f64, f64)], volley: usize, dmg: f64) {
    let n = guns.len().max(1);
    let (gx, gy, _) = guns[state.elapsed_ticks as usize % n];
    let Some(idx) = nearest_ore(state, gx, gy) else {
        return;
    };
    let (ux, uy) = aim_dir(state, gx, gy, idx);
    let base_ang = uy.atan2(ux);
    let spread = 0.55;
    for k in 0..volley {
        let t = if volley == 1 {
            0.0
        } else {
            (k as f64 / (volley - 1) as f64) - 0.5
        };
        let ang = base_ang + t * spread;
        let speed = 2.1;
        state.projectiles.push(Projectile {
            x: gx,
            y: gy,
            vx: ang.cos() * speed,
            vy: ang.sin() * speed,
            damage: dmg,
            life: 18,
            radius: 0.5,
            pierce: 0,
            splash: 0.0,
            kind: WeaponKind::Scatter,
            spin: 0.0,
        });
    }
}

fn fire_arc(state: &mut StarRingState, guns: &[(f64, f64, f64)], volley: usize, dmg: f64) {
    let n = guns.len().max(1);
    for k in 0..volley {
        let (gx, gy, _) = guns[k % n];
        let Some(idx) = nearest_ore(state, gx, gy) else {
            return;
        };
        let (ux, uy) = aim_dir(state, gx, gy, idx);
        let speed = 1.8;
        let spin = if k % 2 == 0 { 0.14 } else { -0.14 };
        state.projectiles.push(Projectile {
            x: gx,
            y: gy,
            vx: ux * speed,
            vy: uy * speed,
            damage: dmg,
            life: 30,
            radius: 0.65,
            pierce: 1,
            splash: 0.0,
            kind: WeaponKind::Arc,
            spin,
        });
    }
}

fn fire_nova(state: &mut StarRingState, guns: &[(f64, f64, f64)], volley: usize, dmg: f64) {
    let n = guns.len().max(1);
    for k in 0..volley {
        let (gx, gy, _) = guns[(k * 3) % n];
        let Some(idx) = nearest_ore(state, gx, gy) else {
            return;
        };
        let (ux, uy) = aim_dir(state, gx, gy, idx);
        let speed = 1.4;
        state.projectiles.push(Projectile {
            x: gx,
            y: gy,
            vx: ux * speed,
            vy: uy * speed,
            damage: dmg,
            life: 26,
            radius: 1.1,
            pierce: 0,
            splash: 5.5,
            kind: WeaponKind::Nova,
            spin: 0.0,
        });
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
        let interval = Layer::spawn_interval_ticks(state.layer());
        for _ in 0..interval {
            tick(&mut state, 1);
        }
        assert!(
            !state.ores.is_empty(),
            "スポーン間隔経過後に鉱石が出現するはず"
        );
    }

    #[test]
    fn killing_ore_with_projectile_increases_shards() {
        let mut state = StarRingState::new();
        let before = state.shards;
        // 砲台付近に弱い鉱石を置き、連射で倒す
        state.ores.push(Ore {
            x: CX + 12.0,
            y: CY,
            vx: 0.0,
            vy: 0.0,
            hp: 0.4,
            kind: OreKind::Dust,
            radius: 1.4,
        });
        for _ in 0..40 {
            tick(&mut state, 1);
            if state.total_kills > 0 {
                break;
            }
        }
        assert!(
            state.total_kills > 0 || state.shards > before,
            "連射弾で撃破できるはず shards={}->{} kills={} projs={}",
            before,
            state.shards,
            state.total_kills,
            state.projectiles.len()
        );
        assert!(state.shards_earned > 0.0 || state.total_kills > 0);
    }

    #[test]
    fn arrival_at_core_does_not_drain_shards() {
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
        assert!(state.missed_count >= 1);
        assert!(
            (state.shards - 50.0).abs() < 1e-9,
            "中心到達で星屑は減らないはず shards={}",
            state.shards
        );
    }

    #[test]
    fn purchase_weapon_stat_spends_shards() {
        let mut state = StarRingState::new();
        state.shards = 1000.0;
        let cost = weapon_stat_cost(&state, WeaponKind::Pulse, WeaponStat::Power);
        assert!(purchase_weapon_stat(
            &mut state,
            WeaponKind::Pulse,
            WeaponStat::Power
        ));
        assert_eq!(state.weapon_stat(WeaponKind::Pulse, WeaponStat::Power), 1);
        assert!((state.shards - (1000.0 - cost)).abs() < 1e-6);
    }

    #[test]
    fn locked_weapon_cannot_be_upgraded() {
        let mut state = StarRingState::new();
        state.shards = 1e9;
        assert_eq!(state.layer(), 1);
        assert!(!purchase_weapon_stat(
            &mut state,
            WeaponKind::Ray,
            WeaponStat::Power
        ));
    }

    #[test]
    fn layer_advances_with_kills_and_unlocks_weapons() {
        let mut state = StarRingState::new();
        assert_eq!(state.layer(), 1);
        assert!(state.is_weapon_unlocked(WeaponKind::Pulse));
        assert!(!state.is_weapon_unlocked(WeaponKind::Ray));

        state.total_kills = Layer::THRESHOLDS[1];
        assert_eq!(state.layer(), 2);
        assert!(state.is_weapon_unlocked(WeaponKind::Ray));
        assert!(state.unlocked_ore_kinds().contains(&OreKind::Rock));

        state.total_kills = Layer::THRESHOLDS[4];
        assert_eq!(state.layer(), 5);
        assert_eq!(state.unlocked_weapons().len(), 5);
    }

    #[test]
    fn layer_thresholds_are_spaced() {
        // ぬるっと上がらないこと: 隣接閾値の間隔が十分ある
        for w in Layer::THRESHOLDS.windows(2) {
            assert!(w[1] >= w[0] + 70, "層間隔が狭すぎる {} -> {}", w[0], w[1]);
        }
    }

    #[test]
    fn higher_layer_spawns_more_and_harder() {
        assert!(Layer::spawn_batch(5) > Layer::spawn_batch(1));
        assert!(Layer::hp_mult(5) > Layer::hp_mult(1));
        assert!(Layer::value_mult(5) > Layer::value_mult(1));
        assert!(Layer::spawn_interval_ticks(5) < Layer::spawn_interval_ticks(1));
    }

    #[test]
    fn pulse_fires_many_weak_projectiles() {
        let mut state = StarRingState::new();
        // 連射・弾数を上げて弾幕感を出す
        state.weapon_levels[0] = [3, 4, 0];
        state.ores.push(Ore {
            x: CX + 20.0,
            y: CY,
            vx: 0.0,
            vy: 0.0,
            hp: 100.0,
            kind: OreKind::Dust,
            radius: 1.4,
        });
        for _ in 0..30 {
            tick(&mut state, 1);
        }
        assert!(
            state.projectiles.len() >= 3 || state.total_kills > 0,
            "流星は複数弾を飛ばすはず projs={}",
            state.projectiles.len()
        );
        // 1発は弱い
        assert!(state.weapon_damage(WeaponKind::Pulse) < 2.0);
    }

    #[test]
    fn manual_strike_applies_boost() {
        let mut state = StarRingState::new();
        manual_strike(&mut state);
        assert_eq!(state.boost_ticks, BOOST_DURATION);
    }

    #[test]
    fn cycle_weapon_skips_locked() {
        let mut state = StarRingState::new();
        state.total_kills = Layer::THRESHOLDS[2]; // layer 3
        state.selected_weapon = WeaponKind::Pulse;
        cycle_selected_weapon(&mut state, 1);
        assert_eq!(state.selected_weapon, WeaponKind::Ray);
        cycle_selected_weapon(&mut state, 1);
        assert_eq!(state.selected_weapon, WeaponKind::Scatter);
        cycle_selected_weapon(&mut state, 1);
        assert_eq!(state.selected_weapon, WeaponKind::Pulse);
    }

    #[test]
    fn layer_flash_triggers_on_advance() {
        let mut state = StarRingState::new();
        state.total_kills = Layer::THRESHOLDS[1] - 1;
        state.last_layer = 1;
        // あと1撃で層2
        state.ores.push(Ore {
            x: CX + 12.0,
            y: CY,
            vx: 0.0,
            vy: 0.0,
            hp: 0.1,
            kind: OreKind::Dust,
            radius: 1.4,
        });
        state.weapon_levels[0][WeaponStat::Power.index()] = 8;
        for _ in 0..50 {
            tick(&mut state, 1);
            if state.layer_flash_ticks > 0 {
                break;
            }
        }
        assert!(
            state.layer() >= 2,
            "撃破で層が進むはず layer={}",
            state.layer()
        );
        assert!(
            state.layer_flash_ticks > 0 || state.last_layer >= 2,
            "層到達フラッシュが走るはず"
        );
    }
}
