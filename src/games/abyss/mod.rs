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
pub mod effects;
pub mod logic;
pub mod policy;
pub mod render;
pub mod state;

#[cfg(test)]
mod simulator;

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use ratzilla::ratatui::layout::Rect;
use ratzilla::ratatui::Frame;

use crate::games::{Game, GameChoice};
use crate::input::{ClickState, InputEvent};

/// performance.now() の薄いラッパ。失敗時 (headless 等) は None を返す。
fn now_ms() -> Option<f64> {
    web_sys::window().and_then(|w| w.performance()).map(|p| p.now())
}

use actions::*;
use effects::AbyssEffects;
use policy::PlayerAction;
use state::{AbyssState, SoulPerk, Tab, UpgradeKind};

pub struct AbyssGame {
    pub state: AbyssState,
    /// 演出マネージャ。render 内で効果を push し、process_effects で適用する。
    /// `Game::render(&self, ...)` が immutable なので RefCell 必須。
    effects: RefCell<AbyssEffects>,
    /// 前フレームの state スナップショット (差分検知用)。
    /// Copy 可能なフィールドだけ保持する軽量スナップショット。
    prev: Cell<PrevSnapshot>,
    /// 前回 render 時の wall-clock (ms)。effect の elapsed 計算に使う。
    last_render_ms: Cell<f64>,
}

/// effect トリガ判定用の軽量 state スナップショット。Copy なフィールドだけ。
///
/// 「rising edge を検知したい」フィールドはここに入れる。フィールド型は state 側と
/// 完全一致させる必要はなく、判定に必要な最小限 (例: bool, u32) で OK。
#[derive(Clone, Copy, Default)]
struct PrevSnapshot {
    floor: u32,
    enemy_hurt_flash: u32,
}

impl AbyssGame {
    pub fn new() -> Self {
        let state = AbyssState::new();
        let prev = PrevSnapshot {
            floor: state.floor,
            enemy_hurt_flash: state.enemy_hurt_flash,
        };
        Self {
            state,
            effects: RefCell::new(AbyssEffects::new()),
            prev: Cell::new(prev),
            last_render_ms: Cell::new(0.0),
        }
    }

    /// state の差分を見て、対応する効果を effects に push する。
    /// render の冒頭 (widget 描画前) に呼ぶ。
    ///
    /// ### 拡張ポイント
    /// 新しい演出を増やす時はこのメソッドに `if prev.X != state.X { effects.push_Y() }`
    /// を追加するだけ。state 自体や logic.rs を触る必要はない。
    fn detect_transitions(&self, area: Rect) {
        let prev = self.prev.get();
        let mut effects = self.effects.borrow_mut();
        let layout = render::compute_layout(area);

        // 階層遷移: floor の増減で別演出
        if self.state.floor > prev.floor {
            // ボス撃破などで深く潜った
            effects.push_descend(area);
        } else if self.state.floor < prev.floor {
            // 撤退または死亡で浅瀬に戻された
            effects.push_ascend_or_death(area);
        }

        // 敵被弾: enemy_hurt_flash の rising edge (0 → N)
        // logic 側で被弾時に enemy_hurt_flash = N にセットされるので、
        // 「直前 0 で今 > 0」が「今フレームで攻撃が当たった」瞬間。
        if prev.enemy_hurt_flash == 0 && self.state.enemy_hurt_flash > 0 {
            effects.push_enemy_hit(layout.enemy_panel);
        }

        // 次の snapshot に更新
        self.prev.set(PrevSnapshot {
            floor: self.state.floor,
            enemy_hurt_flash: self.state.enemy_hurt_flash,
        });
    }

    /// 前回 render からの経過時間を計算する。初回は 0。
    fn compute_elapsed(&self) -> Duration {
        let now = now_ms().unwrap_or(0.0);
        let prev = self.last_render_ms.get();
        self.last_render_ms.set(now);
        if prev == 0.0 {
            Duration::ZERO
        } else {
            // tab backgrounded 等で巨大な値になった場合は 100ms に clamp
            let delta_ms = (now - prev).clamp(0.0, 100.0);
            Duration::from_micros((delta_ms * 1000.0) as u64)
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
            TAB_GACHA => Some(PlayerAction::SetTab(Tab::Gacha)),
            TOGGLE_AUTO_DESCEND => Some(PlayerAction::ToggleAutoDescend),
            RETREAT_TO_SURFACE => Some(PlayerAction::Retreat),
            GACHA_PULL_1 => Some(PlayerAction::GachaPull(1)),
            GACHA_PULL_10 => Some(PlayerAction::GachaPull(10)),
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
            '~' => Some(PlayerAction::SetTab(Tab::Gacha)),
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
            's' | 'S' if matches!(self.state.tab, Tab::Gacha) => Some(PlayerAction::GachaPull(1)),
            'x' | 'X' if matches!(self.state.tab, Tab::Gacha) => Some(PlayerAction::GachaPull(10)),
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
        // 1. state 差分を見て新規 effect を push (area が必要なので render 内で行う)
        self.detect_transitions(area);

        // 2. 通常の widget 描画
        render::render(&self.state, f, area, click_state);

        // 3. 描画後の Buffer に effect を post-process として適用
        let elapsed = self.compute_elapsed();
        self.effects
            .borrow_mut()
            .process(elapsed, f.buffer_mut(), area);
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
