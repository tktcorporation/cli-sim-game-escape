//! 深淵潜行 (Abyss Idle) — 自動戦闘でフロアを潜っていく放置型ローグ。
//!
//! コアループ:
//!   1. 勇者が現フロアの敵と自動戦闘
//!   2. 雑魚 8 体を倒すとボス出現 → 撃破で次フロアへ
//!   3. gold で永続強化、魂で死亡しても残るバフを購入
//!   4. 死亡すると B1F に戻されるが、強化はそのまま残る
//!
//! 戦略性: 自動潜行 ON で深く潜るほどリスクとリターンが増す。
//! OFF にすれば現フロアで安定して周回し gold を稼げる。

pub mod actions;
pub mod config;
pub mod logic;
pub mod policy;
pub mod render;
pub mod state;

#[cfg(test)]
mod simulator;

use std::cell::RefCell;
use std::rc::Rc;

use ratzilla::ratatui::layout::Rect;
use ratzilla::ratatui::Frame;

use crate::games::{Game, GameChoice};
use crate::input::{ClickState, InputEvent};

use actions::*;
use policy::PlayerAction;
use state::{AbyssState, SoulPerk, Tab, UpgradeKind};

pub struct AbyssGame {
    pub state: AbyssState,
}

impl AbyssGame {
    pub fn new() -> Self {
        Self {
            state: AbyssState::new(),
        }
    }

    /// クリック ID を `PlayerAction` に変換する。シミュレータ Policy も同じ
    /// `PlayerAction` を返すので、本体・sim どちらも `logic::apply_action`
    /// 1 本道で処理される (動作の乖離はここで構造的に防ぐ)。
    fn click_to_action(&self, action_id: u16) -> Option<PlayerAction> {
        match action_id {
            TAB_UPGRADES => Some(PlayerAction::SetTab(Tab::Upgrades)),
            TAB_SOULS => Some(PlayerAction::SetTab(Tab::Souls)),
            TAB_STATS => Some(PlayerAction::SetTab(Tab::Stats)),
            TOGGLE_AUTO_DESCEND => Some(PlayerAction::ToggleAutoDescend),
            RETREAT_TO_SURFACE => Some(PlayerAction::Retreat),
            id if (BUY_UPGRADE_BASE..BUY_UPGRADE_BASE + 7).contains(&id) => {
                let idx = (id - BUY_UPGRADE_BASE) as usize;
                UpgradeKind::from_index(idx).map(PlayerAction::BuyUpgrade)
            }
            id if (BUY_SOUL_PERK_BASE..BUY_SOUL_PERK_BASE + 4).contains(&id) => {
                let idx = (id - BUY_SOUL_PERK_BASE) as usize;
                SoulPerk::from_index(idx).map(PlayerAction::BuySoulPerk)
            }
            _ => None,
        }
    }

    fn key_to_action(&self, ch: char) -> Option<PlayerAction> {
        match ch {
            '{' => Some(PlayerAction::SetTab(Tab::Upgrades)),
            '|' => Some(PlayerAction::SetTab(Tab::Souls)),
            '}' => Some(PlayerAction::SetTab(Tab::Stats)),
            'a' | 'A' => Some(PlayerAction::ToggleAutoDescend),
            'p' | 'P' => Some(PlayerAction::Retreat),
            '1'..='7' if matches!(self.state.tab, Tab::Upgrades) => {
                let idx = (ch as u8 - b'1') as usize;
                UpgradeKind::from_index(idx).map(PlayerAction::BuyUpgrade)
            }
            'q' | 'w' | 'e' | 'r' if matches!(self.state.tab, Tab::Souls) => {
                let idx = match ch {
                    'q' => 0,
                    'w' => 1,
                    'e' => 2,
                    'r' => 3,
                    _ => unreachable!(),
                };
                SoulPerk::from_index(idx).map(PlayerAction::BuySoulPerk)
            }
            _ => None,
        }
    }
}

impl Game for AbyssGame {
    fn choice(&self) -> GameChoice {
        GameChoice::Abyss
    }

    fn handle_input(&mut self, event: &InputEvent) -> bool {
        let action = match event {
            InputEvent::Key(c) => self.key_to_action(*c),
            InputEvent::Click(_, id) => self.click_to_action(*id),
        };
        if let Some(a) = action {
            logic::apply_action(&mut self.state, a);
            true
        } else {
            false
        }
    }

    fn tick(&mut self, delta_ticks: u32) {
        logic::tick(&mut self.state, delta_ticks);
    }

    fn render(&self, f: &mut Frame, area: Rect, click_state: &Rc<RefCell<ClickState>>) {
        render::render(&self.state, f, area, click_state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::ClickScope;

    /// Build a `Click` event scoped to this game.
    fn click(id: u16) -> InputEvent {
        InputEvent::Click(ClickScope::Game(GameChoice::Abyss), id)
    }

    #[test]
    fn create_game() {
        let g = AbyssGame::new();
        assert_eq!(g.state.floor, 1);
    }

    #[test]
    fn click_tab_switch() {
        let mut g = AbyssGame::new();
        g.handle_input(&click(TAB_SOULS));
        assert_eq!(g.state.tab, Tab::Souls);
        g.handle_input(&click(TAB_STATS));
        assert_eq!(g.state.tab, Tab::Stats);
        g.handle_input(&click(TAB_UPGRADES));
        assert_eq!(g.state.tab, Tab::Upgrades);
    }

    #[test]
    fn key_buy_upgrade_only_in_upgrades_tab() {
        let mut g = AbyssGame::new();
        g.state.gold = 1000;
        // タブ Souls なら反応しない
        g.state.tab = Tab::Souls;
        g.handle_input(&InputEvent::Key('1'));
        assert_eq!(g.state.upgrades[UpgradeKind::Sword.index()], 0);
        // タブ Upgrades なら買える
        g.state.tab = Tab::Upgrades;
        g.handle_input(&InputEvent::Key('1'));
        assert_eq!(g.state.upgrades[UpgradeKind::Sword.index()], 1);
    }

    #[test]
    fn click_buy_upgrade_works_regardless_of_tab() {
        let mut g = AbyssGame::new();
        g.state.gold = 1000;
        // タブが Souls でもクリックなら反応
        g.state.tab = Tab::Souls;
        g.handle_input(&click(BUY_UPGRADE_BASE));
        assert_eq!(g.state.upgrades[UpgradeKind::Sword.index()], 1);
    }

    #[test]
    fn toggle_auto_descend_via_key() {
        let mut g = AbyssGame::new();
        let before = g.state.auto_descend;
        g.handle_input(&InputEvent::Key('a'));
        assert_ne!(g.state.auto_descend, before);
    }

    #[test]
    fn buy_soul_perk_via_key() {
        let mut g = AbyssGame::new();
        g.state.tab = Tab::Souls;
        g.state.souls = 100;
        g.handle_input(&InputEvent::Key('q'));
        assert_eq!(g.state.soul_perks[SoulPerk::Might.index()], 1);
    }

    #[test]
    fn tick_advances_combat() {
        let mut g = AbyssGame::new();
        g.tick(1);
        assert!(g.state.current_enemy.max_hp > 0);
    }

    #[test]
    fn retreat_via_key() {
        let mut g = AbyssGame::new();
        g.state.floor = 5;
        g.handle_input(&InputEvent::Key('p'));
        assert_eq!(g.state.floor, 1);
    }
}
