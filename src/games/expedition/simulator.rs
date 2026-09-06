//! 遠征団の自動プレイシミュレーター。

use super::logic::{
    acknowledge_result, begin_forming, choose_push, choose_rest, launch_sortie, tick,
};
use super::state::{ExpeditionState, Screen};

fn finish_or_progress(state: &mut ExpeditionState, prefer_push: bool) {
    match state.screen {
        Screen::Camp if state.rations > 0 => {
            let _ = begin_forming(state);
            let _ = launch_sortie(state, state.scout_memos > 0);
        }
        Screen::Forming => {
            let _ = launch_sortie(state, state.scout_memos > 0);
        }
        Screen::Running => tick(state, 1),
        Screen::Choice => {
            if prefer_push {
                let _ = choose_push(state);
            } else {
                let _ = choose_rest(state);
            }
        }
        Screen::Result => {
            let _ = acknowledge_result(state);
        }
        Screen::Camp => tick(state, 1),
    }
}

fn bot_run(ticks: u32, prefer_push: bool) -> ExpeditionState {
    let mut state = ExpeditionState::new();
    for _ in 0..ticks {
        finish_or_progress(&mut state, prefer_push);
    }
    state
}

#[test]
fn long_run_never_panics_and_keeps_ration_bounds() {
    let state = bot_run(30_000, false);
    eprintln!(
        "expedition report: depth={} bond={} rations={}/{} ticks={}",
        state.best_depth,
        state.total_bond(),
        state.rations,
        state.ration_cap(),
        state.elapsed_ticks
    );
    assert!(state.best_depth >= 1);
    assert!(state.rations <= state.ration_cap());
}

#[test]
fn idle_only_does_not_increase_bond() {
    let mut state = ExpeditionState::new();
    let before = state.total_bond();
    tick(&mut state, 20_000);
    assert_eq!(state.total_bond(), before);
    assert!(state.rations > 0);
}

#[test]
fn active_play_grows_bond() {
    let rest = bot_run(12_000, false);
    let push = bot_run(12_000, true);
    eprintln!(
        "rest bond={} depth={} / push bond={} depth={}",
        rest.total_bond(),
        rest.best_depth,
        push.total_bond(),
        push.best_depth
    );
    assert!(rest.total_bond() > 0 || push.total_bond() > 0);
}
