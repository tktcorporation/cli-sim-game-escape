//! 常夜灯 — ゲームロジック (純粋関数)。
//!
//! `tick()` が1tick分の全処理 (灯の移動→湧き→敵の移動→漏れ判定→
//! 発砲→弾の移動→命中判定→宝箱→ボスの特殊攻撃) を順に進める。
//! render.rs はここで更新された `EverlightState` を読むだけ (書き込まない)。

use ratzilla::ratatui::style::Color;

use crate::effects::FlashTimer;

use super::state::{
    BoonKind, BoonOption, CampUpgrades, Chest, Enemy, EnemyKind, EverlightState, Lantern,
    Loadout, OwnedPassive, OwnedWeapon, PassiveKind, Phase, Projectile, WeaponKind,
    BOSS_EVERY_N_WAVES, BREACH_Y, CHEST_BASE_CATCH_RADIUS, CHEST_FALL_SPEED, COLUMNS,
    ELITE_BASE_INTERVAL_TICKS, HALO_DAMAGE_INTERVAL_TICKS, LANTERN_MOVE_UNITS_PER_TICK, LANTERN_Y,
    MAX_LEVEL, MAX_PASSIVE_SLOTS, SPAWN_Y, WAVE_DURATION_TICKS, WORLD_H, WORLD_W,
};

const PROJECTILE_SPEED: f64 = 9.0;
const SPRAY_SPREAD_RAD: f64 = 1.3;
const MAX_ENEMIES_ON_FIELD: usize = 200;
const MAX_PROJECTILES_ON_FIELD: usize = 300;
const LANE_HALF_WIDTH: f64 = WORLD_W / COLUMNS as f64 / 2.0;

const BOSS_ATTACK_PERIOD_TICKS: u64 = 90;
const BOSS_TELEGRAPH_TICKS: u32 = 20;
const BOSS_TELEGRAPH_DAMAGE: i32 = 22;

// ── 乱数 (xorshift32、seed は state に保存してセーブ/シミュレーターで再現可能にする) ──

fn rng_next(seed: &mut u32) -> u32 {
    let mut x = *seed;
    if x == 0 {
        x = 0xDEAD_BEEF;
    }
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *seed = x;
    x
}

fn rng_below(seed: &mut u32, bound: u32) -> u32 {
    if bound == 0 {
        return 0;
    }
    rng_next(seed) % bound
}

// ── tick エントリポイント ─────────────────────────────────────────

pub fn tick_n(state: &mut EverlightState, n: u32) {
    for _ in 0..n {
        tick(state);
    }
}

pub fn tick(state: &mut EverlightState) {
    state.lantern_hurt_flash.tick(1);
    decay_damage_display(&mut state.last_light_damage);
    if state.log_display_ticks > 0 {
        state.log_display_ticks -= 1;
    }

    // 拠点にいる間、またはレベルアップ選択待ちの間はここで止める
    // (モーダルを開いたまま敵に押し切られる理不尽を避ける)。
    if state.phase != Phase::Vigil || state.pending_boons.is_some() {
        return;
    }

    state.elapsed_ticks += 1;
    update_wave(state);
    move_lantern(state);
    spawn_enemies(state);
    move_enemies(state);
    resolve_breaches(state);

    let damage_mult = effective_damage_mult(state);
    fire_weapons(state, damage_mult);
    move_projectiles(state);
    resolve_projectile_hits(state);

    move_chests(state);
    resolve_chest_catch(state);
    resolve_boss_telegraph(state);

    if state.lantern.light <= 0 {
        state.lantern.light = 0;
        end_vigil(state);
    }
}

fn decay_damage_display(display: &mut Option<(i32, u32)>) {
    match display {
        Some((_, ticks)) if *ticks > 1 => *ticks -= 1,
        Some(_) => *display = None,
        None => {}
    }
}

fn effective_damage_mult(state: &EverlightState) -> f64 {
    state.camp.starting_power_mult() * state.loadout.damage_mult()
}

// ── 波 ─────────────────────────────────────────────────────────

pub fn wave_for(elapsed_ticks: u64) -> u32 {
    (elapsed_ticks / WAVE_DURATION_TICKS as u64) as u32 + 1
}

fn wave_difficulty(wave: u32) -> f64 {
    1.0 + wave.saturating_sub(1) as f64 * 0.12
}

fn update_wave(state: &mut EverlightState) {
    let new_wave = wave_for(state.elapsed_ticks);
    if new_wave != state.wave {
        state.wave = new_wave;
        state.boss_spawned_this_wave = false;
        state.add_log(format!("第{new_wave}波突入"));
    }
}

// ── 灯の移動 ────────────────────────────────────────────────────

pub fn set_lantern_target_lane(state: &mut EverlightState, lane: usize) {
    state.lantern.target_lane = lane.min(COLUMNS - 1);
}

pub fn nudge_lantern(state: &mut EverlightState, delta: i32) {
    let cur = state.lantern.target_lane as i32;
    let next = (cur + delta).clamp(0, COLUMNS as i32 - 1);
    state.lantern.target_lane = next as usize;
}

fn move_lantern(state: &mut EverlightState) {
    let target_x = super::state::lane_center_x(state.lantern.target_lane);
    let dx = target_x - state.lantern.x;
    let max_step = LANTERN_MOVE_UNITS_PER_TICK * state.loadout.move_speed_mult();
    if dx.abs() <= max_step {
        state.lantern.x = target_x;
    } else {
        state.lantern.x += max_step * dx.signum();
    }
}

// ── 湧き ───────────────────────────────────────────────────────

fn spawn_interval_ticks(wave: u32) -> u32 {
    let base = 26i32;
    let reduced = base - (wave as i32 - 1);
    reduced.max(6) as u32
}

fn spawn_enemies(state: &mut EverlightState) {
    if state.wave.is_multiple_of(BOSS_EVERY_N_WAVES) && !state.boss_spawned_this_wave {
        let lane = rng_below(&mut state.rng_state, COLUMNS as u32) as usize;
        let name = EnemyKind::Boss.name();
        spawn_enemy_at(state, EnemyKind::Boss, lane);
        state.boss_spawned_this_wave = true;
        state.add_log(format!("{name}が現れた！"));
    }

    state.elite_progress += 1;
    let elite_interval = ELITE_BASE_INTERVAL_TICKS
        .saturating_sub(state.wave.saturating_sub(1) * 2)
        .max(60);
    if state.elite_progress >= elite_interval {
        state.elite_progress = 0;
        let lane = rng_below(&mut state.rng_state, COLUMNS as u32) as usize;
        spawn_enemy_at(state, EnemyKind::Elite, lane);
    }

    state.spawn_progress += 1;
    let interval = spawn_interval_ticks(state.wave);
    if state.spawn_progress >= interval {
        state.spawn_progress = 0;
        let roll = rng_below(&mut state.rng_state, 100);
        if roll < 15 {
            let base_lane = rng_below(&mut state.rng_state, COLUMNS as u32) as usize;
            for offset in 0..3usize {
                let lane = (base_lane + offset).min(COLUMNS - 1);
                spawn_enemy_at(state, EnemyKind::Swarmling, lane);
            }
        } else if roll < 45 {
            let lane = rng_below(&mut state.rng_state, COLUMNS as u32) as usize;
            spawn_enemy_at(state, EnemyKind::Husk, lane);
        } else {
            let lane = rng_below(&mut state.rng_state, COLUMNS as u32) as usize;
            spawn_enemy_at(state, EnemyKind::Wisp, lane);
        }
    }
}

fn spawn_enemy_at(state: &mut EverlightState, kind: EnemyKind, lane: usize) {
    if state.enemies.len() >= MAX_ENEMIES_ON_FIELD {
        return;
    }
    if kind == EnemyKind::Boss {
        state.boss_spawn_count += 1;
    }
    let diff = wave_difficulty(state.wave);
    let hp = (kind.base_hp() as f64 * diff).round() as i32;
    state.enemies.push(Enemy {
        kind,
        x: super::state::lane_center_x(lane),
        y: SPAWN_Y,
        hp,
        max_hp: hp,
        hurt_flash: FlashTimer::new(),
    });
}

fn move_enemies(state: &mut EverlightState) {
    let diff = wave_difficulty(state.wave);
    let lantern_x = state.lantern.x;
    for enemy in state.enemies.iter_mut() {
        enemy.hurt_flash.tick(1);
        enemy.y += enemy.kind.base_speed() * diff;
        if enemy.kind.homes() {
            let dx = lantern_x - enemy.x;
            let step = dx.abs().min(0.5);
            enemy.x += step * dx.signum();
        }
    }
}

/// 防衛線 (`BREACH_Y`) に達した敵を「漏れ」として処理し、灯を削って消す。
fn resolve_breaches(state: &mut EverlightState) {
    let mut total_damage = 0i32;
    let mut breach_count = 0u32;
    let mut last_kind: Option<EnemyKind> = None;
    state.enemies.retain(|e| {
        if e.y >= BREACH_Y {
            total_damage += e.kind.contact_damage();
            breach_count += 1;
            last_kind = Some(e.kind);
            false
        } else {
            true
        }
    });
    if breach_count == 0 {
        return;
    }
    state.breach_count += breach_count;
    state.light_hit_count += breach_count;
    state.lantern.light -= total_damage;
    state.lantern_hurt_flash.trigger(3);
    state.last_light_damage = Some((total_damage, 6));
    if let Some(kind) = last_kind {
        state.add_log(format!("{}が防衛線を突破！灯が{}削れた", kind.name(), total_damage));
    }
}

// ── 討伐処理の共通ヘルパー ─────────────────────────────────────────

struct KillInfo {
    kind: EnemyKind,
    x: f64,
    y: f64,
}

/// HP が尽きた敵を盤面から取り除き、討伐情報を返す。呼び出し側は直前で
/// 対象にダメージを与え終えている前提 (このtick中に他要因で死んだ敵は
/// 残っていない、という不変条件を各戦闘解決関数が維持する)。
fn drain_dead_enemies(state: &mut EverlightState) -> Vec<KillInfo> {
    let mut kills = Vec::new();
    state.enemies.retain(|e| {
        if e.hp <= 0 {
            kills.push(KillInfo { kind: e.kind, x: e.x, y: e.y });
            false
        } else {
            true
        }
    });
    kills
}

fn apply_kills(state: &mut EverlightState, kills: Vec<KillInfo>) {
    if kills.is_empty() {
        return;
    }
    for k in &kills {
        state.ember += k.kind.ember_reward();
        state.kill_count += 1;
        if k.kind.drops_chest() {
            state.chests.push(Chest { x: k.x, y: k.y });
        }
    }
    if kills.len() == 1 {
        state.add_log(format!("{}を討った", kills[0].kind.name()));
    } else {
        state.add_log(format!("{}体を討った", kills.len()));
    }
}

// ── 発砲・弾 ────────────────────────────────────────────────────

/// 光弾の自動照準先を選ぶ。精鋭/魔王 (宝箱を落とす個体) がいれば
/// 最もHPが少ない = 倒しやすい個体を優先して確実に討伐へつなげる —
/// 精鋭は移動が遅いため「防衛線に近い順」で選ぶと足の速い雑魚に埋もれて
/// 万年後回しにされ、宝箱もレベルアップも一向に発生しなくなってしまう。
/// 精鋭/魔王がいない間は、防衛線に最も近い (=最も差し迫った) 敵へ
/// 自動照準して安全網として働く。
fn pick_bolt_target(state: &EverlightState) -> Option<(f64, f64)> {
    if let Some(e) = state.enemies.iter().filter(|e| e.kind.drops_chest()).min_by_key(|e| e.hp) {
        return Some((e.x, e.y));
    }
    state
        .enemies
        .iter()
        .max_by(|a, b| a.y.partial_cmp(&b.y).unwrap_or(std::cmp::Ordering::Equal))
        .map(|e| (e.x, e.y))
}

fn aim_velocity(from_x: f64, from_y: f64, to_x: f64, to_y: f64, speed: f64) -> (f64, f64) {
    let dx = to_x - from_x;
    let dy = to_y - from_y;
    let dist = (dx * dx + dy * dy).sqrt();
    if dist < 1e-6 {
        return (0.0, -speed);
    }
    (dx / dist * speed, dy / dist * speed)
}

fn make_projectile(x: f64, damage: i32, pierce: u32, vx: f64, vy: f64, color: Color) -> Projectile {
    Projectile {
        x,
        y: LANTERN_Y,
        vx,
        vy,
        damage,
        pierce_remaining: pierce.saturating_sub(1),
        radius: 1.6,
        color,
    }
}

fn fire_weapons(state: &mut EverlightState, damage_mult: f64) {
    let lantern_x = state.lantern.x;
    let cooldown_mult = state.loadout.cooldown_mult();
    let mut new_projectiles: Vec<Projectile> = Vec::new();

    for i in 0..state.loadout.weapons.len() {
        let kind = state.loadout.weapons[i].kind;
        if kind == WeaponKind::Halo {
            continue; // 光輪は常時判定なので fire_halo で別途処理する
        }
        if state.loadout.weapons[i].cooldown_remaining > 0 {
            state.loadout.weapons[i].cooldown_remaining -= 1;
            continue;
        }
        let base_cooldown = state.loadout.weapons[i].cooldown_ticks();
        let cooldown = ((base_cooldown as f64) * cooldown_mult).round().max(1.0) as u32;
        state.loadout.weapons[i].cooldown_remaining = cooldown;
        let base_damage = state.loadout.weapons[i].damage();
        let damage = ((base_damage as f64) * damage_mult).round() as i32;
        let pierce = state.loadout.weapons[i].pierce();

        match kind {
            WeaponKind::Bolt => {
                // 灯のレーンに固定せず、防衛線に最も近い敵へ自動照準する
                // — 「装備が薄い序盤でも最低限の安全網になる」信頼できる
                // 単体火力という光弾の役割を成立させるための設計判断。
                // (灯の位置そのものを活かす面制圧は散光/極光/光輪が担う)
                let (vx, vy) = match pick_bolt_target(state) {
                    Some((tx, ty)) => aim_velocity(lantern_x, LANTERN_Y, tx, ty, PROJECTILE_SPEED),
                    None => (0.0, -PROJECTILE_SPEED),
                };
                new_projectiles.push(make_projectile(lantern_x, damage, pierce, vx, vy, kind.color()));
            }
            WeaponKind::Spray => {
                let count = state.loadout.weapons[i].projectile_count();
                for p in 0..count {
                    let t = if count == 1 { 0.5 } else { p as f64 / (count - 1) as f64 };
                    let angle = -std::f64::consts::FRAC_PI_2 + (t - 0.5) * SPRAY_SPREAD_RAD;
                    let vx = angle.cos() * PROJECTILE_SPEED;
                    let vy = angle.sin() * PROJECTILE_SPEED;
                    new_projectiles.push(make_projectile(lantern_x, damage, pierce, vx, vy, kind.color()));
                }
            }
            WeaponKind::Aurora => {
                apply_aurora_hit(state, lantern_x, damage);
            }
            WeaponKind::Halo => unreachable!("Halo は上のcontinueで除外済み"),
        }
    }

    if state.projectiles.len() + new_projectiles.len() > MAX_PROJECTILES_ON_FIELD {
        new_projectiles.truncate(MAX_PROJECTILES_ON_FIELD.saturating_sub(state.projectiles.len()));
    }
    state.projectiles.extend(new_projectiles);

    fire_halo(state, damage_mult);
}

/// 極光: 灯のレーンにいる全ての敵に即ダメージを与える (ヒットスキャン)。
fn apply_aurora_hit(state: &mut EverlightState, lantern_x: f64, damage: i32) {
    for enemy in state.enemies.iter_mut() {
        if (enemy.x - lantern_x).abs() <= LANE_HALF_WIDTH {
            enemy.hp -= damage;
            enemy.hurt_flash.trigger(4);
        }
    }
    let kills = drain_dead_enemies(state);
    apply_kills(state, kills);
}

/// 光輪: 灯の周囲を継続的に判定する近接武器。`HALO_DAMAGE_INTERVAL_TICKS`
/// ごとにまとめて判定することで、範囲内に居座られた時のダメージ計算を
/// 1tickごとの浮動小数累積ではなく整数の一定間隔ダメージにしている。
fn fire_halo(state: &mut EverlightState, damage_mult: f64) {
    let Some(halo) = state.loadout.weapon_mut(WeaponKind::Halo).copied() else {
        return;
    };
    state.halo_tick += 1;
    if !state.halo_tick.is_multiple_of(HALO_DAMAGE_INTERVAL_TICKS) {
        return;
    }
    let damage = ((halo.damage() as f64) * damage_mult).round() as i32;
    let radius = halo.halo_radius();
    let lantern_x = state.lantern.x;
    for enemy in state.enemies.iter_mut() {
        let dx = enemy.x - lantern_x;
        let dy = enemy.y - LANTERN_Y;
        if dx * dx + dy * dy <= radius * radius {
            enemy.hp -= damage;
            enemy.hurt_flash.trigger(3);
        }
    }
    let kills = drain_dead_enemies(state);
    apply_kills(state, kills);
}

fn move_projectiles(state: &mut EverlightState) {
    for p in state.projectiles.iter_mut() {
        p.x += p.vx;
        p.y += p.vy;
    }
}

fn is_in_bounds(p: &Projectile) -> bool {
    p.y > -10.0 && p.y < WORLD_H + 10.0 && p.x > -10.0 && p.x < WORLD_W + 10.0
}

fn resolve_projectile_hits(state: &mut EverlightState) {
    let projectiles = std::mem::take(&mut state.projectiles);
    let mut surviving = Vec::with_capacity(projectiles.len());

    for mut proj in projectiles {
        let mut consumed = false;
        for enemy in state.enemies.iter_mut() {
            if enemy.hp <= 0 {
                continue;
            }
            let dx = enemy.x - proj.x;
            let dy = enemy.y - proj.y;
            let hit_dist = enemy.kind.radius() + proj.radius;
            if dx * dx + dy * dy <= hit_dist * hit_dist {
                enemy.hp -= proj.damage;
                enemy.hurt_flash.trigger(3);
                if proj.pierce_remaining == 0 {
                    consumed = true;
                } else {
                    proj.pierce_remaining -= 1;
                }
                // 貫通弾でも1tickにつき命中は1体まで — スタックした敵の
                // 束を一瞬で焼き払わないための意図的な制限 (残りの敵には
                // 次tickの移動後に改めて命中判定される)。
                break;
            }
        }
        if !consumed && is_in_bounds(&proj) {
            surviving.push(proj);
        }
    }
    state.projectiles = surviving;

    let kills = drain_dead_enemies(state);
    apply_kills(state, kills);
}

// ── 宝箱 ───────────────────────────────────────────────────────

fn move_chests(state: &mut EverlightState) {
    for chest in state.chests.iter_mut() {
        chest.y += CHEST_FALL_SPEED;
    }
}

fn resolve_chest_catch(state: &mut EverlightState) {
    let lantern_x = state.lantern.x;
    let catch_radius = CHEST_BASE_CATCH_RADIUS + state.loadout.magnet_radius_bonus();
    let mut caught = false;
    state.chests.retain(|c| {
        if c.y >= BREACH_Y {
            return false; // 取り逃した
        }
        let dx = (c.x - lantern_x).abs();
        let dy = (c.y - LANTERN_Y).abs();
        if dx <= catch_radius && dy <= 6.0 {
            caught = true;
            state.chest_caught_count += 1;
            false
        } else {
            true
        }
    });
    if caught && state.pending_boons.is_none() {
        open_boon_modal(state);
    }
}

// ── レベルアップ選択 (宝箱を取ると開く) ─────────────────────────────

fn open_boon_modal(state: &mut EverlightState) {
    let options = roll_boon_options(state);
    state.pending_boons = Some(options);
    state.add_log("宝箱を見つけた！強化を選ぼう".to_string());
}

fn candidate_boons(state: &EverlightState) -> Vec<BoonOption> {
    let mut v = Vec::new();
    for &kind in WeaponKind::all() {
        if let Some(w) = state.loadout.weapons.iter().find(|w| w.kind == kind) {
            if w.level < MAX_LEVEL {
                v.push(BoonOption { kind: BoonKind::LevelWeapon(kind) });
            }
        } else if state.loadout.weapons.len() < state.camp.max_weapon_slots() {
            v.push(BoonOption { kind: BoonKind::NewWeapon(kind) });
        }
    }
    for &kind in PassiveKind::all() {
        if let Some(p) = state.loadout.passives.iter().find(|p| p.kind == kind) {
            if p.level < MAX_LEVEL {
                v.push(BoonOption { kind: BoonKind::LevelPassive(kind) });
            }
        } else if state.loadout.passives.len() < MAX_PASSIVE_SLOTS {
            v.push(BoonOption { kind: BoonKind::NewPassive(kind) });
        }
    }
    v
}

fn roll_boon_options(state: &mut EverlightState) -> [BoonOption; 3] {
    let mut pool = candidate_boons(state);
    let mut chosen: Vec<BoonOption> = Vec::with_capacity(3);
    for _ in 0..3 {
        if pool.is_empty() {
            break;
        }
        let idx = rng_below(&mut state.rng_state, pool.len() as u32) as usize;
        chosen.push(pool.remove(idx));
    }
    // 装備・強化が全て上限に達した終盤は候補が3未満になり得る。その場合は
    // 「威力」レベルアップで埋めて、モーダルが必ず3枠揃うようにする
    // (apply_boon は既に MAX_LEVEL のものへは何もしないので安全)。
    while chosen.len() < 3 {
        chosen.push(BoonOption { kind: BoonKind::LevelPassive(PassiveKind::Power) });
    }
    [chosen[0], chosen[1], chosen[2]]
}

/// モーダルの `index` 番目 (0..3) を選ぶ。選択肢が開いていない/範囲外なら
/// 何もせず `false` を返す。
pub fn choose_boon(state: &mut EverlightState, index: usize) -> bool {
    let Some(options) = state.pending_boons else {
        return false;
    };
    if index >= options.len() {
        return false;
    }
    apply_boon(state, options[index].kind);
    state.pending_boons = None;
    true
}

fn apply_boon(state: &mut EverlightState, kind: BoonKind) {
    match kind {
        BoonKind::NewWeapon(k) => {
            state.loadout.weapons.push(OwnedWeapon::new(k));
            state.add_log(format!("『{}』を手に入れた", k.name()));
        }
        BoonKind::LevelWeapon(k) => {
            if let Some(w) = state.loadout.weapon_mut(k) {
                w.level = (w.level + 1).min(MAX_LEVEL);
            }
            state.add_log(format!("『{}』が強化された", k.name()));
        }
        BoonKind::NewPassive(k) => {
            state.loadout.passives.push(OwnedPassive::new(k));
            state.add_log(format!("『{}』を手に入れた", k.name()));
        }
        BoonKind::LevelPassive(k) => {
            if let Some(p) = state.loadout.passive_mut(k) {
                p.level = (p.level + 1).min(MAX_LEVEL);
            }
            state.add_log(format!("『{}』が強化された", k.name()));
        }
    }
    if matches!(kind, BoonKind::NewPassive(PassiveKind::Radiance) | BoonKind::LevelPassive(PassiveKind::Radiance)) {
        recompute_light_max(state);
    }
}

fn recompute_light_max(state: &mut EverlightState) {
    let new_max = state.camp.light_max() + state.loadout.max_light_bonus();
    let delta = new_max - state.lantern.light_max;
    state.lantern.light_max = new_max;
    if delta > 0 {
        state.lantern.light = (state.lantern.light + delta).min(new_max);
    } else {
        state.lantern.light = state.lantern.light.min(new_max);
    }
}

/// レベルアップモーダルのカード表示テキスト (タイトル, 説明) を作る。
/// 現在Lv→次Lvを明示する — 戦略ゲームは情報を隠さず見せる方針
/// (Cookie Factory の設計原則を踏襲)。
pub fn boon_option_text(state: &EverlightState, kind: BoonKind) -> (String, String) {
    match kind {
        BoonKind::NewWeapon(k) => (format!("新武器: {}", k.name()), k.summary().to_string()),
        BoonKind::LevelWeapon(k) => {
            let cur = state.loadout.weapons.iter().find(|w| w.kind == k).map(|w| w.level).unwrap_or(1);
            (format!("{} Lv{}→{}", k.name(), cur, cur + 1), k.summary().to_string())
        }
        BoonKind::NewPassive(k) => (format!("新効果: {}", k.name()), k.summary().to_string()),
        BoonKind::LevelPassive(k) => {
            let cur = state.loadout.passive_level(k);
            (format!("{} Lv{}→{}", k.name(), cur, cur + 1), k.summary().to_string())
        }
    }
}

// ── ボスの特殊攻撃「灯喰らい」───────────────────────────────────────

fn resolve_boss_telegraph(state: &mut EverlightState) {
    let boss_x = state.enemies.iter().find(|e| e.kind == EnemyKind::Boss).map(|e| e.x);

    if let Some((x, ticks_left)) = state.boss_telegraph {
        // 構え中に魔王を討ち取れば不発になる — 「間に合った」満足感のため。
        if boss_x.is_none() {
            state.boss_telegraph = None;
            return;
        }
        if ticks_left <= 1 {
            state.boss_telegraph = None;
            if (state.lantern.x - x).abs() <= LANE_HALF_WIDTH {
                state.lantern.light -= BOSS_TELEGRAPH_DAMAGE;
                state.light_hit_count += 1;
                state.lantern_hurt_flash.trigger(5);
                state.last_light_damage = Some((BOSS_TELEGRAPH_DAMAGE, 8));
                state.add_log("魔王の一撃で灯が大きく削れた！".to_string());
            } else {
                state.add_log("魔王の一撃をかわした！".to_string());
            }
        } else {
            state.boss_telegraph = Some((x, ticks_left - 1));
        }
        return;
    }

    if let Some(x) = boss_x {
        if state.elapsed_ticks > 0 && state.elapsed_ticks.is_multiple_of(BOSS_ATTACK_PERIOD_TICKS) {
            state.boss_telegraph = Some((x, BOSS_TELEGRAPH_TICKS));
            state.add_log("魔王が灯喰らいの構え！".to_string());
        }
    }
}

// ── 拠点 (恒久強化) ────────────────────────────────────────────────

pub fn purchase_light(state: &mut EverlightState) -> bool {
    let cost = state.camp.light_cost();
    if state.ember < cost {
        return false;
    }
    state.ember -= cost;
    state.camp.light_level += 1;
    true
}

pub fn purchase_power(state: &mut EverlightState) -> bool {
    let cost = state.camp.power_cost();
    if state.ember < cost {
        return false;
    }
    state.ember -= cost;
    state.camp.power_level += 1;
    true
}

pub fn purchase_extra_slot(state: &mut EverlightState) -> bool {
    if state.camp.extra_slot_level >= 1 || state.ember < CampUpgrades::EXTRA_SLOT_COST {
        return false;
    }
    state.ember -= CampUpgrades::EXTRA_SLOT_COST;
    state.camp.extra_slot_level = 1;
    true
}

pub fn start_vigil(state: &mut EverlightState) {
    let light_max = state.camp.light_max();
    state.phase = Phase::Vigil;
    state.lantern = Lantern::new(light_max);
    state.enemies.clear();
    state.projectiles.clear();
    state.chests.clear();
    state.loadout = Loadout::default();
    state.loadout.weapons.push(OwnedWeapon::new(WeaponKind::Bolt));
    state.wave = 1;
    state.elapsed_ticks = 0;
    state.spawn_progress = 0;
    state.elite_progress = 0;
    state.boss_spawned_this_wave = false;
    state.halo_tick = 0;
    state.pending_boons = None;
    state.boss_telegraph = None;
    state.kill_count = 0;
    state.breach_count = 0;
    state.last_light_damage = None;
    state.add_log("夜番開始。灯を守れ！".to_string());
}

fn end_vigil(state: &mut EverlightState) {
    if state.wave > state.best_wave {
        state.best_wave = state.wave;
    }
    if state.elapsed_ticks > state.best_survival_ticks {
        state.best_survival_ticks = state.elapsed_ticks;
    }
    state.phase = Phase::Camp;
    state.pending_boons = None;
    let wave = state.wave;
    state.add_log(format!("夜番終了。第{wave}波まで守り抜いた"));
}

/// プレイヤー自身の意思で拠点へ撤退する。灯が尽きた場合と同じ後処理
/// (自己ベスト更新・残光の確定) を共有する — 積み上げた残光は撤退でも
/// 死亡でも失われない設計 (通貨喪失によるペナルティより、生存時間・波数
/// そのものが挑戦の核なので、余計な損失は入れない)。
pub fn retreat_to_camp(state: &mut EverlightState) {
    if state.phase != Phase::Vigil {
        return;
    }
    end_vigil(state);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tick_on_camp_phase_does_not_advance_vigil_fields() {
        let mut state = EverlightState::new();
        tick_n(&mut state, 50);
        assert_eq!(state.elapsed_ticks, 0);
        assert!(state.enemies.is_empty());
    }

    #[test]
    fn start_vigil_resets_run_scoped_fields_and_grants_starting_weapon() {
        let mut state = EverlightState::new();
        state.ember = 100;
        start_vigil(&mut state);
        assert_eq!(state.phase, Phase::Vigil);
        assert_eq!(state.loadout.weapons.len(), 1);
        assert_eq!(state.loadout.weapons[0].kind, WeaponKind::Bolt);
        assert_eq!(state.lantern.light, state.lantern.light_max);
    }

    #[test]
    fn long_vigil_eventually_spawns_enemies_and_fires_projectiles() {
        let mut state = EverlightState::new();
        start_vigil(&mut state);
        tick_n(&mut state, 60);
        assert!(!state.enemies.is_empty(), "60tick(6秒)経てば敵が湧いているはず");
    }

    #[test]
    fn breach_damages_light_and_removes_enemy() {
        let mut state = EverlightState::new();
        start_vigil(&mut state);
        state.enemies.push(Enemy {
            kind: EnemyKind::Wisp,
            x: state.lantern.x,
            y: BREACH_Y,
            hp: 999,
            max_hp: 999,
            hurt_flash: FlashTimer::new(),
        });
        let light_before = state.lantern.light;
        resolve_breaches(&mut state);
        assert!(state.lantern.light < light_before);
        assert!(state.enemies.is_empty());
        assert_eq!(state.breach_count, 1);
    }

    #[test]
    fn projectile_kills_enemy_and_awards_ember() {
        let mut state = EverlightState::new();
        start_vigil(&mut state);
        state.enemies.push(Enemy {
            kind: EnemyKind::Wisp,
            x: state.lantern.x,
            y: LANTERN_Y - 5.0,
            hp: 1,
            max_hp: 7,
            hurt_flash: FlashTimer::new(),
        });
        state.projectiles.push(make_projectile(state.lantern.x, 10, 1, 0.0, -1.0, Color::White));
        state.projectiles[0].y = LANTERN_Y - 5.0;
        let ember_before = state.ember;
        resolve_projectile_hits(&mut state);
        assert!(state.enemies.is_empty());
        assert!(state.ember > ember_before);
        assert_eq!(state.kill_count, 1);
    }

    #[test]
    fn chest_catch_opens_boon_modal_and_pauses_tick() {
        let mut state = EverlightState::new();
        start_vigil(&mut state);
        state.chests.push(Chest { x: state.lantern.x, y: LANTERN_Y });
        resolve_chest_catch(&mut state);
        assert!(state.pending_boons.is_some());
        assert!(state.chests.is_empty());

        let elapsed_before = state.elapsed_ticks;
        tick(&mut state);
        assert_eq!(state.elapsed_ticks, elapsed_before, "モーダル表示中はtickが進まない");
    }

    #[test]
    fn choose_boon_levels_up_owned_weapon() {
        let mut state = EverlightState::new();
        start_vigil(&mut state);
        state.pending_boons = Some([
            BoonOption { kind: BoonKind::LevelWeapon(WeaponKind::Bolt) },
            BoonOption { kind: BoonKind::NewPassive(PassiveKind::Power) },
            BoonOption { kind: BoonKind::NewWeapon(WeaponKind::Spray) },
        ]);
        assert!(choose_boon(&mut state, 0));
        assert_eq!(state.loadout.weapons[0].level, 2);
        assert!(state.pending_boons.is_none());
    }

    #[test]
    fn choose_boon_out_of_range_is_noop() {
        let mut state = EverlightState::new();
        state.pending_boons = Some([
            BoonOption { kind: BoonKind::NewWeapon(WeaponKind::Spray) },
            BoonOption { kind: BoonKind::NewWeapon(WeaponKind::Aurora) },
            BoonOption { kind: BoonKind::NewWeapon(WeaponKind::Halo) },
        ]);
        assert!(!choose_boon(&mut state, 5));
        assert!(state.pending_boons.is_some());
    }

    #[test]
    fn radiance_boon_increases_max_light_and_heals() {
        let mut state = EverlightState::new();
        start_vigil(&mut state);
        state.lantern.light -= 50;
        let max_before = state.lantern.light_max;
        apply_boon(&mut state, BoonKind::NewPassive(PassiveKind::Radiance));
        assert!(state.lantern.light_max > max_before);
        assert!(state.lantern.light > max_before - 50, "灯心強化は現在値も引き上げるはず");
    }

    #[test]
    fn purchase_light_deducts_ember_and_increases_level() {
        let mut state = EverlightState::new();
        state.ember = 100;
        let cost = state.camp.light_cost();
        assert!(purchase_light(&mut state));
        assert_eq!(state.ember, 100 - cost);
        assert_eq!(state.camp.light_level, 1);
    }

    #[test]
    fn purchase_fails_when_ember_insufficient() {
        let mut state = EverlightState::new();
        state.ember = 0;
        assert!(!purchase_light(&mut state));
        assert!(!purchase_power(&mut state));
        assert!(!purchase_extra_slot(&mut state));
    }

    #[test]
    fn wave_escalates_with_elapsed_time() {
        assert_eq!(wave_for(0), 1);
        assert_eq!(wave_for(WAVE_DURATION_TICKS as u64), 2);
        assert_eq!(wave_for(WAVE_DURATION_TICKS as u64 * 4), 5);
    }

    #[test]
    fn retreat_to_camp_banks_best_wave_and_returns_to_camp() {
        let mut state = EverlightState::new();
        start_vigil(&mut state);
        state.wave = 7;
        state.elapsed_ticks = 1234;
        retreat_to_camp(&mut state);
        assert_eq!(state.phase, Phase::Camp);
        assert_eq!(state.best_wave, 7);
        assert_eq!(state.best_survival_ticks, 1234);
    }

    #[test]
    fn light_reaching_zero_ends_vigil_via_tick() {
        let mut state = EverlightState::new();
        start_vigil(&mut state);
        state.lantern.light = 1;
        state.enemies.push(Enemy {
            kind: EnemyKind::Boss,
            x: state.lantern.x,
            y: BREACH_Y,
            hp: 999,
            max_hp: 999,
            hurt_flash: FlashTimer::new(),
        });
        tick(&mut state);
        assert_eq!(state.phase, Phase::Camp);
        assert_eq!(state.lantern.light, 0);
    }
}
