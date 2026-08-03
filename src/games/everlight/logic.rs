//! 常夜灯 — ゲームロジック (純粋関数)。
//!
//! `tick()` が1tick分の全処理 (灯の移動→湧き→敵の移動→漏れ判定→
//! 発砲→弾の移動→命中判定→宝箱→ボスの特殊攻撃) を順に進める。
//! render.rs はここで更新された `EverlightState` を読むだけ (書き込まない)。

use ratzilla::ratatui::style::Color;

use crate::effects::FlashTimer;

use super::state::{
    BoonKind, BoonOption, BossTelegraph, CampUpgrades, Chest, Enemy, EnemyBullet, EnemyKind,
    EverlightState, KillEffect, Lantern, Loadout, OwnedPassive, OwnedWeapon, PassiveKind, Phase,
    Projectile, WeaponKind, BOSS_EVERY_N_WAVES, BREACH_Y, CHEST_BASE_CATCH_RADIUS, CHEST_FALL_SPEED,
    COLUMNS, ELITE_BASE_INTERVAL_TICKS, EVOLUTION_PASSIVE_THRESHOLD, KILL_EFFECT_TICKS,
    LANE_HALF_WIDTH, LANTERN_MOVE_UNITS_PER_TICK, LANTERN_Y, MAX_LEVEL, SPAWN_Y,
    WAVE_DURATION_TICKS, WORLD_H, WORLD_W,
};

const PROJECTILE_SPEED: f64 = 9.0;
const SPRAY_SPREAD_RAD: f64 = 1.3;
const MAX_ENEMIES_ON_FIELD: usize = 200;
const MAX_PROJECTILES_ON_FIELD: usize = 300;
const MAX_ENEMY_BULLETS_ON_FIELD: usize = 300;
/// 極光の薙ぎ払い帯を表示する長さ (tick)。命中の有無に関わらず「発火した」
/// こと自体を見せるための演出用タイマーなので、肉眼で追える長さにしている。
///
/// `GameTime::update` はフレーム落ち・バックグラウンドタブ復帰時に最大
/// 500ms (10 ticks/sec換算で5tick) 分をまとめて処理してから1回だけ
/// renderする (`src/time.rs`)。このフラッシュはtickごとに1ずつ減衰する
/// ため、まとめ処理された同一バッチの最初のtickで発火すると、以降の
/// 最大4回の減衰でrenderが一度も観測しないまま0まで減ってしまう
/// (バッチの先頭で発火→残り4tick分減衰→値が0以下)。5未満にすると
/// この「発火したのに一度も表示されない」退行が起こり得るため、
/// 5を下限として維持すること。
///
/// 光輪には同種のタイマーを使っていない — 光輪のクールダウンは最短5tick
/// (進化・速射パッシブでさらに短縮可能) で、このタイマーの下限(5)を
/// 常に下回るか同じになるため、フラッシュが実質常時アクティブになって
/// しまい「一瞬光る」演出として機能しない。光輪は常時描画するリングと
/// 回転する光点だけで武器の存在・強化 (半径の拡大) を伝えており、発火
/// 頻度が高いためこれだけで十分な視覚フィードバックになっている。
const AURORA_FLASH_TICKS: u32 = 5;
/// 流星の着弾フラッシュ表示tick数。`AURORA_FLASH_TICKS` と同じ
/// まとめtick処理の見逃し対策のため、5を下限として維持すること。
const METEOR_FLASH_TICKS: u32 = 5;

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
    state.meteor_flash.tick(1);
    // 烙印は残りtickが尽きたら自動で外れる。死亡した敵のidも (討伐時に
    // 明示削除するのではなく) 自然減衰に任せることで掃除漏れを防ぐ。
    state.bolt_marks.retain(|_, ticks_left| {
        *ticks_left = ticks_left.saturating_sub(1);
        *ticks_left > 0
    });
    state.kill_effects.retain_mut(|e| {
        e.ticks_left = e.ticks_left.saturating_sub(1);
        e.ticks_left > 0
    });
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
    resolve_boss_summons(state);
    // 敵自身もこのtickで動くため、命中判定 (move_and_resolve_projectiles)
    // では移動前の位置を敵ごとの相対運動の基準として使う。召喚された個体も
    // このtick中に動いてよいので、スナップショットは召喚の後に取る。
    let enemy_prev_positions: std::collections::HashMap<u32, (f64, f64)> =
        state.enemies.iter().map(|e| (e.id, (e.x, e.y))).collect();
    move_enemies(state);
    resolve_breaches(state);
    resolve_ranged_attacks(state);
    resolve_caster_shots(state);
    resolve_wraith_shots(state);
    resolve_boss_bullets(state);
    move_and_resolve_enemy_bullets(state);

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

/// ランク1の夜が終わる波数。ランクが上がるごとに5波ずつ延長される —
/// 「夜が長くなる」ことそのものが上位ランクへ挑む手応えの一部になる。
const FIRST_MILESTONE_WAVE: u32 = 15;
const MILESTONE_WAVE_STEP: u32 = 5;

pub fn milestone_wave(rank: u32) -> u32 {
    FIRST_MILESTONE_WAVE + rank.saturating_sub(1) * MILESTONE_WAVE_STEP
}

/// そのランクの夜の最終波 (`milestone_wave`) を越えて指数関数側の
/// escalationが始まった後、1波ごとに乗算される係数。
const POST_MILESTONE_GROWTH_PER_WAVE: f64 = 1.09;

/// 波とランクから、`milestone_wave` で頭打ちになる線形部分 (+12%/波) の
/// 難易度倍率を出す。マイルストーンを越えても値はここで固定され、
/// これ以上は伸びない — 敵の移動速度 (`move_enemies`) はこちらだけを
/// 参照する。HPの指数関数的escalationまで速度に流用すると、遠くの敵が
/// 1tickで防衛線まで到達する「反応不可能な瞬間死」が発生してしまうため。
fn wave_linear_difficulty(wave: u32, rank: u32) -> f64 {
    let ramp_wave = milestone_wave(rank);
    let linear_wave = wave.min(ramp_wave);
    let base = 1.0 + linear_wave.saturating_sub(1) as f64 * 0.12;
    let rank_mult = 1.0 + rank.saturating_sub(1) as f64 * 0.35;
    base * rank_mult
}

/// 波とランクの両方から敵HPの難易度倍率を出す。`milestone_wave` までは
/// `wave_linear_difficulty` のまま、そこを越えて夜番を続けた場合だけ
/// 指数関数的に跳ね上げる — 拠点の恒久強化 (`power_level` 等) は上限なく
/// 積み上げられるため、線形のままだと「十分強化すればマイルストーンの
/// 先もいつまでも進める」状態になってしまう。
fn wave_difficulty(wave: u32, rank: u32) -> f64 {
    let ramp_wave = milestone_wave(rank);
    let overflow_waves = wave.saturating_sub(ramp_wave);
    let escalation = POST_MILESTONE_GROWTH_PER_WAVE.powi(overflow_waves as i32);
    wave_linear_difficulty(wave, rank) * escalation
}

/// ランクが上がるほど討伐報酬 (残光) も伸びる — 高ランクは危険だが
/// 実入りも良い、というリスク/リターンの牽引力。
fn ember_reward_mult(rank: u32) -> f64 {
    1.0 + rank.saturating_sub(1) as f64 * 0.25
}

/// この波に湧くボスの種類。第5波毎のチェックポイントのうち、そのランクの
/// 夜の最終波 (`milestone_wave`) だけは特別な最終ボスになる — ランクの
/// 偶奇で満月の魔王/大蛇を交互に割り当て、ランクが伸びても定義済みの
/// 2種で終わりなく回せるようにする。それ以外のチェックポイントは
/// 魔王/影の魔女を交互に回す (第10波は必ず影の魔女、等)。
pub fn boss_kind_for(wave: u32, rank: u32) -> EnemyKind {
    if wave == milestone_wave(rank) {
        if rank.saturating_sub(1).is_multiple_of(2) {
            EnemyKind::FullMoonBoss
        } else {
            EnemyKind::Serpent
        }
    } else {
        let checkpoint_index = wave / BOSS_EVERY_N_WAVES;
        if checkpoint_index.is_multiple_of(2) {
            EnemyKind::ShadowWitch
        } else {
            EnemyKind::Boss
        }
    }
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

/// 新しい敵種を「数値インフレ」ではなく「新しい状況」として導入する
/// wave帯の下限。単純に敵を差し替えるのではなく既存の枠に加える形で
/// 出現テーブル (`regular_spawn_table`) へ合流させる。
const SNIPER_MIN_WAVE: u32 = 6;
const CASTER_MIN_WAVE: u32 = 9;
const SHIELDED_MIN_WAVE: u32 = 11;
const CHARGER_MIN_WAVE: u32 = 13;
/// マイルストーン波 (第15波、`milestone_wave(1)`) の直前で解禁する — 第1夜の
/// 最終局面から「打たれ強い遠隔役」という新しい負荷を上乗せする。
const WRAITH_MIN_WAVE: u32 = 14;
const SPLITTER_MIN_WAVE: u32 = 16;
const SPRAY_SHIELDED_MIN_WAVE: u32 = 18;
const AURORA_SHIELDED_MIN_WAVE: u32 = 24;

/// 通常湧きの1回あたりの同時湧き数がこのwaveから増え始める。
/// `spawn_interval_ticks` の間隔短縮は第21波前後で下限に達し頭打ちに
/// なるため、それ以降も物量そのものが伸び続けるようこちらで補う。
///
/// `spawn_interval_ticks` 自体の間隔短縮 (base値) は意図的に触っていない
/// — シミュレーターで検証したところ、そちらは全waveへ均等に効くため
/// 数tick詰めるだけでも序盤の生存に直結し、平均到達波数を大きく崩す
/// (`survival_balance_report`)。この3定数はwaveが進んでから初めて効き
/// 始めるため、序盤の難易度を変えずに後半の物量だけ底上げできる。
const SWARM_RAMP_WAVE: u32 = 9;
const SWARM_WAVE_STEP: u32 = 4;
/// `COLUMNS` (9) 未満という制約の上限いっぱい (8) まで引き上げている。
const SWARM_MAX_BATCH: u32 = 8;
// `spawn_enemies` はbatch分のレーンを `% COLUMNS` で割り当てるため、上限が
// COLUMNS以上だと同一tick内で同じレーンに複数体が重なってしまう。
const _: () = assert!(SWARM_MAX_BATCH < COLUMNS as u32);

/// 通常湧き1回で同時に湧く敵の数。waveが進むほど「間隔を詰める」だけ
/// でなく「一度に出てくる数」自体も増やし、後半にかけて画面が
/// 賑やかになっていく体感を作る。
fn regular_spawn_batch_size(wave: u32) -> u32 {
    let extra = wave.saturating_sub(SWARM_RAMP_WAVE) / SWARM_WAVE_STEP;
    (1 + extra).min(SWARM_MAX_BATCH)
}

/// 通常湧き (精鋭・ボスを除く) の重み付き抽選テーブル。waveが進むほど
/// 候補が増える — 数字が伸びるだけでなく対応すべき状況そのものが
/// 増えていく体感を作る。
fn regular_spawn_table(wave: u32) -> Vec<(EnemyKind, u32)> {
    // 既存3種の重み (15/30/55) は元の確率をそのまま保つ — 新種は「既存の
    // 枠を削って割り込む」のではなく「新しい選択肢が追加される」形で
    // 合流させ、序盤 (wave帯未解禁) のバランスを変えない。
    let mut table = vec![(EnemyKind::Swarmling, 15u32), (EnemyKind::Husk, 30), (EnemyKind::Wisp, 55)];
    if wave >= SNIPER_MIN_WAVE {
        table.push((EnemyKind::Sniper, 15));
    }
    if wave >= CASTER_MIN_WAVE {
        table.push((EnemyKind::Caster, 8));
    }
    if wave >= SHIELDED_MIN_WAVE {
        table.push((EnemyKind::Shielded, 10));
    }
    if wave >= CHARGER_MIN_WAVE {
        table.push((EnemyKind::Charger, 8));
    }
    if wave >= WRAITH_MIN_WAVE {
        table.push((EnemyKind::Wraith, 8));
    }
    if wave >= SPLITTER_MIN_WAVE {
        table.push((EnemyKind::Splitter, 10));
    }
    if wave >= SPRAY_SHIELDED_MIN_WAVE {
        table.push((EnemyKind::SprayShielded, 8));
    }
    if wave >= AURORA_SHIELDED_MIN_WAVE {
        table.push((EnemyKind::AuroraShielded, 8));
    }
    table
}

fn pick_weighted(state: &mut EverlightState, table: &[(EnemyKind, u32)]) -> EnemyKind {
    let total: u32 = table.iter().map(|&(_, w)| w).sum();
    let mut roll = rng_below(&mut state.rng_state, total.max(1));
    for &(kind, weight) in table {
        if roll < weight {
            return kind;
        }
        roll -= weight;
    }
    table.last().map(|&(k, _)| k).unwrap_or(EnemyKind::Wisp)
}

fn spawn_enemies(state: &mut EverlightState) {
    if state.wave.is_multiple_of(BOSS_EVERY_N_WAVES) && !state.boss_spawned_this_wave {
        let lane = rng_below(&mut state.rng_state, COLUMNS as u32) as usize;
        let boss_kind = boss_kind_for(state.wave, state.rank);
        let is_milestone_boss = state.wave == milestone_wave(state.rank);
        // 湧く"前"にidを控えておく — `spawn_enemy_at_xy` は現在の
        // `next_enemy_id` をそのまま割り当てるので、成功した場合はこれが
        // 実際に湧いた個体のidと一致する。
        let spawned_id = state.next_enemy_id;
        // 敵数上限で実際には湧けなかった時に `boss_spawned_this_wave` を
        // 立ててしまうと、ログだけ「出現した」と嘘をつく上、そのウェーブ中
        // 二度とボスの湧き抽選が行われなくなる。実際に湧いた時だけ確定させ、
        // 上限で弾かれた場合は次tick以降に再抽選させる。
        if spawn_enemy_at(state, boss_kind, lane) {
            state.boss_spawned_this_wave = true;
            if is_milestone_boss {
                // `maybe_trigger_dawn` はwaveの一致ではなくこのidの討伐で
                // 判定する — 最終ボスはHPが高く、湧いた波(300 tick)以内に
                // 倒しきれず次の波へ持ち越されることがあるため。
                state.milestone_boss_id = Some(spawned_id);
            }
            state.add_log(format!("{}が現れた！", boss_kind.name()));
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
        let table = regular_spawn_table(state.wave);
        let picked = pick_weighted(state, &table);
        // 羽虫は元々3体一斉湧きの群れ敵なので、wave由来のbatchがそれを
        // 下回っても3体を下限として保証する。
        let swarmling_floor = if picked == EnemyKind::Swarmling { 3 } else { 1 };
        let batch = regular_spawn_batch_size(state.wave).max(swarmling_floor);
        let base_lane = rng_below(&mut state.rng_state, COLUMNS as u32) as usize;
        for offset in 0..batch as usize {
            let lane = (base_lane + offset) % COLUMNS;
            spawn_enemy_at(state, picked, lane);
        }
    }
}

/// 敵を1体、湧き出し端 (`SPAWN_Y`) の指定レーンに湧かせる。フィールドの
/// 敵数上限で弾かれた場合は `false` を返す。
fn spawn_enemy_at(state: &mut EverlightState, kind: EnemyKind, lane: usize) -> bool {
    spawn_enemy_at_xy(state, kind, super::state::lane_center_x(lane), SPAWN_Y)
}

/// 敵を1体、任意の座標に湧かせる。分裂体の子のように盤面の途中から
/// 発生する敵は `SPAWN_Y` 固定の `spawn_enemy_at` では表現できないため、
/// こちらを直接使う。
fn spawn_enemy_at_xy(state: &mut EverlightState, kind: EnemyKind, x: f64, y: f64) -> bool {
    if state.enemies.len() >= MAX_ENEMIES_ON_FIELD {
        return false;
    }
    if kind.is_boss() {
        state.boss_spawn_count += 1;
    }
    let diff = wave_difficulty(state.wave, state.rank);
    let hp = (kind.base_hp() as f64 * diff).round() as i32;
    let id = state.next_enemy_id;
    state.next_enemy_id += 1;
    state.enemies.push(Enemy { id, kind, x, y, hp, max_hp: hp, hurt_flash: FlashTimer::new(), ranged_charge: None });
    true
}

/// 狙撃者 (`EnemyKind::Sniper`) が接近をやめて遠隔攻撃に専念し始める
/// y座標。ここまでは他の敵と同じく普通に近づいてくる。
const SNIPER_STOP_Y: f64 = WORLD_H * 0.55;

/// 詠唱者 (`EnemyKind::Caster`) が接近をやめる y座標。狙撃者よりずっと
/// 手前 (湧き出し端に近い位置) で止まる — 「奥まで進んでから構える」
/// 狙撃者とは違う、常に後方から撃ってくる後方支援役の立ち位置にする。
const CASTER_STOP_Y: f64 = WORLD_H * 0.18;
const _: () = assert!(CASTER_STOP_Y < SNIPER_STOP_Y);

/// 突進者 (`EnemyKind::Charger`) がここを越えると `CHARGER_BOOST_MULT` で
/// 急加速する。それまでは他の敵と同じ速度で直進する。
const CHARGER_TRIGGER_Y: f64 = WORLD_H * 0.75;
const CHARGER_BOOST_MULT: f64 = 2.8;

/// 浮遊霊 (`EnemyKind::Wraith`) が接近をやめる y座標。詠唱者と狙撃者の
/// 中間 — 完全な後方支援(詠唱者)ほど安全でもなく、狙撃者ほど深く踏み
/// 込みもしない「居座る中距離の脅威」にする。
const WRAITH_STOP_Y: f64 = WORLD_H * 0.45;
const _: () = assert!(CASTER_STOP_Y < WRAITH_STOP_Y && WRAITH_STOP_Y < SNIPER_STOP_Y);

/// 浮遊霊の横揺れの1tickあたりの速度振幅。位相 (`WRAITH_SWAY_ANGULAR_STEP`)
/// と合わせて振幅 ≈ WRAITH_SWAY_STEP / WRAITH_SWAY_ANGULAR_STEP ≈ 6 ワールド
/// 単位になるよう選んでいる — `LANE_HALF_WIDTH` (5.0) を上回るため、
/// 隣のレーンへ実際にはみ出すことがあり「今どのレーンにいるか」を
/// 常時見極めさせる。
const WRAITH_SWAY_STEP: f64 = 0.6;
/// 揺れの角速度 (rad/tick)。1周期 ≈ 2π/0.1 ≈ 63tick ≈ 6.3秒。
const WRAITH_SWAY_ANGULAR_STEP: f64 = 0.1;
/// 個体ごとに位相をずらすための倍率。`enemy.id` に掛けることで、同時に
/// 複数体が湧いても横揺れが同期して見えないようにする。
const WRAITH_SWAY_PHASE_ID_SCALE: f64 = 0.9;

/// ボス級 (`EnemyKind::is_boss`) の「ふよふよ」した浮遊感を出す揺れの
/// 速度振幅。x/yを別位相 (sin/cos) で揺らし円を描くような軌道にすることで、
/// 直進+左右揺れの浮遊霊とは違う「その場で漂う」質感にする。既存の
/// 接近ロジック (homing/直進) の上に加算するだけなので、進行方向を
/// 逆転させるほどの大きさにはしていない。
const BOSS_BOB_X_STEP: f64 = 0.3;
const BOSS_BOB_Y_STEP: f64 = 0.16;
/// 揺れの角速度 (rad/tick)。1周期 ≈ 2π/0.15 ≈ 42tick ≈ 4.2秒。
const BOSS_BOB_ANGULAR_STEP: f64 = 0.15;
const BOSS_BOB_PHASE_ID_SCALE: f64 = 0.7;

fn move_enemies(state: &mut EverlightState) {
    let diff = wave_linear_difficulty(state.wave, state.rank);
    let lantern_x = state.lantern.x;
    let elapsed_ticks = state.elapsed_ticks as f64;
    for enemy in state.enemies.iter_mut() {
        enemy.hurt_flash.tick(1);
        let holding = (enemy.kind == EnemyKind::Sniper && enemy.y >= SNIPER_STOP_Y)
            || (enemy.kind == EnemyKind::Caster && enemy.y >= CASTER_STOP_Y)
            || (enemy.kind == EnemyKind::Wraith && enemy.y >= WRAITH_STOP_Y);
        if !holding {
            let charge_boost =
                if enemy.kind == EnemyKind::Charger && enemy.y >= CHARGER_TRIGGER_Y { CHARGER_BOOST_MULT } else { 1.0 };
            enemy.y += enemy.kind.base_speed() * diff * charge_boost;
        }
        if enemy.kind.homes() {
            // 灯へ寄ってくる敵をおとりにして1レーンへ集め、極光で薙ぐ、
            // という自力発見してほしいシナジー。誘引が弱すぎると気付かれ
            // ないため、はっきり体感できる速さにしている。
            let dx = lantern_x - enemy.x;
            let step = dx.abs().min(1.0);
            enemy.x += step * dx.signum();
        } else if enemy.kind == EnemyKind::Wraith {
            // 灯へは向かわず、時間と自身のidだけで決まる正弦波でx座標を
            // 揺らす。専用フィールドを増やさずに済むよう、絶対位置を
            // 都度計算するのではなく速度として毎tick加算する — 積分結果は
            // 有界な振動になる (ただし振動の中心は湧いた瞬間の位相次第で
            // 元のxから最大 `WRAITH_SWAY_STEP/WRAITH_SWAY_ANGULAR_STEP` ぶん
            // ずれ得る。厳密に中心を揃えたい場合はx0を`Enemy`に持たせる
            // 必要があるが、既存のリテラル構築箇所が多いため見送っている)。
            let phase = (elapsed_ticks + enemy.id as f64 * WRAITH_SWAY_PHASE_ID_SCALE) * WRAITH_SWAY_ANGULAR_STEP;
            enemy.x = (enemy.x + WRAITH_SWAY_STEP * phase.sin()).clamp(0.0, WORLD_W);
        }
        if enemy.kind.is_boss() {
            let phase = (elapsed_ticks + enemy.id as f64 * BOSS_BOB_PHASE_ID_SCALE) * BOSS_BOB_ANGULAR_STEP;
            enemy.x = (enemy.x + BOSS_BOB_X_STEP * phase.sin()).clamp(0.0, WORLD_W);
            enemy.y += BOSS_BOB_Y_STEP * phase.cos();
        }
    }
}

const SNIPER_CHARGE_TICKS: u32 = 24;
/// `EnemyKind::contact_damage` (漏れダメージ) と同じく、waveやrankでは
/// スケールさせない固定値。狙撃者の脅威はダメージ量ではなくレーンを
/// 塞ぐ位置取りの強制にあるため、ここを吊り上げるとレーン拘束という
/// 本来の役割より単なる被弾量インフレになってしまう。
const SNIPER_DAMAGE: i32 = 6;

/// 狙撃者の遠隔攻撃。`SNIPER_STOP_Y` で停止した個体が、灯のレーンにいる
/// 間だけ一定間隔で直接ダメージを飛ばす — 「漏れさえ避ければ安全」という
/// 均衡を崩し、遠くにいる相手にも対応を迫る。
fn resolve_ranged_attacks(state: &mut EverlightState) {
    let lantern_x = state.lantern.x;
    let mut total_damage = 0i32;
    let mut hits = 0u32;
    for enemy in state.enemies.iter_mut() {
        if enemy.kind != EnemyKind::Sniper || enemy.y < SNIPER_STOP_Y {
            continue;
        }
        match enemy.ranged_charge {
            None => enemy.ranged_charge = Some(SNIPER_CHARGE_TICKS),
            Some(t) if t <= 1 => {
                enemy.ranged_charge = Some(SNIPER_CHARGE_TICKS);
                if (enemy.x - lantern_x).abs() <= LANE_HALF_WIDTH {
                    total_damage += SNIPER_DAMAGE;
                    hits += 1;
                }
            }
            Some(t) => enemy.ranged_charge = Some(t - 1),
        }
    }
    if hits == 0 {
        return;
    }
    state.light_hit_count += hits;
    state.lantern.light -= total_damage;
    state.lantern_hurt_flash.trigger(3);
    state.last_light_damage = Some((total_damage, 6));
    state.add_log(format!("狙撃者の一撃で灯が{total_damage}削れた"));
}

const CASTER_FIRE_INTERVAL_TICKS: u32 = 30;
/// 狙撃者の一撃 (`SNIPER_DAMAGE`=6) より低め。弾は避けられる分、
/// 避けそこねた時のダメージまで同じ重さにすると理不尽になる。
const CASTER_BULLET_DAMAGE: i32 = 4;
const CASTER_BULLET_SPEED: f64 = 2.2;

/// 実体弾を撃つ敵 (詠唱者・浮遊霊) 共通: 弾がどの方向へ飛ぶかを決める。
/// この一手が体感を大きく左右する:
///
/// - 発射時の灯の位置を狙って直進させる (`aim_velocity` と同じ「2点を
///   結ぶ向き×速度」) → 弾が見えた瞬間に別レーンへ逃げる予測回避の
///   駆け引きになる
/// - 自身のレーンをまっすぐ縦に落とすだけ (`vx=0`) → 「このレーンに
///   敵がいる」という位置取りのパズルになる (現在の実装)
///
/// 今はシンプルな後者を選んでいるが、前者に変えると難易度も駆け引きの
/// 質もかなり変わる。両方の呼び出し元で共有しているため、変更すれば
/// 詠唱者・浮遊霊の弾が同時に切り替わる。`shooter_x`/`shooter_y`/
/// `lantern_x` は狙い方を変える際に使う値として引数に残してある。
fn aim_enemy_bullet(_shooter_x: f64, _shooter_y: f64, _lantern_x: f64, speed: f64) -> (f64, f64) {
    (0.0, speed)
}

/// 詠唱者の実体弾攻撃。`CASTER_STOP_Y` で停止した個体が一定間隔で
/// `EnemyBullet` を撃つ。狙撃者の `resolve_ranged_attacks` と違い命中は
/// 撃った瞬間に確定せず、`move_and_resolve_enemy_bullets` が弾の移動と
/// 併せて毎tick判定する — 弾が飛んでいる間はプレイヤーが灯を動かして
/// 避けられる。
fn resolve_caster_shots(state: &mut EverlightState) {
    let lantern_x = state.lantern.x;
    let mut new_bullets: Vec<EnemyBullet> = Vec::new();
    for enemy in state.enemies.iter_mut() {
        if enemy.kind != EnemyKind::Caster || enemy.y < CASTER_STOP_Y {
            continue;
        }
        match enemy.ranged_charge {
            None => enemy.ranged_charge = Some(CASTER_FIRE_INTERVAL_TICKS),
            Some(t) if t <= 1 => {
                enemy.ranged_charge = Some(CASTER_FIRE_INTERVAL_TICKS);
                let (vx, vy) = aim_enemy_bullet(enemy.x, enemy.y, lantern_x, CASTER_BULLET_SPEED);
                new_bullets.push(EnemyBullet {
                    x: enemy.x,
                    y: enemy.y,
                    vx,
                    vy,
                    damage: CASTER_BULLET_DAMAGE,
                    source: EnemyKind::Caster,
                });
            }
            Some(t) => enemy.ranged_charge = Some(t - 1),
        }
    }
    if state.enemy_bullets.len() + new_bullets.len() > MAX_ENEMY_BULLETS_ON_FIELD {
        new_bullets.truncate(MAX_ENEMY_BULLETS_ON_FIELD.saturating_sub(state.enemy_bullets.len()));
    }
    state.enemy_bullets.extend(new_bullets);
}

/// 詠唱者 (`CASTER_FIRE_INTERVAL_TICKS`=30) より長め。横揺れそのものが
/// 常時プレッシャーを掛ける分、発砲自体は少し緩めて負荷を弾一辺倒に
/// しない。
const WRAITH_FIRE_INTERVAL_TICKS: u32 = 34;
/// 詠唱者 (4) より高いが狙撃者 (6) より低め。横揺れで飛来レーンの予測が
/// 難しい分、被弾1回あたりの重さは詠唱者側に寄せている。
const WRAITH_BULLET_DAMAGE: i32 = 5;
const WRAITH_BULLET_SPEED: f64 = 2.0;

/// 浮遊霊の実体弾攻撃。`WRAITH_STOP_Y` 到達後、`move_enemies` の横揺れで
/// 常に位置が変わり続ける自身のx座標から、詠唱者と同じく縦にまっすぐ
/// 弾を落とす (`aim_enemy_bullet`)。狙う側の位置が固定されない分、
/// プレイヤーは「弾が出る瞬間のレーン」をその都度見て判断する必要がある。
fn resolve_wraith_shots(state: &mut EverlightState) {
    let lantern_x = state.lantern.x;
    let mut new_bullets: Vec<EnemyBullet> = Vec::new();
    for enemy in state.enemies.iter_mut() {
        if enemy.kind != EnemyKind::Wraith || enemy.y < WRAITH_STOP_Y {
            continue;
        }
        match enemy.ranged_charge {
            None => enemy.ranged_charge = Some(WRAITH_FIRE_INTERVAL_TICKS),
            Some(t) if t <= 1 => {
                enemy.ranged_charge = Some(WRAITH_FIRE_INTERVAL_TICKS);
                let (vx, vy) = aim_enemy_bullet(enemy.x, enemy.y, lantern_x, WRAITH_BULLET_SPEED);
                new_bullets.push(EnemyBullet {
                    x: enemy.x,
                    y: enemy.y,
                    vx,
                    vy,
                    damage: WRAITH_BULLET_DAMAGE,
                    source: EnemyKind::Wraith,
                });
            }
            Some(t) => enemy.ranged_charge = Some(t - 1),
        }
    }
    if state.enemy_bullets.len() + new_bullets.len() > MAX_ENEMY_BULLETS_ON_FIELD {
        new_bullets.truncate(MAX_ENEMY_BULLETS_ON_FIELD.saturating_sub(state.enemy_bullets.len()));
    }
    state.enemy_bullets.extend(new_bullets);
}

/// 詠唱者の弾を移動させ、灯との衝突判定まで一緒に行う。判定は他の
/// レーン系の当たり判定 (`resolve_ranged_attacks`/`resolve_breaches`) と
/// 揃え、円形の当たり半径ではなく「灯のレーン×灯の高さ付近」の帯で見る
/// — 弾自体の見た目上の大きさに関わらず、レーンを移動できていれば
/// 確実に避けられるようにするため。
fn move_and_resolve_enemy_bullets(state: &mut EverlightState) {
    let lantern_x = state.lantern.x;
    let mut total_damage = 0i32;
    let mut hits = 0u32;
    // ログは「最後に命中した1発」の撃ち手の名前で出す (`resolve_breaches`の
    // `last_kind`と同じ割り切り) — 同一tickで詠唱者/浮遊霊が両方命中しても
    // 1行に集約する都合上、両方を律儀に列挙はしない。
    let mut last_source: Option<EnemyKind> = None;
    state.enemy_bullets.retain_mut(|b| {
        b.x += b.vx;
        b.y += b.vy;
        let in_bounds = b.y > -10.0 && b.y < WORLD_H + 10.0 && b.x > -10.0 && b.x < WORLD_W + 10.0;
        if !in_bounds {
            return false;
        }
        let in_lantern_lane = (b.x - lantern_x).abs() <= LANE_HALF_WIDTH;
        let reached_lantern_row = (b.y - LANTERN_Y).abs() <= 6.0;
        if in_lantern_lane && reached_lantern_row {
            total_damage += b.damage;
            hits += 1;
            last_source = Some(b.source);
            return false;
        }
        true
    });
    if hits == 0 {
        return;
    }
    state.light_hit_count += hits;
    state.lantern.light -= total_damage;
    state.lantern_hurt_flash.trigger(3);
    state.last_light_damage = Some((total_damage, 6));
    if let Some(kind) = last_source {
        state.add_log(format!("{}の弾で灯が{total_damage}削れた", kind.name()));
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
    id: u32,
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
            kills.push(KillInfo { id: e.id, kind: e.kind, x: e.x, y: e.y });
            false
        } else {
            true
        }
    });
    kills
}

/// 分裂体 (`EnemyKind::Splitter`) が死ぬ位置から左右へ子を離す距離。
const SPLIT_OFFSET: f64 = 2.0;

fn apply_kills(state: &mut EverlightState, kills: Vec<KillInfo>) {
    if kills.is_empty() {
        return;
    }
    let ember_mult = ember_reward_mult(state.rank);
    let mut split_spawns: Vec<(f64, f64)> = Vec::new();
    for k in &kills {
        state.ember += ((k.kind.ember_reward() as f64) * ember_mult).round() as u32;
        state.kill_count += 1;
        state.kill_effects.push(KillEffect { x: k.x, y: k.y, ticks_left: KILL_EFFECT_TICKS });
        if k.kind.drops_chest() {
            state.chests.push(Chest { x: k.x, y: k.y });
        }
        if k.kind == EnemyKind::Splitter {
            // 分裂体は撃破された位置から羽虫2体を残して散る。子は
            // `EnemyKind::Swarmling` として湧かせるので、この分岐に
            // 二度と入らない (=無限分裂しない) ことが型的に保証される。
            split_spawns.push(((k.x - SPLIT_OFFSET).clamp(0.0, WORLD_W), k.y));
            split_spawns.push(((k.x + SPLIT_OFFSET).clamp(0.0, WORLD_W), k.y));
        }
    }
    for (x, y) in split_spawns {
        spawn_enemy_at_xy(state, EnemyKind::Swarmling, x, y);
    }
    if kills.len() == 1 {
        state.add_log(format!("{}を討った", kills[0].kind.name()));
    } else {
        state.add_log(format!("{}体を討った", kills.len()));
    }
    maybe_trigger_dawn(state, &kills);
}

/// この夜番でランクのマイルストーン波の最終ボスを初めて討った瞬間、
/// 「Dawn」を確定する — `max_unlocked_rank` を即座に更新することで、
/// 直後に灯が消えても/リロードされても達成が失われないようにする
/// (呼び出し元の `mod.rs::tick()` が変化を検知して即座に保存する)。
fn maybe_trigger_dawn(state: &mut EverlightState, kills: &[KillInfo]) {
    if state.dawn_reached_this_vigil {
        return;
    }
    // `state.wave == milestone_wave(state.rank)` では判定しない: 最終ボスは
    // HPが高く、湧いた波(300 tick)以内に倒しきれず次の波へ持ち越されることが
    // ある。持ち越された後に倒してもDawnが成立するよう、「湧いた時点で
    // マイルストーンの最終ボスだった個体」をidで追跡し、そのidが討伐された
    // かどうかだけを見る (`spawn_enemies` が湧いた瞬間に記録する)。
    let Some(boss_id) = state.milestone_boss_id else {
        return;
    };
    if !kills.iter().any(|k| k.id == boss_id) {
        return;
    }
    state.dawn_reached_this_vigil = true;
    state.camp.max_unlocked_rank = state.camp.max_unlocked_rank.max(state.rank + 1);
    state.dawn_count += 1;
    state.add_log(format!("夜明けが来た。挑めるランクが第{}夜まで広がった", state.camp.max_unlocked_rank));
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

fn make_projectile(x: f64, damage: i32, pierce: u32, vx: f64, vy: f64, color: Color, source: WeaponKind) -> Projectile {
    Projectile {
        x,
        y: LANTERN_Y,
        vx,
        vy,
        damage,
        pierce_remaining: pierce.saturating_sub(1),
        radius: 1.6,
        color,
        source,
        hit_enemy_ids: Vec::new(),
    }
}

/// 装甲系 (`EnemyKind::weak_to()` が `Some` を返す種) は、対応する弱点武器
/// 以外から受けるダメージを軽減する。
const SHIELD_DAMAGE_REDUCTION: f64 = 0.5;

fn effective_damage_against(base_damage: i32, source: WeaponKind, target_kind: EnemyKind) -> i32 {
    match target_kind.weak_to() {
        Some(weak) if source != weak => {
            ((base_damage as f64) * (1.0 - SHIELD_DAMAGE_REDUCTION)).round().max(1.0) as i32
        }
        _ => base_damage,
    }
}

// ── 武器の組み合わせシナジー ─────────────────────────────────────────
//
// 進化 (武器+対応する受動効果) とは別に、武器"同士"を同時装備すると
// 発動する追加効果。効果自体は説明せず、`newly_completed_synergy_partners`
// が新規成立の瞬間だけログで気配を残す (進化の色合わせヒントと同じ
// 「点と点を線にする」設計)。

/// 光弾+極光「烙印」: 光弾の命中が残す `bolt_marks` の持続tick。
const MARK_DURATION_TICKS: u32 = 40;
/// 烙印を消費した極光の命中に掛かるダメージ倍率。
const MARK_BONUS_MULT: f64 = 1.5;
/// 光輪+流星「増幅」: 光輪装備中に流星の着弾ダメージへ掛かる倍率。
const METEOR_HALO_SYNERGY_MULT: f64 = 1.25;

/// 同時装備で追加効果が発動する武器の組み合わせ一覧。
const WEAPON_SYNERGY_PAIRS: [(WeaponKind, WeaponKind); 3] = [
    (WeaponKind::Bolt, WeaponKind::Aurora),
    (WeaponKind::Spray, WeaponKind::Halo),
    (WeaponKind::Halo, WeaponKind::Meteor),
];

/// `acquired` を新たに手に入れたことで今まさに揃ったシナジーの相方を
/// 全て返す。`WeaponKind::Halo` のように複数の組み合わせに属する武器も
/// あるため、最初の1件で打ち切らず全て集める — でなければ、揃った
/// 複数のシナジーのうち1つしか発見ログに気配が残らなくなる。
/// `state.loadout` はまだ `acquired` を含まない時点で呼ぶこと
/// (呼び出し元の `apply_boon` 参照)。
fn newly_completed_synergy_partners(state: &EverlightState, acquired: WeaponKind) -> Vec<WeaponKind> {
    WEAPON_SYNERGY_PAIRS
        .iter()
        .filter_map(|&(a, b)| {
            let partner = if a == acquired {
                b
            } else if b == acquired {
                a
            } else {
                return None;
            };
            state.loadout.has(partner).then_some(partner)
        })
        .collect()
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
                new_projectiles.push(make_projectile(lantern_x, damage, pierce, vx, vy, kind.color(), kind));
            }
            WeaponKind::Spray => {
                // シナジー「共鳴」: 光輪を同時装備していると1発増える。
                let mut count = state.loadout.weapons[i].projectile_count();
                if state.loadout.has(WeaponKind::Halo) {
                    count += 1;
                }
                for p in 0..count {
                    let t = if count == 1 { 0.5 } else { p as f64 / (count - 1) as f64 };
                    let angle = -std::f64::consts::FRAC_PI_2 + (t - 0.5) * SPRAY_SPREAD_RAD;
                    let vx = angle.cos() * PROJECTILE_SPEED;
                    let vy = angle.sin() * PROJECTILE_SPEED;
                    new_projectiles.push(make_projectile(lantern_x, damage, pierce, vx, vy, kind.color(), kind));
                }
            }
            WeaponKind::Aurora => {
                let width_mult = state.loadout.weapons[i].aurora_width_mult();
                apply_aurora_hit(state, lantern_x, damage, width_mult);
            }
            WeaponKind::Meteor => {
                // シナジー「増幅」: 光輪を同時装備していると着弾ダメージが上がる。
                let radius = state.loadout.weapons[i].meteor_radius();
                let synergy_damage = if state.loadout.has(WeaponKind::Halo) {
                    (damage as f64 * METEOR_HALO_SYNERGY_MULT).round() as i32
                } else {
                    damage
                };
                apply_meteor_hit(state, synergy_damage, radius);
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
            let mut dmg = effective_damage_against(damage, WeaponKind::Aurora, enemy.kind);
            // シナジー「烙印」: 光弾が付けた印を極光が消費して追加ダメージ。
            if state.bolt_marks.remove(&enemy.id).is_some() {
                dmg = (dmg as f64 * MARK_BONUS_MULT).round() as i32;
            }
            enemy.hp -= dmg;
            enemy.hurt_flash.trigger(4);
        }
    }
    let kills = drain_dead_enemies(state);
    apply_kills(state, kills);
}

/// 流星の着弾地点を選ぶ。敵を `lane_index_of` でレーンごとに集計し、
/// 最も密集しているレーンのうち防衛線に最も近い個体の座標を返す —
/// 光弾 (単体特化) や極光 (灯のレーン固定) では対応しにくい、灯から
/// 離れた場所の横広がりの密集を焼き払う役割を持たせるため。
fn pick_meteor_target(state: &EverlightState) -> Option<(f64, f64)> {
    if state.enemies.is_empty() {
        return None;
    }
    let mut lane_counts = [0u32; COLUMNS];
    for enemy in &state.enemies {
        lane_counts[lane_index_of(enemy.x)] += 1;
    }
    let (dense_lane, _) = lane_counts.iter().enumerate().max_by_key(|&(_, &c)| c)?;
    state
        .enemies
        .iter()
        .filter(|e| lane_index_of(e.x) == dense_lane)
        .max_by(|a, b| a.y.partial_cmp(&b.y).unwrap_or(std::cmp::Ordering::Equal))
        .map(|e| (e.x, e.y))
}

/// 流星: `pick_meteor_target` が選んだ地点を中心に範囲ダメージを与える
/// 即着弾のヒットスキャン。対象がいなければ何もしない (クールダウンの
/// 消費は呼び出し元の `fire_weapons` が既に行っている)。
fn apply_meteor_hit(state: &mut EverlightState, damage: i32, radius: f64) {
    let Some((cx, cy)) = pick_meteor_target(state) else {
        return;
    };
    state.meteor_flash.trigger(METEOR_FLASH_TICKS);
    state.meteor_flash_pos = (cx, cy);
    for enemy in state.enemies.iter_mut() {
        let dx = enemy.x - cx;
        let dy = enemy.y - cy;
        if dx * dx + dy * dy <= radius * radius {
            enemy.hp -= effective_damage_against(damage, WeaponKind::Meteor, enemy.kind);
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
    let lantern_x = state.lantern.x;
    let damage = ((halo.damage() as f64) * damage_mult).round() as i32;
    let radius = halo.halo_radius();
    for enemy in state.enemies.iter_mut() {
        let dx = enemy.x - lantern_x;
        let dy = enemy.y - LANTERN_Y;
        if dx * dx + dy * dy <= radius * radius {
            enemy.hp -= effective_damage_against(damage, WeaponKind::Halo, enemy.kind);
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
            enemy.hp -= effective_damage_against(proj.damage, proj.source, enemy.kind);
            enemy.hurt_flash.trigger(3);
            proj.hit_enemy_ids.push(enemy.id);
            let hit_enemy_id = enemy.id;
            if proj.source == WeaponKind::Bolt && state.loadout.has(WeaponKind::Aurora) {
                state.bolt_marks.insert(hit_enemy_id, MARK_DURATION_TICKS);
            }
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
        } else if state.loadout.weapons.len() < state.camp.max_weapon_slots() {
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
            let synergy_partners = newly_completed_synergy_partners(state, k);
            state.loadout.weapons.push(OwnedWeapon::new(k));
            if synergy_partners.is_empty() {
                state.add_log(format!("『{}』を手に入れた", k.name()));
            } else {
                let partner_names: Vec<&str> = synergy_partners.iter().map(|p| p.name()).collect();
                state.add_log(format!(
                    "『{}』を手に入れた。『{}』と呼応している気がする",
                    k.name(),
                    partner_names.join("』『")
                ));
            }
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

// ── ボスの特殊攻撃 (灯喰らい/実体弾/召喚) ───────────────────────────

/// 構え中にレーンが移動するボス (大蛇) の、レーンを1つ移す間隔。
const SWEEP_STEP_TICKS: u32 = 4;

/// ランクが上がるほどボスの攻撃周期は短くなる (=気を抜ける時間が減る)。
fn boss_attack_period_ticks(rank: u32) -> u64 {
    (BOSS_ATTACK_PERIOD_TICKS as i64 - rank.saturating_sub(1) as i64 * 6).max(40) as u64
}

/// ランクが上がるほど一撃のダメージも重くなる。
fn boss_telegraph_damage(rank: u32) -> i32 {
    BOSS_TELEGRAPH_DAMAGE + rank.saturating_sub(1) as i32 * 4
}

fn lane_index_of(x: f64) -> usize {
    let lane_w = WORLD_W / COLUMNS as f64;
    ((x / lane_w) as i64).clamp(0, COLUMNS as i64 - 1) as usize
}

/// `x` の隣のレーン中心座標 (右端なら左隣)。影の魔女/満月の魔王が同時に
/// 警告する2レーン目を決めるのに使う。
fn adjacent_lane_x(x: f64) -> f64 {
    let lane = lane_index_of(x);
    let other = if lane + 1 < COLUMNS { lane + 1 } else { lane.saturating_sub(1) };
    super::state::lane_center_x(other)
}

/// ボスの種類ごとに構えのレーン構成を決める。影の魔女/満月の魔王は
/// 隣接2レーンを同時に、大蛇は1レーンだが構え中に横へ移動する — 同じ
/// 「レーンから逃げる」判断でも毎回違う読み合いになるようにする。
fn new_boss_telegraph(kind: EnemyKind, source_enemy_id: u32, boss_x: f64, rng_state: &mut u32) -> BossTelegraph {
    match kind {
        EnemyKind::ShadowWitch | EnemyKind::FullMoonBoss => BossTelegraph {
            kind,
            source_enemy_id,
            lane_xs: vec![boss_x, adjacent_lane_x(boss_x)],
            ticks_left: BOSS_TELEGRAPH_TICKS,
            sweep_direction: None,
        },
        EnemyKind::Serpent => {
            let lane = lane_index_of(boss_x);
            // 端のレーンでは外向きの方向を選ぶと `lane_index_of` のクランプで
            // 毎tick同じレーンへ戻され、警告が動かないまま止まって見える。
            // 端では内向き固定にして、必ず動くことを保証する。
            let direction = match lane {
                0 => 1,
                l if l == COLUMNS - 1 => -1,
                _ => {
                    if rng_below(rng_state, 2) == 0 {
                        -1
                    } else {
                        1
                    }
                }
            };
            BossTelegraph {
                kind,
                source_enemy_id,
                lane_xs: vec![boss_x],
                ticks_left: BOSS_TELEGRAPH_TICKS,
                sweep_direction: Some(direction),
            }
        }
        _ => BossTelegraph {
            kind,
            source_enemy_id,
            lane_xs: vec![boss_x],
            ticks_left: BOSS_TELEGRAPH_TICKS,
            sweep_direction: None,
        },
    }
}

fn resolve_boss_telegraph(state: &mut EverlightState) {
    let boss = state.enemies.iter().find(|e| e.kind.is_boss()).map(|e| (e.kind, e.id, e.x));

    if let Some(mut telegraph) = state.boss_telegraph.take() {
        // 構え中に「その」ボスを討ち取れば不発になる — 「間に合った」満足感の
        // ため。敵種ではなく個体idで判定する: チェックポイントの間隔より
        // 討伐が遅れて別種のボスと同時に生存する状況では、種類だけで見ると
        // 構えたボスとは別個体が生きているだけで誤って「不発にならない」と
        // 判定してしまう。
        let source_alive = state.enemies.iter().any(|e| e.id == telegraph.source_enemy_id);
        if !source_alive {
            return;
        }
        if telegraph.ticks_left <= 1 {
            let hit = telegraph.lane_xs.iter().any(|&x| (state.lantern.x - x).abs() <= LANE_HALF_WIDTH);
            if hit {
                let damage = boss_telegraph_damage(state.rank);
                state.lantern.light -= damage;
                state.light_hit_count += 1;
                state.lantern_hurt_flash.trigger(5);
                state.last_light_damage = Some((damage, 8));
                state.add_log(format!("{}の一撃で灯が大きく削れた！", telegraph.kind.name()));
            } else {
                state.add_log(format!("{}の一撃をかわした！", telegraph.kind.name()));
            }
            return;
        }
        telegraph.ticks_left -= 1;
        if let Some(direction) = telegraph.sweep_direction {
            if telegraph.ticks_left.is_multiple_of(SWEEP_STEP_TICKS) {
                let lane = lane_index_of(telegraph.lane_xs[0]);
                let next_lane = (lane as i32 + direction).clamp(0, COLUMNS as i32 - 1) as usize;
                telegraph.lane_xs[0] = super::state::lane_center_x(next_lane);
            }
        }
        state.boss_telegraph = Some(telegraph);
        return;
    }

    if let Some((kind, id, x)) = boss {
        let period = boss_attack_period_ticks(state.rank);
        if state.elapsed_ticks > 0 && state.elapsed_ticks.is_multiple_of(period) {
            let telegraph = new_boss_telegraph(kind, id, x, &mut state.rng_state);
            state.boss_telegraph = Some(telegraph);
            state.add_log(format!("{}が灯喰らいの構え！", kind.name()));
        }
    }
}

/// ランクが上がるほど魔王/満月の魔王の実体弾攻撃も速くなる。
/// `boss_attack_period_ticks` (基準90、rank5で66) とは意図的に異なる
/// 基準値・減り方にしている — 周期を揃えると「灯喰らいの構えと実体弾が
/// 毎回同時に来る/毎回ズレて来る」の単調な繰り返しになるため、独立した
/// 周期にして重なる時と重ならない時が入り混じるようにする。
fn boss_bullet_period_ticks(rank: u32) -> u64 {
    (BOSS_BULLET_PERIOD_TICKS as i64 - rank.saturating_sub(1) as i64 * 4).max(30) as u64
}
const BOSS_BULLET_PERIOD_TICKS: u64 = 54;
/// 詠唱者(4)/浮遊霊(5)の弾よりさらに重い — ボス級の攻撃という位置付け。
const BOSS_BULLET_DAMAGE: i32 = 8;
/// 詠唱者/浮遊霊の弾 (2.0〜2.2、レーン固定で縦に落ちるだけ) より速く、
/// かつ発射時の灯位置へ直進する (`aim_velocity`)。狙われた瞬間に見えた
/// 位置から逃げる、より短い判断時間を要求する別種の脅威にする。
const BOSS_BULLET_SPEED: f64 = 2.6;

/// 魔王/満月の魔王の実体弾攻撃。`resolve_boss_telegraph` (灯喰らい) とは
/// 別の独立した攻撃系統 — 灯喰らいが「レーンへの警告→回避」の駆け引き
/// なのに対し、こちらは発射時の灯位置を狙って直進する実弾という異なる
/// 質の脅威を足す。
fn resolve_boss_bullets(state: &mut EverlightState) {
    let shooter = state
        .enemies
        .iter()
        .find(|e| matches!(e.kind, EnemyKind::Boss | EnemyKind::FullMoonBoss))
        .map(|e| (e.kind, e.x, e.y));
    let Some((kind, x, y)) = shooter else {
        return;
    };
    let period = boss_bullet_period_ticks(state.rank);
    if state.elapsed_ticks == 0 || !state.elapsed_ticks.is_multiple_of(period) {
        return;
    }
    if state.enemy_bullets.len() >= MAX_ENEMY_BULLETS_ON_FIELD {
        return;
    }
    let (vx, vy) = aim_velocity(x, y, state.lantern.x, LANTERN_Y, BOSS_BULLET_SPEED);
    state.enemy_bullets.push(EnemyBullet { x, y, vx, vy, damage: BOSS_BULLET_DAMAGE, source: kind });
    state.add_log(format!("{}が灯めがけて撃ち放った！", kind.name()));
}

/// 影の魔女/大蛇が雑魚を呼び寄せる周期。`boss_bullet_period_ticks`と同じ
/// 理由でボス系の他の攻撃周期とは独立させている。
fn boss_summon_period_ticks(rank: u32) -> u64 {
    (BOSS_SUMMON_PERIOD_TICKS as i64 - rank.saturating_sub(1) as i64 * 3).max(45) as u64
}
const BOSS_SUMMON_PERIOD_TICKS: u64 = 80;
/// 召喚した2体をボスの左右へ離す距離。`SPLIT_OFFSET`と同じ理由 (同座標に
/// 重ねて湧かせない) だが値は独立させている。
const BOSS_SUMMON_OFFSET: f64 = 4.0;

/// 影の魔女/大蛇の召喚攻撃。羽虫2体をボスの左右へ湧かせる。単体の脅威
/// (ボス本体・灯喰らい・実体弾) への対応で手一杯になっている間にも物量で
/// 圧をかける — 「敵召喚して飛ばしてくる」という、灯喰らいの回避判断とは
/// 別種の「手数を割かれる」プレッシャーを足す。
fn resolve_boss_summons(state: &mut EverlightState) {
    let summoner = state
        .enemies
        .iter()
        .find(|e| matches!(e.kind, EnemyKind::ShadowWitch | EnemyKind::Serpent))
        .map(|e| (e.kind, e.x, e.y));
    let Some((kind, x, y)) = summoner else {
        return;
    };
    let period = boss_summon_period_ticks(state.rank);
    if state.elapsed_ticks == 0 || !state.elapsed_ticks.is_multiple_of(period) {
        return;
    }
    spawn_enemy_at_xy(state, EnemyKind::Swarmling, (x - BOSS_SUMMON_OFFSET).clamp(0.0, WORLD_W), y);
    spawn_enemy_at_xy(state, EnemyKind::Swarmling, (x + BOSS_SUMMON_OFFSET).clamp(0.0, WORLD_W), y);
    state.add_log(format!("{}が魔物を呼び寄せた！", kind.name()));
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

pub fn purchase_extra_weapon_slot(state: &mut EverlightState) -> bool {
    if state.camp.extra_weapon_slot_level >= 1 || state.ember < CampUpgrades::EXTRA_WEAPON_SLOT_COST {
        return false;
    }
    state.ember -= CampUpgrades::EXTRA_WEAPON_SLOT_COST;
    state.camp.extra_weapon_slot_level = 1;
    true
}

/// 拠点で挑戦ランクを選ぶ。範囲外の指定は `max_unlocked_rank` 側へ
/// クランプする (未解放のランクへは進めない)。
pub fn select_rank(state: &mut EverlightState, rank: u32) {
    state.camp.selected_rank = rank.clamp(1, state.camp.max_unlocked_rank.max(1));
}

pub fn start_vigil(state: &mut EverlightState) {
    let light_max = state.camp.light_max();
    state.phase = Phase::Vigil;
    state.lantern = Lantern::new(light_max);
    state.enemies.clear();
    state.projectiles.clear();
    state.enemy_bullets.clear();
    state.chests.clear();
    state.kill_effects.clear();
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
    state.rank = state.camp.effective_selected_rank();
    state.dawn_reached_this_vigil = false;
    state.milestone_boss_id = None;
    // next_enemy_id をここで0へ戻すため、前の夜番の烙印がid再利用で
    // 誤って新しい敵に適用されないようクリアする。
    state.bolt_marks.clear();
    state.kill_count = 0;
    // breach_count はリセットしない: detect_transitions が前回renderとの
    // 差分で演出をトリガーする単調増加カウンタ (state.rsのコメント参照)。
    // ここで0に戻すと、前の夜番で漏れが発生していた場合に「減った」と
    // 誤検知され、拠点→次の夜番の遷移で無関係な漏れ演出が誤発火する。
    state.last_light_damage = None;
    state.add_log(format!("夜番開始 (第{}夜)。灯を守れ！", state.rank));
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
            ranged_charge: None,
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
                ranged_charge: None,
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
            ranged_charge: None,
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

    // ── 流星 (Meteor) ───────────────────────────────────────────────

    fn push_enemy_at(state: &mut EverlightState, id: u32, x: f64, y: f64) {
        state.enemies.push(Enemy {
            id,
            kind: EnemyKind::Wisp,
            x,
            y,
            hp: 999,
            max_hp: 999,
            hurt_flash: FlashTimer::new(),
            ranged_charge: None,
        });
    }

    #[test]
    fn meteor_targets_the_most_crowded_lane_nearest_the_breach() {
        let mut state = EverlightState::new();
        start_vigil(&mut state);
        let lane_center_x = super::super::state::lane_center_x;
        // 疎なレーン (1体) と密なレーン (3体) を作る。密なレーンの中でも
        // 最も防衛線に近い個体 (yが最大) が着弾中心になるはず。
        push_enemy_at(&mut state, 1, lane_center_x(0), 20.0);
        push_enemy_at(&mut state, 2, lane_center_x(5), 10.0);
        push_enemy_at(&mut state, 3, lane_center_x(5), 40.0);
        push_enemy_at(&mut state, 4, lane_center_x(5), 25.0);

        let (x, y) = pick_meteor_target(&state).expect("敵がいれば着弾先が選ばれるはず");
        assert_eq!(x, lane_center_x(5), "密集レーンが選ばれるはず");
        assert_eq!(y, 40.0, "密集レーン内では防衛線に最も近い個体が中心になるはず");
    }

    #[test]
    fn meteor_returns_no_target_without_enemies() {
        let state = EverlightState::new();
        assert!(pick_meteor_target(&state).is_none());
    }

    #[test]
    fn meteor_hit_damages_enemies_within_radius_but_not_beyond() {
        let mut state = EverlightState::new();
        start_vigil(&mut state);
        let lane_center_x = super::super::state::lane_center_x;
        push_enemy_at(&mut state, 1, lane_center_x(4), 50.0);
        push_enemy_at(&mut state, 2, lane_center_x(4) + 3.0, 50.0);
        push_enemy_at(&mut state, 3, lane_center_x(4) + 40.0, 50.0);

        apply_meteor_hit(&mut state, 20, 6.0);

        let hp_by_id: std::collections::HashMap<u32, i32> = state.enemies.iter().map(|e| (e.id, e.hp)).collect();
        assert!(hp_by_id[&1] < 999, "着弾地点そのものの敵はダメージを受けるはず");
        assert!(hp_by_id[&2] < 999, "半径内の敵はダメージを受けるはず");
        assert_eq!(hp_by_id[&3], 999, "半径外の敵はダメージを受けないはず");
    }

    #[test]
    fn meteor_fire_does_not_flash_without_a_target() {
        let mut state = EverlightState::new();
        start_vigil(&mut state);
        state.loadout.weapons.clear();
        state.loadout.weapons.push(OwnedWeapon { kind: WeaponKind::Meteor, level: 1, cooldown_remaining: 0, evolved: false });
        assert!(state.enemies.is_empty());

        fire_weapons(&mut state, 1.0);

        assert!(
            !state.meteor_flash.is_active(),
            "対象がいなければ着弾しないので、極光と違ってフラッシュも立たないはず"
        );
    }

    // ── 武器の組み合わせシナジー ───────────────────────────────────────

    #[test]
    fn bolt_mark_boosts_a_later_aurora_hit_only_when_both_are_equipped() {
        let mut state = EverlightState::new();
        start_vigil(&mut state);
        state.loadout.weapons.clear();
        state.loadout.weapons.push(OwnedWeapon::new(WeaponKind::Bolt));
        state.loadout.weapons.push(OwnedWeapon::new(WeaponKind::Aurora));
        let lantern_x = state.lantern.x;
        push_enemy_at(&mut state, 1, lantern_x, 50.0);

        // vy を「1tickでちょうど敵のy座標まで届く」値にして、スイープ
        // 判定の距離計算に頼らず命中を確定させる。
        state.projectiles.push(Projectile {
            x: lantern_x,
            y: LANTERN_Y,
            vx: 0.0,
            vy: 50.0 - LANTERN_Y,
            damage: 10,
            pierce_remaining: 0,
            radius: 1.6,
            color: WeaponKind::Bolt.color(),
            source: WeaponKind::Bolt,
            hit_enemy_ids: Vec::new(),
        });
        move_and_resolve_projectiles(&mut state, &std::collections::HashMap::new());
        assert!(state.bolt_marks.contains_key(&1), "光弾+極光を両方装備していれば命中で烙印が付くはず");

        let hp_before = state.enemies[0].hp;
        apply_aurora_hit(&mut state, lantern_x, 10, 1.0);
        let marked_loss = hp_before - state.enemies[0].hp;
        assert!(!state.bolt_marks.contains_key(&1), "烙印は極光の命中で消費されるはず");

        // 比較対象: 烙印が無い状態での極光ダメージ (=素のeffective_damage_against)。
        let unmarked_loss = 10;
        assert!(marked_loss > unmarked_loss, "烙印を消費した極光は素のダメージより大きいはず");
    }

    #[test]
    fn bolt_does_not_mark_without_aurora_equipped() {
        let mut state = EverlightState::new();
        start_vigil(&mut state);
        state.loadout.weapons.clear();
        state.loadout.weapons.push(OwnedWeapon::new(WeaponKind::Bolt));
        let lantern_x = state.lantern.x;
        push_enemy_at(&mut state, 1, lantern_x, 50.0);

        // vy を「1tickでちょうど敵のy座標まで届く」値にして、スイープ
        // 判定の距離計算に頼らず命中を確定させる。
        state.projectiles.push(Projectile {
            x: lantern_x,
            y: LANTERN_Y,
            vx: 0.0,
            vy: 50.0 - LANTERN_Y,
            damage: 10,
            pierce_remaining: 0,
            radius: 1.6,
            color: WeaponKind::Bolt.color(),
            source: WeaponKind::Bolt,
            hit_enemy_ids: Vec::new(),
        });
        move_and_resolve_projectiles(&mut state, &std::collections::HashMap::new());
        assert!(!state.bolt_marks.contains_key(&1), "極光を装備していなければ烙印は付かないはず");
    }

    #[test]
    fn spray_gains_an_extra_projectile_when_halo_is_equipped() {
        let mut state = EverlightState::new();
        start_vigil(&mut state);
        state.loadout.weapons.clear();
        state.loadout.weapons.push(OwnedWeapon::new(WeaponKind::Spray));
        fire_weapons(&mut state, 1.0);
        let count_without_halo = state.projectiles.len();

        let mut state = EverlightState::new();
        start_vigil(&mut state);
        state.loadout.weapons.clear();
        state.loadout.weapons.push(OwnedWeapon::new(WeaponKind::Spray));
        state.loadout.weapons.push(OwnedWeapon::new(WeaponKind::Halo));
        fire_weapons(&mut state, 1.0);
        let count_with_halo = state.projectiles.len();

        assert_eq!(count_with_halo, count_without_halo + 1, "光輪との共鳴で散光の弾数が1つ増えるはず");
    }

    #[test]
    fn meteor_damage_increases_when_halo_is_equipped() {
        // 流星の着弾地点 (y=50) は光輪の判定半径 (灯周囲、y=LANTERN_Y付近)
        // の外なので、光輪自身の命中を混ぜずに増幅シナジーだけを検証できる。
        let mut state = EverlightState::new();
        start_vigil(&mut state);
        state.loadout.weapons.clear();
        state.loadout.weapons.push(OwnedWeapon::new(WeaponKind::Meteor));
        let lantern_x = state.lantern.x;
        push_enemy_at(&mut state, 1, lantern_x, 50.0);
        fire_weapons(&mut state, 1.0);
        let loss_without_halo = 999 - state.enemies[0].hp;

        let mut state = EverlightState::new();
        start_vigil(&mut state);
        state.loadout.weapons.clear();
        state.loadout.weapons.push(OwnedWeapon::new(WeaponKind::Meteor));
        state.loadout.weapons.push(OwnedWeapon::new(WeaponKind::Halo));
        let lantern_x = state.lantern.x;
        push_enemy_at(&mut state, 1, lantern_x, 50.0);
        fire_weapons(&mut state, 1.0);
        let loss_with_halo = 999 - state.enemies[0].hp;

        assert!(loss_with_halo > loss_without_halo, "光輪との増幅で流星のダメージが上がるはず");
    }

    #[test]
    fn acquiring_a_synergy_partner_logs_a_discovery_hint() {
        let mut state = EverlightState::new();
        start_vigil(&mut state);
        state.loadout.weapons.clear();
        state.loadout.weapons.push(OwnedWeapon::new(WeaponKind::Bolt));

        apply_boon(&mut state, BoonKind::NewWeapon(WeaponKind::Aurora));

        assert!(
            state.log.last().is_some_and(|l| l.contains("呼応")),
            "光弾を持っている状態で極光を手に入れたら、シナジー成立のヒントログが出るはず"
        );
    }

    #[test]
    fn acquiring_a_weapon_completing_two_synergies_at_once_mentions_both_partners() {
        // 光輪は「散光+光輪」「光輪+流星」の2組に属する。両方を先に持って
        // いる状態で光輪を手に入れると、両方のシナジーが同時に成立する
        // — `newly_completed_synergy_partners` が最初の1件で打ち切ると
        // 片方の発見ログが欠落してしまう回帰テスト。
        let mut state = EverlightState::new();
        start_vigil(&mut state);
        state.loadout.weapons.clear();
        state.loadout.weapons.push(OwnedWeapon::new(WeaponKind::Spray));
        state.loadout.weapons.push(OwnedWeapon::new(WeaponKind::Meteor));

        apply_boon(&mut state, BoonKind::NewWeapon(WeaponKind::Halo));

        let log = state.log.last().cloned().unwrap_or_default();
        assert!(log.contains(WeaponKind::Spray.name()), "散光との共鳴も気配として出るはず: {log}");
        assert!(log.contains(WeaponKind::Meteor.name()), "流星との増幅も気配として出るはず: {log}");
    }

    #[test]
    fn acquiring_a_weapon_without_a_partner_does_not_log_a_hint() {
        let mut state = EverlightState::new();
        start_vigil(&mut state);
        state.loadout.weapons.clear();

        apply_boon(&mut state, BoonKind::NewWeapon(WeaponKind::Bolt));

        assert!(
            state.log.last().is_some_and(|l| !l.contains("呼応")),
            "相方をまだ持っていなければヒントログは出ないはず"
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
            ranged_charge: None,
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
            ranged_charge: None,
        });
        state.projectiles.push(make_projectile(state.lantern.x, 10, 1, 0.0, -1.0, Color::White, WeaponKind::Bolt));
        state.projectiles[0].y = LANTERN_Y - 5.0;
        let ember_before = state.ember;
        move_and_resolve_projectiles(&mut state, &std::collections::HashMap::new());
        assert!(state.enemies.is_empty());
        assert!(state.ember > ember_before);
        assert_eq!(state.kill_count, 1);
    }

    #[test]
    fn killing_an_enemy_spawns_a_kill_effect_at_its_position() {
        let mut state = EverlightState::new();
        start_vigil(&mut state);
        let (kill_x, kill_y) = (state.lantern.x, LANTERN_Y - 5.0);
        state.enemies.push(Enemy {
            id: 4,
            kind: EnemyKind::Wisp,
            x: kill_x,
            y: kill_y,
            hp: 1,
            max_hp: 7,
            hurt_flash: FlashTimer::new(),
            ranged_charge: None,
        });
        state.projectiles.push(make_projectile(kill_x, 10, 1, 0.0, -1.0, Color::White, WeaponKind::Bolt));
        state.projectiles[0].y = kill_y;
        assert!(state.kill_effects.is_empty());
        move_and_resolve_projectiles(&mut state, &std::collections::HashMap::new());
        assert_eq!(state.kill_effects.len(), 1, "討伐位置に爆破演出が1つ残るはず");
        let effect = &state.kill_effects[0];
        assert_eq!((effect.x, effect.y), (kill_x, kill_y), "演出は討伐位置に残るはず");
        assert_eq!(effect.ticks_left, KILL_EFFECT_TICKS);
    }

    #[test]
    fn kill_effects_decay_and_are_removed_via_tick() {
        let mut state = EverlightState::new();
        start_vigil(&mut state);
        state.kill_effects.push(KillEffect { x: 10.0, y: 20.0, ticks_left: 2 });
        tick(&mut state);
        assert_eq!(state.kill_effects[0].ticks_left, 1, "毎tick1ずつ減るはず");
        tick(&mut state);
        assert!(state.kill_effects.is_empty(), "残りtickが尽きたら取り除かれるはず");
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
            ranged_charge: None,
        });
        state.projectiles.push(make_projectile(x, 10, 1, 0.0, -9.0, Color::White, WeaponKind::Bolt));
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
            ranged_charge: None,
        });
        let mut proj = make_projectile(x, 10, 2, 0.0, -9.0, Color::White, WeaponKind::Bolt);
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
            ranged_charge: None,
        });
        state.enemies.push(Enemy {
            id: 2,
            kind: EnemyKind::Wisp,
            x,
            y: 3.0,
            hp: 999,
            max_hp: 999,
            hurt_flash: FlashTimer::new(),
            ranged_charge: None,
        });
        let mut proj = make_projectile(x, 10, 2, 0.0, -9.0, Color::White, WeaponKind::Bolt);
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
            ranged_charge: None,
        });
        state.enemies.push(Enemy {
            id: 2,
            kind: EnemyKind::Wisp,
            x,
            y: 7.0,
            hp: 999,
            max_hp: 999,
            hurt_flash: FlashTimer::new(),
            ranged_charge: None,
        });
        let mut proj = make_projectile(x, 10, 1, 0.0, -9.0, Color::White, WeaponKind::Bolt); // pierce=1 → 命中は1発分のみ
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
            ranged_charge: None,
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
            ranged_charge: None,
        });
        let mut proj = make_projectile(x, 10, 1, 0.0, -9.0, Color::White, WeaponKind::Bolt); // pierce=1 → 命中は1発分のみ
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
            ranged_charge: None,
        });
        let mut proj = make_projectile(x, 10, 1, 0.0, -9.0, Color::White, WeaponKind::Bolt);
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
        assert!(!purchase_extra_weapon_slot(&mut state));
    }

    #[test]
    fn purchase_extra_weapon_slot_deducts_ember_and_is_one_time() {
        let mut state = EverlightState::new();
        state.ember = 1000;
        let slots_before = state.camp.max_weapon_slots();
        assert!(purchase_extra_weapon_slot(&mut state));
        assert_eq!(state.camp.extra_weapon_slot_level, 1);
        assert_eq!(state.camp.max_weapon_slots(), slots_before + 1, "拡張枠購入で武器スロットが1つ増えるはず");
        assert!(!purchase_extra_weapon_slot(&mut state), "一度きりの解放なので2回目は買えないはず");
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
            ranged_charge: None,
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
            ranged_charge: None,
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
            ranged_charge: None,
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
            ranged_charge: None,
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
    fn extra_slot_upgrade_unlocks_a_5th_passive() {
        let mut state = EverlightState::new();
        start_vigil(&mut state);
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
    fn extra_weapon_slot_upgrade_unlocks_a_5th_weapon() {
        // 流星の追加で WeaponKind::all() が5種になったため、基本スロット数
        // (MAX_WEAPON_SLOTS=4) のままだと必ず1種は持てない — 受動効果と
        // 同じ「拡張枠を買わない限り全種は揃わない」構図に揃えている。
        let mut state = EverlightState::new();
        start_vigil(&mut state);
        state.loadout.weapons.clear();
        for &kind in WeaponKind::all().iter().take(4) {
            state.loadout.weapons.push(OwnedWeapon::new(kind));
        }
        assert_eq!(state.loadout.weapons.len(), 4);
        assert!(
            !candidate_boons(&state).iter().any(|o| matches!(o.kind, BoonKind::NewWeapon(_))),
            "拡張前は基本4枠が埋まっているのでNewWeaponは出ないはず"
        );

        state.camp.extra_weapon_slot_level = 1;
        assert!(
            candidate_boons(&state).iter().any(|o| matches!(o.kind, BoonKind::NewWeapon(_))),
            "武器スロット拡張後は5種目の武器がNewWeaponとして出るはず"
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

    // ── ランク/夜のマイルストーン ─────────────────────────────────────

    #[test]
    fn milestone_wave_extends_by_five_per_rank() {
        assert_eq!(milestone_wave(1), 15);
        assert_eq!(milestone_wave(2), 20);
        assert_eq!(milestone_wave(3), 25);
    }

    #[test]
    fn boss_kind_for_alternates_regular_checkpoints_and_ends_each_rank_with_a_finale() {
        // ランク1の夜: 第5波=魔王, 第10波=影の魔女, 第15波(最終)=満月の魔王。
        assert_eq!(boss_kind_for(5, 1), EnemyKind::Boss);
        assert_eq!(boss_kind_for(10, 1), EnemyKind::ShadowWitch);
        assert_eq!(boss_kind_for(15, 1), EnemyKind::FullMoonBoss);
        // ランク2の夜は最終が第20波なので、第15波はまだ通常チェックポイント。
        assert_eq!(boss_kind_for(15, 2), EnemyKind::Boss);
        assert_eq!(boss_kind_for(20, 2), EnemyKind::Serpent);
    }

    #[test]
    fn wave_difficulty_scales_with_rank_independently_of_wave() {
        let rank1 = wave_difficulty(5, 1);
        let rank2 = wave_difficulty(5, 2);
        assert!(rank2 > rank1, "同じ波でもランクが高いほど難易度は上がるはず");
    }

    #[test]
    fn wave_difficulty_stays_linear_up_to_the_milestone_wave() {
        // マイルストーンまでは、隣り合う波の"差分"(比ではなく差)が一定に
        // なるはず — 等差 (線形) 成長のままである証拠。指数関数の場合は
        // 差分ではなく比が一定になるので、差分の一定性は線形であることの
        // 直接的な確認になる。
        let rank = 1;
        let milestone = milestone_wave(rank);
        let diff_early = wave_difficulty(3, rank) - wave_difficulty(2, rank);
        let diff_late = wave_difficulty(milestone, rank) - wave_difficulty(milestone - 1, rank);
        assert!(
            (diff_early - diff_late).abs() < 1e-9,
            "マイルストーンまでは波ごとの差分が一定のはず: diff_early={diff_early} diff_late={diff_late}"
        );
    }

    #[test]
    fn wave_difficulty_escalates_exponentially_past_the_milestone_wave() {
        // マイルストーンを越えた後は、隣り合う波の"比"が毎波
        // `POST_MILESTONE_GROWTH_PER_WAVE` で一定になるはず — 恒久強化を
        // 積み続けてもいつまでも先へ進めてしまわないようにする指数関数側の
        // escalation本体を直接検証する。
        let rank = 1;
        let milestone = milestone_wave(rank);
        for waves_past_milestone in 0..10u32 {
            let cur = wave_difficulty(milestone + waves_past_milestone, rank);
            let next = wave_difficulty(milestone + waves_past_milestone + 1, rank);
            let ratio = next / cur;
            assert!(
                (ratio - POST_MILESTONE_GROWTH_PER_WAVE).abs() < 1e-9,
                "マイルストーンの{waves_past_milestone}波先での増分比が指数関数の想定と違う: ratio={ratio}"
            );
        }
    }

    #[test]
    fn wave_linear_difficulty_freezes_past_the_milestone_wave() {
        // `wave_difficulty` の指数関数側escalationは敵のHPだけに使うべきで、
        // 移動速度 (`move_enemies`) には波が進んでも頭打ちのこちらを使う。
        let rank = 1;
        let milestone = milestone_wave(rank);
        let at_milestone = wave_linear_difficulty(milestone, rank);
        let far_past_milestone = wave_linear_difficulty(milestone + 100, rank);
        assert_eq!(
            at_milestone, far_past_milestone,
            "マイルストーンを越えても線形難易度は凍結されたままのはず"
        );
    }

    #[test]
    fn move_enemies_never_lets_an_enemy_cross_the_field_in_a_single_tick_even_deep_past_milestone() {
        // HPの指数関数的escalationを移動速度にまで流用すると、遠くの敵が
        // 1tickで防衛線に到達する「反応不可能な瞬間死」が起こり得る。
        // マイルストーンを大きく超えた極端な状況でもそれが起きないことを
        // 直接 `move_enemies` を叩いて確認する回帰テスト。
        let mut state = EverlightState::new();
        state.wave = milestone_wave(5) + 100;
        state.rank = 5;
        state.enemies.push(Enemy {
            id: 1,
            kind: EnemyKind::Swarmling,
            x: 0.0,
            y: 0.0,
            hp: 1,
            max_hp: 1,
            hurt_flash: FlashTimer::new(),
            ranged_charge: None,
        });
        move_enemies(&mut state);
        let moved = state.enemies[0].y;
        assert!(
            moved < WORLD_H,
            "マイルストーンを100波超えても、1tickで盤面を横断してはいけない: moved_y={moved} WORLD_H={WORLD_H}"
        );
    }

    #[test]
    fn ember_reward_mult_increases_with_rank() {
        assert_eq!(ember_reward_mult(1), 1.0);
        assert!(ember_reward_mult(2) > 1.0, "高ランクほど討伐報酬も伸びるはず");
    }

    #[test]
    fn select_rank_clamps_to_unlocked_range() {
        let mut state = EverlightState::new();
        state.camp.max_unlocked_rank = 2;
        select_rank(&mut state, 5);
        assert_eq!(state.camp.selected_rank, 2, "未解放のランクへは進めないはず");
        select_rank(&mut state, 0);
        assert_eq!(state.camp.selected_rank, 1, "ランクは1未満にはならないはず");
    }

    #[test]
    fn defeating_the_finale_boss_at_the_milestone_wave_triggers_dawn() {
        let mut state = EverlightState::new();
        start_vigil(&mut state);
        state.wave = milestone_wave(state.rank);
        state.milestone_boss_id = Some(1);
        state.enemies.push(Enemy {
            id: 1,
            kind: EnemyKind::FullMoonBoss,
            x: state.lantern.x,
            y: 50.0,
            hp: 0,
            max_hp: 505,
            hurt_flash: FlashTimer::new(),
            ranged_charge: None,
        });
        let max_before = state.camp.max_unlocked_rank;
        let kills = drain_dead_enemies(&mut state);
        apply_kills(&mut state, kills);

        assert!(state.dawn_reached_this_vigil);
        assert_eq!(state.camp.max_unlocked_rank, max_before + 1, "Dawnで次のランクが解放されるはず");
        assert_eq!(state.dawn_count, 1);
    }

    #[test]
    fn defeating_the_finale_boss_after_its_wave_has_passed_still_triggers_dawn() {
        // 最終ボスはHPが高く、湧いた波(300 tick)以内に倒しきれず次の波へ
        // 持ち越されることがある。waveの一致だけで判定すると、追いついて
        // 討伐した瞬間には既に次の波へ進んでおりDawnを取りこぼす回帰テスト。
        let mut state = EverlightState::new();
        start_vigil(&mut state);
        let milestone = milestone_wave(state.rank);
        state.milestone_boss_id = Some(1);
        state.wave = milestone + 1; // 討伐が次の波にずれ込んだ状況を再現する。
        state.enemies.push(Enemy {
            id: 1,
            kind: EnemyKind::FullMoonBoss,
            x: state.lantern.x,
            y: 50.0,
            hp: 0,
            max_hp: 505,
            hurt_flash: FlashTimer::new(),
            ranged_charge: None,
        });
        let max_before = state.camp.max_unlocked_rank;
        let kills = drain_dead_enemies(&mut state);
        apply_kills(&mut state, kills);

        assert!(state.dawn_reached_this_vigil, "waveが進んでいても、湧いた時のマイルストーンボスを倒せばDawnするはず");
        assert_eq!(state.camp.max_unlocked_rank, max_before + 1);
    }

    #[test]
    fn dawn_does_not_trigger_twice_in_the_same_vigil() {
        let mut state = EverlightState::new();
        start_vigil(&mut state);
        state.wave = milestone_wave(state.rank);
        state.milestone_boss_id = Some(1);
        state.dawn_reached_this_vigil = true;
        let max_before = state.camp.max_unlocked_rank;
        state.enemies.push(Enemy {
            id: 1,
            kind: EnemyKind::FullMoonBoss,
            x: state.lantern.x,
            y: 50.0,
            hp: 0,
            max_hp: 505,
            hurt_flash: FlashTimer::new(),
            ranged_charge: None,
        });
        let kills = drain_dead_enemies(&mut state);
        apply_kills(&mut state, kills);
        assert_eq!(state.camp.max_unlocked_rank, max_before, "同じ夜番で二重にDawnが確定してはいけない");
    }

    #[test]
    fn defeating_a_different_enemy_than_the_tracked_milestone_boss_does_not_trigger_dawn() {
        let mut state = EverlightState::new();
        start_vigil(&mut state);
        state.wave = milestone_wave(state.rank);
        // マイルストーンの最終ボス(id=2, まだ生存中)とは別の個体(id=1)を倒す。
        state.milestone_boss_id = Some(2);
        state.enemies.push(Enemy {
            id: 1,
            kind: EnemyKind::Boss,
            x: state.lantern.x,
            y: 50.0,
            hp: 0,
            max_hp: 385,
            hurt_flash: FlashTimer::new(),
            ranged_charge: None,
        });
        let max_before = state.camp.max_unlocked_rank;
        let kills = drain_dead_enemies(&mut state);
        apply_kills(&mut state, kills);
        assert_eq!(state.camp.max_unlocked_rank, max_before, "追跡中の個体と別の敵を倒してもDawnしないはず");
        assert!(!state.dawn_reached_this_vigil);
    }

    #[test]
    fn spawning_the_milestone_boss_records_its_id_for_dawn_tracking() {
        let mut state = EverlightState::new();
        start_vigil(&mut state);
        state.wave = milestone_wave(state.rank);
        assert!(state.milestone_boss_id.is_none());

        spawn_enemies(&mut state);

        let id = state.milestone_boss_id.expect("マイルストーン波では最終ボスのidが記録されるはず");
        let boss = state.enemies.iter().find(|e| e.id == id).expect("記録されたidの個体が盤面にいるはず");
        assert_eq!(boss.kind, boss_kind_for(state.wave, state.rank));
    }

    // ── 新しい敵種 ───────────────────────────────────────────────────

    #[test]
    fn shielded_enemy_takes_reduced_damage_from_non_weak_weapons() {
        let full = effective_damage_against(10, WeaponKind::Aurora, EnemyKind::Shielded);
        let weak = effective_damage_against(10, WeaponKind::Bolt, EnemyKind::Shielded);
        assert!(full < 10, "弱点以外の武器はダメージが軽減されるはず");
        assert_eq!(weak, 10, "弱点武器は軽減されないはず");
    }

    #[test]
    fn shielded_damage_reduction_does_not_apply_to_other_enemies() {
        assert_eq!(effective_damage_against(10, WeaponKind::Aurora, EnemyKind::Wisp), 10);
    }

    #[test]
    fn each_armored_variant_is_weak_to_a_different_weapon() {
        // 装甲バリアントが増えても弱点が1種に収束しないことを保証する —
        // 収束すると「弱点武器を切り替える判断」自体が消えてしまう。
        let weak_points: Vec<WeaponKind> = [EnemyKind::Shielded, EnemyKind::SprayShielded, EnemyKind::AuroraShielded]
            .iter()
            .map(|k| k.weak_to().expect("装甲系は弱点武器を持つはず"))
            .collect();
        let unique: std::collections::HashSet<_> = weak_points.iter().collect();
        assert_eq!(unique.len(), weak_points.len(), "装甲バリアントごとに弱点武器は異なるはず");
    }

    #[test]
    fn spray_shielded_and_aurora_shielded_take_reduced_damage_from_non_weak_weapons() {
        for (kind, weak) in [
            (EnemyKind::SprayShielded, WeaponKind::Spray),
            (EnemyKind::AuroraShielded, WeaponKind::Aurora),
        ] {
            let full = effective_damage_against(10, WeaponKind::Halo, kind);
            let reduced = effective_damage_against(10, weak, kind);
            assert!(full < 10, "{kind:?} は弱点以外の武器で軽減されるはず");
            assert_eq!(reduced, 10, "{kind:?} は弱点武器 {weak:?} で軽減されないはず");
        }
    }

    #[test]
    fn sniper_stops_advancing_once_it_reaches_the_stop_line() {
        let mut state = EverlightState::new();
        start_vigil(&mut state);
        state.enemies.push(Enemy {
            id: 1,
            kind: EnemyKind::Sniper,
            x: state.lantern.x,
            y: SNIPER_STOP_Y - 0.1,
            hp: 999,
            max_hp: 999,
            hurt_flash: FlashTimer::new(),
            ranged_charge: None,
        });
        move_enemies(&mut state);
        assert!(state.enemies[0].y >= SNIPER_STOP_Y, "停止線を越えたら以後は静止するはず");
        let y_after_stop = state.enemies[0].y;
        move_enemies(&mut state);
        assert_eq!(state.enemies[0].y, y_after_stop, "停止後はそれ以上進まないはず");
    }

    #[test]
    fn sniper_charges_then_damages_lantern_when_sharing_its_lane() {
        let mut state = EverlightState::new();
        start_vigil(&mut state);
        let lane_x = state.lantern.x;
        state.enemies.push(Enemy {
            id: 1,
            kind: EnemyKind::Sniper,
            x: lane_x,
            y: SNIPER_STOP_Y,
            hp: 999,
            max_hp: 999,
            hurt_flash: FlashTimer::new(),
            ranged_charge: None,
        });
        let light_before = state.lantern.light;
        for _ in 0..=SNIPER_CHARGE_TICKS {
            resolve_ranged_attacks(&mut state);
        }
        assert!(state.lantern.light < light_before, "同じレーンにいれば構え完了で被弾するはず");
    }

    #[test]
    fn sniper_misses_when_lantern_is_in_a_different_lane() {
        let mut state = EverlightState::new();
        start_vigil(&mut state);
        state.enemies.push(Enemy {
            id: 1,
            kind: EnemyKind::Sniper,
            x: 0.0,
            y: SNIPER_STOP_Y,
            hp: 999,
            max_hp: 999,
            hurt_flash: FlashTimer::new(),
            ranged_charge: None,
        });
        set_lantern_target_lane(&mut state, COLUMNS - 1);
        for _ in 0..40 {
            move_lantern(&mut state);
        }
        let light_before = state.lantern.light;
        for _ in 0..=SNIPER_CHARGE_TICKS {
            resolve_ranged_attacks(&mut state);
        }
        assert_eq!(state.lantern.light, light_before, "別レーンにいれば被弾しないはず");
    }

    // ── 詠唱者 (Caster) と敵弾 ───────────────────────────────────────

    #[test]
    fn caster_stops_advancing_much_earlier_than_the_sniper() {
        let mut state = EverlightState::new();
        start_vigil(&mut state);
        state.enemies.push(Enemy {
            id: 1,
            kind: EnemyKind::Caster,
            x: state.lantern.x,
            y: CASTER_STOP_Y - 0.1,
            hp: 999,
            max_hp: 999,
            hurt_flash: FlashTimer::new(),
            ranged_charge: None,
        });
        move_enemies(&mut state);
        assert!(state.enemies[0].y >= CASTER_STOP_Y, "停止線を越えたら以後は静止するはず");
        let y_after_stop = state.enemies[0].y;
        move_enemies(&mut state);
        assert_eq!(state.enemies[0].y, y_after_stop, "停止後はそれ以上進まないはず");
    }

    #[test]
    fn caster_fires_a_bullet_after_charging() {
        let mut state = EverlightState::new();
        start_vigil(&mut state);
        state.enemies.push(Enemy {
            id: 1,
            kind: EnemyKind::Caster,
            x: state.lantern.x,
            y: CASTER_STOP_Y,
            hp: 999,
            max_hp: 999,
            hurt_flash: FlashTimer::new(),
            ranged_charge: None,
        });
        assert!(state.enemy_bullets.is_empty());
        for _ in 0..=CASTER_FIRE_INTERVAL_TICKS {
            resolve_caster_shots(&mut state);
        }
        assert_eq!(state.enemy_bullets.len(), 1, "チャージが完了したら弾を1発撃つはず");
    }

    #[test]
    fn enemy_bullet_damages_the_lantern_when_it_reaches_the_same_lane() {
        let mut state = EverlightState::new();
        start_vigil(&mut state);
        let lantern_x = state.lantern.x;
        state.enemy_bullets.push(EnemyBullet {
            x: lantern_x,
            y: LANTERN_Y - 1.0,
            vx: 0.0,
            vy: 1.0,
            damage: 4,
            source: EnemyKind::Caster,
        });
        let light_before = state.lantern.light;
        move_and_resolve_enemy_bullets(&mut state);
        assert!(state.lantern.light < light_before, "同じレーンに届いた弾は灯を削るはず");
        assert!(state.enemy_bullets.is_empty(), "命中した弾は消費されるはず");
    }

    #[test]
    fn enemy_bullet_can_be_dodged_by_moving_out_of_its_lane_first() {
        // 「弾を見てから避ける」という詠唱者の存在意義そのものの回帰テスト。
        // 発射レーンから灯が離れていれば、弾が同じy帯へ届いても被弾しない。
        let mut state = EverlightState::new();
        start_vigil(&mut state);
        state.enemy_bullets.push(EnemyBullet {
            x: 0.0,
            y: LANTERN_Y - 1.0,
            vx: 0.0,
            vy: 1.0,
            damage: 4,
            source: EnemyKind::Caster,
        });
        set_lantern_target_lane(&mut state, COLUMNS - 1);
        for _ in 0..40 {
            move_lantern(&mut state);
        }
        let light_before = state.lantern.light;
        move_and_resolve_enemy_bullets(&mut state);
        assert_eq!(state.lantern.light, light_before, "灯が別レーンへ避けていれば被弾しないはず");
    }

    #[test]
    fn enemy_bullet_leaving_the_field_is_removed_without_a_hit() {
        let mut state = EverlightState::new();
        start_vigil(&mut state);
        state.enemy_bullets.push(EnemyBullet {
            x: state.lantern.x,
            y: WORLD_H + 15.0,
            vx: 0.0,
            vy: 1.0,
            damage: 4,
            source: EnemyKind::Caster,
        });
        let light_before = state.lantern.light;
        move_and_resolve_enemy_bullets(&mut state);
        assert_eq!(state.lantern.light, light_before);
        assert!(state.enemy_bullets.is_empty(), "画面外へ出た弾は消えるはず");
    }

    #[test]
    fn enemy_bullet_hit_log_names_the_actual_shooter() {
        // 命中ログは `EnemyBullet::source` の名前を反映するはず — 浮遊霊の
        // 弾が命中した時も正しく浮遊霊の名前で出ることの回帰テスト。
        let mut state = EverlightState::new();
        start_vigil(&mut state);
        let lantern_x = state.lantern.x;
        state.enemy_bullets.push(EnemyBullet {
            x: lantern_x,
            y: LANTERN_Y - 1.0,
            vx: 0.0,
            vy: 1.0,
            damage: 4,
            source: EnemyKind::Wraith,
        });
        move_and_resolve_enemy_bullets(&mut state);
        let log = state.visible_log().expect("命中したのでログが出るはず");
        assert!(log.contains(EnemyKind::Wraith.name()), "浮遊霊の弾なのに浮遊霊の名前が出ていない: {log}");
    }

    // ── 浮遊霊 (Wraith) ───────────────────────────────────────────────

    #[test]
    fn wraith_stops_advancing_between_the_caster_and_sniper_stop_lines() {
        let mut state = EverlightState::new();
        start_vigil(&mut state);
        state.enemies.push(Enemy {
            id: 1,
            kind: EnemyKind::Wraith,
            x: state.lantern.x,
            y: WRAITH_STOP_Y - 0.1,
            hp: 999,
            max_hp: 999,
            hurt_flash: FlashTimer::new(),
            ranged_charge: None,
        });
        move_enemies(&mut state);
        assert!(state.enemies[0].y >= WRAITH_STOP_Y, "停止線を越えたら以後y方向へは進まないはず");
        let y_after_stop = state.enemies[0].y;
        move_enemies(&mut state);
        assert_eq!(state.enemies[0].y, y_after_stop, "停止後はそれ以上近づかないはず");
    }

    #[test]
    fn wraith_sways_side_to_side_instead_of_homing_toward_the_lantern() {
        // 突進者のような一直線の接近でも、Husk/Bossのような`homes()`の
        // 灯への直行でもないことの回帰テスト — x座標が単調に灯へ寄る
        // のではなく、往復して初期位置の前後をまたぐはず。
        let mut state = EverlightState::new();
        start_vigil(&mut state);
        set_lantern_target_lane(&mut state, COLUMNS - 1);
        for _ in 0..40 {
            move_lantern(&mut state);
        }
        let spawn_x = super::super::state::lane_center_x(0);
        state.enemies.push(Enemy {
            id: 7,
            kind: EnemyKind::Wraith,
            x: spawn_x,
            y: WRAITH_STOP_Y,
            hp: 999,
            max_hp: 999,
            hurt_flash: FlashTimer::new(),
            ranged_charge: None,
        });
        let mut saw_above = false;
        let mut saw_below = false;
        for _ in 0..200 {
            move_enemies(&mut state);
            state.elapsed_ticks += 1;
            let x = state.enemies[0].x;
            if x > spawn_x {
                saw_above = true;
            }
            if x < spawn_x {
                saw_below = true;
            }
        }
        assert!(saw_above && saw_below, "横揺れなら初期x座標の両側を行き来するはず");
        assert!(
            state.enemies[0].x < state.lantern.x - 10.0,
            "灯のレーンへ直行するhomes()と違い、灯とは無関係な位置に留まり続けるはず"
        );
    }

    #[test]
    fn wraith_fires_a_bullet_after_charging() {
        let mut state = EverlightState::new();
        start_vigil(&mut state);
        state.enemies.push(Enemy {
            id: 1,
            kind: EnemyKind::Wraith,
            x: state.lantern.x,
            y: WRAITH_STOP_Y,
            hp: 999,
            max_hp: 999,
            hurt_flash: FlashTimer::new(),
            ranged_charge: None,
        });
        assert!(state.enemy_bullets.is_empty());
        for _ in 0..=WRAITH_FIRE_INTERVAL_TICKS {
            resolve_wraith_shots(&mut state);
        }
        assert_eq!(state.enemy_bullets.len(), 1, "チャージが完了したら弾を1発撃つはず");
        assert_eq!(state.enemy_bullets[0].source, EnemyKind::Wraith);
    }

    #[test]
    fn charger_accelerates_only_past_the_trigger_line() {
        let mut state = EverlightState::new();
        start_vigil(&mut state);
        state.enemies.push(Enemy {
            id: 1,
            kind: EnemyKind::Charger,
            x: state.lantern.x,
            y: CHARGER_TRIGGER_Y - 5.0,
            hp: 999,
            max_hp: 999,
            hurt_flash: FlashTimer::new(),
            ranged_charge: None,
        });
        move_enemies(&mut state);
        let step_before_trigger = state.enemies[0].y - (CHARGER_TRIGGER_Y - 5.0);

        state.enemies[0].y = CHARGER_TRIGGER_Y;
        let y_at_trigger = state.enemies[0].y;
        move_enemies(&mut state);
        let step_after_trigger = state.enemies[0].y - y_at_trigger;

        assert!(step_after_trigger > step_before_trigger * 2.0, "トリガー到達後は大きく加速するはず");
    }

    #[test]
    fn splitter_death_spawns_two_swarmlings_that_do_not_split_further() {
        let mut state = EverlightState::new();
        start_vigil(&mut state);
        state.enemies.push(Enemy {
            id: 1,
            kind: EnemyKind::Splitter,
            x: state.lantern.x,
            y: 50.0,
            hp: 0,
            max_hp: 14,
            hurt_flash: FlashTimer::new(),
            ranged_charge: None,
        });
        let kills = drain_dead_enemies(&mut state);
        apply_kills(&mut state, kills);
        assert_eq!(state.enemies.len(), 2, "分裂体の死亡で子が2体湧くはず");
        assert!(state.enemies.iter().all(|e| e.kind == EnemyKind::Swarmling), "子は羽虫として湧くはず");

        // 子 (Swarmling) を倒しても、さらに分裂体の子は湧かない。
        for e in state.enemies.iter_mut() {
            e.hp = 0;
        }
        let kills = drain_dead_enemies(&mut state);
        apply_kills(&mut state, kills);
        assert!(state.enemies.is_empty(), "羽虫はさらに分裂しないはず");
    }

    // ── ボスの浮遊 (ふよふよ) ─────────────────────────────────────────

    #[test]
    fn boss_sways_side_to_side_via_the_bob_even_without_homing() {
        // 大蛇は`homes()`=falseなので、この揺れが「灯へのhoming」ではなく
        // 独立した浮遊(bob)由来であることを切り分けて検証できる。
        let mut state = EverlightState::new();
        start_vigil(&mut state);
        let spawn_x = super::super::state::lane_center_x(4);
        state.enemies.push(Enemy {
            id: 3,
            kind: EnemyKind::Serpent,
            x: spawn_x,
            y: 20.0,
            hp: 999,
            max_hp: 999,
            hurt_flash: FlashTimer::new(),
            ranged_charge: None,
        });
        let mut saw_above = false;
        let mut saw_below = false;
        for _ in 0..60 {
            move_enemies(&mut state);
            state.elapsed_ticks += 1;
            let x = state.enemies[0].x;
            if x > spawn_x {
                saw_above = true;
            }
            if x < spawn_x {
                saw_below = true;
            }
        }
        assert!(saw_above && saw_below, "ふよふよ揺れるなら初期x座標の両側を行き来するはず");
    }

    #[test]
    fn boss_bob_does_not_prevent_forward_progress_toward_the_breach() {
        let mut state = EverlightState::new();
        start_vigil(&mut state);
        let start_y = 20.0;
        state.enemies.push(Enemy {
            id: 5,
            kind: EnemyKind::Boss,
            x: state.lantern.x,
            y: start_y,
            hp: 999,
            max_hp: 999,
            hurt_flash: FlashTimer::new(),
            ranged_charge: None,
        });
        for _ in 0..60 {
            move_enemies(&mut state);
            state.elapsed_ticks += 1;
        }
        assert!(state.enemies[0].y > start_y + 10.0, "浮遊で揺れても全体としては着実に前進するはず");
    }

    // ── ボスの構え中攻撃 ───────────────────────────────────────────────

    #[test]
    fn shadow_witch_telegraph_threatens_two_lanes_simultaneously() {
        let mut state = EverlightState::new();
        start_vigil(&mut state);
        state.enemies.push(Enemy {
            id: 1,
            kind: EnemyKind::ShadowWitch,
            x: state.lantern.x,
            y: 50.0,
            hp: 999,
            max_hp: 999,
            hurt_flash: FlashTimer::new(),
            ranged_charge: None,
        });
        state.elapsed_ticks = boss_attack_period_ticks(state.rank);
        resolve_boss_telegraph(&mut state);
        let telegraph = state.boss_telegraph.as_ref().expect("影の魔女は構えを取るはず");
        assert_eq!(telegraph.lane_xs.len(), 2, "影の魔女は2レーン同時に警告するはず");
    }

    #[test]
    fn serpent_telegraph_never_gets_stuck_at_arena_edges() {
        // 端のレーンで外向きの方向を引くと、`lane_index_of` のクランプで
        // 毎tick同じレーンへ戻され「動く警告」が実際には静止して見えて
        // しまう回帰テスト。端では必ず内向きになることを確認する。
        let mut left = EverlightState::new();
        start_vigil(&mut left);
        let telegraph =
            new_boss_telegraph(EnemyKind::Serpent, 1, super::super::state::lane_center_x(0), &mut left.rng_state);
        assert_eq!(telegraph.sweep_direction, Some(1), "左端では右向き固定のはず");

        let mut right = EverlightState::new();
        start_vigil(&mut right);
        let telegraph = new_boss_telegraph(
            EnemyKind::Serpent,
            1,
            super::super::state::lane_center_x(COLUMNS - 1),
            &mut right.rng_state,
        );
        assert_eq!(telegraph.sweep_direction, Some(-1), "右端では左向き固定のはず");
    }

    #[test]
    fn serpent_telegraph_lane_moves_while_warning() {
        let mut state = EverlightState::new();
        start_vigil(&mut state);
        state.enemies.push(Enemy {
            id: 1,
            kind: EnemyKind::Serpent,
            x: state.lantern.x,
            y: 50.0,
            hp: 999,
            max_hp: 999,
            hurt_flash: FlashTimer::new(),
            ranged_charge: None,
        });
        state.elapsed_ticks = boss_attack_period_ticks(state.rank);
        resolve_boss_telegraph(&mut state);
        let start_x = state.boss_telegraph.as_ref().unwrap().lane_xs[0];
        for _ in 0..SWEEP_STEP_TICKS {
            resolve_boss_telegraph(&mut state);
        }
        let moved_x = state.boss_telegraph.as_ref().expect("構え中のはず").lane_xs[0];
        assert_ne!(start_x, moved_x, "大蛇の警告レーンは構え中に移動するはず");
    }

    #[test]
    fn regular_spawn_table_gates_new_enemy_kinds_by_wave() {
        let early = regular_spawn_table(1);
        for kind in [
            EnemyKind::Sniper,
            EnemyKind::Caster,
            EnemyKind::Shielded,
            EnemyKind::Charger,
            EnemyKind::Splitter,
            EnemyKind::SprayShielded,
            EnemyKind::AuroraShielded,
            EnemyKind::Wraith,
        ] {
            assert!(!early.iter().any(|&(k, _)| k == kind), "第1波では{kind:?}はまだ出ないはず");
        }

        let late = regular_spawn_table(30);
        for kind in [
            EnemyKind::Sniper,
            EnemyKind::Caster,
            EnemyKind::Shielded,
            EnemyKind::Charger,
            EnemyKind::Splitter,
            EnemyKind::SprayShielded,
            EnemyKind::AuroraShielded,
            EnemyKind::Wraith,
        ] {
            assert!(late.iter().any(|&(k, _)| k == kind), "第30波では{kind:?}が出るはず");
        }
    }

    #[test]
    fn regular_spawn_batch_size_grows_with_wave_then_caps() {
        assert_eq!(regular_spawn_batch_size(1), 1, "序盤は同時湧き数が1のままのはず");
        assert_eq!(regular_spawn_batch_size(SWARM_RAMP_WAVE), 1, "物量が増え始める波でもまだ1のはず");
        assert!(
            regular_spawn_batch_size(SWARM_RAMP_WAVE + SWARM_WAVE_STEP) > 1,
            "物量が増え始める波を過ぎたら同時湧き数が増えるはず"
        );
        assert_eq!(
            regular_spawn_batch_size(1000),
            SWARM_MAX_BATCH,
            "同時湧き数は上限で頭打ちになるはず (敵数上限や処理負荷の暴走を防ぐため)"
        );
    }

    #[test]
    fn boss_telegraph_damage_and_period_scale_with_rank() {
        assert!(boss_telegraph_damage(2) > boss_telegraph_damage(1), "高ランクほど一撃が重くなるはず");
        assert!(boss_attack_period_ticks(2) < boss_attack_period_ticks(1), "高ランクほど攻撃間隔は短くなるはず");
    }

    #[test]
    fn killing_the_telegraphing_boss_cancels_the_attack_even_if_another_boss_kind_survives() {
        // チェックポイントの間隔(5波)より討伐が遅れると、種類の異なるボスが
        // 同時に生存しうる。この時「敵種が生きているか」ではなく「構えた
        // "その個体" が生きているか」で不発判定しないと、既に倒したボスの
        // 攻撃が誤って命中してしまう回帰テスト。
        let mut state = EverlightState::new();
        start_vigil(&mut state);
        let x = state.lantern.x;
        state.enemies.push(Enemy {
            id: 1,
            kind: EnemyKind::Boss,
            x,
            y: 50.0,
            hp: 999,
            max_hp: 999,
            hurt_flash: FlashTimer::new(),
            ranged_charge: None,
        });
        state.enemies.push(Enemy {
            id: 2,
            kind: EnemyKind::ShadowWitch,
            x,
            y: 60.0,
            hp: 999,
            max_hp: 999,
            hurt_flash: FlashTimer::new(),
            ranged_charge: None,
        });
        state.elapsed_ticks = boss_attack_period_ticks(state.rank);
        resolve_boss_telegraph(&mut state);
        let telegraph = state.boss_telegraph.as_ref().expect("先頭に登録された魔王(id=1)が構えるはず");
        assert_eq!(telegraph.source_enemy_id, 1);

        // 構えた魔王(id=1)だけを討伐する。影の魔女(id=2)は生き残る。
        state.enemies.retain(|e| e.id != 1);
        let light_before = state.lantern.light;

        resolve_boss_telegraph(&mut state);

        assert!(state.boss_telegraph.is_none(), "構えた本体が死んだので不発になるはず");
        assert_eq!(
            state.lantern.light, light_before,
            "別種のボスが生きているだけで誤って命中してはいけない"
        );
    }

    #[test]
    fn boss_bullet_and_summon_periods_scale_with_rank_independently_of_the_telegraph() {
        assert!(boss_bullet_period_ticks(2) < boss_bullet_period_ticks(1), "高ランクほど実体弾の間隔は短くなるはず");
        assert!(boss_summon_period_ticks(2) < boss_summon_period_ticks(1), "高ランクほど召喚の間隔は短くなるはず");
        assert_ne!(
            boss_bullet_period_ticks(1),
            boss_attack_period_ticks(1),
            "実体弾は灯喰らいと同じ周期にならないはず (重なりっぱなしを避ける)"
        );
    }

    #[test]
    fn boss_fires_a_bullet_aimed_at_the_lantern_on_its_own_period() {
        let mut state = EverlightState::new();
        start_vigil(&mut state);
        // 灯とは別レーンに配置し、弾が「発射時の灯位置」を狙って直進する
        // (詠唱者/浮遊霊のような縦落下ではない) ことを速度の向きで確認する。
        set_lantern_target_lane(&mut state, COLUMNS - 1);
        for _ in 0..40 {
            move_lantern(&mut state);
        }
        state.enemies.push(Enemy {
            id: 1,
            kind: EnemyKind::Boss,
            x: super::super::state::lane_center_x(0),
            y: 50.0,
            hp: 999,
            max_hp: 999,
            hurt_flash: FlashTimer::new(),
            ranged_charge: None,
        });
        state.elapsed_ticks = boss_bullet_period_ticks(state.rank);
        assert!(state.enemy_bullets.is_empty());
        resolve_boss_bullets(&mut state);
        assert_eq!(state.enemy_bullets.len(), 1, "周期に到達したら実体弾を1発撃つはず");
        let bullet = &state.enemy_bullets[0];
        assert_eq!(bullet.source, EnemyKind::Boss);
        assert!(bullet.vx > 0.0, "灯が右側にいるので右向きに直進するはず (vx={})", bullet.vx);
    }

    #[test]
    fn shadow_witch_summons_two_swarmlings_on_its_own_period() {
        let mut state = EverlightState::new();
        start_vigil(&mut state);
        state.enemies.push(Enemy {
            id: 1,
            kind: EnemyKind::ShadowWitch,
            x: state.lantern.x,
            y: 50.0,
            hp: 999,
            max_hp: 999,
            hurt_flash: FlashTimer::new(),
            ranged_charge: None,
        });
        state.elapsed_ticks = boss_summon_period_ticks(state.rank);
        assert_eq!(state.enemies.len(), 1);
        resolve_boss_summons(&mut state);
        assert_eq!(state.enemies.len(), 3, "ボス本体+召喚された羽虫2体になるはず");
        assert_eq!(
            state.enemies.iter().filter(|e| e.kind == EnemyKind::Swarmling).count(),
            2,
            "召喚される個体は羽虫のはず"
        );
    }

    #[test]
    fn boss_bullets_and_summons_do_not_fire_off_period() {
        let mut state = EverlightState::new();
        start_vigil(&mut state);
        state.enemies.push(Enemy {
            id: 1,
            kind: EnemyKind::FullMoonBoss,
            x: state.lantern.x,
            y: 50.0,
            hp: 999,
            max_hp: 999,
            hurt_flash: FlashTimer::new(),
            ranged_charge: None,
        });
        state.enemies.push(Enemy {
            id: 2,
            kind: EnemyKind::Serpent,
            x: state.lantern.x,
            y: 50.0,
            hp: 999,
            max_hp: 999,
            hurt_flash: FlashTimer::new(),
            ranged_charge: None,
        });
        state.elapsed_ticks = boss_bullet_period_ticks(state.rank) + 1;
        resolve_boss_bullets(&mut state);
        resolve_boss_summons(&mut state);
        assert!(state.enemy_bullets.is_empty(), "周期のtickでなければ実体弾は撃たないはず");
        assert_eq!(state.enemies.len(), 2, "周期のtickでなければ召喚しないはず");
    }
}
