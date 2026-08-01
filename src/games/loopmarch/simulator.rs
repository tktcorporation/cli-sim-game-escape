//! 周回討伐 シミュレーションランナー。
//!
//! 長時間の自動プレイでパニックが起きないこと、状態の不変条件
//! (HPの範囲・配列長など) が壊れないことを検証する。

#![cfg(test)]

use super::logic::{self, UpgradeKind};
use super::state::{LoopMarchState, Monster, Phase, Terrain, HAND_MAX, PATH_LEN};

/// 単純な自動プレイ方策: 拠点では買えるものを買って出発し、遠征中は
/// 手札を空いている道に置き続け、資源があれば補充する。
fn auto_play_tick(state: &mut LoopMarchState, seed: &mut u32) {
    match state.phase {
        Phase::Camp => {
            logic::purchase_upgrade(state, UpgradeKind::MaxHp);
            logic::purchase_upgrade(state, UpgradeKind::Attack);
            logic::purchase_upgrade(state, UpgradeKind::ExtraCard);
            logic::start_or_resume_expedition(state);
        }
        Phase::Expedition => {
            *seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            if let Some(hand_index) = state.hand.iter().position(|c| c.is_some()) {
                logic::select_hand(state, hand_index);
                let path_index = (*seed as usize) % PATH_LEN;
                logic::place_selected(state, path_index);
            }
            logic::refill_hand(state);
        }
    }
    logic::tick(state);
}

#[test]
fn long_run_never_panics_and_keeps_invariants() {
    let mut state = LoopMarchState::new();
    let mut seed = 42u32;

    for _ in 0..20_000 {
        auto_play_tick(&mut state, &mut seed);

        assert!(state.hero.hp >= 0, "HPが負になってはいけない");
        assert!(
            state.hero.hp <= state.hero.max_hp,
            "HPが最大値を超えてはいけない"
        );
        assert_eq!(state.path.len(), PATH_LEN);
        assert_eq!(state.hand.len(), HAND_MAX);
        assert!(state.hero.position < PATH_LEN);
    }

    assert!(
        state.best_lap >= 1 || state.soul > 0,
        "20000tick回しても何も進行していない"
    );
}

/// 初期手札(森/岩山/ランダム)を隣接3マスにそのまま置く、という典型的な
/// 初回プレイの挙動で、初回の手札補充(木材3+石材3)成立前に死亡してしまう
/// 割合を大量のシード試行で測る。ここが高すぎると「補充できたことがない」
/// という体感になる — 敵性能を調整した際の回帰を検知するための基準値。
#[test]
fn first_refill_is_reachable_without_excessive_death_rate() {
    use super::logic::start_or_resume_expedition;

    const RUNS: u32 = 1000;
    const MAX_TICKS: u32 = 20_000;

    let mut died_before_success = 0u32;

    for seed in 1..=RUNS {
        let mut s = LoopMarchState::new();
        s.rng_state = seed;
        start_or_resume_expedition(&mut s);
        for (i, slot) in [0usize, 1, 2].into_iter().enumerate() {
            logic::select_hand(&mut s, i);
            logic::place_selected(&mut s, slot);
        }

        for _ in 0..MAX_TICKS {
            let was_run_active = s.run_active;
            logic::tick(&mut s);
            if was_run_active && !s.run_active {
                died_before_success += 1;
                break;
            }
            if s.wood >= 3 && s.stone >= 3 {
                break;
            }
        }
    }

    let death_rate = died_before_success as f64 / RUNS as f64;
    assert!(
        death_rate < 0.15,
        "初回補充成立前の死亡率が高すぎる: {:.1}% (died={died_before_success}/{RUNS})",
        death_rate * 100.0
    );
}

#[test]
fn death_does_not_reduce_persistent_soul() {
    // handle_death が永続資源 (魂) に誤って触れていないかの回帰テスト。
    let mut state = LoopMarchState::new();
    logic::start_or_resume_expedition(&mut state);
    state.soul = 10;
    state.hero.attack = 0;
    state.hero.hp = 1;
    state.hero.position = 0;
    state.path[0].monster = Some(Monster {
        terrain: Terrain::Graveyard,
        hp: 100,
        max_hp: 100,
        attack: 999,
        elite: false,
    });

    logic::tick(&mut state);

    assert_eq!(state.phase, Phase::Camp);
    assert_eq!(state.soul, 10, "死亡で魂が減ってはいけない");
}
