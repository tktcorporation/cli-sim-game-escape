//! 周回討伐 シミュレーションランナー。
//!
//! 長時間の自動プレイでパニックが起きないこと、状態の不変条件
//! (HPの範囲・配列長など) が壊れないこと、および死亡率・所要時間のような
//! 統計的なバランスが意図した範囲に収まっていることを検証する。
//!
//! バランス調整の下調べや効果測定には
//! `cargo test loopmarch::simulator -- --nocapture` でレポートを見ながら
//! `simulate_time_to_first_refill` の `hp_override`/`attack_override` を
//! 変えて感度を確認するとよい。

#![cfg(test)]

use super::logic::{self, UpgradeKind};
use super::state::{LoopMarchState, Monster, Phase, Terrain, HAND_MAX, PATH_LEN, TERRAIN_TIER_MAX};

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

/// 「盤面が埋まった後も投資先が尽きない」設計 (地形強化tier) を実際に
/// 使い切る自動プレイ方策。手札の地形と一致する既存タイルがあれば重ね置き
/// (強化) を優先し、無ければ従来通りランダムな空きマスへ広げる。
fn auto_play_tick_with_tier_stacking(state: &mut LoopMarchState, seed: &mut u32) {
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
                let terrain = state.hand[hand_index].unwrap();
                logic::select_hand(state, hand_index);
                let upgrade_target = state
                    .path
                    .iter()
                    .position(|slot| slot.terrain == Some(terrain) && slot.tier < TERRAIN_TIER_MAX);
                let target = upgrade_target.unwrap_or((*seed as usize) % PATH_LEN);
                logic::place_selected(state, target);
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
/// 初回プレイの挙動を大量のシード試行でシミュレートし、初回の手札補充
/// (木材3+石材3) 成立までの所要tick数と、それ以前に死亡してしまった数を返す。
///
/// `hp_override`/`attack_override` は勇者ステータスを変えた場合の感度を
/// 見たい時に使う (バランス調整時の効果測定用、通常は `None`)。
fn simulate_time_to_first_refill(
    runs: u32,
    hp_override: Option<i32>,
    attack_override: Option<i32>,
) -> (Vec<u32>, u32) {
    use super::logic::start_or_resume_expedition;

    const MAX_TICKS: u32 = 20_000;

    let mut ticks_to_success = Vec::new();
    let mut died_before_success = 0u32;

    for seed in 1..=runs {
        let mut s = LoopMarchState::new();
        s.rng_state = seed;
        start_or_resume_expedition(&mut s);
        if let Some(hp) = hp_override {
            s.hero.max_hp = hp;
            s.hero.hp = hp;
        }
        if let Some(atk) = attack_override {
            s.hero.attack = atk;
        }
        for (i, slot) in [0usize, 1, 2].into_iter().enumerate() {
            logic::select_hand(&mut s, i);
            logic::place_selected(&mut s, slot);
        }

        for t in 0..MAX_TICKS {
            let was_run_active = s.run_active;
            logic::tick(&mut s);
            if was_run_active && !s.run_active {
                died_before_success += 1;
                break;
            }
            if s.wood >= 3 && s.stone >= 3 {
                ticks_to_success.push(t);
                break;
            }
        }
    }

    ticks_to_success.sort_unstable();
    (ticks_to_success, died_before_success)
}

/// `cargo test loopmarch::simulator::first_refill_balance_report --
/// --nocapture` で確認できる、初回補充バランスの人間向けレポート。
#[test]
fn first_refill_balance_report() {
    const RUNS: u32 = 1000;
    let (ticks_to_success, died) = simulate_time_to_first_refill(RUNS, None, None);
    let median = ticks_to_success
        .get(ticks_to_success.len() / 2)
        .copied()
        .unwrap_or(0);
    let p90 = ticks_to_success
        .get(ticks_to_success.len() * 90 / 100)
        .copied()
        .unwrap_or(0);
    eprintln!(
        "[first_refill] runs={RUNS} success={} died_before_success={died}({:.1}%) median={median}tick({:.1}s) p90={p90}tick({:.1}s)",
        ticks_to_success.len(),
        died as f64 / RUNS as f64 * 100.0,
        median as f64 / 10.0,
        p90 as f64 / 10.0,
    );
}

/// 初回の手札補充成立前に死亡してしまう割合が高すぎないことを検証する。
/// ここが高すぎると「補充できたことがない」という体感になる —
/// 敵性能を調整した際の回帰を検知するための基準値。
#[test]
fn first_refill_is_reachable_without_excessive_death_rate() {
    const RUNS: u32 = 1000;
    let (_, died_before_success) = simulate_time_to_first_refill(RUNS, None, None);

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
        tier: 0,
        cluster_bonus: 0,
    });

    logic::tick(&mut state);

    assert_eq!(state.phase, Phase::Camp);
    assert_eq!(state.soul, 10, "死亡で魂が減ってはいけない");
}

/// 遠征開始からの経過tick列を舐めて `run_active: true→false` の回数
/// (=死亡回数) を数える。`auto_play_tick` 系の関数は死亡直後の Camp フェーズ
/// で即座に次の遠征を開始し直すため、呼び出し前後の状態だけでは検知できない
/// (Camp分岐がtick()より先に走り、同じ呼び出し内で false→true に戻ってしまう)。
/// 呼び出し後の状態を1回ごとに記録し、隣接する2回分を比較することで
/// 死亡イベントを取りこぼさず数える。
fn count_deaths(mut tick_fn: impl FnMut(&mut LoopMarchState, &mut u32), ticks: u32, seed: u32) -> u32 {
    let mut state = LoopMarchState::new();
    let mut s = seed;
    let mut prev_active = false;
    let mut deaths = 0u32;
    for _ in 0..ticks {
        tick_fn(&mut state, &mut s);
        let now_active = state.run_active;
        if prev_active && !now_active {
            deaths += 1;
        }
        prev_active = now_active;
    }
    deaths
}

/// 地形強化tierを使い切る自動プレイでも、長時間パニックせず・tierが
/// 上限を超えないことを検証する (盤面が埋まった後の投資先という設計の
/// 骨子そのものの回帰テスト)。
#[test]
fn tier_stacking_strategy_never_panics_and_respects_tier_cap() {
    let mut state = LoopMarchState::new();
    let mut seed = 7u32;

    for _ in 0..20_000 {
        auto_play_tick_with_tier_stacking(&mut state, &mut seed);

        assert!(state.hero.hp >= 0, "HPが負になってはいけない");
        assert!(state.hero.hp <= state.hero.max_hp, "HPが最大値を超えてはいけない");
        assert!(
            state.path.iter().all(|slot| slot.tier <= TERRAIN_TIER_MAX),
            "tierが上限を超えてはいけない"
        );
    }

    assert!(
        state.path.iter().any(|slot| slot.tier > 0),
        "重ね置き戦略を続ければ、少なくとも1マスは強化されているはず"
    );
}

/// `cargo test loopmarch::simulator::tier_stacking_report -- --nocapture` で
/// 確認できる、地形強化tier導入前後のバランス比較レポート。
#[test]
fn tier_stacking_report() {
    const RUNS: u32 = 300;
    const TICKS: u32 = 20_000;

    let mut spread_lap = 0u64;
    let mut spread_soul = 0u64;
    let mut spread_deaths = 0u64;
    let mut stack_lap = 0u64;
    let mut stack_soul = 0u64;
    let mut stack_deaths = 0u64;

    for seed in 1..=RUNS {
        let mut spread = LoopMarchState::new();
        let mut s1 = seed;
        let mut d1 = 0u64;
        let mut prev1 = false;
        for _ in 0..TICKS {
            auto_play_tick(&mut spread, &mut s1);
            if prev1 && !spread.run_active {
                d1 += 1;
            }
            prev1 = spread.run_active;
        }
        spread_lap += spread.best_lap as u64;
        spread_soul += spread.soul as u64;
        spread_deaths += d1;

        let mut stack = LoopMarchState::new();
        let mut s2 = seed;
        let mut d2 = 0u64;
        let mut prev2 = false;
        for _ in 0..TICKS {
            auto_play_tick_with_tier_stacking(&mut stack, &mut s2);
            if prev2 && !stack.run_active {
                d2 += 1;
            }
            prev2 = stack.run_active;
        }
        stack_lap += stack.best_lap as u64;
        stack_soul += stack.soul as u64;
        stack_deaths += d2;
    }

    eprintln!(
        "[tier_stacking] runs={RUNS} ticks={TICKS}\n  spread: avg_best_lap={:.1} avg_soul={:.1} avg_deaths={:.1}\n  stack : avg_best_lap={:.1} avg_soul={:.1} avg_deaths={:.1}",
        spread_lap as f64 / RUNS as f64,
        spread_soul as f64 / RUNS as f64,
        spread_deaths as f64 / RUNS as f64,
        stack_lap as f64 / RUNS as f64,
        stack_soul as f64 / RUNS as f64,
        stack_deaths as f64 / RUNS as f64,
    );
}

/// 重ね置き戦略 (高tierの敵と戦い続ける) が、素朴な拡散戦略と比べて
/// 極端に多く死ぬわけではないことを検証する。tier倍率は報酬と敵の強さを
/// 同時に引き上げる設計だが、リスクがリターンに見合わずプレイヤーを
/// 選択肢ごと殺すバランスになっていないかの回帰基準。
///
/// 閾値は絶対値の勘ではなく、同じ実行内で計測した拡散戦略の死亡率との
/// 相対比で判定する — 絶対値基準だと「両戦略とも一律に危険になった」
/// ような回帰を見逃す一方、閾値を厳しくしすぎると無関係な変動でも
/// 頻繁に落ちてしまうため、両戦略間のバランスが崩れた時だけ検知したい。
#[test]
fn tier_stacking_death_rate_is_not_excessive_relative_to_spread() {
    const RUNS: u32 = 60;
    const TICKS: u32 = 20_000;

    let mut spread_total = 0u64;
    let mut stack_total = 0u64;
    for seed in 1..=RUNS {
        spread_total += count_deaths(auto_play_tick, TICKS, seed) as u64;
        stack_total += count_deaths(auto_play_tick_with_tier_stacking, TICKS, seed) as u64;
    }
    let spread_avg = spread_total as f64 / RUNS as f64;
    let stack_avg = stack_total as f64 / RUNS as f64;

    assert!(
        stack_avg <= spread_avg * 1.5,
        "重ね置き戦略の死亡率が拡散戦略に対して高すぎる: spread={spread_avg:.1} stack={stack_avg:.1} (runs={RUNS})"
    );
}
