//! つぶ牧場 (Tsubu Ranch) — 育成・コレクション・自動対戦の放置ゲーム。
//!
//! コアループ:
//!   1. 個体は tick 毎に成長し、Lv `state::MATURE_LEVEL` 以上で成熟する
//!   2. 成熟個体がいれば一定確率+食料で増殖する
//!   3. 同種の成熟個体が閾値数集まると確率で次階層の種に進化する (餌やりの蓄積が分岐先に影響)
//!   4. 対戦チームに編成した種の最強個体の合計ステータスで、ステージの敵と自動的に戦う

pub mod actions;
pub mod logic;
pub mod render;
pub mod save;
pub mod state;

use std::cell::RefCell;
use std::rc::Rc;

use ratzilla::ratatui::layout::Rect;
use ratzilla::ratatui::Frame;

use crate::games::{Game, GameChoice};
use crate::input::{ClickState, InputEvent};
use crate::sound;

use actions::PlayerAction;
use state::RanchState;

pub struct RanchGame {
    pub state: RanchState,
    save_countdown: u32,
}

/// この `PlayerAction` を適用したらセーブを発火させるか。
fn is_save_worthy(action: PlayerAction) -> bool {
    matches!(
        action,
        PlayerAction::Feed(_) | PlayerAction::UpgradeCapacity | PlayerAction::ToggleTeamMember(_)
    )
}

impl RanchGame {
    pub fn new() -> Self {
        #[allow(unused_mut)]
        let mut state = RanchState::new();

        #[cfg(target_arch = "wasm32")]
        if save::load_game(&mut state) {
            state.add_log("セーブデータをロードしました");
        }

        Self {
            state,
            save_countdown: save::AUTOSAVE_INTERVAL,
        }
    }

    fn flush_save(&mut self) {
        #[cfg(target_arch = "wasm32")]
        save::save_game(&self.state);
        self.save_countdown = save::AUTOSAVE_INTERVAL;
    }
}

impl Default for RanchGame {
    fn default() -> Self {
        Self::new()
    }
}

impl Game for RanchGame {
    fn choice(&self) -> GameChoice {
        GameChoice::Ranch
    }

    fn handle_input(&mut self, event: &InputEvent) -> bool {
        let action = match event {
            InputEvent::Key(ch) => actions::action_for_key(*ch, self.state.tab),
            InputEvent::Click(_, id) => actions::action_for_click(*id),
        };
        let Some(action) = action else {
            return false;
        };

        let save_after = is_save_worthy(action);
        let ok = logic::apply_action(&mut self.state, action);

        match action {
            PlayerAction::Feed(_) => sound::play(if ok { sound::PURCHASE } else { sound::ERROR }),
            PlayerAction::UpgradeCapacity => {
                sound::play(if ok { sound::ENHANCE } else { sound::ERROR })
            }
            PlayerAction::ToggleTeamMember(_) | PlayerAction::SetTab(_) => {
                sound::play(sound::CLICK)
            }
            PlayerAction::ScrollUp | PlayerAction::ScrollDown => {}
        }

        if save_after && ok {
            self.flush_save();
        }
        true
    }

    fn tick(&mut self, delta_ticks: u32) {
        let prev_stage = self.state.stage;
        logic::tick(&mut self.state, delta_ticks);

        self.save_countdown = self.save_countdown.saturating_sub(delta_ticks);
        let event_save = self.state.stage != prev_stage;
        if event_save || self.save_countdown == 0 {
            self.flush_save();
        }
    }

    fn render(&self, f: &mut Frame, area: Rect, click_state: &Rc<RefCell<ClickState>>) {
        render::render(&self.state, f, area, click_state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::ClickScope;
    use actions::{FEED_BASE, TAB_BATTLE, TAB_DEX, TAB_HABITAT, TOGGLE_TEAM_BASE, UPGRADE_CAPACITY};
    use state::{Affinity, Species, Tab};

    fn click(id: u16) -> InputEvent {
        InputEvent::Click(ClickScope::Game(GameChoice::Ranch), id)
    }

    #[test]
    fn create_game() {
        let g = RanchGame::new();
        assert_eq!(g.state.stage, 1);
        assert_eq!(g.state.tab, Tab::Habitat);
    }

    #[test]
    fn click_tab_switch() {
        let mut g = RanchGame::new();
        g.handle_input(&click(TAB_DEX));
        assert_eq!(g.state.tab, Tab::Dex);
        g.handle_input(&click(TAB_BATTLE));
        assert_eq!(g.state.tab, Tab::Battle);
        g.handle_input(&click(TAB_HABITAT));
        assert_eq!(g.state.tab, Tab::Habitat);
    }

    #[test]
    fn click_feed_consumes_food() {
        let mut g = RanchGame::new();
        g.state.food = 1000;
        let before = g.state.food;
        g.handle_input(&click(FEED_BASE + Affinity::Aqua.index() as u16));
        assert!(g.state.food < before);
    }

    #[test]
    fn click_upgrade_capacity_when_affordable() {
        let mut g = RanchGame::new();
        g.state.food = 1_000_000;
        let cap_before = g.state.capacity();
        g.handle_input(&click(UPGRADE_CAPACITY));
        assert!(g.state.capacity() > cap_before);
    }

    #[test]
    fn click_toggle_team_member() {
        let mut g = RanchGame::new();
        g.handle_input(&click(TOGGLE_TEAM_BASE + Species::Tsubu.index() as u16));
        assert_eq!(g.state.team[0], Some(Species::Tsubu));
    }

    #[test]
    fn unknown_click_is_not_consumed() {
        let mut g = RanchGame::new();
        assert!(!g.handle_input(&click(65000)));
    }

    #[test]
    fn tick_advances_total_ticks() {
        let mut g = RanchGame::new();
        g.tick(3);
        assert_eq!(g.state.total_ticks, 3);
    }

    #[test]
    fn timer_save_fires_after_autosave_interval() {
        let mut g = RanchGame::new();
        assert_eq!(g.save_countdown, save::AUTOSAVE_INTERVAL);
        g.tick(save::AUTOSAVE_INTERVAL);
        assert_eq!(g.save_countdown, save::AUTOSAVE_INTERVAL);
    }

    /// 収容数拡張はイベントセーブ発火 → タイマー満タンに戻る。
    #[test]
    fn event_save_resets_timer_to_avoid_double_write() {
        let mut g = RanchGame::new();
        g.state.food = 1_000_000;
        g.tick(100);
        assert_eq!(g.save_countdown, save::AUTOSAVE_INTERVAL - 100);
        g.handle_input(&click(UPGRADE_CAPACITY));
        assert_eq!(g.save_countdown, save::AUTOSAVE_INTERVAL);
    }

    #[test]
    fn save_worthy_actions_classified_correctly() {
        assert!(is_save_worthy(PlayerAction::Feed(Affinity::Aqua)));
        assert!(is_save_worthy(PlayerAction::UpgradeCapacity));
        assert!(is_save_worthy(PlayerAction::ToggleTeamMember(Species::Tsubu)));
        assert!(!is_save_worthy(PlayerAction::SetTab(Tab::Dex)));
        assert!(!is_save_worthy(PlayerAction::ScrollUp));
    }

    #[test]
    fn key_feed_shortcut_on_habitat_tab() {
        let mut g = RanchGame::new();
        g.state.food = 1000;
        let before = g.state.food;
        g.handle_input(&InputEvent::Key('1'));
        assert!(g.state.food < before);
    }
}
