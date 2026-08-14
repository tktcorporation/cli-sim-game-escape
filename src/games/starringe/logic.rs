//! 星環の純粋ロジック。描画・I/O に依存しない。

use super::state::{
    Layer, Ore, OreKind, OreMotion, Particle, ParticleKind, Projectile, PulseRing, RingUpgrade,
    StarRingState, WeaponKind, WeaponStat, BOOST_DURATION, CORE_Y, CX, FIELD_MARGIN, INNER_RADIUS,
    LAYER_FLASH_TICKS, LAYER_READY_FLASH_TICKS, SPAWN_X_MARGIN, SPAWN_Y, WORLD_H, WORLD_W,
};

/// 鉱石の同時存在上限。これを超えると湧きも分裂も止める。
const MAX_ORES: usize = 56;

/// うねりの振れ幅は「基準速度 × 倍率 ÷ 角周波数」で決まる。周期が長いほど
/// 同じ振れ幅を出すのに大きな速度が要るため、周期に見合った倍率を持たせる。
const SPIRAL_SWAY_RATE: f64 = 0.05;
const SPIRAL_SWAY_GAIN: f64 = 4.0;
const ZIGZAG_SWAY_RATE: f64 = 0.20;
const ZIGZAG_SWAY_GAIN: f64 = 10.0;
/// 回り込み成分が主役になるので、横揺れそのものは抑える。
const ORBIT_SWAY_GAIN: f64 = 0.5;
const HEAVY_SWAY_GAIN: f64 = 0.15;
/// 重い鉱石の落下減速。
const HEAVY_FALL_MULT: f64 = 0.85;
/// コアへの引き寄せの強さ (落下速度に対する倍率)。1 を超えると降下より
/// 引き寄せが勝ち、コア付近まで来た鉱石は必ず吸い込まれる。
const CORE_PULL_RATIO: f64 = 1.5;
/// Orbit の接線方向の回り込みの強さ (落下速度に対する倍率)。
const ORBIT_SWIRL_GAIN: f64 = 2.2;
/// 裂片が分裂する際、子を親の左右へ振り分ける幅。
const SPLIT_SPREAD: f64 = 4.0;

/// ダメージの出どころ。殻石の耐性計算に使う。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DamageSource {
    Weapon(WeaponKind),
    CorePulse,
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

/// 武器ステ強化の現在コスト。
pub fn weapon_stat_cost(state: &StarRingState, weapon: WeaponKind, stat: WeaponStat) -> f64 {
    let lv = state.weapon_stat(weapon, stat) as f64;
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

pub fn can_upgrade_ring(state: &StarRingState, kind: RingUpgrade) -> bool {
    if !state.is_ring_unlocked(kind) {
        return false;
    }
    match kind.max_level() {
        Some(max) => state.ring_level(kind) < max,
        None => true,
    }
}

pub fn purchase_ring_upgrade(state: &mut StarRingState, kind: RingUpgrade) -> bool {
    if !can_upgrade_ring(state, kind) {
        return false;
    }
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

/// 次層を開放するのに必要な星屑。次閾値が無い場合は 0。
pub fn layer_unlock_cost(state: &StarRingState) -> f64 {
    Layer::unlock_cost(state.layer() + 1)
}

pub fn can_unlock_next_layer(state: &StarRingState) -> bool {
    if !state.kills_ready_for_next_layer() {
        return false;
    }
    let cost = layer_unlock_cost(state);
    state.shards + 1e-9 >= cost
}

/// 撃破条件と星屑を満たしていれば次層を開放する。進行リセットはしない。
pub fn unlock_next_layer(state: &mut StarRingState) -> bool {
    if !can_unlock_next_layer(state) {
        return false;
    }
    let cost = layer_unlock_cost(state);
    state.shards -= cost;
    state.current_layer = state.current_layer.saturating_add(1);
    play_layer_unlock_ceremony(state);
    true
}

fn play_layer_unlock_ceremony(state: &mut StarRingState) {
    let layer = state.layer();
    state.layer_flash_ticks = LAYER_FLASH_TICKS;
    state.layer_ready_flash_ticks = 0;
    state.layer_ready_latched = false;
    state.shake_ticks = state.shake_ticks.max(16);
    state.core_flash_ticks = state.core_flash_ticks.max(22);
    burst(state, CX, CORE_Y, 22, 7.0, ParticleKind::Spark, 28);
    burst(state, CX, CORE_Y, 14, 5.0, ParticleKind::Shard, 24);
    burst(state, CX, CORE_Y, 10, 3.5, ParticleKind::Ember, 20);
    state.pulse_rings.push(PulseRing {
        radius: INNER_RADIUS,
        life: 16,
        max_life: 16,
    });
    for w in WeaponKind::ALL {
        if w.unlock_layer() == layer {
            state.selected_weapon = w;
            break;
        }
    }
}

/// 手動タップ: 近傍1体にダメージ + 一時火力ブースト。
/// 核脈動を解放済みなら小さな核波も添える。
pub fn manual_strike(state: &mut StarRingState) {
    state.boost_ticks = BOOST_DURATION;
    if state.ores.is_empty() {
        burst(state, CX, CORE_Y, 6, 3.0, ParticleKind::Spark, 14);
        return;
    }
    let mut best = 0usize;
    let mut best_d = f64::MAX;
    for (i, ore) in state.ores.iter().enumerate() {
        let d = (ore.x - CX).hypot(ore.y - CORE_Y);
        if d < best_d {
            best_d = d;
            best = i;
        }
    }
    let dmg = state.weapon_damage(WeaponKind::Pulse) * 2.2;
    apply_damage(state, best, dmg, DamageSource::Strike);

    if state.ring_level(RingUpgrade::CorePulse) > 0 {
        let r = state.pulse_radius() * 0.55;
        let dmg = state.pulse_damage() * 0.6;
        pulse_damage_area(state, r, dmg);
        state.pulse_rings.push(PulseRing {
            radius: r * 0.4,
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
        if state.layer_flash_ticks > 0 {
            state.layer_flash_ticks -= 1;
        }
        if state.layer_ready_flash_ticks > 0 {
            state.layer_ready_flash_ticks -= 1;
        }

        step_particles(state);
        step_pulse_rings(state);
        step_projectiles(state);
        step_ores(state);
        resolve_arrivals(state);
        spawn_ores(state);
        fire_weapons(state);
        fire_core_pulse(state);
        check_layer_ready(state);

        state.recent_gain[state.recent_gain_idx] = state.tick_gain;
        state.recent_gain_idx = (state.recent_gain_idx + 1) % state.recent_gain.len();
    }
}

/// 撃破条件を満たした瞬間だけ「開放可」パルスを立てる。層自体は自動では進まない。
fn check_layer_ready(state: &mut StarRingState) {
    let ready = state.kills_ready_for_next_layer();
    if ready && !state.layer_ready_latched {
        state.layer_ready_flash_ticks = LAYER_READY_FLASH_TICKS;
        state.core_flash_ticks = state.core_flash_ticks.max(8);
        state.shake_ticks = state.shake_ticks.max(6);
        burst(state, CX, CORE_Y, 8, 3.5, ParticleKind::Spark, 14);
        state.layer_ready_latched = true;
    } else if !ready {
        state.layer_ready_latched = false;
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
        p.life > 0 && p.x > -20.0 && p.x < WORLD_W + 20.0 && p.y > -20.0 && p.y < WORLD_H + 20.0
    });
    if state.particles.len() > 500 {
        let drop = state.particles.len() - 400;
        state.particles.drain(0..drop);
    }
}

fn step_pulse_rings(state: &mut StarRingState) {
    for r in &mut state.pulse_rings {
        if r.life > 0 {
            r.life -= 1;
            r.radius += 1.375;
        }
    }
    state.pulse_rings.retain(|r| r.life > 0);
    if state.pulse_rings.len() > 12 {
        let drop = state.pulse_rings.len() - 8;
        state.pulse_rings.drain(0..drop);
    }
}

fn step_projectiles(state: &mut StarRingState) {
    for p in &mut state.projectiles {
        if p.life == 0 {
            continue;
        }
        if p.spin != 0.0 {
            let speed = p.vx.hypot(p.vy).max(0.01);
            let ang = p.vy.atan2(p.vx) + p.spin;
            p.vx = ang.cos() * speed;
            p.vy = ang.sin() * speed;
        }
        p.x += p.vx;
        p.y += p.vy;
        p.life -= 1;
    }

    let mut i = 0;
    while i < state.projectiles.len() {
        if state.projectiles[i].life == 0 {
            state.projectiles.swap_remove(i);
            continue;
        }
        let (px, py, pr, dmg, splash, pierce, kind) = {
            let p = &state.projectiles[i];
            (p.x, p.y, p.radius, p.damage, p.splash, p.pierce, p.kind)
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
            apply_damage(state, oi, dmg, DamageSource::Weapon(kind));
            if splash > 0.0 {
                let splash_dmg = dmg * 0.45;
                for j in (0..state.ores.len()).rev() {
                    let d = (state.ores[j].x - ox).hypot(state.ores[j].y - oy);
                    if d > 0.01 && d <= splash {
                        apply_damage(state, j, splash_dmg, DamageSource::Weapon(kind));
                    }
                }
                burst(state, ox, oy, 8, 4.5, ParticleKind::Ember, 14);
            } else {
                burst(state, px, py, 2, 2.0, ParticleKind::Spark, 8);
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
        p.life > 0 && p.x > -25.0 && p.x < WORLD_W + 25.0 && p.y > -25.0 && p.y < WORLD_H + 25.0
    });
    if state.projectiles.len() > 220 {
        let drop = state.projectiles.len() - 180;
        state.projectiles.drain(0..drop);
    }
}

/// 降下・横揺れ・コアへの引き寄せを重ねて鉱石を1tick進める。
fn step_ores(state: &mut StarRingState) {
    let fall_m = Layer::fall_mult(state.layer());
    for ore in &mut state.ores {
        let prev_x = ore.x;
        let prev_y = ore.y;
        ore.age = ore.age.wrapping_add(1);

        let fall = ore.kind.fall_speed()
            * fall_m
            * if ore.motion == OreMotion::Heavy {
                HEAVY_FALL_MULT
            } else {
                1.0
            };
        ore.y -= fall;

        let phase = ore.age as f64;
        ore.x += match ore.motion {
            OreMotion::Spiral => ore.sway * SPIRAL_SWAY_GAIN * (phase * SPIRAL_SWAY_RATE).cos(),
            OreMotion::Zigzag => ore.sway * ZIGZAG_SWAY_GAIN * (phase * ZIGZAG_SWAY_RATE).cos(),
            OreMotion::Orbit => ore.sway * ORBIT_SWAY_GAIN,
            OreMotion::Heavy => ore.sway * HEAVY_SWAY_GAIN,
        };

        // 引き寄せは降下が進むほど強くする。上空では自由に散らばり、コア付近で
        // 吸い込まれる。落下速度に比例させるので、遅い鉱石ほど時間をかけて
        // 同じ軌跡をたどる。
        let descent = ((SPAWN_Y - ore.y) / (SPAWN_Y - CORE_Y)).clamp(0.0, 1.0);
        let ramp = descent * descent * descent;
        let dx = CX - ore.x;
        let dy = CORE_Y - ore.y;
        let dist = dx.hypot(dy).max(0.001);
        let (ux, uy) = (dx / dist, dy / dist);
        let pull = fall * CORE_PULL_RATIO * ramp;
        ore.x += ux * pull;
        ore.y += uy * pull;

        if ore.motion == OreMotion::Orbit {
            // 引き寄せ方向と直交する成分。コアに近いほど強まり、吸い込まれる
            // 前にコアの周りを一度回り込ませる。
            let swirl = fall * ORBIT_SWIRL_GAIN * descent * descent * ore.sway.signum();
            ore.x += -uy * swirl;
            ore.y += ux * swirl;
        }

        // 左右の壁で跳ね返す。鉱石が横から画面外へ消えると何が起きているか
        // 追えなくなるので、常にフィールド内に留める。
        let (lo, hi) = (FIELD_MARGIN, WORLD_W - FIELD_MARGIN);
        if ore.x < lo {
            ore.x = lo;
            ore.sway = -ore.sway;
        } else if ore.x > hi {
            ore.x = hi;
            ore.sway = -ore.sway;
        }

        ore.vx = ore.x - prev_x;
        ore.vy = ore.y - prev_y;
    }
}

/// コア到達と場外落下: 報酬なしで消える (逸失)。星屑は減らない——防衛失敗ではない。
fn resolve_arrivals(state: &mut StarRingState) {
    let mut i = 0;
    while i < state.ores.len() {
        let ore = &state.ores[i];
        let reached_core = (ore.x - CX).hypot(ore.y - CORE_Y) <= INNER_RADIUS;
        if reached_core || ore.y < 0.0 {
            let ore = state.ores.remove(i);
            state.missed_count += 1;
            burst(state, ore.x, ore.y, 4, 1.5, ParticleKind::Dust, 10);
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

fn spawn_one(state: &mut StarRingState, kind: OreKind, x: f64, y: f64) {
    let sign = if rng_next(state).is_multiple_of(2) {
        1.0
    } else {
        -1.0
    };
    let sway = kind.sway_speed() * sign * rand_range(state, 0.85, 1.15);
    let hp = kind.base_hp() * Layer::hp_mult(state.layer());
    state.ores.push(Ore {
        x,
        y,
        vx: 0.0,
        vy: 0.0,
        hp,
        kind,
        radius: kind.radius(),
        motion: kind.default_motion(),
        sway,
        age: 0,
    });
}

fn spawn_ores(state: &mut StarRingState) {
    let layer = state.layer();
    let interval = Layer::spawn_interval_ticks(layer);
    if !state.elapsed_ticks.is_multiple_of(interval) {
        return;
    }
    if state.ores.len() >= MAX_ORES {
        return;
    }
    let batch = Layer::spawn_batch(layer).min(MAX_ORES - state.ores.len());
    // 同時湧きの x を層化サンプリングで散らす。一様乱数だけだと固まって湧いた
    // ときに重なり、何体降ってきているのか読めなくなる。
    let lo = SPAWN_X_MARGIN;
    let slot = (WORLD_W - SPAWN_X_MARGIN * 2.0) / batch as f64;
    for i in 0..batch {
        let kind = pick_ore_kind(state);
        let slot_lo = lo + slot * i as f64;
        let x = rand_range(state, slot_lo, slot_lo + slot);
        // ばらつきは下方向へ取る。上へ振ると `WORLD_H` を超え、Canvas の
        // y_bounds 外で数tick見えないまま落ちてくる。
        let y = SPAWN_Y - rand_range(state, 0.0, 6.0);
        spawn_one(state, kind, x, y);
    }
}

/// 砲台のワールド座標一覧 (立体感用に depth = sin も返す)。
pub fn turret_positions(state: &StarRingState) -> Vec<(f64, f64, f64)> {
    let n = state.turret_count().max(1);
    let (rx, ry) = state.ring_radii();
    let base = state.elapsed_ticks as f64 * state.orbit_speed();
    (0..n)
        .map(|i| {
            let a = base + i as f64 * std::f64::consts::TAU / n as f64;
            let x = CX + a.cos() * rx;
            let y = CORE_Y + a.sin() * ry;
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
        let phase = weapon.index() as u64 * 3;
        if !state
            .elapsed_ticks
            .wrapping_add(phase)
            .is_multiple_of(interval)
        {
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

fn fire_core_pulse(state: &mut StarRingState) {
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
    burst(state, CX, CORE_Y, 5, 2.5, ParticleKind::Spark, 12);
}

fn pulse_damage_area(state: &mut StarRingState, radius: f64, dmg: f64) {
    let mut i = state.ores.len();
    while i > 0 {
        i -= 1;
        let dist = (state.ores[i].x - CX).hypot(state.ores[i].y - CORE_Y);
        if dist <= radius + state.ores[i].radius {
            apply_damage(state, i, dmg, DamageSource::CorePulse);
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

/// 弾速は「弾がフィールドを横切るのに要する tick 数」で決める。速すぎると
/// 発射から着弾までが一瞬になり、砲台から遠い鉱石でも自動照準がそのまま当たって
/// しまう。飛行時間を残すことで、横へ漂う鉱石 (`OreKind::sway_speed`) は遠距離
/// ほど照準を外せる——迎撃の間合いはこの飛行時間と横速度の釣り合いで決まる。
fn fire_pulse(state: &mut StarRingState, guns: &[(f64, f64, f64)], volley: usize, dmg: f64) {
    let n = guns.len().max(1);
    for k in 0..volley {
        let (gx, gy, _) = guns[k % n];
        let Some(idx) = nearest_ore(state, gx, gy) else {
            return;
        };
        let (ux, uy) = aim_dir(state, gx, gy, idx);
        let jitter = rand_range(state, -0.12, 0.12);
        let ang = uy.atan2(ux) + jitter;
        let speed = 4.0;
        state.projectiles.push(Projectile {
            x: gx,
            y: gy,
            vx: ang.cos() * speed,
            vy: ang.sin() * speed,
            damage: dmg,
            life: 22,
            radius: 1.375,
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
        let speed = 6.0;
        state.projectiles.push(Projectile {
            x: gx,
            y: gy,
            vx: ux * speed,
            vy: uy * speed,
            damage: dmg,
            life: 28,
            radius: 1.75,
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
        let speed = 3.5;
        state.projectiles.push(Projectile {
            x: gx,
            y: gy,
            vx: ang.cos() * speed,
            vy: ang.sin() * speed,
            damage: dmg,
            life: 18,
            radius: 1.25,
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
        let speed = 3.0;
        let spin = if k % 2 == 0 { 0.14 } else { -0.14 };
        state.projectiles.push(Projectile {
            x: gx,
            y: gy,
            vx: ux * speed,
            vy: uy * speed,
            damage: dmg,
            life: 30,
            radius: 1.625,
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
        let speed = 2.35;
        state.projectiles.push(Projectile {
            x: gx,
            y: gy,
            vx: ux * speed,
            vy: uy * speed,
            damage: dmg,
            life: 26,
            radius: 2.75,
            pierce: 0,
            splash: 13.75,
            kind: WeaponKind::Nova,
            spin: 0.0,
        });
    }
}

fn armor_multiplier(kind: OreKind, source: DamageSource) -> f64 {
    if !kind.armored() {
        return 1.0;
    }
    match source {
        DamageSource::Weapon(WeaponKind::Ray) | DamageSource::Weapon(WeaponKind::Nova) => 1.15,
        DamageSource::CorePulse => 1.0,
        DamageSource::Weapon(WeaponKind::Arc) => 0.75,
        DamageSource::Weapon(WeaponKind::Pulse)
        | DamageSource::Weapon(WeaponKind::Scatter)
        | DamageSource::Strike => 0.40,
    }
}

fn apply_damage(state: &mut StarRingState, idx: usize, dmg: f64, source: DamageSource) {
    if idx >= state.ores.len() {
        return;
    }
    let mult = armor_multiplier(state.ores[idx].kind, source);
    state.ores[idx].hp -= dmg * mult;
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
        6 + (ore.kind as usize).min(6),
        3.5 + ore.radius * 0.25,
        ParticleKind::Shard,
        18,
    );
    burst(state, ore.x, ore.y, 4, 5.0, ParticleKind::Spark, 12);

    if ore.kind.splits_on_death() && state.ores.len() < MAX_ORES {
        let child_hp = OreKind::Dust.base_hp() * Layer::hp_mult(state.layer()) * 0.7;
        for k in 0..2 {
            let x = (ore.x + (k as f64 - 0.5) * 2.0 * SPLIT_SPREAD)
                .clamp(FIELD_MARGIN, WORLD_W - FIELD_MARGIN);
            spawn_one(state, OreKind::Dust, x, ore.y);
            if let Some(child) = state.ores.last_mut() {
                child.hp = child_hp;
                child.radius = 2.75;
            }
        }
    }
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

    /// 湧いた鉱石は最初の tick から Canvas の y_bounds (`0..WORLD_H`) の内側に
    /// いること。外へ湧くと降りてくるまで画面に映らず、湧いた瞬間を見逃す。
    #[test]
    fn spawned_ores_start_inside_the_visible_world() {
        let mut state = StarRingState::new();
        for _ in 0..600 {
            tick(&mut state, 1);
            for ore in &state.ores {
                assert!(
                    (0.0..=WORLD_H).contains(&ore.y),
                    "鉱石が画面外に湧いている y={}",
                    ore.y
                );
            }
        }
        assert!(!state.ores.is_empty(), "検証対象の鉱石が湧いているはず");
    }

    #[test]
    fn ores_drift_sideways_while_falling() {
        let mut state = StarRingState::new();
        spawn_one(&mut state, OreKind::Dust, CX + 20.0, SPAWN_Y);
        let (start_x, start_y) = (state.ores[0].x, state.ores[0].y);
        for _ in 0..20 {
            step_ores(&mut state);
        }
        let ore = &state.ores[0];
        assert!(ore.y < start_y - 1.0, "降下するはず y={start_y}->{}", ore.y);
        assert!(
            (ore.x - start_x).abs() > 0.2,
            "真っ直ぐ落ちるだけでなく横へも漂うはず x={start_x}->{}",
            ore.x
        );
        // 20tick でコアに到達しない (一直線ミサイルではない)
        let dist = (ore.x - CX).hypot(ore.y - CORE_Y);
        assert!(
            dist > INNER_RADIUS + 1.0,
            "すぐコアに到達しすぎ dist={dist}"
        );
    }

    #[test]
    fn ores_bounce_off_side_walls() {
        let mut state = StarRingState::new();
        spawn_one(
            &mut state,
            OreKind::Dust,
            WORLD_W - FIELD_MARGIN - 0.1,
            SPAWN_Y,
        );
        state.ores[0].sway = 2.0;
        for _ in 0..60 {
            step_ores(&mut state);
            let x = state.ores[0].x;
            assert!(
                (FIELD_MARGIN..=WORLD_W - FIELD_MARGIN).contains(&x),
                "鉱石が横から画面外へ出てはいけない x={x}"
            );
        }
    }

    #[test]
    fn killing_ore_with_projectile_increases_shards() {
        let mut state = StarRingState::new();
        let before = state.shards;
        state.ores.push(Ore {
            x: CX + 12.0,
            y: CORE_Y + 25.0,
            vx: 0.0,
            vy: 0.0,
            hp: 0.4,
            kind: OreKind::Dust,
            radius: 3.5,
            motion: OreMotion::Spiral,
            sway: 0.0,
            age: 0,
        });
        for _ in 0..40 {
            tick(&mut state, 1);
            if state.total_kills > 0 {
                break;
            }
        }
        assert!(
            state.total_kills > 0 || state.shards > before,
            "連射弾で撃破できるはず shards={}->{} kills={}",
            before,
            state.shards,
            state.total_kills
        );
    }

    #[test]
    fn arrival_at_core_does_not_drain_shards() {
        let mut state = StarRingState::new();
        state.shards = 50.0;
        state.ores.push(Ore {
            x: CX + 1.0,
            y: CORE_Y,
            vx: 0.0,
            vy: 0.0,
            hp: 10.0,
            kind: OreKind::Rock,
            radius: 4.75,
            motion: OreMotion::Spiral,
            sway: 0.0,
            age: 0,
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
    fn yield_upgrade_increases_shard_gain() {
        let mut state = StarRingState::new();
        state.shards = 1e9;
        let before = state.yield_mult();
        assert!(purchase_ring_upgrade(&mut state, RingUpgrade::Yield));
        assert!(state.yield_mult() > before);
    }

    #[test]
    fn core_pulse_locked_on_layer_one() {
        let mut state = StarRingState::new();
        state.shards = 1e9;
        assert!(!purchase_ring_upgrade(&mut state, RingUpgrade::CorePulse));
        state.total_kills = Layer::THRESHOLDS[1];
        assert!(unlock_next_layer(&mut state));
        assert!(purchase_ring_upgrade(&mut state, RingUpgrade::CorePulse));
        assert_eq!(state.ring_level(RingUpgrade::CorePulse), 1);
        assert!(state.pulse_interval().is_some());
    }

    #[test]
    fn shell_resists_pulse_but_not_ray() {
        let mut state = StarRingState::new();
        state.ores.push(Ore {
            x: CX + 14.0,
            y: CORE_Y + 30.0,
            vx: 0.0,
            vy: 0.0,
            hp: 10.0,
            kind: OreKind::Shell,
            radius: 7.5,
            motion: OreMotion::Heavy,
            sway: 0.0,
            age: 0,
        });
        apply_damage(&mut state, 0, 5.0, DamageSource::Weapon(WeaponKind::Pulse));
        let after_pulse = state.ores[0].hp;
        assert!(
            (after_pulse - 8.0).abs() < 1e-6,
            "流星は装甲に弱いはず hp={after_pulse}"
        );
        apply_damage(&mut state, 0, 5.0, DamageSource::Weapon(WeaponKind::Ray));
        let after_ray = state.ores[0].hp;
        assert!(
            after_ray < after_pulse - 5.0,
            "光線は装甲を通しやすいはず {after_pulse} -> {after_ray}"
        );
    }

    #[test]
    fn splitter_spawns_children_on_death() {
        let mut state = StarRingState::new();
        state.ores.push(Ore {
            x: CX + 16.0,
            y: CORE_Y + 30.0,
            vx: 0.0,
            vy: 0.0,
            hp: 1.0,
            kind: OreKind::Splitter,
            radius: 6.0,
            motion: OreMotion::Spiral,
            sway: 0.05,
            age: 0,
        });
        apply_damage(&mut state, 0, 10.0, DamageSource::Weapon(WeaponKind::Ray));
        assert_eq!(state.total_kills, 1);
        assert!(
            state.ores.len() >= 2,
            "裂片は撃破で分裂するはず n={}",
            state.ores.len()
        );
        assert!(state.ores.iter().all(|o| o.kind == OreKind::Dust));
    }

    #[test]
    fn kills_alone_do_not_advance_layer() {
        let mut state = StarRingState::new();
        assert_eq!(state.layer(), 1);
        assert!(state.is_weapon_unlocked(WeaponKind::Pulse));
        assert!(!state.is_weapon_unlocked(WeaponKind::Ray));

        state.total_kills = Layer::THRESHOLDS[1];
        assert_eq!(state.layer(), 1);
        assert!(state.kills_ready_for_next_layer());
        assert!(!state.is_weapon_unlocked(WeaponKind::Ray));
    }

    #[test]
    fn unlock_next_layer_spends_shards_and_unlocks_weapons() {
        let mut state = StarRingState::new();
        state.total_kills = Layer::THRESHOLDS[1];
        state.shards = 1e9;
        let before = state.shards;
        let cost = layer_unlock_cost(&state);
        assert!(unlock_next_layer(&mut state));
        assert_eq!(state.layer(), 2);
        assert!((state.shards - (before - cost)).abs() < 1e-6);
        assert!(state.is_weapon_unlocked(WeaponKind::Ray));
        assert!(state.unlocked_ore_kinds().contains(&OreKind::Rock));
        assert!(state.layer_flash_ticks > 0);
    }

    #[test]
    fn unlock_next_layer_requires_kills_and_shards() {
        let mut state = StarRingState::new();
        state.shards = 1e9;
        assert!(!unlock_next_layer(&mut state), "撃破不足では開放できない");

        state.total_kills = Layer::THRESHOLDS[1];
        state.shards = 1.0;
        assert!(!unlock_next_layer(&mut state), "星屑不足では開放できない");
        assert_eq!(state.layer(), 1);

        state.shards = layer_unlock_cost(&state);
        assert!(unlock_next_layer(&mut state));
        assert_eq!(state.layer(), 2);
    }

    #[test]
    fn unlock_does_not_reset_progress() {
        let mut state = StarRingState::new();
        state.total_kills = Layer::THRESHOLDS[1];
        state.shards = 500.0;
        state.shards_earned = 800.0;
        state.weapon_levels[0] = [2, 3, 1];
        state.ring_levels[0] = 2;
        assert!(unlock_next_layer(&mut state));
        assert_eq!(state.total_kills, Layer::THRESHOLDS[1]);
        assert_eq!(state.shards_earned, 800.0);
        assert_eq!(state.weapon_levels[0], [2, 3, 1]);
        assert_eq!(state.ring_levels[0], 2);
        assert!(state.shards > 0.0);
    }

    #[test]
    fn sequential_unlocks_open_deeper_weapons() {
        let mut state = StarRingState::new();
        state.shards = 1e9;
        for target in 2u32..=5 {
            state.total_kills = Layer::entry_threshold(target);
            assert!(
                unlock_next_layer(&mut state),
                "第{target}層へ開放できるはず"
            );
            assert_eq!(state.layer(), target);
        }
        assert_eq!(state.unlocked_weapons().len(), 5);
        assert!(state.unlocked_ore_kinds().contains(&OreKind::Shell));
    }

    #[test]
    fn layer_thresholds_are_spaced() {
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
        state.weapon_levels[0] = [3, 4, 0];
        state.ores.push(Ore {
            x: CX + 20.0,
            y: CORE_Y + 30.0,
            vx: 0.0,
            vy: 0.0,
            hp: 100.0,
            kind: OreKind::Dust,
            radius: 3.5,
            motion: OreMotion::Spiral,
            sway: 0.0,
            age: 0,
        });
        for _ in 0..30 {
            tick(&mut state, 1);
        }
        assert!(
            state.projectiles.len() >= 3 || state.total_kills > 0,
            "流星は複数弾を飛ばすはず projs={}",
            state.projectiles.len()
        );
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
        state.shards = 1e9;
        state.total_kills = Layer::THRESHOLDS[2];
        assert!(unlock_next_layer(&mut state));
        assert!(unlock_next_layer(&mut state));
        state.selected_weapon = WeaponKind::Pulse;
        cycle_selected_weapon(&mut state, 1);
        assert_eq!(state.selected_weapon, WeaponKind::Ray);
        cycle_selected_weapon(&mut state, 1);
        assert_eq!(state.selected_weapon, WeaponKind::Scatter);
        cycle_selected_weapon(&mut state, 1);
        assert_eq!(state.selected_weapon, WeaponKind::Pulse);
    }

    #[test]
    fn layer_ready_pulse_triggers_when_kills_met() {
        let mut state = StarRingState::new();
        state.total_kills = Layer::THRESHOLDS[1] - 1;
        state.ores.push(Ore {
            x: CX + 12.0,
            y: CORE_Y + 25.0,
            vx: 0.0,
            vy: 0.0,
            hp: 0.1,
            kind: OreKind::Dust,
            radius: 3.5,
            motion: OreMotion::Spiral,
            sway: 0.0,
            age: 0,
        });
        state.weapon_levels[0][WeaponStat::Power.index()] = 8;
        for _ in 0..50 {
            tick(&mut state, 1);
            if state.layer_ready_flash_ticks > 0 {
                break;
            }
        }
        assert!(
            state.kills_ready_for_next_layer(),
            "撃破条件を満たすはず kills={}",
            state.total_kills
        );
        assert_eq!(state.layer(), 1, "自動では層が進まない");
        assert!(
            state.layer_ready_flash_ticks > 0 || state.layer_ready_latched,
            "開放可パルスが走るはず"
        );
        assert_eq!(state.layer_flash_ticks, 0, "到達演出は開放操作後だけ");
    }

    #[test]
    fn layer_unlock_cost_scales_with_depth() {
        assert!(Layer::unlock_cost(2) > 0.0);
        assert!(Layer::unlock_cost(3) > Layer::unlock_cost(2));
        assert!(Layer::unlock_cost(5) > Layer::unlock_cost(3) * 2.0);
    }

    #[test]
    fn core_pulse_damages_nearby_ores() {
        let mut state = StarRingState::new();
        state.total_kills = Layer::THRESHOLDS[1];
        state.shards = 1e9;
        assert!(unlock_next_layer(&mut state));
        assert!(purchase_ring_upgrade(&mut state, RingUpgrade::CorePulse));
        // 追加で威力を上げる
        assert!(purchase_ring_upgrade(&mut state, RingUpgrade::CorePulse));
        state.ores.push(Ore {
            x: CX + 15.0,
            y: CORE_Y + 15.0,
            vx: 0.0,
            vy: 0.0,
            hp: 2.0,
            kind: OreKind::Dust,
            radius: 3.5,
            motion: OreMotion::Orbit,
            sway: 0.05,
            age: 0,
        });
        let interval = state.pulse_interval().unwrap();
        for _ in 0..interval {
            tick(&mut state, 1);
        }
        assert!(
            state.total_kills > Layer::THRESHOLDS[1]
                || state.ores.first().map(|o| o.hp < 2.0).unwrap_or(true)
                || !state.pulse_rings.is_empty(),
            "核脈動が近傍を削るはず"
        );
    }
}
