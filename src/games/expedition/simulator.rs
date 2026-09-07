//! 遠征団の自動プレイ・バランス用シミュレータ。

use super::logic::{
    acknowledge_result, confirm_placement, drop_medal, launch_sortie, pick_level_hero,
    primary_depart, set_hub_tab, tick,
};
use super::state::{ExpeditionState, HubTab, Screen, ORB_NEED, PUSH_W};

fn finish_or_progress(state: &mut ExpeditionState) {
    match state.screen {
        Screen::Camp => {
            if state.medals > 0 || state.orb_gauge > 0 {
                let _ = set_hub_tab(state, HubTab::Arcade);
                for _ in 0..40 {
                    if state.pending_level_pick {
                        let id = (state.elapsed_ticks % 4) as u8;
                        let _ = pick_level_hero(state, id);
                        break;
                    }
                    if state.medals == 0 {
                        break;
                    }
                    let lane = (state.elapsed_ticks as usize) % PUSH_W;
                    if !drop_medal(state, lane) {
                        break;
                    }
                    // 押しを進める
                    tick(state, 8);
                }
                let _ = set_hub_tab(state, HubTab::Camp);
            }
            let _ = primary_depart(state);
            if state.screen == Screen::Placing {
                let _ = confirm_placement(state);
            }
        }
        Screen::Forming => {
            let _ = launch_sortie(state);
            if state.screen == Screen::Placing {
                let _ = confirm_placement(state);
            }
        }
        Screen::Placing => {
            let _ = confirm_placement(state);
        }
        Screen::Running => {}
        Screen::Result => {
            let failed = state.last_failed;
            let _ = acknowledge_result(state);
            if failed {
                let _ = set_hub_tab(state, HubTab::Arcade);
                for _ in 0..30 {
                    if state.pending_level_pick {
                        let id = (state.elapsed_ticks % 4) as u8;
                        let _ = pick_level_hero(state, id);
                        break;
                    }
                    if state.medals == 0 {
                        break;
                    }
                    let lane = (state.elapsed_ticks as usize) % PUSH_W;
                    let _ = drop_medal(state, lane);
                    tick(state, 6);
                }
                let _ = set_hub_tab(state, HubTab::Camp);
            }
        }
    }
}

fn bot_run(ticks: u32) -> ExpeditionState {
    let mut state = ExpeditionState::new();
    for _ in 0..ticks {
        finish_or_progress(&mut state);
        tick(&mut state, 1);
    }
    state
}

#[test]
fn long_run_never_panics_and_keeps_ration_bounds() {
    let state = bot_run(8_000);
    eprintln!(
        "expedition report: chapter={} stage={} level={} medals={} orbs={}/{} rations={}/{}",
        state.chapter,
        state.stage,
        state.total_level(),
        state.medals,
        state.orb_gauge,
        ORB_NEED,
        state.rations,
        state.ration_cap()
    );
    assert!(state.rations <= state.ration_cap());
    assert!(state.chapter >= 1);
    assert!((1..=4).contains(&state.stage));
}

#[test]
fn idle_only_does_not_increase_level() {
    let mut state = ExpeditionState::new();
    let before = state.total_level();
    for _ in 0..5_000 {
        tick(&mut state, 1);
    }
    assert_eq!(state.total_level(), before);
}

#[test]
fn active_play_advances_map_or_levels_via_arcade() {
    let state = bot_run(12_000);
    let progressed = |s: &ExpeditionState| {
        s.chapter > 1 || s.stage > 1 || s.total_level() > 4 || s.medals > 0 || s.orb_gauge > 0
    };
    assert!(progressed(&state), "bot should progress map, medals, or levels");
}
