//! 破壊デモの純粋ロジック。見た目の手触りを優先した簡易物理。

use super::state::{
    DemoStyle, Particle, ParticleKind, Scene, ShatterLabState, WORLD_H, WORLD_W,
};

const CX: f64 = WORLD_W * 0.5;
const GROUND_Y: f64 = 12.0;

/// 疑似乱数 (決定的)。見た目用なので暗号強度は不要。
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
        step_particles(state);
        match state.style {
            DemoStyle::OreBomb => tick_ore_bomb(state),
            DemoStyle::PressCrush => tick_press(state),
            DemoStyle::PlanetPeel => tick_planet(state),
            DemoStyle::CityCollapse => tick_city(state),
        }
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
                p.vy -= 0.02;
                p.vx *= 0.96;
            }
            ParticleKind::Spark | ParticleKind::Ember => {
                p.vy -= 0.04;
                p.vx *= 0.98;
            }
            ParticleKind::Debris | ParticleKind::Shard => {
                p.vy -= 0.12;
                p.vx *= 0.99;
            }
        }
        p.life -= 1;
    }
    state
        .particles
        .retain(|p| p.alive() && p.y > -4.0 && p.x > -4.0 && p.x < WORLD_W + 4.0);
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

fn spray_side(
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
        let dir = if i % 2 == 0 { -1.0 } else { 1.0 };
        let s = speed * rand_range(base.wrapping_add(i as u64 * 29 + 7), 0.55, 1.25);
        let lift = rand_range(base.wrapping_add(i as u64 * 11 + 3), 0.3, 1.6);
        state.particles.push(Particle {
            x,
            y,
            vx: dir * s,
            vy: lift,
            life,
            kind,
        });
    }
}

fn tick_ore_bomb(state: &mut ShatterLabState) {
    let Scene::OreBomb {
        mut phase,
        mut phase_t,
        mut bomb_y,
        mut rock_hp_frac,
    } = state.scene.clone()
    else {
        return;
    };

    phase_t += 1;
    let mut reset = false;
    match phase {
        0 => {
            if phase_t > 18 {
                phase = 1;
                phase_t = 0;
                bomb_y = WORLD_H - 6.0;
            }
        }
        1 => {
            bomb_y -= 2.4;
            let rock_top = GROUND_Y + 14.0;
            if bomb_y <= rock_top {
                phase = 2;
                phase_t = 0;
                rock_hp_frac = 0.0;
                bomb_y = rock_top;
                state.shake_ticks = 8;
                burst(state, CX, rock_top, 28, 2.8, ParticleKind::Debris, 28);
                burst(state, CX, rock_top, 18, 3.6, ParticleKind::Spark, 22);
                burst(state, CX, rock_top, 14, 1.6, ParticleKind::Dust, 36);
                burst(state, CX, rock_top, 10, 2.2, ParticleKind::Ember, 24);
            }
        }
        _ => {
            if phase_t > 40 {
                reset = true;
            }
        }
    }

    if reset {
        state.scene = ShatterLabState::fresh_scene(DemoStyle::OreBomb);
    } else {
        state.scene = Scene::OreBomb {
            phase,
            phase_t,
            bomb_y,
            rock_hp_frac,
        };
    }
}

fn tick_press(state: &mut ShatterLabState) {
    let Scene::PressCrush {
        mut phase,
        mut phase_t,
        mut press_y,
        mut rock_squash,
    } = state.scene.clone()
    else {
        return;
    };

    phase_t += 1;
    let mut reset = false;
    let mut do_spray = false;
    let mut do_final = false;

    match phase {
        0 => {
            if phase_t > 12 {
                phase = 1;
                phase_t = 0;
            }
        }
        1 => {
            press_y -= 1.8;
            let contact = GROUND_Y + 16.0;
            if press_y <= contact {
                press_y = contact;
                phase = 2;
                phase_t = 0;
            }
        }
        2 => {
            rock_squash = (rock_squash + 0.08).min(1.0);
            press_y = (GROUND_Y + 16.0) - rock_squash * 6.0;
            if phase_t == 1 || phase_t == 4 || phase_t == 8 {
                do_spray = true;
            }
            if rock_squash >= 1.0 && phase_t > 18 {
                phase = 3;
                phase_t = 0;
                do_final = true;
            }
        }
        _ => {
            if phase_t > 28 {
                reset = true;
            }
        }
    }

    if do_spray {
        spray_side(state, CX, GROUND_Y + 10.0, 10, 2.4, ParticleKind::Shard, 26);
        burst(state, CX, GROUND_Y + 10.0, 6, 1.2, ParticleKind::Dust, 30);
        state.shake_ticks = 4;
    }
    if do_final {
        burst(state, CX, GROUND_Y + 8.0, 22, 2.6, ParticleKind::Debris, 30);
        burst(state, CX, GROUND_Y + 8.0, 12, 3.2, ParticleKind::Spark, 20);
        state.shake_ticks = 10;
    }

    if reset {
        state.scene = ShatterLabState::fresh_scene(DemoStyle::PressCrush);
    } else {
        state.scene = Scene::PressCrush {
            phase,
            phase_t,
            press_y,
            rock_squash,
        };
    }
}

fn tick_planet(state: &mut ShatterLabState) {
    let Scene::PlanetPeel {
        mut layers_left,
        mut phase_t,
        mut crack,
    } = state.scene.clone()
    else {
        return;
    };

    phase_t += 1;
    let cy = WORLD_H * 0.48;

    if layers_left == 0 {
        if phase_t > 36 {
            state.scene = ShatterLabState::fresh_scene(DemoStyle::PlanetPeel);
        } else {
            state.scene = Scene::PlanetPeel {
                layers_left,
                phase_t,
                crack,
            };
        }
        return;
    }

    crack = (crack + 0.035).min(1.0);

    let mut chip = false;
    let mut chip_xy = (0.0, 0.0);
    let mut chip_ember = false;
    if phase_t % 3 == 0 && crack > 0.2 {
        let ang = rand_range(state.elapsed_ticks, 0.0, std::f64::consts::TAU);
        let r = match layers_left {
            3 => 14.0,
            2 => 10.0,
            _ => 6.0,
        };
        chip_xy = (CX + ang.cos() * r, cy + ang.sin() * r);
        chip = true;
        chip_ember = layers_left == 1;
    }

    let mut peel = false;
    let mut peel_r = 0.0;
    let mut final_boom = false;
    if crack >= 1.0 {
        peel_r = match layers_left {
            3 => 14.0,
            2 => 10.0,
            _ => 6.0,
        };
        peel = true;
        layers_left -= 1;
        crack = 0.0;
        phase_t = 0;
        final_boom = layers_left == 0;
    }

    if chip {
        burst(state, chip_xy.0, chip_xy.1, 3, 1.4, ParticleKind::Shard, 18);
        if chip_ember {
            burst(state, chip_xy.0, chip_xy.1, 2, 1.8, ParticleKind::Ember, 16);
        }
    }
    if peel {
        burst(
            state,
            CX,
            cy,
            20,
            2.2 + peel_r * 0.08,
            ParticleKind::Debris,
            28,
        );
        burst(state, CX, cy, 12, 2.8, ParticleKind::Spark, 22);
        state.shake_ticks = 6;
    }
    if final_boom {
        burst(state, CX, cy, 36, 3.4, ParticleKind::Ember, 32);
        burst(state, CX, cy, 24, 2.0, ParticleKind::Dust, 40);
        state.shake_ticks = 12;
    }

    state.scene = Scene::PlanetPeel {
        layers_left,
        phase_t,
        crack,
    };
}

fn tick_city(state: &mut ShatterLabState) {
    let Scene::CityCollapse {
        mut floors_left,
        mut phase_t,
        mut falling_y,
    } = state.scene.clone()
    else {
        return;
    };

    phase_t += 1;
    let floor_h = 7.0;

    if floors_left == 0 {
        if phase_t > 40 {
            state.scene = ShatterLabState::fresh_scene(DemoStyle::CityCollapse);
            return;
        }
        if phase_t % 2 == 0 {
            let x = CX + rand_range(state.elapsed_ticks, -10.0, 10.0);
            burst(state, x, GROUND_Y + 4.0, 2, 0.8, ParticleKind::Dust, 34);
        }
        state.scene = Scene::CityCollapse {
            floors_left,
            phase_t,
            falling_y,
        };
        return;
    }

    let drop_after = 10;
    if phase_t < drop_after {
        state.scene = Scene::CityCollapse {
            floors_left,
            phase_t,
            falling_y,
        };
        return;
    }

    falling_y += 2.6;
    let top = GROUND_Y + (floors_left as f64) * floor_h;
    if falling_y >= floor_h {
        burst(state, CX, top - floor_h, 14, 1.8, ParticleKind::Dust, 32);
        burst(state, CX, top - floor_h, 10, 2.2, ParticleKind::Debris, 24);
        state.shake_ticks = 5;
        floors_left -= 1;
        falling_y = 0.0;
        phase_t = 0;
    }

    state.scene = Scene::CityCollapse {
        floors_left,
        phase_t,
        falling_y,
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_styles_loop_without_panic_for_many_ticks() {
        for style in DemoStyle::ALL {
            let mut state = ShatterLabState::new();
            state.set_style(style);
            for _ in 0..500 {
                tick(&mut state, 1);
            }
            assert!(
                state.particles.len() < 400,
                "{style:?} particles={}",
                state.particles.len()
            );
        }
    }

    #[test]
    fn switching_style_resets_scene() {
        let mut state = ShatterLabState::new();
        tick(&mut state, 30);
        state.set_style(DemoStyle::PlanetPeel);
        assert!(matches!(
            state.scene,
            Scene::PlanetPeel { layers_left: 3, .. }
        ));
        assert!(state.particles.is_empty());
    }
}
