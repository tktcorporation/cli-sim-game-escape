//! 星環の純粋ロジック。描画・I/O に依存しない。

use super::state::{
    Layer, Ore, OreKind, OreMotion, Particle, ParticleKind, Projectile, PulseRing, RingUpgrade,
    StarRingState, WeaponKind, WeaponStat, BOOST_DURATION, CORE_Y, CX, FIELD_MARGIN, INNER_RADIUS,
    LAYER_FLASH_TICKS, LAYER_READY_FLASH_TICKS, SPAWN_X_MARGIN, SPAWN_Y, VISIBLE_Y_HI,
    VISIBLE_Y_LO, WORLD_H, WORLD_W,
};

/// 鉱石の同時存在上限。これを超えると湧きも分裂も止める。
pub(super) const MAX_ORES: usize = 56;

// うねりの振れ幅は「基準速度 × 倍率 ÷ 角周波数」で決まる。角周波数 (RATE) が
// 小さい = 周期が長いほど同じ倍率でも振れ幅が伸びるので、倍率は周期と釣り合う
// 大きさへ揃える。Zigzag は Spiral より 4 倍速い周期に 2.5 倍の倍率を当てるので、
// 1tick あたりの横移動は大きく、振れ幅そのものは狭くなる。
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
/// 核脈動の波面が1tickで外へ進む距離。
///
/// 波が舐めるのは中心距離なので、比べる相手は落下速度そのものではなく「中心
/// 距離が1tickで詰まる量」になる。詰まり方は落下と引き寄せの合成 (`step_ores`)
/// で `fall_speed × Layer::fall_mult × (1 + CORE_PULL_RATIO × ramp)`——引き寄せが
/// 最大 (`ramp` = 1) の星塵なら第7層で約 1.79/tick まで伸びる。核が脈打つたびに
/// 上空を舐めていく動きとして読めるよう、波はその倍以上の速さで外へ抜ける。
/// `Layer::fall_mult` は層とともに線形に伸びるので、深い層ほど差は詰まる。
const PULSE_WAVE_SPEED: f64 = 4.25;

// 弾の半径 (ワールド単位)。鉱石 (`OreKind::radius`) と同じく「画面の広さに対して
// どう見えるか」で決め、`WORLD_W` の 1.25%〜1.75% の帯へ5種を並べる。
//
// 帯の上端は「弾より的が確実に大きい」から決まる。最小の鉱石 (`OreKind::Dust`)
// は `WORLD_W` の 3.0% で、幅33桁のモバイルでは 4×4 点に描かれる。弾がそこで
// 3 点に届くと的と同じ塊に見えてしまうので、端数がどこに落ちても 2 点に収まる
// 大きさ (半径 1.8 未満) を上限に取る。帯の中の並びは武器の性格に沿わせ、
// ばら撒く散弾がいちばん小さく、着弾で爆ぜる新星がいちばん大きい。
//
// 当たり判定はこの半径に `HIT_TOLERANCE` を足して取るので、見た目の大きさと
// 迎撃の手応えは別々に動かせる。
pub(super) const SCATTER_PROJECTILE_RADIUS: f64 = 1.25;
pub(super) const PULSE_PROJECTILE_RADIUS: f64 = 1.375;
pub(super) const ARC_PROJECTILE_RADIUS: f64 = 1.5;
pub(super) const RAY_PROJECTILE_RADIUS: f64 = 1.625;
pub(super) const NOVA_PROJECTILE_RADIUS: f64 = 1.75;
/// 弾と鉱石の当たり判定を、両者の円が触れる距離からどれだけ甘くするか。
///
/// 砲台は撃つ瞬間の位置へ撃つ (`aim_dir`) ので、迎撃の手応えは「弾の飛行時間の
/// あいだに鉱石が横へ逃げ切れるか」で決まる。的の見た目の大きさは画面の広さに
/// 合わせて決めたいが、そこへ判定を直結させると、絵を縮めただけで迎撃が
/// 成立しなくなる。見た目と手応えを別々に動かせるよう、余裕を独立した値で持つ。
const HIT_TOLERANCE: f64 = 0.8;
/// 層開放の演出で立つ波が届く距離。鉱石には触れない波なので、どの脈動レベルの
/// 到達距離とも噛み合わせず、開放の瞬間だけ上空まで駆け上がる長さを取る。
const CEREMONY_WAVE_REACH: f64 = 70.0;
/// 同時に描く波の本数の上限。波は点描で描かれるので、本数がそのまま1フレームの
/// 点数になる。タップは入力イベントごとに波を立てられ、10 ticks/sec の歩みに
/// 縛られない——上限を置かないと連打のぶんだけ描画コストが伸びる。
const MAX_PULSE_RINGS: usize = 12;
/// 上限を越えたときに残す本数。1本ずつ削ると連打のあいだ毎tickで削り続けるので、
/// 一度にまとめて減らして次の切り詰めまでの間隔を空ける。
const KEPT_PULSE_RINGS: usize = 8;
/// 裂片が分裂する際、子を親の左右へ振り分ける幅。
const SPLIT_SPREAD: f64 = 4.0;
/// 分裂子は星塵を一回り小さくした個体として湧く。HP と半径へ同じ係数を掛け、
/// 「小さいのが2つ出た」と見た目と手応えを揃える。
const SPLIT_CHILD_SCALE: f64 = 0.7;

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
    spawn_pulse_wave(state, CEREMONY_WAVE_REACH, 0.0);
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
        spawn_pulse_wave(state, state.pulse_reach() * 0.55, state.pulse_damage() * 0.6);
    }
}

/// 核脈動の波を1つ立てる。`reach` まで広がったところで消える。
/// `damage` が 0 の波は演出だけで、鉱石には触れない。
///
/// 寿命は「広がる tick 数 + 1」。最後の 1 tick は広がらず、波面が到達距離へ
/// 着いたその tick の鉱石の動きだけを見る (`step_pulse_rings`)。
fn spawn_pulse_wave(state: &mut StarRingState, reach: f64, damage: f64) {
    let reach = reach.max(INNER_RADIUS);
    let expand = ((reach - INNER_RADIUS) / PULSE_WAVE_SPEED).ceil().max(1.0) as u32;
    let life = expand + 1;
    state.pulse_rings.push(PulseRing {
        radius: INNER_RADIUS,
        reach,
        life,
        max_life: life,
        damage,
    });
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

/// 波を1tick広げ、その間に波面が跨いだ鉱石を削る。
///
/// 判定を「波面が中心を通過したか」に置くので、1つの波が同じ鉱石を削るのは
/// 1度きりになる。核の近くに居座るほど連続で削られる当たり方にすると、
/// 核へ吸い込まれる直前の鉱石だけが極端に有利になり、上空へ広がる波という
/// 見た目と噛み合わない。
///
/// 「1度きり」が成り立つのは、1本の波が1tickに1つの輪帯しか通さず、その輪帯が
/// 前 tick の輪帯と継ぎ目なく隣り合うからで、判定そのものに当たった記録は無い
/// (`pulse_wave_damage`)。1本につき `pulse_wave_damage` の呼び出しを1度に保つ
/// ことが不変条件の実体なので、この関数はどの経路を通っても波1本あたり1回しか
/// 呼ばない形にしてある。
///
/// 最後の1tickだけは広がらず、幅ゼロの輪帯 `[reach, reach]` で判定する。
/// `pulse_wave_damage` は前 tick の移動区間を見るので、波面が到達距離へ
/// 着いた tick に鉱石がその距離を跨ぐ動きは、次の tick でしか見えない——
/// この 1tick が無いと、波面が追い越したはずの鉱石が到達距離の際でだけ
/// すり抜ける。輪帯の幅がゼロなので、波が届く距離自体は伸びない。
///
/// 本数が上限を越えたら、最も広がった波から畳む。畳む波はこの tick が最後の
/// 一歩になり、進む先が `radius + PULSE_WAVE_SPEED` ではなく `reach` へ変わる。
/// 残りの輪帯をこの tick の輪帯と地続きの1区間として通すので、タップした回数
/// ぶんの手応えを残しながら、同じ鉱石を2度削ることもない。
fn step_pulse_rings(state: &mut StarRingState) {
    // 畳む本数は波を広げる前に決める。この tick で寿命が尽きる波は放っておいても
    // 消えるので、数えるのは生き残る波だけにする。
    let surviving = state.pulse_rings.iter().filter(|r| r.life > 1).count();
    let mut to_retire = if surviving > MAX_PULSE_RINGS {
        surviving - KEPT_PULSE_RINGS
    } else {
        0
    };

    for i in 0..state.pulse_rings.len() {
        let ring = &mut state.pulse_rings[i];
        if ring.life == 0 {
            continue;
        }
        ring.life -= 1;
        // 畳むのは最も広がった波から。古い順に並んでいるので先頭から取る。
        let retiring = ring.life > 0 && to_retire > 0;
        if retiring {
            to_retire -= 1;
            ring.life = 0;
        }
        let inner = ring.radius;
        ring.radius = if retiring {
            ring.reach
        } else if ring.life > 0 {
            // 最後の一歩は端数になるので、到達距離で頭打ちにする。満額進めると
            // `pulse_reach` の外に居る鉱石まで削れ、描かれる波も強化の範囲から
            // はみ出す。
            (inner + PULSE_WAVE_SPEED).min(ring.reach)
        } else {
            inner
        };
        let (outer, dmg) = (ring.radius, ring.damage);
        if dmg > 0.0 {
            pulse_wave_damage(state, inner, outer, dmg);
        }
    }
    state.pulse_rings.retain(|r| r.life > 0);
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
            if d <= ore.radius + pr + HIT_TOLERANCE {
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
            // 引き寄せ方向と直交する成分。引き寄せと同じく降下が進むほど強まる
            // ので、上空では素直に落ち、コアへ吸い込まれる手前で横へ流れて
            // 一度回り込む。
            let swirl = fall * ORBIT_SWIRL_GAIN * descent * descent * ore.sway.signum();
            ore.x += -uy * swirl;
            ore.y += ux * swirl;
        }

        // 左右の壁で跳ね返す。鉱石が横から画面外へ消えると何が起きているか
        // 追えなくなるので、常にフィールド内に留める。境界は中心ではなく円の
        // 端で取る——中心を壁に張り付けると半径ぶんが Canvas の x_bounds の外へ
        // 出て、跳ね返った鉱石ほど欠けて描かれる。
        let (lo, hi) = (
            FIELD_MARGIN + ore.radius,
            WORLD_W - FIELD_MARGIN - ore.radius,
        );
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
///
/// 判定の取り方は 2 つで意味が違う。コア到達は中心距離で取る——核へ吸い込まれた
/// かどうかの判定であり、核そのものが描かれている位置なので欠けは起きない。
/// 場外落下は円の下端で取る——中心が `VISIBLE_Y_LO` へ届くまで待つと、その下へ
/// はみ出した半径ぶんが切れた鉱石として何十 tick も描かれる。
fn resolve_arrivals(state: &mut StarRingState) {
    let mut i = 0;
    while i < state.ores.len() {
        let ore = &state.ores[i];
        let reached_core = (ore.x - CX).hypot(ore.y - CORE_Y) <= INNER_RADIUS;
        if reached_core || ore.y - ore.radius <= VISIBLE_Y_LO {
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

/// 湧きの基準高さ。円の上端がちょうど `VISIBLE_Y_HI` に接する高さへ置く。
///
/// 画面シェイクで上へ振れた tick も含めて Canvas の y_bounds に収めたいので、
/// 突き合わせる相手は `WORLD_H` ではなくシェイクを見込んだ `VISIBLE_Y_HI` に
/// なる。これより上げたぶんだけ、湧いた直後の大きい鉱石が上を欠いて見える。
/// 円の下端はこの高さから直径ぶん下がった位置に来る——どの鉱石も直径が
/// `VISIBLE_Y_HI - SPAWN_Y` を上回るので、円は採掘境界 (`SPAWN_Y`) をまたぎ、
/// 境界の向こうから現れる見え方になる。
fn spawn_base_y(kind: OreKind) -> f64 {
    VISIBLE_Y_HI - kind.radius()
}

/// 湧きの x の有効範囲。`spawn_base_y` と同じく円の端で取る。
///
/// 中心をそのまま `SPAWN_X_MARGIN` まで寄せると、半径がマージンを上回る鉱石は
/// 湧いた瞬間から Canvas の x_bounds の外へはみ出して欠けて描かれる。円の端が
/// フィールド端から `SPAWN_X_MARGIN` 離れる位置を境界にすると、どの大きさでも
/// 全体が見えたまま降り始める。
fn spawn_x_range(kind: OreKind) -> (f64, f64) {
    let r = kind.radius();
    (SPAWN_X_MARGIN + r, WORLD_W - SPAWN_X_MARGIN - r)
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
    for i in 0..batch {
        let kind = pick_ore_kind(state);
        // 同時湧きの x を層化サンプリングで散らす。一様乱数だけだと固まって湧いた
        // ときに重なり、何体降ってきているのか読めなくなる。有効範囲は鉱石の
        // 大きさで変わるので、その幅をバッチ数で等分して i 番目のスロットから取る。
        let (lo, hi) = spawn_x_range(kind);
        let slot = (hi - lo) / batch as f64;
        let slot_lo = lo + slot * i as f64;
        let x = rand_range(state, slot_lo, slot_lo + slot);
        // ばらつきは下方向へ取る。上へ振ると `WORLD_H` を超え、Canvas の
        // y_bounds 外で数tick見えないまま落ちてくる。
        let y = spawn_base_y(kind) - rand_range(state, 0.0, 6.0);
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

/// 各武装の発射 (`fire_*`) が持つ `speed` と `life` は、飛行時間と射程
/// (`speed × life`) の兼ね合いで決める。速すぎると発射から着弾までが一瞬になり、
/// 砲台から遠い鉱石でも自動照準がそのまま当たってしまう。飛行時間を残すことで、
/// 横へ漂う鉱石 (`OreKind::sway_speed`) は遠距離ほど照準を外せる——迎撃の間合いは
/// この飛行時間と横速度の釣り合いで決まる。`life` はそのうえで、弾が届いてほしい
/// 距離を飛び切ったところで消えるよう合わせる。
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
    spawn_pulse_wave(state, state.pulse_reach(), state.pulse_damage());
    state.core_flash_ticks = state.core_flash_ticks.max(4);
    burst(state, CX, CORE_Y, 5, 2.5, ParticleKind::Spark, 12);
}

/// この tick に波面が `inner` から `outer` へ進む間、波面が追い越した鉱石を削る。
///
/// 鉱石も同じ tick に核へ近づくので、判定はその移動ぶんを含めた掃引で取る。
/// 中心距離の瞬間値だけを見ると、波面のわずかに外に居た鉱石が次の tick までに
/// 前 tick の `outer` の内側へ入り込み、波が通り抜けたのに一度も削られない
/// 個体が出る。
/// 当たりが tick の位相任せになると、核脈動という強化の効きが読めなくなる。
///
/// `prev` は `vx`/`vy` から復元した前 tick の中心距離。ある tick の `dist` は
/// 次の tick の `prev` と一致し、`outer` は次の tick の `inner` と一致するので、
/// 判定区間は隣り合う tick で継ぎ目なく並ぶ——1 つの波が同じ鉱石を削るのは
/// 1 度きりになる。
fn pulse_wave_damage(state: &mut StarRingState, inner: f64, outer: f64, dmg: f64) {
    let mut i = state.ores.len();
    while i > 0 {
        i -= 1;
        let ore = &state.ores[i];
        let dist = (ore.x - CX).hypot(ore.y - CORE_Y);
        let prev = (ore.x - ore.vx - CX).hypot(ore.y - ore.vy - CORE_Y);
        if prev > inner && dist <= outer {
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
            radius: PULSE_PROJECTILE_RADIUS,
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
            radius: RAY_PROJECTILE_RADIUS,
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
            radius: SCATTER_PROJECTILE_RADIUS,
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
            radius: ARC_PROJECTILE_RADIUS,
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
            radius: NOVA_PROJECTILE_RADIUS,
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

    if ore.kind.splits_on_death() {
        // 親を取り除いた後の残り枠のぶんだけ湧かせる。2 体を固定で足すと、上限
        // まで埋まった盤面では裂片を割るたびに `MAX_ORES` を超えていく。
        let room = MAX_ORES.saturating_sub(state.ores.len()).min(2);
        let child_hp = OreKind::Dust.base_hp() * Layer::hp_mult(state.layer()) * SPLIT_CHILD_SCALE;
        let child_radius = OreKind::Dust.radius() * SPLIT_CHILD_SCALE;
        for k in 0..room {
            // 左右へ振る幅は 2 体を見分けるためのものなので、1 体しか入らない
            // ときは親の位置をそのまま使う——片方だけを寄せると、理由の見えない
            // 横ずれとして残る。
            let spread = if room == 2 {
                (k as f64 * 2.0 - 1.0) * SPLIT_SPREAD
            } else {
                0.0
            };
            // 子も円の端で壁に収める。分裂は反射処理より後に走るので、中心だけを
            // 壁へ寄せるとその tick のあいだ Canvas の外へはみ出したまま描かれる。
            let x = (ore.x + spread).clamp(
                FIELD_MARGIN + child_radius,
                WORLD_W - FIELD_MARGIN - child_radius,
            );
            spawn_one(state, OreKind::Dust, x, ore.y);
            if let Some(child) = state.ores.last_mut() {
                child.hp = child_hp;
                child.radius = child_radius;
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
    use crate::games::starringe::state::{MAX_TURRETS, TURRET_NEAR_RADIUS};

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

    /// 湧いた鉱石は最初の tick から円の全体が Canvas (`0..WORLD_W` × `0..WORLD_H`)
    /// に収まること。縦は画面シェイクの振れ (`VISIBLE_Y_LO`/`VISIBLE_Y_HI`) 込みで
    /// 見る。上端がはみ出すと、降りてくるまでの数十 tick は大きい鉱石ほど上を
    /// 欠いた形で描かれる。左右も同じで、はみ出したまま降り始めた鉱石は端で
    /// 欠けて見える。
    ///
    /// 湧いた後も内側に留まり続けることは
    /// `simulator::ores_stay_inside_the_field_over_a_long_run` が見る。
    #[test]
    fn spawned_ores_fit_inside_the_canvas_from_the_first_tick() {
        for kind in OreKind::ALL {
            let base = spawn_base_y(kind);
            assert!(
                (base + kind.radius() - VISIBLE_Y_HI).abs() < 1e-9,
                "{kind:?} の湧き高さが描画範囲の上端に接していない top={}",
                base + kind.radius()
            );
            // 円が採掘境界をまたぐこと。半径が小さすぎると境界より上へ丸ごと
            // 収まってしまい、「境界の向こうから降りてくる」形にならない。
            assert!(
                base - kind.radius() < SPAWN_Y,
                "{kind:?} が採掘境界より上へ丸ごと収まっている bottom={}",
                base - kind.radius()
            );

            let (lo, hi) = spawn_x_range(kind);
            assert!(
                lo - kind.radius() >= 0.0 && hi + kind.radius() <= WORLD_W,
                "{kind:?} の湧き x 範囲が Canvas をはみ出す lo={lo} hi={hi} r={}",
                kind.radius()
            );
            assert!(lo < hi, "{kind:?} の湧き x 範囲が潰れている lo={lo} hi={hi}");
        }

        let mut state = StarRingState::new();
        let mut checked = 0u32;
        for _ in 0..600 {
            tick(&mut state, 1);
            for ore in state.ores.iter().filter(|o| o.age == 0) {
                assert!(
                    ore.y + ore.radius <= VISIBLE_Y_HI + 1e-9 && ore.y - ore.radius >= VISIBLE_Y_LO,
                    "湧いた鉱石が画面からはみ出している y={} r={}",
                    ore.y,
                    ore.radius
                );
                assert!(
                    ore.x - ore.radius >= 0.0 && ore.x + ore.radius <= WORLD_W + 1e-9,
                    "湧いた鉱石が画面からはみ出している x={} r={}",
                    ore.x,
                    ore.radius
                );
                checked += 1;
            }
        }
        assert!(checked > 0, "検証対象の鉱石が湧いているはず");
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
            let ore = &state.ores[0];
            assert!(
                ore.x - ore.radius >= FIELD_MARGIN - 1e-9
                    && ore.x + ore.radius <= WORLD_W - FIELD_MARGIN + 1e-9,
                "鉱石が横から画面外へ出てはいけない x={} r={}",
                ore.x,
                ore.radius
            );
        }
    }

    /// 下端へ抜ける鉱石は、円が Canvas を割る前に消えること。
    ///
    /// 核から遠い壁際を降りた鉱石は引き寄せの上向き成分が落下速度に届かず、
    /// 核へ吸い込まれないまま下端へ抜ける。中心が 0 に達するまで残すと、その間
    /// ずっと半径ぶんを欠いた鉱石が下端に描かれる。
    ///
    /// 長期運転で内側に留まり続けることは
    /// `simulator::ores_stay_inside_the_field_over_a_long_run` が見る。
    #[test]
    fn ores_leaving_the_bottom_vanish_before_the_circle_is_clipped() {
        let mut fell_out = 0u32;
        for kind in OreKind::ALL {
            let mut state = StarRingState::new();
            // 核と同じ高さの壁際から降ろす。核への向きがほぼ真横になるので
            // 引き寄せが落下を止められず、下端へ抜ける経路に入る。
            spawn_one(&mut state, kind, FIELD_MARGIN + kind.radius(), CORE_Y);
            state.ores[0].sway = 0.0;

            let mut last = (state.ores[0].x, state.ores[0].y);
            let mut vanished = false;
            for _ in 0..1_000 {
                step_ores(&mut state);
                resolve_arrivals(&mut state);
                let Some(ore) = state.ores.first() else {
                    vanished = true;
                    break;
                };
                assert!(
                    ore.y - ore.radius > 0.0,
                    "{kind:?} が下端で欠けたまま残っている y={} r={}",
                    ore.y,
                    ore.radius
                );
                last = (ore.x, ore.y);
            }
            assert!(vanished, "{kind:?} が消えずに残り続けた last={last:?}");
            if (last.0 - CX).hypot(last.1 - CORE_Y) > INNER_RADIUS {
                fell_out += 1;
            }
        }
        assert!(
            fell_out > 0,
            "どの鉱石も核へ吸い込まれてしまい、下端の逸失経路を通っていない"
        );
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

    /// 分裂しても同時存在数は `MAX_ORES` を超えないこと。
    ///
    /// 親を取り除いてから子を足すので、上限まで埋まった盤面で裂片を割ると
    /// 残り枠は 1 つしかない。残り枠を見ずに 2 体足すと、割るたびに盤面が
    /// 上限を 1 体ずつ超えていき、上限そのものが意味を失う。
    #[test]
    fn splitting_never_exceeds_the_ore_cap() {
        for filler in [MAX_ORES - 1, MAX_ORES] {
            let mut state = StarRingState::new();
            state.ores.push(Ore {
                x: CX,
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
            while state.ores.len() < filler {
                push_test_ore(&mut state, CX, CORE_Y + 40.0, 100.0);
            }
            apply_damage(&mut state, 0, 10.0, DamageSource::Weapon(WeaponKind::Ray));
            assert!(
                state.ores.len() <= MAX_ORES,
                "分裂で上限を超えた filler={filler} n={} 上限={MAX_ORES}",
                state.ores.len()
            );
        }
    }

    /// 残り枠が 1 つのときの子は親の位置へ湧くこと。
    ///
    /// 左右へ振る幅は 2 体を見分けるためのものなので、1 体だけを片側へ寄せると
    /// 理由の見えない横ずれになる。
    #[test]
    fn a_lone_split_child_keeps_the_parent_position() {
        let mut state = StarRingState::new();
        let parent_x = CX + 16.0;
        state.ores.push(Ore {
            x: parent_x,
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
        while state.ores.len() < MAX_ORES {
            push_test_ore(&mut state, CX, CORE_Y + 40.0, 100.0);
        }
        apply_damage(&mut state, 0, 10.0, DamageSource::Weapon(WeaponKind::Ray));
        let child = state.ores.last().expect("残り枠1つぶんの子が湧いていない");
        assert_eq!(child.kind, OreKind::Dust);
        assert!(
            (child.x - parent_x).abs() < 1e-9,
            "1体だけの子が横へずれている child_x={} parent_x={parent_x}",
            child.x
        );
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

    /// 核脈動を解放した状態を作る。
    fn state_with_core_pulse(levels: u32) -> StarRingState {
        let mut state = StarRingState::new();
        state.total_kills = Layer::THRESHOLDS[1];
        state.shards = 1e9;
        assert!(unlock_next_layer(&mut state));
        for _ in 0..levels {
            assert!(purchase_ring_upgrade(&mut state, RingUpgrade::CorePulse));
        }
        state.total_kills = 0;
        state
    }

    fn push_test_ore(state: &mut StarRingState, x: f64, y: f64, hp: f64) {
        state.ores.push(Ore {
            x,
            y,
            vx: 0.0,
            vy: 0.0,
            hp,
            kind: OreKind::Dust,
            radius: 3.5,
            motion: OreMotion::Heavy,
            sway: 0.0,
            age: 0,
        });
    }

    /// 波は核の真上へ `pulse_reach` ぶん伸び、そこに居る鉱石を削ること。
    ///
    /// 核はフィールド下端に座っているので、届く距離がそのまま「上空のどの高さを
    /// 舐めるか」になる。ここが縮むと、鉱石が降下の大半を過ごす高い位置に波が
    /// 触れなくなり、核脈動は買っても何も起きない強化になる。
    #[test]
    fn core_pulse_wave_sweeps_ores_high_above_the_core() {
        let mut state = state_with_core_pulse(3);
        let reach = state.pulse_reach();
        assert!(
            CORE_Y + reach > SPAWN_Y * 0.6,
            "波が降下レーンの高い側へ届いていない reach={reach}"
        );

        let y = CORE_Y + reach * 0.9;
        push_test_ore(&mut state, CX, y, 1e6);
        // 波が湧いてから鉱石の高さを通過し切るまで走らせる。
        for _ in 0..state.pulse_interval().unwrap() + 24 {
            tick(&mut state, 1);
        }
        let hp = state.ores.first().map(|o| o.hp).unwrap_or(0.0);
        assert!(
            hp < 1e6,
            "核から {:.1} 離れた鉱石を波が削っていない y={y} hp={hp}",
            y - CORE_Y
        );
    }

    /// 1つの波が同じ鉱石を削るのは 1 度きり。核の近くに居座るほど連続で削られる
    /// 当たり方にすると、上空へ広がる波という見た目と噛み合わなくなる。
    #[test]
    fn a_single_pulse_wave_hits_each_ore_only_once() {
        let mut state = state_with_core_pulse(1);
        let dmg = state.pulse_damage();
        push_test_ore(&mut state, CX, CORE_Y + 20.0, 1e6);
        state.ores[0].kind = OreKind::Dust;

        let reach = state.pulse_reach();
        spawn_pulse_wave(&mut state, reach, dmg);
        let before = state.ores[0].hp;
        for _ in 0..40 {
            step_pulse_rings(&mut state);
        }
        let dealt = before - state.ores[0].hp;
        assert!(
            (dealt - dmg).abs() < 1e-6,
            "1波で与えたダメージが1発ぶんでない dealt={dealt} dmg={dmg}"
        );
    }

    /// 波が通り抜けた鉱石は、tick の位相によらず必ず 1 度だけ削られること。
    ///
    /// 鉱石は波へ近づく向きに動くので、その tick の中心距離だけを見ていると
    /// 波面の外から内へ一気に潜り込んだ個体を取りこぼす。当たりが初期位置の
    /// 端数任せになると、核脈動を積んでも効きが体感できない。
    #[test]
    fn pulse_wave_hits_every_ore_it_passes_regardless_of_phase() {
        let reach = state_with_core_pulse(3).pulse_reach();
        let dmg = state_with_core_pulse(3).pulse_damage();
        // 波面は 1tick で PULSE_WAVE_SPEED 進むので、その幅より細かく初期位置を
        // ずらせば波面と鉱石の位相関係を一巡できる。
        let steps = (PULSE_WAVE_SPEED / 0.4).ceil() as u32 + 2;
        for step in 0..steps {
            let mut state = state_with_core_pulse(3);
            let y = CORE_Y + reach * 0.5 + step as f64 * 0.4;
            push_test_ore(&mut state, CX, y, 1e6);
            spawn_pulse_wave(&mut state, reach, dmg);
            let before = state.ores[0].hp;
            // tick と同じ順序 (波を広げてから鉱石を動かす) で回す。
            for _ in 0..40 {
                step_pulse_rings(&mut state);
                step_ores(&mut state);
            }
            let dealt = before - state.ores[0].hp;
            assert!(
                (dealt - dmg).abs() < 1e-6,
                "y={y} の鉱石への被弾が1発ぶんでない dealt={dealt} dmg={dmg}"
            );
        }
    }

    /// 砲台の円は、砲台数が上限でも画面シェイク込みで Canvas
    /// (`0..WORLD_W` × `0..WORLD_H`) に収まること。
    ///
    /// 環の縦半径は砲台数とともに広がるので、中心だけを見て伸ばすと最下点の
    /// 砲台が下端を割り、周回のたびに下側が欠けて描かれる。
    #[test]
    fn turrets_fit_inside_the_canvas_at_every_turret_count() {
        for count in 1..=MAX_TURRETS {
            let mut state = StarRingState::new();
            state.weapon_levels[0][WeaponStat::Count.index()] = count - 1;
            assert_eq!(state.turret_count(), count);

            // 公転位相を一巡させ、最下点・最上点・左右端を通す。
            for t in 0..400u64 {
                state.elapsed_ticks = t;
                for (x, y, depth) in turret_positions(&state) {
                    let r = if depth <= 0.0 {
                        TURRET_NEAR_RADIUS
                    } else {
                        1.0
                    };
                    assert!(
                        y - r >= VISIBLE_Y_LO && y + r <= VISIBLE_Y_HI,
                        "砲台{count}基の円が縦にはみ出す y={y} r={r}"
                    );
                    assert!(
                        x - r >= 0.0 && x + r <= WORLD_W,
                        "砲台{count}基の円が横にはみ出す x={x} r={r}"
                    );
                }
            }
        }
    }

    /// 波面はどの脈動レベルでも `pulse_reach` ちょうどで止まること。
    ///
    /// 波面は1tickに `PULSE_WAVE_SPEED` ずつ進むが、到達距離がその整数倍とは
    /// 限らない。最後の一歩を満額進めると、`pulse_reach` が示す範囲の外に居る
    /// 鉱石まで削れ、描かれる波も強化の効果と食い違う。
    #[test]
    fn pulse_wave_stops_at_its_reach_on_every_level() {
        for lv in 1..=12u32 {
            let mut state = state_with_core_pulse(1);
            state.ring_levels[RingUpgrade::CorePulse.index()] = lv;
            let reach = state.pulse_reach();
            let dmg = state.pulse_damage();
            // 層開放の演出波が残っていると、そちらの半径を測ってしまう。
            state.pulse_rings.clear();
            spawn_pulse_wave(&mut state, reach, dmg);

            let mut outermost: f64 = 0.0;
            for _ in 0..64 {
                step_pulse_rings(&mut state);
                match state.pulse_rings.first() {
                    Some(ring) => outermost = outermost.max(ring.radius),
                    None => break,
                }
            }
            assert!(
                state.pulse_rings.is_empty(),
                "脈Lv{lv} の波が寿命内に消えていない"
            );
            assert!(
                (outermost - reach).abs() < 1e-9,
                "脈Lv{lv} の波面が到達距離とずれている outermost={outermost} reach={reach}"
            );
        }
    }

    /// 波面が到達距離へ着いた tick に、鉱石がちょうどその距離を跨いだ場合も
    /// 削られること。
    ///
    /// 波面と鉱石は互いへ向かって動くので、波が広がり切る tick に鉱石が最外周を
    /// 内側へ跨ぐ組み合わせがある。その動きを見ないまま波を消すと、当たりが
    /// 到達距離の際でだけ tick の位相任せになる。
    #[test]
    fn pulse_wave_hits_an_ore_crossing_its_outer_edge_on_the_last_tick() {
        let reach = state_with_core_pulse(3).pulse_reach();
        let dmg = state_with_core_pulse(3).pulse_damage();
        let expand = ((reach - INNER_RADIUS) / PULSE_WAVE_SPEED).ceil().max(1.0) as u32;

        let mut crossings = 0;
        for step in 0..90 {
            let y = CORE_Y + reach + step as f64 * 0.15;

            // 波を出さずに同じ鉱石を走らせ、波面が到達距離へ着く tick
            // (`expand` 回目の移動) にその距離を跨ぐ初期位置だけを選ぶ。
            let mut probe = state_with_core_pulse(3);
            push_test_ore(&mut probe, CX, y, 1e6);
            for _ in 0..expand - 1 {
                step_ores(&mut probe);
            }
            let entering = (probe.ores[0].y - CORE_Y).hypot(probe.ores[0].x - CX);
            step_ores(&mut probe);
            let leaving = (probe.ores[0].y - CORE_Y).hypot(probe.ores[0].x - CX);
            if entering <= reach || leaving > reach {
                continue;
            }
            crossings += 1;

            let mut state = state_with_core_pulse(3);
            push_test_ore(&mut state, CX, y, 1e6);
            spawn_pulse_wave(&mut state, reach, dmg);
            let before = state.ores[0].hp;
            for _ in 0..expand + 4 {
                step_pulse_rings(&mut state);
                step_ores(&mut state);
            }
            let dealt = before - state.ores[0].hp;
            assert!(
                (dealt - dmg).abs() < 1e-6,
                "到達距離 {reach:.2} を跨いだ鉱石 (y={y}) への被弾が1発ぶんでない dealt={dealt}"
            );
        }
        assert!(crossings > 0, "到達距離を跨ぐ初期位置を1つも作れていない");
    }

    /// 波の到達距離の外に居続けた鉱石は削られないこと。
    ///
    /// 波の寿命には到達距離を跨ぐ動きを見るための1tickが含まれるが、その1tickでは
    /// 輪帯が広がらない——波が届く距離そのものは `pulse_reach` のままになる。
    #[test]
    fn pulse_wave_leaves_ores_beyond_its_reach_untouched() {
        let reach = state_with_core_pulse(3).pulse_reach();
        let dmg = state_with_core_pulse(3).pulse_damage();
        let expand = ((reach - INNER_RADIUS) / PULSE_WAVE_SPEED).ceil().max(1.0) as u32;

        let mut state = state_with_core_pulse(3);
        // 波が消えるまでに降りてこられない高さへ置く。
        let y = CORE_Y + reach + expand as f64 * 2.0 + 10.0;
        push_test_ore(&mut state, CX, y, 1e6);
        spawn_pulse_wave(&mut state, reach, dmg);
        let before = state.ores[0].hp;
        for _ in 0..expand + 4 {
            step_pulse_rings(&mut state);
            step_ores(&mut state);
        }
        assert_eq!(
            state.ores[0].hp, before,
            "到達距離の外に居た鉱石を波が削っている y={y}"
        );
    }

    /// タップを連打しても、立てた波それぞれが鉱石をちょうど1度ずつ削ること。
    ///
    /// タップは入力イベントごとに波を立てるので、10 ticks/sec の歩みより速く
    /// 積み上がる。描画のために本数を切り詰めるとき、まだ広がり切っていない波を
    /// そのまま消すと押した回数ぶんの手応えが消え、逆に残りの輪帯をその tick の
    /// 輪帯と重ねて通すと同じ鉱石が2度削られる。どちらへ転んでも合計が合わなく
    /// なるので、過不足の両方をダメージの一致で見る。
    ///
    /// 鉱石を静止させたまま回すと前 tick の中心距離が現在値と一致し、輪帯の
    /// 掃引判定が素通りしてしまう。tick と同じ順序で `step_ores` を挟み、波面を
    /// 実際に跨がせて測る。二重計上は「その tick に波面を内側へ跨いだ鉱石」で
    /// だけ起きるので、跨ぐ位相が切り詰めの tick と噛み合うよう初期高さを
    /// 1tick ぶんの詰まり幅より細かく振って舐める。
    #[test]
    fn rapid_taps_deal_damage_for_every_wave() {
        // 1 tick の合間に一気に押す場合と、tick をまたいで押し続ける場合の両方で
        // 本数の上限を越えさせる。
        for (taps_per_step, steps) in [(24, 1), (3, 12)] {
            for phase in 0..12 {
                let mut state = state_with_core_pulse(6);
                // 層開放の演出波は鉱石に触れないが、本数の枠は食う。
                state.pulse_rings.clear();
                // 手動波が届く範囲の内側に、削り切られない硬い鉱石を置く。波面が
                // 下から追い越していくので、降りてくる鉱石と必ずすれ違う。
                let reach = state.pulse_reach() * 0.55;
                let y = CORE_Y + reach * 0.75 + phase as f64 * 0.45;
                push_test_ore(&mut state, CX, y, 1e6);
                let before = state.ores[0].hp;

                let mut expected = 0.0;
                for _ in 0..steps {
                    for _ in 0..taps_per_step {
                        manual_strike(&mut state);
                        // タップが載せたブースト込みの値。`manual_strike` が内部で
                        // 使う値と一致する。
                        expected += state.weapon_damage(WeaponKind::Pulse) * 2.2
                            + state.pulse_damage() * 0.6;
                    }
                    step_pulse_rings(&mut state);
                    step_ores(&mut state);
                }
                // 残った波が広がり切るまで回す。
                while !state.pulse_rings.is_empty() {
                    step_pulse_rings(&mut state);
                    step_ores(&mut state);
                }

                let dealt = before - state.ores[0].hp;
                assert!(
                    (dealt - expected).abs() < 1e-6,
                    "{taps_per_step}連打×{steps}tick (phase={phase}) のダメージが \
                     波の本数と合わない dealt={dealt} expected={expected}"
                );
            }
        }
    }

    /// 弾は5種とも鉱石より一回り小さく、武器ごとの大小関係を保つこと。
    ///
    /// 弾と鉱石は同じ画面に同時に居るので、寸法が近づくと「降ってくる的」と
    /// 「自分の撃った弾」の区別が色だけになる。点グリッドの上で見分けがつくかは
    /// `render` のテストが押さえるので、ここは寸法そのものの並びを見る。
    #[test]
    fn projectiles_stay_smaller_than_every_ore() {
        let ladder = [
            SCATTER_PROJECTILE_RADIUS,
            PULSE_PROJECTILE_RADIUS,
            ARC_PROJECTILE_RADIUS,
            RAY_PROJECTILE_RADIUS,
            NOVA_PROJECTILE_RADIUS,
        ];
        for pair in ladder.windows(2) {
            assert!(
                pair[0] < pair[1],
                "弾の大小関係が崩れている {} >= {}",
                pair[0],
                pair[1]
            );
        }

        let smallest_ore = OreKind::ALL
            .iter()
            .map(|k| k.radius())
            .fold(f64::INFINITY, f64::min);
        assert!(
            NOVA_PROJECTILE_RADIUS * 1.5 <= smallest_ore,
            "最大の弾 {NOVA_PROJECTILE_RADIUS} が最小の鉱石 {smallest_ore} に迫っている"
        );
    }

    /// 層開放の演出で立つ波は鉱石に触れないこと。
    #[test]
    fn ceremony_wave_does_not_damage_ores() {
        let mut state = StarRingState::new();
        state.total_kills = Layer::THRESHOLDS[1];
        state.shards = 1e9;
        push_test_ore(&mut state, CX, CORE_Y + 25.0, 50.0);
        assert!(unlock_next_layer(&mut state));
        assert!(!state.pulse_rings.is_empty());
        for _ in 0..30 {
            step_pulse_rings(&mut state);
        }
        assert_eq!(state.ores[0].hp, 50.0, "演出だけの波が鉱石を削っている");
    }
}
