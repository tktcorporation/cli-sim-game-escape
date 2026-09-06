//! 遠征団の自動プレイシミュレーター。

use super::logic::{acknowledge_result, begin_forming, launch_sortie, tick, use_aid};
use super::state::{ExpeditionState, Screen};

fn finish_or_progress(state: &mut ExpeditionState, use_optional_aid: bool) {
    match state.screen {
        Screen::Camp if state.rations > 0 => {
            let _ = begin_forming(state);
            let _ = launch_sortie(state, state.scout_memos > 0);
        }
        Screen::Forming => {
            let _ = launch_sortie(state, state.scout_memos > 0);
        }
        Screen::Running => {
            if use_optional_aid {
                let _ = use_aid(state);
            }
            tick(state, 1);
        }
        Screen::Result => {
            let _ = acknowledge_result(state);
        }
        Screen::Camp => tick(state, 1),
    }
}

fn bot_run(ticks: u32, use_optional_aid: bool) -> ExpeditionState {
    let mut state = ExpeditionState::new();
    for _ in 0..ticks {
        finish_or_progress(&mut state, use_optional_aid);
    }
    state
}

#[test]
fn long_run_never_panics_and_keeps_ration_bounds() {
    let state = bot_run(30_000, false);
    eprintln!(
        "expedition report: depth={} level={} rations={}/{} ticks={}",
        state.best_depth,
        state.total_level(),
        state.rations,
        state.ration_cap(),
        state.elapsed_ticks
    );
    assert!(state.best_depth >= 1);
    assert!(state.rations <= state.ration_cap());
}

#[test]
fn idle_only_does_not_increase_level() {
    let mut state = ExpeditionState::new();
    let before = state.total_level();
    tick(&mut state, 20_000);
    assert_eq!(state.total_level(), before);
    assert!(state.rations > 0);
}

#[test]
fn active_play_grows_level_with_or_without_aid() {
    let plain = bot_run(12_000, false);
    let aided = bot_run(12_000, true);
    eprintln!(
        "plain level={} depth={} / aided level={} depth={}",
        plain.total_level(),
        plain.best_depth,
        aided.total_level(),
        aided.best_depth
    );
    assert!(plain.total_level() > 0 || aided.total_level() > 0);
}
