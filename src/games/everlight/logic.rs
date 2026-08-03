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
    ELITE_BASE_INTERVAL_TICKS, EVOLUTION_PASSIVE_THRESHOLD, LANE_HALF_WIDTH,
    LANTERN_MOVE_UNITS_PER_TICK, LANTERN_Y, MAX_LEVEL, MAX_WEAPON_SLOTS, SPAWN_Y,
    WAVE_DURATION_TICKS, WORLD_H, WORLD_W,
};

const PROJECTILE_SPEED: f64 = 9.0;
const SPRAY_SPREAD_RAD: f64 = 1.3;
const MAX_ENEMIES_ON_FIELD: usize = 200;
const MAX_PROJECTILES_ON_FIELD: usize = 300;
/// 極光の薙ぎ払い帯・光輪のパルスリングを表示する長さ (tick)。命中の
/// 有無に関わらず「発火した」こと自体を見せるための演出用タイマーなので、
/// 肉眼で追える長さにしている。
///
/// `GameTime::update` はフレーム落ち・バックグラウンドタブ復帰時に最大
/// 500ms (10 ticks/sec換算で5tick) 分をまとめて処理してから1回だけ
/// renderする (`src/time.rs`)。このフラッシュはtickごとに1ずつ減衰する
/// ため、まとめ処理された同一バッチの最初のtickで発火すると、以降の
/// 最大4回の減衰でrenderが一度も観測しないまま0まで減ってしまう
/// (バッチの先頭で発火→残り4tick分減衰→値が0以下)。5未満にすると
/// この「発火したのに一度も表示されない」退行が起こり得るため、
/// 5を下限として維持すること。
const AURORA_FLASH_TICKS: u32 = 5;
const HALO_FLASH_TICKS: u32 = 5;

const BOSS_ATTACK_PERIOD_TICKS: u64 = 90;
const BOSS_TELEGRAPH_TICKS: u32 = 20;
const BOSS_TELEGRAPH_DAMAGE: i32 = 22;

const INSTANT_HEAL_AMOUNT: i32 = 30;
const EMBER_WINDFALL_AMOUNT: u32 = 25;

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
    state.aurora_flash.tick(1);
    state.halo_flash.tick(1);
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
    // 敵自身もこのtickで動くため、命中判定 (move_and_resolve_projectiles)
    // では移動前の位置を敵ごとの相対運動の基準として使う。
    let enemy_prev_positions: std::collections::HashMap<u32, (f64, f64)> =
        state.enemies.iter().map(|e| (e.id, (e.x, e.y))).collect();
    move_enemies(state);
    resolve_breaches(state);

    let damage_mult = effective_damage_mult(state);
    fire_weapons(state, damage_mult);
    move_and_resolve_projectiles(state, &enemy_prev_positions);

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
        // 敵数上限で実際には湧けなかった時に `boss_spawned_this_wave` を
        // 立ててしまうと、ログだけ「出現した」と嘘をつく上、そのウェーブ中
        // 二度とボスの湧き抽選が行われなくなる。実際に湧いた時だけ確定させ、
        // 上限で弾かれた場合は次tick以降に再抽選させる。
        if spawn_enemy_at(state, EnemyKind::Boss, lane) {
            state.boss_spawned_this_wave = true;
            state.add_log(format!("{}が現れた！", EnemyKind::Boss.name()));
        }
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

/// 敵を1体湧かせる。フィールドの敵数上限で弾かれた場合は `false` を返す。
fn spawn_enemy_at(state: &mut EverlightState, kind: EnemyKind, lane: usize) -> bool {
    if state.enemies.len() >= MAX_ENEMIES_ON_FIELD {
        return false;
    }
    if kind == EnemyKind::Boss {
        state.boss_spawn_count += 1;
    }
    let diff = wave_difficulty(state.wave);
    let hp = (kind.base_hp() as f64 * diff).round() as i32;
    let id = state.next_enemy_id;
    state.next_enemy_id += 1;
    state.enemies.push(Enemy {
        id,
        kind,
        x: super::state::lane_center_x(lane),
        y: SPAWN_Y,
        hp,
        max_hp: hp,
        hurt_flash: FlashTimer::new(),
    });
    true
}

fn move_enemies(state: &mut EverlightState) {
    let diff = wave_difficulty(state.wave);
    let lantern_x = state.lantern.x;
    for enemy in state.enemies.iter_mut() {
        enemy.hurt_flash.tick(1);
        enemy.y += enemy.kind.base_speed() * diff;
        if enemy.kind.homes() {
            // 灯へ寄ってくる敵をおとりにして1レーンへ集め、極光で薙ぐ、
            // という自力発見してほしいシナジー。誘引が弱すぎると気付かれ
            // ないため、はっきり体感できる速さにしている。
            let dx = lantern_x - enemy.x;
            let step = dx.abs().min(1.0);
            enemy.x += step * dx.signum();
        }
    }
}

/// 防衛線 (`BREACH_Y`) に達した敵を「漏れ」として処理し、灯を削って消す。
fn resolve_breaches(state: &mut EverlightState) {
    let lantern_x = state.lantern.x;
    let mut total_damage = 0i32;
    let mut breach_count = 0u32;
    let mut last_kind: Option<EnemyKind> = None;
    state.enemies.retain(|e| {
        if e.y >= BREACH_Y {
            let base = e.kind.contact_damage();
            // 灯が今まさに漏れようとしている敵と同じレーンにいれば、灯の光に
            // 炙られて弱った状態で突破する (=ダメージ半減)。「位置取りが
            // 生存に効く」というタワーディフェンスらしい緊張感の核。
            let in_lantern_lane = (e.x - lantern_x).abs() <= LANE_HALF_WIDTH;
            let dmg = if in_lantern_lane { (base / 2).max(1) } else { base };
            total_damage += dmg;
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
        hit_enemy_ids: Vec::new(),
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
        // 発火した「今」のtickをcooldown 1周期に含めるため -1 する。
        // ここを`cooldown`のまま代入すると、次に0になった次のtickまで
        // 待ってから発火するため、実際の発射間隔が `cooldown_ticks()+1`
        // tickになってしまう (バランス数値と実挙動がズレるオフバイワン)。
        state.loadout.weapons[i].cooldown_remaining = cooldown.saturating_sub(1);
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
                let width_mult = state.loadout.weapons[i].aurora_width_mult();
                apply_aurora_hit(state, lantern_x, damage, width_mult);
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
/// `width_mult` は進化 (`OwnedWeapon::aurora_width_mult`) で広がる判定幅。
fn apply_aurora_hit(state: &mut EverlightState, lantern_x: f64, damage: i32, width_mult: f64) {
    // 命中の有無に関わらず、発火した事実そのものをrender.rsに伝える。
    // 位置は現在の`state.lantern.x`ではなく、実際に判定に使ったこの
    // `lantern_x` をスナップショットする — まとめtick処理中に灯が移動した
    // 後にまとめて1回だけrenderされると、現在位置は実際に判定したレーンと
    // ズレてしまうため。
    state.aurora_flash.trigger(AURORA_FLASH_TICKS);
    state.aurora_flash_x = lantern_x;
    let half_width = LANE_HALF_WIDTH * width_mult;
    for enemy in state.enemies.iter_mut() {
        if (enemy.x - lantern_x).abs() <= half_width {
            enemy.hp -= damage;
            enemy.hurt_flash.trigger(4);
        }
    }
    let kills = drain_dead_enemies(state);
    apply_kills(state, kills);
}

/// 光輪: 灯の周囲を継続的に判定する近接武器。一定間隔でまとめて判定する
/// ことで、範囲内に居座られた時のダメージ計算を1tickごとの浮動小数累積
/// ではなく整数の一定間隔ダメージにしている。間隔は他の武器と同じく
/// `cooldown_ticks()` (レベル/進化) と速射パッシブの `cooldown_mult()` を
/// 反映する — でなければ光輪だけ「全武器のクールダウン短縮」という
/// 速射パッシブの説明文が嘘になってしまう。
fn fire_halo(state: &mut EverlightState, damage_mult: f64) {
    let Some(halo) = state.loadout.weapon_mut(WeaponKind::Halo).copied() else {
        return;
    };
    let cooldown_mult = state.loadout.cooldown_mult();
    let interval = ((halo.cooldown_ticks() as f64) * cooldown_mult).round().max(1.0) as u32;
    state.halo_tick += 1;
    if !state.halo_tick.is_multiple_of(interval) {
        return;
    }
    // 判定した事実そのものをrender.rsに伝え、パルスリングを重ねさせる。
    // 位置は現在の`state.lantern.x`ではなく、実際に判定に使うこの
    // `lantern_x` をスナップショットする (`apply_aurora_hit`と同じ理由)。
    let lantern_x = state.lantern.x;
    state.halo_flash.trigger(HALO_FLASH_TICKS);
    state.halo_flash_x = lantern_x;
    let damage = ((halo.damage() as f64) * damage_mult).round() as i32;
    let radius = halo.halo_radius();
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

fn is_in_bounds(p: &Projectile) -> bool {
    p.y > -10.0 && p.y < WORLD_H + 10.0 && p.x > -10.0 && p.x < WORLD_W + 10.0
}

/// 線分 `start→end` が、中心 `center` 半径 `radius` の円と交わる最も早い
/// 時刻 `t∈[0,1]` (=境界に最初に触れる時刻) を返す (交わらなければ `None`)。
/// 弾は9ワールド単位/tick、最小の合算当たり半径は3.2しかないため、
/// 移動後の終点だけで判定すると閉じるスピードの速い相手をすり抜ける
/// (両者が接近しながらすれ違うと、始点でも終点でも半径内に入らないまま
/// 通り過ぎてしまう)。移動した経路全体を線分として判定することで防ぐ。
///
/// `t` を返すのは、貫通弾が同tick中に複数の敵と交差した際、スポーン順
/// ではなく実際に弾が通過する順で命中を解決するため — 中心への最接近
/// 時刻ではなく、線分と円の方程式を解いた実際の境界進入時刻でなければ、
/// 半径の異なる敵同士 (魔王等の大型と雑魚) で順序を誤る。
fn segment_hits_circle_at(start: (f64, f64), end: (f64, f64), center: (f64, f64), radius: f64) -> Option<f64> {
    let (dx, dy) = (end.0 - start.0, end.1 - start.1);
    let (fx, fy) = (start.0 - center.0, start.1 - center.1);
    let a = dx * dx + dy * dy;
    if a < 1e-9 {
        // 始点=終点 (実質移動なし) — 始点が既に円内にあるかだけを見る。
        return (fx * fx + fy * fy <= radius * radius).then_some(0.0);
    }
    let b = 2.0 * (fx * dx + fy * dy);
    let c = fx * fx + fy * fy - radius * radius;
    let disc = b * b - 4.0 * a * c;
    if disc < 0.0 {
        return None;
    }
    let sqrt_disc = disc.sqrt();
    let t_enter = (-b - sqrt_disc) / (2.0 * a);
    let t_exit = (-b + sqrt_disc) / (2.0 * a);
    if t_exit < 0.0 || t_enter > 1.0 {
        return None;
    }
    // t_enter が0未満なら始点が既に円内にある — その場合はこのtick中の
    // 最速命中時刻として t=0 を返す。
    Some(t_enter.clamp(0.0, 1.0))
}

/// 弾を移動させ、その移動経路上での命中判定まで一度に行う。
/// (移動前位置が無いと `segment_hits_circle_at` によるすり抜け対策ができない
/// ため、移動と命中判定は分離せずここで一体にしている)
///
/// `enemy_prev_positions` はこのtickで敵が動く前の位置 (id→(x,y))。敵も
/// 高waveでは1tickに数ワールド単位動くため、弾の経路だけを敵の移動後
/// (静止した) 位置に対してスイープすると、両者が同tick中にすれ違う
/// ケースを見逃す。弾と敵それぞれの始点・終点から相対運動の線分を作り、
/// それを原点中心の円と交わるか判定することで、両者の移動をまとめて
/// 考慮する。
fn move_and_resolve_projectiles(state: &mut EverlightState, enemy_prev_positions: &std::collections::HashMap<u32, (f64, f64)>) {
    let projectiles = std::mem::take(&mut state.projectiles);
    let mut surviving = Vec::with_capacity(projectiles.len());

    for mut proj in projectiles {
        let start = (proj.x, proj.y);
        proj.x += proj.vx;
        proj.y += proj.vy;
        let end = (proj.x, proj.y);

        // スポーン地点でのクランプにより複数の敵が密集する状況では、
        // 移動経路1本が同tick中に複数体と交差しうる。`state.enemies` の
        // 配列順 (=スポーン順) のまま処理すると、若く速い敵が古く遅い
        // 敵を追い越して先に貫通を消費してしまう。まず全ての交差を
        // 集めて実際に弾が通過する順 (`t` 昇順) に並べ替えてから命中を
        // 解決する。
        let mut hits: Vec<(usize, f64)> = Vec::new();
        for (idx, enemy) in state.enemies.iter().enumerate() {
            if enemy.hp <= 0 || proj.hit_enemy_ids.contains(&enemy.id) {
                continue;
            }
            let hit_dist = enemy.kind.radius() + proj.radius;
            let enemy_prev = enemy_prev_positions.get(&enemy.id).copied().unwrap_or((enemy.x, enemy.y));
            let rel_start = (start.0 - enemy_prev.0, start.1 - enemy_prev.1);
            let rel_end = (end.0 - enemy.x, end.1 - enemy.y);
            if let Some(t) = segment_hits_circle_at(rel_start, rel_end, (0.0, 0.0), hit_dist) {
                hits.push((idx, t));
            }
        }
        hits.sort_by(|a, b| a.1.total_cmp(&b.1));

        let mut consumed = false;
        for (idx, _) in hits {
            if consumed {
                break;
            }
            let enemy = &mut state.enemies[idx];
            enemy.hp -= proj.damage;
            enemy.hurt_flash.trigger(3);
            proj.hit_enemy_ids.push(enemy.id);
            // 同じ敵への再命中だけは `hit_enemy_ids` で恒久的に除外する —
            // 合算当たり半径 (最大16.2) が1tickの移動距離 (9) を超える
            // 大型の敵 (魔王等) では、複数tickにわたって当たり判定内に
            // 留まり続けることがあるため。
            if proj.pierce_remaining == 0 {
                consumed = true;
            } else {
                proj.pierce_remaining -= 1;
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
    let mut caught_count = 0u32;
    state.chests.retain(|c| {
        if c.y >= BREACH_Y {
            return false; // 取り逃した
        }
        let dx = (c.x - lantern_x).abs();
        let dy = (c.y - LANTERN_Y).abs();
        if dx <= catch_radius && dy <= 6.0 {
            caught_count += 1;
            false
        } else {
            true
        }
    });
    if caught_count == 0 {
        return;
    }
    state.chest_caught_count += caught_count;
    // 極光/光輪で精鋭・魔王を同一tickにまとめて倒すと、宝箱も同じ座標・
    // 落下速度で同時に湧くため、同一tickでの複数キャッチが十分起こり得る。
    // モーダルは1つずつしか開けないので、2個目以降は「次のモーダルが
    // 閉じたら続けて開く」ようキューに積む (「宝箱を取ると必ず1回強化
    // 選択が開く」という約束を、同時キャッチでも守るため)。
    debug_assert!(state.pending_boons.is_none(), "tickはモーダル表示中に進まないはず");
    open_boon_modal(state);
    state.queued_boon_rolls += caught_count - 1;
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
            } else if !w.evolved
                && state.loadout.passive_level(kind.evolution_partner()) >= EVOLUTION_PASSIVE_THRESHOLD
            {
                v.push(BoonOption { kind: BoonKind::Evolve(kind) });
            }
        } else if state.loadout.weapons.len() < MAX_WEAPON_SLOTS {
            v.push(BoonOption { kind: BoonKind::NewWeapon(kind) });
        }
    }
    for &kind in PassiveKind::all() {
        if let Some(p) = state.loadout.passives.iter().find(|p| p.kind == kind) {
            if p.level < MAX_LEVEL {
                v.push(BoonOption { kind: BoonKind::LevelPassive(kind) });
            }
        } else if state.loadout.passives.len() < state.camp.max_passive_slots() {
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
    // 常に効果のある回復/残光で埋め、モーダルが必ず3枠揃うようにする。
    // (かつて「威力」レベルアップで埋めていたが、既にLvMAXなら選んでも
    // 何も起きない「空洞の選択肢」になってしまっていた)
    // 灯が満タンの時は InstantHeal 自体が空洞化する (回復量が0になる) ので
    // 候補から外し、常に効果のある EmberWindfall だけで埋める。
    let fallback: &[BoonKind] = if state.lantern.light < state.lantern.light_max {
        &[BoonKind::InstantHeal, BoonKind::EmberWindfall]
    } else {
        &[BoonKind::EmberWindfall]
    };
    let mut fallback_idx = 0usize;
    while chosen.len() < 3 {
        chosen.push(BoonOption { kind: fallback[fallback_idx % fallback.len()] });
        fallback_idx += 1;
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
    // 同一tickに複数の宝箱を取っていた分、続けてもう1回モーダルを開く。
    if state.queued_boon_rolls > 0 {
        state.queued_boon_rolls -= 1;
        open_boon_modal(state);
    }
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
        BoonKind::Evolve(k) => {
            if let Some(w) = state.loadout.weapon_mut(k) {
                w.evolved = true;
            }
            state.add_log(format!("『{}』が『{}』へ進化した！", k.name(), k.evolved_name()));
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
        BoonKind::InstantHeal => {
            state.lantern.light = (state.lantern.light + INSTANT_HEAL_AMOUNT).min(state.lantern.light_max);
            state.add_log(format!("灯が{INSTANT_HEAL_AMOUNT}回復した"));
        }
        BoonKind::EmberWindfall => {
            state.ember += EMBER_WINDFALL_AMOUNT;
            state.add_log(format!("残光を{EMBER_WINDFALL_AMOUNT}得た"));
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
        BoonKind::Evolve(k) => {
            (format!("進化: {}", k.evolved_name()), format!("『{}』の真価が解き放たれる", k.name()))
        }
        BoonKind::NewPassive(k) => (format!("新効果: {}", k.name()), k.summary().to_string()),
        BoonKind::LevelPassive(k) => {
            let cur = state.loadout.passive_level(k);
            (format!("{} Lv{}→{}", k.name(), cur, cur + 1), k.summary().to_string())
        }
        BoonKind::InstantHeal => ("灯を回復".to_string(), format!("灯を{INSTANT_HEAL_AMOUNT}回復する")),
        BoonKind::EmberWindfall => ("残光の欠片".to_string(), format!("残光+{EMBER_WINDFALL_AMOUNT} (即時)")),
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
    state.next_enemy_id = 0;
    state.boss_spawned_this_wave = false;
    state.halo_tick = 0;
    state.pending_boons = None;
    state.queued_boon_rolls = 0;
    state.boss_telegraph = None;
    state.kill_count = 0;
    // breach_count はリセットしない: detect_transitions が前回renderとの
    // 差分で演出をトリガーする単調増加カウンタ (state.rsのコメント参照)。
    // ここで0に戻すと、前の夜番で漏れが発生していた場合に「減った」と
    // 誤検知され、拠点→次の夜番の遷移で無関係な漏れ演出が誤発火する。
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
    // 灯が0になったのと同tickで宝箱を複数取っていた場合、開くはずだった
    // モーダルのキューも一緒に破棄する (次の `start_vigil` でも改めて0に
    // なるので実害は無いが、フィールドの意味が「今の夜番の残りモーダル数」
    // である以上、夜番が終わった時点でここでも明示的に閉じておく)。
    state.queued_boon_rolls = 0;
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
    fn breach_count_survives_start_vigil_so_effect_diffing_stays_correct() {
        // breach_count は detect_transitions (mod.rs) が前回renderとの差分で
        // 演出をトリガーする単調増加カウンタ。ここでリセットされると、
        // 前の夜番で漏れが発生していた場合に「値が減った」と誤検知され、
        // 拠点→次の夜番の遷移で無関係な漏れ演出が誤発火してしまう。
        let mut state = EverlightState::new();
        start_vigil(&mut state);
        state.enemies.push(Enemy {
            id: 1,
            kind: EnemyKind::Wisp,
            x: state.lantern.x,
            y: BREACH_Y,
            hp: 999,
            max_hp: 999,
            hurt_flash: FlashTimer::new(),
        });
        resolve_breaches(&mut state);
        assert_eq!(state.breach_count, 1);

        retreat_to_camp(&mut state);
        start_vigil(&mut state);
        assert_eq!(
            state.breach_count, 1,
            "breach_countはstart_vigilでリセットされてはいけない (演出の誤発火防止)"
        );
    }

    #[test]
    fn long_vigil_eventually_spawns_enemies_and_fires_projectiles() {
        let mut state = EverlightState::new();
        start_vigil(&mut state);
        tick_n(&mut state, 60);
        assert!(!state.enemies.is_empty(), "60tick(6秒)経てば敵が湧いているはず");
    }

    #[test]
    fn boss_spawn_flag_is_not_set_when_enemy_cap_prevents_actual_spawn() {
        let mut state = EverlightState::new();
        start_vigil(&mut state);
        state.wave = BOSS_EVERY_N_WAVES;
        for _ in 0..MAX_ENEMIES_ON_FIELD {
            state.enemies.push(Enemy {
                id: 2,
                kind: EnemyKind::Wisp,
                x: 0.0,
                y: 0.0,
                hp: 1,
                max_hp: 1,
                hurt_flash: FlashTimer::new(),
            });
        }

        spawn_enemies(&mut state);
        assert!(
            !state.boss_spawned_this_wave,
            "上限で実際には湧けなかったのに『出現した』フラグが立ってはいけない"
        );

        state.enemies.clear();
        spawn_enemies(&mut state);
        assert!(state.boss_spawned_this_wave, "上限が解消されれば改めて出現するはず");
    }

    #[test]
    fn weapon_fires_every_cooldown_ticks_exactly() {
        let mut state = EverlightState::new();
        start_vigil(&mut state);
        let cooldown = state.loadout.weapons[0].cooldown_ticks();
        let mut fire_ticks = Vec::new();
        let mut prev_count = 0usize;
        for t in 1..=(cooldown * 2) {
            tick(&mut state);
            if state.projectiles.len() > prev_count {
                fire_ticks.push(t);
            }
            prev_count = state.projectiles.len();
            if fire_ticks.len() >= 2 {
                break;
            }
        }
        assert_eq!(fire_ticks.len(), 2, "2回分の発射が観測できるはず");
        assert_eq!(
            fire_ticks[1] - fire_ticks[0],
            cooldown,
            "発射間隔は cooldown_ticks() と正確に一致するはず (オフバイワン回帰防止)"
        );
    }

    #[test]
    fn halo_damage_interval_shrinks_with_fire_rate_passive() {
        // 光輪だけ `cooldown_mult()` を無視すると、速射パッシブの
        // 「全武器のクールダウン短縮」という説明文が嘘になる。パッシブ
        // レベルを上げると実際にダメージ判定の間隔も縮むことを確認する。
        let mut state = EverlightState::new();
        start_vigil(&mut state);
        state.loadout.weapons.clear();
        state.loadout.weapons.push(OwnedWeapon { kind: WeaponKind::Halo, level: 1, cooldown_remaining: 0, evolved: false });
        state.loadout.passives.push(OwnedPassive { kind: PassiveKind::FireRate, level: 5 });
        let x = state.lantern.x;
        state.enemies.push(Enemy {
            id: 1,
            kind: EnemyKind::Wisp,
            x,
            y: LANTERN_Y,
            hp: 999_999,
            max_hp: 999_999,
            hurt_flash: FlashTimer::new(),
        });

        let expected_interval =
            ((state.loadout.weapons[0].cooldown_ticks() as f64) * state.loadout.cooldown_mult()).round().max(1.0) as u32;
        assert!(expected_interval < 5, "速射Lv5なら光輪の基礎間隔5より短くなるはず");

        let mut hit_ticks = Vec::new();
        for t in 1..=(expected_interval * 3) {
            let hp_before = state.enemies[0].hp;
            fire_halo(&mut state, 1.0);
            if state.enemies[0].hp < hp_before {
                hit_ticks.push(t);
            }
            if hit_ticks.len() >= 2 {
                break;
            }
        }
        assert_eq!(hit_ticks.len(), 2, "2回分のダメージ判定が観測できるはず");
        assert_eq!(
            hit_ticks[1] - hit_ticks[0],
            expected_interval,
            "光輪のダメージ間隔は速射パッシブのcooldown_multを反映するはず"
        );
    }

    #[test]
    fn aurora_fire_triggers_flash_even_without_enemies_in_lane() {
        // 極光は即着弾のヒットスキャンで実体弾を残さない。命中の有無に
        // 関わらず発火自体をrender.rsへ伝えるフラグが無いと、敵がいない
        // レーンを薙いだ時に武器の存在を確認する手段が一切無くなる
        // (「武器を取っても強化しても何も起きていない」バグ報告の原因)。
        let mut state = EverlightState::new();
        start_vigil(&mut state);
        state.loadout.weapons.clear();
        state.loadout.weapons.push(OwnedWeapon { kind: WeaponKind::Aurora, level: 1, cooldown_remaining: 0, evolved: false });
        assert!(state.enemies.is_empty());
        assert!(!state.aurora_flash.is_active());

        fire_weapons(&mut state, 1.0);

        assert!(state.aurora_flash.is_active(), "敵がいなくても発火した瞬間にフラッシュが立つはず");
    }

    #[test]
    fn halo_fire_triggers_flash_exactly_on_damage_ticks() {
        let mut state = EverlightState::new();
        start_vigil(&mut state);
        state.loadout.weapons.clear();
        state.loadout.weapons.push(OwnedWeapon { kind: WeaponKind::Halo, level: 1, cooldown_remaining: 0, evolved: false });
        let interval = state.loadout.weapons[0].cooldown_ticks();

        for t in 1..interval {
            fire_halo(&mut state, 1.0);
            assert!(!state.halo_flash.is_active(), "間隔に達する前はパルスが立たないはず (tick {t})");
        }
        fire_halo(&mut state, 1.0);
        assert!(state.halo_flash.is_active(), "間隔に達した瞬間にパルスが立つはず");
    }

    #[test]
    fn aurora_flash_survives_a_worst_case_five_tick_catch_up_batch() {
        // GameTime::update はフレーム落ち・タブ復帰時に最大5tick (500ms分)
        // をまとめて処理してから1回だけrenderする (src/time.rs)。バッチの
        // 先頭tickで発火した場合、残り4tick分の減衰を経てもバッチ終了
        // 時点でまだアクティブでなければ、render に一度も観測されないまま
        // 消えてしまう (AURORA_FLASH_TICKS のコメント参照)。
        let mut state = EverlightState::new();
        start_vigil(&mut state);
        state.loadout.weapons.clear();
        state.loadout.weapons.push(OwnedWeapon { kind: WeaponKind::Aurora, level: 1, cooldown_remaining: 0, evolved: false });

        tick_n(&mut state, 5);

        assert!(
            state.aurora_flash.is_active(),
            "5tickまとめ処理の先頭で発火しても、バッチ終了時点でまだフラッシュが有効なはず"
        );
    }

    #[test]
    fn halo_flash_survives_a_worst_case_five_tick_catch_up_batch() {
        let mut state = EverlightState::new();
        start_vigil(&mut state);
        state.loadout.weapons.clear();
        state.loadout.weapons.push(OwnedWeapon { kind: WeaponKind::Halo, level: 1, cooldown_remaining: 0, evolved: false });
        let interval = state.loadout.weapons[0].cooldown_ticks();

        // 発火直前まで進めてから、発火がバッチの先頭tickに来る5tick
        // バッチを与える。
        tick_n(&mut state, interval - 1);
        assert!(!state.halo_flash.is_active());
        tick_n(&mut state, 5);

        assert!(
            state.halo_flash.is_active(),
            "5tickまとめ処理の先頭で発火しても、バッチ終了時点でまだフラッシュが有効なはず"
        );
    }

    #[test]
    fn aurora_flash_x_snapshots_firing_position_not_current_lantern_position() {
        // render.rsは薙ぎ払い帯を`aurora_flash_x`から描く。もし代わりに
        // `state.lantern.x`(現在位置)を使うと、まとめtick処理中に灯が
        // 動いた後にまとめて1回だけrenderされた場合、実際に判定した
        // レーンとは違う位置に帯が表示されてしまう (発火直後に灯だけを
        // 動かして再現する)。
        let mut state = EverlightState::new();
        start_vigil(&mut state);
        state.loadout.weapons.clear();
        state.loadout.weapons.push(OwnedWeapon { kind: WeaponKind::Aurora, level: 1, cooldown_remaining: 0, evolved: false });
        let fired_at_x = state.lantern.x;

        fire_weapons(&mut state, 1.0);
        assert_eq!(state.aurora_flash_x, fired_at_x);

        state.lantern.x = fired_at_x + 30.0;
        assert_eq!(
            state.aurora_flash_x, fired_at_x,
            "aurora_flash_x は発火時点の位置を保持し続けるはず (現在位置に追従しない)"
        );
    }

    #[test]
    fn halo_flash_x_snapshots_firing_position_not_current_lantern_position() {
        // render.rsはパルスリングを`halo_flash_x`から描く。もし代わりに
        // `state.lantern.x`(現在位置)を使うと、まとめtick処理中に灯が
        // 動いた後にまとめて1回だけrenderされた場合、実際に判定した
        // 位置とは違う位置にパルスが表示されてしまう。
        let mut state = EverlightState::new();
        start_vigil(&mut state);
        state.loadout.weapons.clear();
        state.loadout.weapons.push(OwnedWeapon { kind: WeaponKind::Halo, level: 1, cooldown_remaining: 0, evolved: false });
        let interval = state.loadout.weapons[0].cooldown_ticks();
        tick_n(&mut state, interval);
        let fired_at_x = state.halo_flash_x;
        assert!(state.halo_flash.is_active());

        state.lantern.x = fired_at_x + 30.0;
        assert_eq!(
            state.halo_flash_x, fired_at_x,
            "halo_flash_x は発火時点の位置を保持し続けるはず (現在位置に追従しない)"
        );
    }

    #[test]
    fn breach_damages_light_and_removes_enemy() {
        let mut state = EverlightState::new();
        start_vigil(&mut state);
        state.enemies.push(Enemy {
            id: 3,
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
            id: 4,
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
        move_and_resolve_projectiles(&mut state, &std::collections::HashMap::new());
        assert!(state.enemies.is_empty());
        assert!(state.ember > ember_before);
        assert_eq!(state.kill_count, 1);
    }

    #[test]
    fn segment_hits_circle_at_returns_true_entry_time_not_closest_approach() {
        // 半径の異なる2つの円が同じ直線上にある場合、「境界に最初に触れる
        // 時刻」は中心までの距離の近さだけでは決まらない。半径の大きい円
        // ほど境界は中心より手前にあるため、中心距離が遠くても先に境界へ
        // 入りうる。9ユニットのセグメント上で、中心距離5・半径3.2の円は
        // 1.8で境界に入るが、中心距離9・半径8.1の円はより早い0.9で入る。
        let start = (0.0, 0.0);
        let end = (9.0, 0.0);

        let small = segment_hits_circle_at(start, end, (5.0, 0.0), 3.2).expect("小さい円に命中するはず");
        let large = segment_hits_circle_at(start, end, (9.0, 0.0), 8.1).expect("大きい円に命中するはず");

        assert!((small - 1.8 / 9.0).abs() < 1e-9, "小さい円への進入時刻は (5-3.2)/9 のはず: got {small}");
        assert!((large - 0.9 / 9.0).abs() < 1e-9, "大きい円への進入時刻は (9-8.1)/9 のはず: got {large}");
        assert!(large < small, "中心までの距離は遠くても、半径が大きい円の方が先に境界へ入るはず");
    }

    #[test]
    fn fast_projectile_does_not_tunnel_through_an_enemy_mid_tick() {
        // 弾は9ワールド単位/tick動くが、最小の合算当たり半径は3.2しかない。
        // 敵をちょうど移動経路の中間に置くと、始点・終点どちらの距離判定
        // でも半径内に入らない (=旧・終点のみの判定だと見逃す) が、
        // 経路全体を見る判定なら確実に命中するはず。
        let mut state = EverlightState::new();
        start_vigil(&mut state);
        let x = state.lantern.x;
        state.enemies.push(Enemy {
            id: 5,
            kind: EnemyKind::Wisp,
            x,
            y: 5.5, // 弾の始点(10.0)と終点(1.0)のちょうど中間
            hp: 999,
            max_hp: 999,
            hurt_flash: FlashTimer::new(),
        });
        state.projectiles.push(make_projectile(x, 10, 1, 0.0, -9.0, Color::White));
        state.projectiles[0].y = 10.0;

        move_and_resolve_projectiles(&mut state, &std::collections::HashMap::new());

        assert_eq!(
            state.enemies[0].hp,
            999 - 10,
            "始点・終点だけでなく移動経路全体で命中判定されるはず (すり抜け防止)"
        );
    }

    #[test]
    fn piercing_shot_does_not_repeatedly_hit_the_same_large_enemy_across_ticks() {
        // 魔王 (半径6.5) + 弾 (半径1.6) の合算当たり半径8.1は、弾の
        // 1tick移動距離9とほぼ同じ — 弾は複数tickにわたって魔王の当たり
        // 判定内に留まり得る。スイープ判定は各tick独立なので、履歴が無いと
        // 同じ魔王に毎tick命中し続けて貫通を無駄遣いしてしまう。
        let mut state = EverlightState::new();
        start_vigil(&mut state);
        let x = state.lantern.x;
        state.enemies.push(Enemy {
            id: 1,
            kind: EnemyKind::Boss,
            x,
            y: 0.0,
            hp: 999,
            max_hp: 999,
            hurt_flash: FlashTimer::new(),
        });
        let mut proj = make_projectile(x, 10, 2, 0.0, -9.0, Color::White);
        proj.y = 10.0;
        state.projectiles.push(proj);

        move_and_resolve_projectiles(&mut state, &std::collections::HashMap::new());
        let hp_after_first_hit = state.enemies[0].hp;
        assert_eq!(hp_after_first_hit, 999 - 10, "1回目は命中するはず");

        move_and_resolve_projectiles(&mut state, &std::collections::HashMap::new());
        assert_eq!(
            state.enemies[0].hp, hp_after_first_hit,
            "同じ弾が同じ敵に連続してもう一度命中してはいけない"
        );
    }

    #[test]
    fn piercing_shot_hits_multiple_stacked_enemies_within_the_same_tick() {
        // 密集スポーンで同レーンに複数の敵が並ぶと、1tickの移動経路が
        // 複数体を同時に横切りうる。次tickでは弾は既にその塊を通り過ぎて
        // いるため、1tick目で最初の1体しか処理しないと残りの貫通は
        // 二度と敵に届かず無駄になってしまう。
        let mut state = EverlightState::new();
        start_vigil(&mut state);
        let x = state.lantern.x;
        state.enemies.push(Enemy {
            id: 1,
            kind: EnemyKind::Wisp,
            x,
            y: 7.0,
            hp: 999,
            max_hp: 999,
            hurt_flash: FlashTimer::new(),
        });
        state.enemies.push(Enemy {
            id: 2,
            kind: EnemyKind::Wisp,
            x,
            y: 3.0,
            hp: 999,
            max_hp: 999,
            hurt_flash: FlashTimer::new(),
        });
        let mut proj = make_projectile(x, 10, 2, 0.0, -9.0, Color::White);
        proj.y = 10.0;
        state.projectiles.push(proj);

        move_and_resolve_projectiles(&mut state, &std::collections::HashMap::new());

        assert_eq!(state.enemies[0].hp, 999 - 10, "1体目は同tick内で命中するはず");
        assert_eq!(
            state.enemies[1].hp,
            999 - 10,
            "残りの貫通が生きているなら2体目にも同tick内で命中するはず"
        );
    }

    #[test]
    fn piercing_shot_resolves_hits_in_travel_order_not_array_order() {
        // 配列の並び順 (=スポーン順) と実際の弾道上の通過順が食い違う
        // 状況で、貫通の予算が配列の先頭にいる「遠い」敵に浪費されず、
        // 実際に先に通過する「近い」敵へ命中することを確認する。
        let mut state = EverlightState::new();
        start_vigil(&mut state);
        let x = state.lantern.x;
        // 配列の先頭(index 0)に、弾の進路(y: 10→1)上では「遠い」y=3.0の
        // 敵を置き、後ろ(index 1)に「近い」y=7.0の敵を置く — 配列順と
        // 通過順を意図的に逆転させる。
        state.enemies.push(Enemy {
            id: 1,
            kind: EnemyKind::Wisp,
            x,
            y: 3.0,
            hp: 999,
            max_hp: 999,
            hurt_flash: FlashTimer::new(),
        });
        state.enemies.push(Enemy {
            id: 2,
            kind: EnemyKind::Wisp,
            x,
            y: 7.0,
            hp: 999,
            max_hp: 999,
            hurt_flash: FlashTimer::new(),
        });
        let mut proj = make_projectile(x, 10, 1, 0.0, -9.0, Color::White); // pierce=1 → 命中は1発分のみ
        proj.y = 10.0;
        state.projectiles.push(proj);

        move_and_resolve_projectiles(&mut state, &std::collections::HashMap::new());

        assert_eq!(
            state.enemies[1].hp,
            999 - 10,
            "弾の進路上で先に通過する近い敵(y=7)に命中するはず"
        );
        assert_eq!(
            state.enemies[0].hp, 999,
            "配列の先頭でも進路上遠い敵(y=3)には届かないはず (貫通1発分は近い敵で使い切る)"
        );
    }

    #[test]
    fn piercing_shot_prioritizes_by_boundary_entry_not_center_distance() {
        // 中心までの距離が近い雑魚より、中心までの距離は遠くても半径が
        // 巨大な魔王の方が境界へは先に触れることがある。中心距離だけで
        // 順序を決めると、この魔王より先に (本来は届かないはずの) 雑魚へ
        // 貫通1発分を浪費してしまう。
        let mut state = EverlightState::new();
        start_vigil(&mut state);
        let x = state.lantern.x;
        // 中心距離4・半径3.8(合算)の雑魚。境界進入は (4-3.8)=0.2。
        state.enemies.push(Enemy {
            id: 1,
            kind: EnemyKind::Wisp,
            x,
            y: 6.0,
            hp: 999,
            max_hp: 999,
            hurt_flash: FlashTimer::new(),
        });
        // 中心距離8.2・半径8.1(合算)の魔王。中心距離は雑魚より遠いが、
        // 境界進入は (8.2-8.1)=0.1 と雑魚より先。
        state.enemies.push(Enemy {
            id: 2,
            kind: EnemyKind::Boss,
            x,
            y: 1.8,
            hp: 999,
            max_hp: 999,
            hurt_flash: FlashTimer::new(),
        });
        let mut proj = make_projectile(x, 10, 1, 0.0, -9.0, Color::White); // pierce=1 → 命中は1発分のみ
        proj.y = 10.0;
        state.projectiles.push(proj);

        move_and_resolve_projectiles(&mut state, &std::collections::HashMap::new());

        assert_eq!(
            state.enemies[1].hp,
            999 - 10,
            "中心距離は遠くても、境界へ先に触れる魔王に命中するはず"
        );
        assert_eq!(
            state.enemies[0].hp, 999,
            "中心距離が近いだけの雑魚には、貫通1発分が魔王で尽きて届かないはず"
        );
    }

    #[test]
    fn fast_enemy_movement_is_swept_against_projectile_movement() {
        // 高waveのSwarmlingのように敵自身が1tickで大きく動くと、弾の経路を
        // 敵の「移動後」の1点だけに対してスイープしても、両者が同tick中に
        // すれ違うケースを見逃す。敵がy=9.9→13.58 (差3.68) へ動く間に弾が
        // y=10→1へ動く場合、移動後の敵位置(13.58)は弾の経路(y∈[1,10])から
        // 3.58離れていて合算当たり半径3.2の外に出てしまうが、実際には
        // 両者の経路はすれ違う瞬間に交差している。
        let mut state = EverlightState::new();
        start_vigil(&mut state);
        let x = state.lantern.x;
        state.enemies.push(Enemy {
            id: 1,
            kind: EnemyKind::Swarmling,
            x,
            y: 13.58,
            hp: 999,
            max_hp: 999,
            hurt_flash: FlashTimer::new(),
        });
        let mut proj = make_projectile(x, 10, 1, 0.0, -9.0, Color::White);
        proj.y = 10.0;
        state.projectiles.push(proj);

        let mut enemy_prev_positions = std::collections::HashMap::new();
        enemy_prev_positions.insert(1u32, (x, 9.9));
        move_and_resolve_projectiles(&mut state, &enemy_prev_positions);

        assert_eq!(
            state.enemies[0].hp,
            999 - 10,
            "敵自身の移動も合わせてスイープすれば、すれ違いざまの命中を検知できるはず"
        );
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
    fn catching_two_chests_in_the_same_tick_queues_a_second_modal() {
        // 極光/光輪で精鋭を同時に複数倒すと、宝箱も同一tickで同時に
        // キャッチされ得る。どちらの宝箱も強化選択の機会を失ってはいけない。
        let mut state = EverlightState::new();
        start_vigil(&mut state);
        state.chests.push(Chest { x: state.lantern.x, y: LANTERN_Y });
        state.chests.push(Chest { x: state.lantern.x, y: LANTERN_Y });

        resolve_chest_catch(&mut state);
        assert!(state.pending_boons.is_some(), "1個目でモーダルが開くはず");
        assert_eq!(state.queued_boon_rolls, 1, "2個目の権利はキューに積まれるはず");

        assert!(choose_boon(&mut state, 0));
        assert!(
            state.pending_boons.is_some(),
            "1個目を選び終えたら、キューに積んであった2個目のモーダルが続けて開くはず"
        );
        assert_eq!(state.queued_boon_rolls, 0);

        assert!(choose_boon(&mut state, 0));
        assert!(state.pending_boons.is_none(), "2個消化したらモーダルは閉じたまま");
    }

    #[test]
    fn catching_three_chests_in_the_same_tick_queues_two_more_modals() {
        let mut state = EverlightState::new();
        start_vigil(&mut state);
        for _ in 0..3 {
            state.chests.push(Chest { x: state.lantern.x, y: LANTERN_Y });
        }

        resolve_chest_catch(&mut state);
        assert_eq!(state.queued_boon_rolls, 2, "3個同時キャッチなら残り2個分がキューに積まれるはず");

        for _ in 0..3 {
            assert!(state.pending_boons.is_some());
            assert!(choose_boon(&mut state, 0));
        }
        assert_eq!(state.queued_boon_rolls, 0);
        assert!(state.pending_boons.is_none(), "3個すべて消化したらモーダルは閉じたまま");
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
            id: 6,
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

    #[test]
    fn dying_the_same_tick_a_chest_is_caught_discards_the_modal_cleanly() {
        // 灯が0になるのと同一tickで宝箱を取っていた場合、開いたモーダルは
        // 表示される前に夜番終了で破棄される (死んだ後にレベルアップは
        // 選べない、という意図した挙動)。ここではその破棄がpanicや
        // 状態の食い違いを残さず綺麗に行われることを保証する。
        let mut state = EverlightState::new();
        start_vigil(&mut state);
        state.lantern.light = 1;
        state.enemies.push(Enemy {
            id: 7,
            kind: EnemyKind::Boss,
            x: state.lantern.x,
            y: BREACH_Y,
            hp: 999,
            max_hp: 999,
            hurt_flash: FlashTimer::new(),
        });
        for _ in 0..2 {
            state.chests.push(Chest { x: state.lantern.x, y: LANTERN_Y });
        }

        tick(&mut state);

        assert_eq!(state.phase, Phase::Camp);
        assert!(state.pending_boons.is_none(), "夜番終了時にモーダルは残らないはず");
        assert_eq!(state.queued_boon_rolls, 0, "キューも一緒に破棄されるはず");
    }

    #[test]
    fn breach_in_lantern_lane_deals_half_damage() {
        let mut same_lane = EverlightState::new();
        start_vigil(&mut same_lane);
        same_lane.enemies.push(Enemy {
            id: 8,
            kind: EnemyKind::Husk,
            x: same_lane.lantern.x,
            y: BREACH_Y,
            hp: 999,
            max_hp: 999,
            hurt_flash: FlashTimer::new(),
        });
        let light_before = same_lane.lantern.light;
        resolve_breaches(&mut same_lane);
        let same_lane_damage = light_before - same_lane.lantern.light;

        let mut other_lane = EverlightState::new();
        start_vigil(&mut other_lane);
        other_lane.enemies.push(Enemy {
            id: 9,
            kind: EnemyKind::Husk,
            x: (other_lane.lantern.x + WORLD_W / 2.0) % WORLD_W,
            y: BREACH_Y,
            hp: 999,
            max_hp: 999,
            hurt_flash: FlashTimer::new(),
        });
        let light_before = other_lane.lantern.light;
        resolve_breaches(&mut other_lane);
        let other_lane_damage = light_before - other_lane.lantern.light;

        assert_eq!(other_lane_damage, EnemyKind::Husk.contact_damage());
        assert_eq!(
            same_lane_damage,
            (EnemyKind::Husk.contact_damage() / 2).max(1),
            "灯と同じレーンで漏れた敵はダメージが半減するはず"
        );
    }

    #[test]
    fn weapon_evolves_when_maxed_with_partner_passive_and_boon_chosen() {
        let mut state = EverlightState::new();
        start_vigil(&mut state);
        state.loadout.weapons[0].level = MAX_LEVEL; // Bolt
        state.loadout.passives.push(OwnedPassive { kind: PassiveKind::FireRate, level: EVOLUTION_PASSIVE_THRESHOLD });

        let candidates = candidate_boons(&state);
        assert!(
            candidates.iter().any(|o| o.kind == BoonKind::Evolve(WeaponKind::Bolt)),
            "LvMAXの光弾+速射Lv3が揃えば進化が選択肢に出るはず"
        );

        apply_boon(&mut state, BoonKind::Evolve(WeaponKind::Bolt));
        assert!(state.loadout.weapons[0].evolved);
        let evolved_damage = state.loadout.weapons[0].damage();
        state.loadout.weapons[0].evolved = false;
        let base_damage = state.loadout.weapons[0].damage();
        assert!(evolved_damage > base_damage, "進化後は威力が上がるはず");
    }

    #[test]
    fn evolve_is_not_offered_without_partner_passive() {
        let mut state = EverlightState::new();
        start_vigil(&mut state);
        state.loadout.weapons[0].level = MAX_LEVEL; // Bolt, 速射(FireRate)なし
        let candidates = candidate_boons(&state);
        assert!(!candidates.iter().any(|o| matches!(o.kind, BoonKind::Evolve(_))));
    }

    #[test]
    fn extra_slot_upgrade_unlocks_a_5th_passive_not_a_5th_weapon() {
        // 武器種は WeaponKind::all() がちょうど4種なので、基本スロット数
        // (MAX_WEAPON_SLOTS=4) だけで全種持てる — 拡張枠は無意味になる。
        // 受動効果は5種あるため、拡張枠が意味を持つのはこちら側。
        let mut state = EverlightState::new();
        start_vigil(&mut state);
        for &kind in WeaponKind::all() {
            if state.loadout.weapon_mut(kind).is_none() {
                state.loadout.weapons.push(OwnedWeapon::new(kind));
            }
        }
        assert_eq!(state.loadout.weapons.len(), WeaponKind::all().len());
        assert!(
            !candidate_boons(&state).iter().any(|o| matches!(o.kind, BoonKind::NewWeapon(_))),
            "武器は基本スロットだけで全種類持てるので、これ以上NewWeaponは出ないはず"
        );

        for &kind in PassiveKind::all().iter().take(4) {
            state.loadout.passives.push(OwnedPassive::new(kind));
        }
        assert_eq!(state.loadout.passives.len(), 4);
        assert!(
            !candidate_boons(&state).iter().any(|o| matches!(o.kind, BoonKind::NewPassive(_))),
            "拡張前は基本4枠が埋まっているのでNewPassiveは出ないはず"
        );

        state.camp.extra_slot_level = 1;
        assert!(
            candidate_boons(&state).iter().any(|o| matches!(o.kind, BoonKind::NewPassive(_))),
            "拡張枠を買えば5種目の受動効果がNewPassiveとして出るはず"
        );
    }

    #[test]
    fn fallback_boons_always_have_a_real_effect_even_when_everything_is_maxed() {
        // 装備・受動効果を全て上限まで積んだ「もう成長先が無い」状態を作り、
        // 埋め草の選択肢 (InstantHeal/EmberWindfall) が実際に効果を持つ
        // ことを確認する (かつては効果ゼロの「空洞の選択肢」になり得た)。
        let mut state = EverlightState::new();
        start_vigil(&mut state);
        state.loadout.weapons.clear();
        for &kind in WeaponKind::all() {
            let mut w = OwnedWeapon::new(kind);
            w.level = MAX_LEVEL;
            w.evolved = true;
            state.loadout.weapons.push(w);
        }
        state.loadout.passives.clear();
        for &kind in PassiveKind::all() {
            state.loadout.passives.push(OwnedPassive { kind, level: MAX_LEVEL });
        }

        let options = roll_boon_options(&mut state);
        state.lantern.light -= 20;
        let light_before = state.lantern.light;
        let ember_before = state.ember;
        for opt in options {
            apply_boon(&mut state, opt.kind);
        }
        assert!(
            state.lantern.light > light_before || state.ember > ember_before,
            "全て上限に達していても埋め草の選択肢は何かしら実際の効果を持つはず"
        );
    }

    #[test]
    fn fallback_boons_are_all_effective_when_light_is_already_full() {
        // 装備・受動効果を全て上限まで積み、かつ灯が満タンの状態を作る。
        // 灯が満タンだとInstantHealは回復量0の空洞な選択肢になるため、
        // 埋め草からは除外され、常に効果のあるEmberWindfallだけで
        // 3枠とも埋まるはず。
        let mut state = EverlightState::new();
        start_vigil(&mut state);
        state.loadout.weapons.clear();
        for &kind in WeaponKind::all() {
            let mut w = OwnedWeapon::new(kind);
            w.level = MAX_LEVEL;
            w.evolved = true;
            state.loadout.weapons.push(w);
        }
        state.loadout.passives.clear();
        for &kind in PassiveKind::all() {
            state.loadout.passives.push(OwnedPassive { kind, level: MAX_LEVEL });
        }
        assert_eq!(state.lantern.light, state.lantern.light_max, "灯は満タンのままのはず");

        let options = roll_boon_options(&mut state);
        let ember_before = state.ember;
        for opt in options {
            apply_boon(&mut state, opt.kind);
        }
        assert_eq!(
            state.ember,
            ember_before + EMBER_WINDFALL_AMOUNT * 3,
            "灯が満タンの時は3枠とも効果のある残光獲得で埋まるはず"
        );
    }
}
