//! 周回討伐 — ループ状の道に地形タイルを配置し、勇者が自動で周回して
//! 戦うローグライト。地形は資源(木材/石材/魂)を生むが敵の湧きや強さも
//! 上げる、諸刃の剣として機能する。
//!
//! コアループ:
//!   1. 拠点で恒久強化 (魂で購入) を整えて遠征へ出発
//!   2. 手札の地形カードを道に配置しながら、勇者が自動周回して戦う
//!   3. 死亡すると拠点へ撤退。木材/石材と道の配置は失うが、魂と拠点強化は残る

pub mod actions;
pub mod logic;
pub mod render;
pub mod save;
pub mod state;

#[cfg(test)]
mod simulator;

use std::cell::RefCell;
use std::rc::Rc;

use ratzilla::ratatui::layout::Rect;
use ratzilla::ratatui::Frame;

use crate::games::{Game, GameChoice};
use crate::input::{ClickState, InputEvent};
use crate::widgets::ClickableGrid;

use actions::*;
use logic::UpgradeKind;
use state::{Phase, HAND_MAX, RING_W};

/// 拠点画面のスクロール1クリックあたりの行数。
const CAMP_SCROLL_STEP: i32 = 2;

pub struct LoopMarchGame {
    pub state: state::LoopMarchState,
    save_countdown: u32,
}

impl Default for LoopMarchGame {
    fn default() -> Self {
        Self::new()
    }
}

impl LoopMarchGame {
    pub fn new() -> Self {
        let state = state::LoopMarchState::new();

        #[cfg(target_arch = "wasm32")]
        let state = {
            let mut s = state;
            save::load_game(&mut s);
            s
        };

        Self {
            state,
            save_countdown: save::AUTOSAVE_INTERVAL,
        }
    }

    fn handle_click(&mut self, action_id: u16) -> bool {
        match action_id {
            CAMP_UPGRADE_MAX_HP => {
                logic::purchase_upgrade(&mut self.state, UpgradeKind::MaxHp);
                true
            }
            CAMP_UPGRADE_ATTACK => {
                logic::purchase_upgrade(&mut self.state, UpgradeKind::Attack);
                true
            }
            CAMP_UPGRADE_EXTRA_CARD => {
                logic::purchase_upgrade(&mut self.state, UpgradeKind::ExtraCard);
                true
            }
            CAMP_START_OR_RESUME => {
                logic::start_or_resume_expedition(&mut self.state);
                true
            }
            REFILL_HAND => {
                logic::refill_hand(&mut self.state);
                true
            }
            GO_TO_CAMP => {
                logic::go_to_camp(&mut self.state);
                true
            }
            CAMP_SCROLL_UP => {
                self.state.scroll_camp(-CAMP_SCROLL_STEP);
                true
            }
            CAMP_SCROLL_DOWN => {
                self.state.scroll_camp(CAMP_SCROLL_STEP);
                true
            }
            id if (HAND_CLICK_BASE..HAND_CLICK_BASE + HAND_MAX as u16).contains(&id) => {
                logic::select_hand(&mut self.state, (id - HAND_CLICK_BASE) as usize);
                true
            }
            id if id >= PATH_CLICK_BASE => {
                if let Some((gx, gy)) = ClickableGrid::decode(PATH_CLICK_BASE, RING_W, id) {
                    if let Some(path_index) = logic::ring_index_at(gx, gy) {
                        logic::place_selected(&mut self.state, path_index);
                    }
                }
                true
            }
            _ => false,
        }
    }

    fn handle_key(&mut self, key: char) -> bool {
        match self.state.phase {
            Phase::Camp => match key {
                '1' => {
                    logic::purchase_upgrade(&mut self.state, UpgradeKind::MaxHp);
                    true
                }
                '2' => {
                    logic::purchase_upgrade(&mut self.state, UpgradeKind::Attack);
                    true
                }
                '3' => {
                    logic::purchase_upgrade(&mut self.state, UpgradeKind::ExtraCard);
                    true
                }
                ' ' | 's' => {
                    logic::start_or_resume_expedition(&mut self.state);
                    true
                }
                _ => false,
            },
            Phase::Expedition => match key {
                '1' | '2' | '3' | '4' => {
                    let idx = (key as u8 - b'1') as usize;
                    logic::select_hand(&mut self.state, idx);
                    true
                }
                'r' => {
                    logic::refill_hand(&mut self.state);
                    true
                }
                'c' => {
                    logic::go_to_camp(&mut self.state);
                    true
                }
                _ => false,
            },
        }
    }
}

impl Game for LoopMarchGame {
    fn choice(&self) -> GameChoice {
        GameChoice::LoopMarch
    }

    fn handle_input(&mut self, event: &InputEvent) -> bool {
        match event {
            InputEvent::Key(c) => self.handle_key(*c),
            InputEvent::Click(_, id) => self.handle_click(*id),
        }
    }

    fn tick(&mut self, delta_ticks: u32) {
        let was_run_active = self.state.run_active;
        logic::tick_n(&mut self.state, delta_ticks);

        // 死亡直後(run_active: true→false)はタイマーを待たず即セーブする。
        // 「死んでも魂は残る」が核となる約束なので、その直後にリロード/
        // タブを閉じられても失われないようにする。
        let died_this_tick = was_run_active && !self.state.run_active;

        self.save_countdown = self.save_countdown.saturating_sub(delta_ticks);
        if self.save_countdown == 0 || died_this_tick {
            #[cfg(target_arch = "wasm32")]
            save::save_game(&self.state);
            self.save_countdown = save::AUTOSAVE_INTERVAL;
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

    fn click(id: u16) -> InputEvent {
        InputEvent::Click(ClickScope::Game(GameChoice::LoopMarch), id)
    }

    #[test]
    fn new_game_starts_in_camp() {
        let game = LoopMarchGame::new();
        assert_eq!(game.state.phase, Phase::Camp);
    }

    #[test]
    fn click_start_expedition_switches_phase() {
        let mut game = LoopMarchGame::new();
        game.handle_input(&click(CAMP_START_OR_RESUME));
        assert_eq!(game.state.phase, Phase::Expedition);
    }

    #[test]
    fn key_space_in_camp_starts_expedition() {
        let mut game = LoopMarchGame::new();
        game.handle_input(&InputEvent::Key(' '));
        assert_eq!(game.state.phase, Phase::Expedition);
    }

    #[test]
    fn hand_click_selects_card() {
        let mut game = LoopMarchGame::new();
        game.handle_input(&click(CAMP_START_OR_RESUME));
        game.handle_input(&click(HAND_CLICK_BASE));
        assert_eq!(game.state.selected_hand, Some(0));
    }

    #[test]
    fn path_click_places_selected_card() {
        let mut game = LoopMarchGame::new();
        game.handle_input(&click(CAMP_START_OR_RESUME));
        game.handle_input(&click(HAND_CLICK_BASE));
        // ring_index_at(0, 0) == Some(0) (リング先頭 = 矩形の左上)。
        let action_id = PATH_CLICK_BASE; // gx=0, gy=0
        game.handle_input(&click(action_id));
        assert!(game.state.path[0].terrain.is_some());
    }

    #[test]
    fn path_click_on_interior_cell_is_noop() {
        let mut game = LoopMarchGame::new();
        game.handle_input(&click(CAMP_START_OR_RESUME));
        game.handle_input(&click(HAND_CLICK_BASE));
        // (1,1) はリング内部 (道ではない)。RING_W=8 なので id = base + 1*8 + 1
        let action_id = PATH_CLICK_BASE + RING_W as u16 + 1;
        let consumed = game.handle_input(&click(action_id));
        assert!(consumed, "内部セルのクリックもイベント自体は消費する");
        assert_eq!(game.state.selected_hand, Some(0), "内部クリックでは配置されない");
    }

    #[test]
    fn tick_advances_game_logic() {
        let mut game = LoopMarchGame::new();
        game.handle_input(&click(CAMP_START_OR_RESUME));
        game.tick(state::MOVE_TICKS);
        assert_eq!(game.state.hero.position, 1);
    }

    #[test]
    fn death_forces_immediate_save_countdown_reset() {
        let mut game = LoopMarchGame::new();
        game.handle_input(&click(CAMP_START_OR_RESUME));
        game.save_countdown = save::AUTOSAVE_INTERVAL + 1000; // タイマーにはまだ遠い
        game.state.hero.attack = 0;
        game.state.hero.hp = 1;
        game.state.hero.position = 0;
        game.state.path[0].monster = Some(state::Monster {
            terrain: state::Terrain::Graveyard,
            hp: 100,
            max_hp: 100,
            attack: 999,
            elite: false,
        });

        game.tick(1);

        assert_eq!(game.state.phase, Phase::Camp, "この tick で死亡しているはず");
        assert_eq!(
            game.save_countdown,
            save::AUTOSAVE_INTERVAL,
            "死亡直後はタイマー未満でも即セーブ扱いになる"
        );
    }

    #[test]
    fn unknown_click_id_not_consumed() {
        let mut game = LoopMarchGame::new();
        // 9 は CAMP_*/CAMP_SCROLL_* (1-8) にも HAND_CLICK_BASE.. (10-13) にも
        // PATH_CLICK_BASE.. (100+) にも属さない未使用の隙間。
        assert!(!game.handle_input(&click(9)));
    }

    /// id >= PATH_CLICK_BASE の範囲は ClickableGrid の登録先 (100..131) に
    /// 限られるはずだが、handle_click 自体は factory と同じ規約で
    /// 「decode に失敗しても click イベントとしては消費する」。
    #[test]
    fn out_of_range_grid_id_is_still_consumed_but_ignored() {
        let mut game = LoopMarchGame::new();
        game.handle_input(&click(CAMP_START_OR_RESUME));
        let before = game.state.selected_hand;
        assert!(game.handle_input(&click(9999)));
        assert_eq!(game.state.selected_hand, before);
    }
}
