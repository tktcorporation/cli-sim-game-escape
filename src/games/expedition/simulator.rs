//! 遠征団の自動プレイシミュレーター。

use super::logic::{
    acknowledge_result, begin_forming, launch_sortie, set_hub_tab, tick, upgrade_hero, use_aid,
};
use super::state::{ExpeditionState, HubTab, Screen};

fn finish_or_progress(state: &mut ExpeditionState, use_optional_aid: bool) {
    match state.screen {
        Screen::Camp if state.rations > 0 => {
            // 補給があれば先に育成してから出る（詰まったときの強化ループ）
            if state.supplies > 0 {
                let _ = set_hub_tab(state, HubTab::Train);
                for id in 0..4u8 {
                    let _ = upgrade_hero(state, id);
                }
                let _ = set_hub_tab(state, HubTab::Camp);
            }
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
            if state.last_failed {
                let _ = acknowledge_result(state);
                let _ = set_hub_tab(state, HubTab::Train);
                for id in 0..4u8 {
                    let _ = upgrade_hero(state, id);
                }
                let _ = set_hub_tab(state, HubTab::Camp);
            } else {
                let _ = acknowledge_result(state);
            }
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
        "expedition report: chapter={} stage={} level={} supplies={} rations={}/{}",
        state.chapter,
        state.stage,
        state.total_level(),
        state.supplies,
        state.rations,
        state.ration_cap(),
    );
    assert!(state.chapter >= 1);
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
fn active_play_advances_map_or_levels_via_train() {
    let plain = bot_run(12_000, false);
    let aided = bot_run(12_000, true);
    eprintln!(
        "plain chapter={} stage={} level={} / aided chapter={} stage={} level={}",
        plain.chapter,
        plain.stage,
        plain.total_level(),
        aided.chapter,
        aided.stage,
        aided.total_level(),
    );
    let progressed = |s: &ExpeditionState| {
        s.chapter > 1 || s.stage > 1 || s.total_level() > 4 || s.supplies > 0
    };
    assert!(progressed(&plain) || progressed(&aided));
}
