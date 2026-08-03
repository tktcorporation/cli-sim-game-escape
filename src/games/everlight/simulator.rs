//! 常夜灯 シミュレーションランナー。
//!
//! 長時間の自動プレイでパニックが起きないこと、状態の不変条件 (灯の範囲・
//! 座標の範囲など) が壊れないこと、および平均到達波数のような統計的な
//! バランスが意図した範囲に収まっていることを検証する。
//!
//! バランス調整の下調べには `cargo test everlight::simulator -- --nocapture`
//! でレポートを見ながら数値を調整するとよい。

#![cfg(test)]

use super::logic;
use super::state::{EverlightState, Phase, COLUMNS, MAX_LEVEL, WORLD_W};

/// 拠点では買える強化を買い切ってから出陣し、夜番中はレベルアップを
/// 常に1番目の選択肢で受け取り、5tickごとに最も敵が密集しているレーン
/// (宝箱があればそちらを優先) へ灯を寄せる、という単純な自動プレイ方策。
fn auto_play_tick(state: &mut EverlightState, tick_index: u64) {
    match state.phase {
        Phase::Camp => {
            while logic::purchase_light(state) || logic::purchase_power(state) || logic::purchase_extra_slot(state) {}
            logic::start_vigil(state);
        }
        Phase::Vigil => {
            if state.pending_boons.is_some() {
                logic::choose_boon(state, 0);
            } else if tick_index.is_multiple_of(5) {
                reposition_lantern(state);
            }
        }
    }
    logic::tick(state);
}

fn lane_of(x: f64) -> usize {
    let lane_w = WORLD_W / COLUMNS as f64;
    ((x / lane_w) as usize).min(COLUMNS - 1)
}

fn reposition_lantern(state: &mut EverlightState) {
    if let Some(chest) = state.chests.first() {
        logic::set_lantern_target_lane(state, lane_of(chest.x));
        return;
    }
    let mut counts = [0u32; COLUMNS];
    for e in &state.enemies {
        counts[lane_of(e.x)] += 1;
    }
    if let Some((lane, _)) = counts.iter().enumerate().max_by_key(|&(_, c)| *c) {
        logic::set_lantern_target_lane(state, lane);
    }
}

#[test]
fn long_run_never_panics_and_keeps_invariants() {
    let mut state = EverlightState::new();

    for i in 0..20_000u64 {
        auto_play_tick(&mut state, i);

        assert!(state.lantern.light >= 0, "灯が負になってはいけない");
        assert!(state.lantern.light <= state.lantern.light_max, "灯が最大値を超えてはいけない");
        assert!(
            (0.0..=WORLD_W).contains(&state.lantern.x),
            "灯のx座標が戦場の外に出た: {}",
            state.lantern.x
        );
        assert!(state.enemies.len() <= 200, "敵の数が上限を超えて増え続けている: {}", state.enemies.len());
        assert!(state.projectiles.len() <= 300, "弾の数が上限を超えて増え続けている: {}", state.projectiles.len());
        for w in &state.loadout.weapons {
            assert!(w.level >= 1 && w.level <= MAX_LEVEL, "武器レベルが範囲外: {}", w.level);
        }
    }

    assert!(state.best_wave >= 1 || state.ember > 0, "20000tick回しても何も進行していない");
}

/// 1回の夜番 (拠点で出陣してから灯が消えるまで) をシミュレートし、
/// 到達した波数と生存tick数を返す。無限ループ防止に `MAX_TICKS` で
/// 打ち切り、その場合も打ち切り時点の記録を返す。
fn simulate_one_vigil(seed: u32) -> (u32, u64) {
    const MAX_TICKS: u64 = 20_000;
    let mut state = EverlightState::new();
    state.rng_state = seed;
    // 直前の永続進行 (ember/camp) は毎回リセットしたシングルランで測る —
    // 恒久強化を積み上げた終盤ではなく「初回プレイの体感」を計測したいため。

    let mut i = 0u64;
    while state.phase != Phase::Vigil && i < 10 {
        auto_play_tick(&mut state, i);
        i += 1;
    }

    for t in 0..MAX_TICKS {
        auto_play_tick(&mut state, t);
        if state.phase == Phase::Camp {
            return (state.best_wave, state.best_survival_ticks);
        }
    }
    (state.wave, state.elapsed_ticks)
}

#[test]
fn average_survival_reaches_at_least_wave_three() {
    const RUNS: u32 = 40;
    let mut total_wave = 0u64;
    for seed in 1..=RUNS {
        let (wave, _) = simulate_one_vigil(seed);
        total_wave += wave as u64;
    }
    let avg = total_wave as f64 / RUNS as f64;
    assert!(
        avg >= 3.0,
        "初回プレイの平均到達波数が低すぎる (詰みに近い): avg_wave={avg:.2} (runs={RUNS})"
    );
}

/// `cargo test everlight::simulator::survival_balance_report -- --nocapture`
/// で確認できる、初回プレイの生存バランスの人間向けレポート。
#[test]
fn survival_balance_report() {
    const RUNS: u32 = 60;
    let mut waves = Vec::with_capacity(RUNS as usize);
    let mut survival_secs = Vec::with_capacity(RUNS as usize);
    for seed in 1..=RUNS {
        let (wave, ticks) = simulate_one_vigil(seed);
        waves.push(wave);
        survival_secs.push(ticks as f64 / 10.0);
    }
    waves.sort_unstable();
    survival_secs.sort_by(|a, b| a.partial_cmp(b).unwrap());

    let median_wave = waves[waves.len() / 2];
    let p10_wave = waves[waves.len() / 10];
    let median_secs = survival_secs[survival_secs.len() / 2];

    eprintln!(
        "[everlight/survival] runs={RUNS} median_wave={median_wave} p10_wave={p10_wave} median_survival={median_secs:.1}s"
    );
}

/// レベルアップ選択肢が実際に武器/効果の獲得へつながっているかの回帰
/// テスト。ここが壊れると「宝箱を取っても何も強くならない」体感になる。
#[test]
fn boons_are_actually_acquired_over_a_long_vigil() {
    let mut state = EverlightState::new();
    logic::start_vigil(&mut state);
    for t in 0..6_000u64 {
        if state.phase != Phase::Vigil {
            break;
        }
        if state.pending_boons.is_some() {
            logic::choose_boon(&mut state, 0);
        } else if t.is_multiple_of(5) {
            reposition_lantern(&mut state);
        }
        logic::tick(&mut state);
    }
    let total_levels: u32 = state.loadout.weapons.iter().map(|w| w.level).sum::<u32>()
        + state.loadout.passives.iter().map(|p| p.level).sum::<u32>();
    assert!(
        state.loadout.weapons.len() + state.loadout.passives.len() > 1 || total_levels > 1,
        "宝箱を取り続けても装備が全く増減しなかった"
    );
}

/// 拠点の恒久強化が複数回の夜番を経て実際に積み上がることを確認する
/// (「投資が無意味」になっていないかの回帰)。
#[test]
fn camp_upgrades_accumulate_across_multiple_vigils() {
    let mut state = EverlightState::new();
    for i in 0..30_000u64 {
        auto_play_tick(&mut state, i);
    }
    let total_camp_levels = state.camp.light_level + state.camp.power_level + state.camp.extra_slot_level;
    assert!(total_camp_levels > 0, "30000tick経っても拠点強化が一度も購入されていない");
}
