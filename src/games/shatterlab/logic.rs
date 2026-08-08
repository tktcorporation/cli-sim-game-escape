//! 破壊デモの純粋ロジック。威力Lvで密度・速度・規模が変わる。

use super::state::{
    BeamFlash, DemoStyle, Particle, ParticleKind, PowerLevel, ShatterLabState, Target, WORLD_H,
    WORLD_W, AUTO_POWER_TICKS,
};

const CX: f64 = WORLD_W * 0.5;

fn hash_u32(n: u64) -> u32 {
    let mut x = n.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    x ^= x >> 32;
    x as u32
}

fn rand01(seed: u64) -> f64 {
    (hash_u32(seed) as f64) / (u32::MAX as f64)
}

fn rand_range(seed: u64, lo: f64, hi: f64) -> f64 {
    lo + (hi - lo) * rand01(seed)
}

pub fn tick(state: &mut ShatterLabState, delta_ticks: u32) {
    for _ in 0..delta_ticks {
        state.elapsed_ticks = state.elapsed_ticks.wrapping_add(1);
        if state.shake_ticks > 0 {
            state.shake_ticks -= 1;
        }

        if state.auto_power {
            state.auto_power_t += 1;
            if state.auto_power_t >= AUTO_POWER_TICKS {
                state.auto_power_t = 0;
                state.power = state.power.next();
                // 強化瞬間のフィードバック
                state.shake_ticks = 6;
                burst(
                    state,
                    CX,
                    WORLD_H * 0.35,
                    12,
                    2.0,
                    ParticleKind::Spark,
                    18,
                );
            }
        }

        // 前進スクロール (クルーズ/採掘/列車で速度がLv依存)
        let scroll_speed = match state.style {
            DemoStyle::SatDefense => 0.15,
            _ => match state.power {
                PowerLevel::Low => 0.35,
                PowerLevel::Mid => 0.7,
                PowerLevel::High => 1.2,
            },
        };
        state.scroll = (state.scroll + scroll_speed) % 40.0;

        step_particles(state);
        step_beams(state);
        step_targets(state);
        spawn_targets(state);
        fire_weapons(state);
    }
}

fn step_particles(state: &mut ShatterLabState) {
    for p in &mut state.particles {
        if p.life == 0 {
            continue;
        }
        p.x += p.vx;
        p.y += p.vy;
        match p.kind {
            ParticleKind::Dust => {
                p.vy -= 0.015;
                p.vx *= 0.96;
            }
            ParticleKind::Spark | ParticleKind::Ember | ParticleKind::Beam => {
                p.vy -= 0.03;
                p.vx *= 0.98;
            }
            ParticleKind::Debris | ParticleKind::Shard => {
                p.vy -= 0.1;
                p.vx *= 0.99;
            }
        }
        p.life -= 1;
    }
    state
        .particles
        .retain(|p| p.alive() && p.y > -6.0 && p.x > -6.0 && p.x < WORLD_W + 6.0);
}

fn step_beams(state: &mut ShatterLabState) {
    for b in &mut state.beams {
        if b.life > 0 {
            b.life -= 1;
        }
    }
    state.beams.retain(|b| b.life > 0);
}

fn step_targets(state: &mut ShatterLabState) {
    for t in &mut state.targets {
        t.x += t.vx;
        t.y += t.vy;
    }
    // 画面外へ抜けたものは消す (撃破せず)
    state
        .targets
        .retain(|t| t.y > -8.0 && t.y < WORLD_H + 8.0 && t.x > -10.0 && t.x < WORLD_W + 10.0);
}

fn burst(
    state: &mut ShatterLabState,
    x: f64,
    y: f64,
    count: usize,
    speed: f64,
    kind: ParticleKind,
    life: u32,
) {
    let base = state.elapsed_ticks;
    for i in 0..count {
        let a = rand_range(base.wrapping_add(i as u64 * 17), 0.0, std::f64::consts::TAU);
        let s = speed * rand_range(base.wrapping_add(i as u64 * 31 + 3), 0.45, 1.15);
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

fn spawn_targets(state: &mut ShatterLabState) {
    let interval = state.power.spawn_interval();
    if state.elapsed_ticks % interval as u64 != 0 {
        return;
    }
    // 強Lvほど同時スポーン数が多い
    let batch = match state.power {
        PowerLevel::Low => 1,
        PowerLevel::Mid => 2,
        PowerLevel::High => 3,
    };
    let varieties = match state.power {
        PowerLevel::Low => 1u8,
        PowerLevel::Mid => 3,
        PowerLevel::High => 5,
    };
    let base = state.elapsed_ticks;
    for i in 0..batch {
        let seed = base.wrapping_add(i as u64 * 97);
        let variety = (hash_u32(seed) % varieties as u32) as u8;
        let (x, y, vx, vy, radius, hp) = match state.style {
            DemoStyle::SpaceCruise => {
                let x = rand_range(seed, 6.0, WORLD_W - 6.0);
                let r = match state.power {
                    PowerLevel::Low => rand_range(seed + 1, 1.6, 2.4),
                    PowerLevel::Mid => rand_range(seed + 1, 1.8, 3.2),
                    PowerLevel::High => rand_range(seed + 1, 2.0, 4.5),
                };
                (x, WORLD_H + 2.0, 0.0, -1.1 - state.power as u8 as f64 * 0.25, r, 1)
            }
            DemoStyle::OrbitMine => {
                let y = rand_range(seed, 20.0, WORLD_H - 10.0);
                let r = 1.8 + variety as f64 * 0.5;
                (
                    WORLD_W + 3.0,
                    y,
                    -0.9 - state.power as u8 as f64 * 0.3,
                    ((seed % 5) as f64 - 2.0) * 0.05,
                    r,
                    1 + variety / 2,
                )
            }
            DemoStyle::RailBreak => {
                let lane = (hash_u32(seed) % 3) as f64;
                let x = CX - 10.0 + lane * 10.0;
                let r = 2.2 + variety as f64 * 0.4;
                (x, WORLD_H + 2.0, 0.0, -1.4 - state.power as u8 as f64 * 0.35, r, 1)
            }
            DemoStyle::SatDefense => {
                let x = rand_range(seed, 4.0, WORLD_W - 4.0);
                let r = 1.5 + variety as f64 * 0.55;
                (
                    x,
                    WORLD_H + 2.0,
                    rand_range(seed + 3, -0.2, 0.2),
                    -0.8 - state.power as u8 as f64 * 0.35,
                    r,
                    1,
                )
            }
        };
        state.targets.push(Target {
            x,
            y,
            vx,
            vy,
            radius,
            hp,
            variety,
        });
    }
}

fn ship_guns(state: &ShatterLabState) -> Vec<(f64, f64)> {
    let scale = state.power.ship_scale();
    let n = state.power.gun_count();
    match state.style {
        DemoStyle::SpaceCruise => {
            let sy = 14.0;
            let spread = 8.0 * scale;
            (0..n)
                .map(|i| {
                    let t = if n == 1 {
                        0.5
                    } else {
                        i as f64 / (n - 1) as f64
                    };
                    (CX - spread + spread * 2.0 * t, sy + 6.0 * scale)
                })
                .collect()
        }
        DemoStyle::OrbitMine => {
            let sx = 12.0;
            let sy = WORLD_H * 0.5;
            (0..n)
                .map(|i| {
                    let t = if n == 1 {
                        0.5
                    } else {
                        i as f64 / (n - 1) as f64
                    };
                    (sx, sy - 10.0 * scale + 20.0 * scale * t)
                })
                .collect()
        }
        DemoStyle::RailBreak => {
            let sy = 18.0;
            vec![(CX, sy + 10.0 * scale)]
                .into_iter()
                .chain((1..n).map(|i| {
                    let side = if i % 2 == 0 { -1.0 } else { 1.0 };
                    (CX + side * (3.0 + i as f64), sy + 4.0 * scale)
                }))
                .collect()
        }
        DemoStyle::SatDefense => {
            let orbit_r = 10.0 + scale * 6.0;
            let cy = 22.0;
            (0..n)
                .map(|i| {
                    let a = state.elapsed_ticks as f64 * 0.04
                        + i as f64 * std::f64::consts::TAU / n.max(1) as f64;
                    (CX + a.cos() * orbit_r, cy + a.sin() * orbit_r * 0.45)
                })
                .collect()
        }
    }
}

fn fire_weapons(state: &mut ShatterLabState) {
    // 発射間隔: 強いほど短い
    let fire_every = match state.power {
        PowerLevel::Low => 6u64,
        PowerLevel::Mid => 3,
        PowerLevel::High => 2,
    };
    if state.elapsed_ticks % fire_every != 0 {
        return;
    }
    if state.targets.is_empty() {
        return;
    }

    let guns = ship_guns(state);
    let burst_n = state.power.burst_count();
    let mut hits: Vec<(usize, f64, f64, f64, f64)> = Vec::new(); // (idx, gx, gy, tx, ty)

    for &(gx, gy) in &guns {
        // 各砲が最も近いターゲットを狙う
        let mut best: Option<(usize, f64)> = None;
        for (i, t) in state.targets.iter().enumerate() {
            let d = (t.x - gx).hypot(t.y - gy);
            if best.map(|(_, bd)| d < bd).unwrap_or(true) {
                best = Some((i, d));
            }
        }
        if let Some((i, _)) = best {
            let t = &state.targets[i];
            hits.push((i, gx, gy, t.x, t.y));
        }
    }

    // インデックス重複をまとめつつHPを減らす
    hits.sort_by_key(|(i, _, _, _, _)| *i);
    let mut damaged = Vec::new();
    for (i, gx, gy, tx, ty) in hits {
        state.beams.push(BeamFlash {
            x0: gx,
            y0: gy,
            x1: tx,
            y1: ty,
            life: match state.power {
                PowerLevel::Low => 3,
                PowerLevel::Mid => 4,
                PowerLevel::High => 5,
            },
        });
        // ビーム粒子
        state.particles.push(Particle {
            x: (gx + tx) * 0.5,
            y: (gy + ty) * 0.5,
            vx: 0.0,
            vy: 0.0,
            life: 2,
            kind: ParticleKind::Beam,
        });
        if !damaged.contains(&i) {
            damaged.push(i);
        }
        let _ = (gx, gy); // silence
    }

    // HP処理は後ろから
    damaged.sort_unstable();
    damaged.dedup();
    for &i in damaged.iter().rev() {
        if i >= state.targets.len() {
            continue;
        }
        if state.targets[i].hp > 0 {
            state.targets[i].hp -= 1;
        }
        if state.targets[i].hp == 0 {
            let t = state.targets.remove(i);
            state.cleared += 1;
            let kind_main = match t.variety % 5 {
                0 => ParticleKind::Debris,
                1 => ParticleKind::Shard,
                2 => ParticleKind::Ember,
                3 => ParticleKind::Spark,
                _ => ParticleKind::Dust,
            };
            burst(state, t.x, t.y, burst_n / 2 + 4, 1.6 + t.radius * 0.3, kind_main, 22);
            burst(state, t.x, t.y, burst_n / 3, 2.4, ParticleKind::Spark, 16);
            if matches!(state.power, PowerLevel::High) {
                burst(state, t.x, t.y, 8, 1.2, ParticleKind::Dust, 28);
                state.shake_ticks = state.shake_ticks.max(3);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_styles_and_powers_run_without_panic() {
        for style in DemoStyle::ALL {
            for power in PowerLevel::ALL {
                let mut state = ShatterLabState::new();
                state.set_style(style);
                state.set_power(power);
                for _ in 0..200 {
                    tick(&mut state, 1);
                }
                assert!(
                    state.particles.len() < 600,
                    "{style:?}/{power:?} particles={}",
                    state.particles.len()
                );
            }
        }
    }

    #[test]
    fn higher_power_clears_more_in_same_ticks() {
        let run = |power: PowerLevel| {
            let mut state = ShatterLabState::new();
            state.set_style(DemoStyle::SpaceCruise);
            state.set_power(power);
            for _ in 0..300 {
                tick(&mut state, 1);
            }
            state.cleared
        };
        let low = run(PowerLevel::Low);
        let high = run(PowerLevel::High);
        assert!(
            high > low,
            "強Lvの撃破数({high})は弱Lv({low})より多いはず"
        );
    }

    #[test]
    fn auto_power_cycles() {
        let mut state = ShatterLabState::new();
        state.enable_auto_power();
        assert_eq!(state.power, PowerLevel::Low);
        for _ in 0..AUTO_POWER_TICKS {
            tick(&mut state, 1);
        }
        assert_eq!(state.power, PowerLevel::Mid);
    }
}
