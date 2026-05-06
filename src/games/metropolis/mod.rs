//! Idle Metropolis — AI-driven city builder.
//!
//! The player buys upgrades and sets strategy; an automated CPU does the
//! actual placement.  Because the lowest AI tier is intentionally dumb,
//! `simulator.rs` is provided up-front to verify that even a bad CPU keeps
//! the game progressing (cash & population trending up over time).
//!
//! Architecture follows the project's "pure logic" pattern:
//!   • `state.rs`  — all data, no behavior.
//!   • `logic.rs`  — pure functions (tick, income, construction).
//!   • `ai.rs`     — strategy brains, one function per tier.
//!   • `simulator.rs` — balance tests (cargo test, no rendering).

pub mod ai;
pub mod logic;
pub mod render;
pub mod save;
pub mod simulator;
pub mod state;
pub mod terrain;

use std::cell::RefCell;
use std::rc::Rc;

use ratzilla::ratatui::layout::Rect;
use ratzilla::ratatui::Frame;

use crate::games::{Game, GameChoice};
use crate::input::{ClickState, InputEvent};

use state::{City, PanelTab, Strategy};

// ── Action IDs scoped to MetropolisGame ─────────────────────────
//
// These are click/key actions on the manager panel.  Keep them stable —
// they're persisted through Click events keyed by `ClickScope::Game(...)`.
pub const ACT_STRATEGY_GROWTH: u16 = 1;
pub const ACT_STRATEGY_INCOME: u16 = 2;
/// 旧 `ACT_STRATEGY_BALANCED` の枠を流用。Tech 戦略 (建設速度 +20% / 収入 -20%)。
/// 数値 ID は永続クリックスコープのため変更しない。
pub const ACT_STRATEGY_TECH: u16 = 3;
pub const ACT_HIRE_WORKER: u16 = 4;
pub const ACT_UPGRADE_AI: u16 = 5;
/// Eco 戦略 (建設 -10% / 収入 +5% / Forest を切らない)。
/// 既存 ID 1-5 と重複しないよう新しい番号を取得。
pub const ACT_STRATEGY_ECO: u16 = 6;
/// 開拓機材を派遣 (= AI に Outpost を 1 基置かせる)。
/// 高コスト ($600) で 60 sec の長時間建設、市域拡張の戦略行動。
pub const ACT_DISPATCH_OUTPOST: u16 = 7;
/// 撤去モードのトグル。ON にするとグリッドの全 Built セルがクリック可能に。
pub const ACT_TOGGLE_DEMOLISH: u16 = 8;
/// AI に撤去判断を一任する。手動撤去モードと並列で利用可。
pub const ACT_AUTO_DEMOLISH: u16 = 9;

/// グリッドセルクリック (撤去モード時のみ反応) のアクション ID 基準値。
///
/// `(y, x)` に対して `DEMOLISH_CELL_BASE + y * GRID_W + x` を割り当てる。
/// 32 * 16 = 512 セルなので、1000..=1511 の範囲を占有。タブ ID (10-13) や
/// その他のシングルトンアクションと重複しない。`ClickableGrid::decode` を
/// 使ってデコード。
pub const DEMOLISH_CELL_BASE: u16 = 1000;

// タブ切替アクション (10-13 を予約; 戦略の隣だが衝突しない)。
pub const ACT_TAB_STATUS: u16 = 10;
pub const ACT_TAB_MANAGER: u16 = 11;
pub const ACT_TAB_EVENTS: u16 = 12;
pub const ACT_TAB_WORLD: u16 = 13;

pub struct MetropolisGame {
    pub state: City,
    /// オートセーブまでの残り tick 数。`save::AUTOSAVE_INTERVAL` から減算。
    save_countdown: u32,
}

impl MetropolisGame {
    pub fn new() -> Self {
        let mut state = City::new();

        // WASM ビルド時のみ localStorage からロードを試みる。
        // ロード成功時はセーブ復元メッセージを events に出して、ユーザーに
        // 「進捗が引き継がれた」ことを伝える。
        #[cfg(target_arch = "wasm32")]
        let state = {
            let mut s = state;
            if save::load_game(&mut s) {
                s.push_event("💾 セーブデータをロードしました".to_string());
            } else {
                s.push_event("🏙 都市建設を開始しました".to_string());
            }
            s
        };
        #[cfg(not(target_arch = "wasm32"))]
        {
            state.push_event("🏙 都市建設を開始しました".to_string());
        }

        Self {
            state,
            save_countdown: save::AUTOSAVE_INTERVAL,
        }
    }
}

impl Default for MetropolisGame {
    fn default() -> Self {
        Self::new()
    }
}

impl Game for MetropolisGame {
    fn choice(&self) -> GameChoice {
        GameChoice::Metropolis
    }

    fn handle_input(&mut self, event: &InputEvent) -> bool {
        let action_id = match event {
            InputEvent::Click(_, id) => *id,
            InputEvent::Key(c) => match c {
                'g' | 'G' => ACT_STRATEGY_GROWTH,
                'i' | 'I' => ACT_STRATEGY_INCOME,
                't' | 'T' => ACT_STRATEGY_TECH,
                'e' | 'E' => ACT_STRATEGY_ECO,
                'w' | 'W' => ACT_HIRE_WORKER,
                'u' | 'U' => ACT_UPGRADE_AI,
                'o' | 'O' => ACT_DISPATCH_OUTPOST,
                'd' | 'D' => ACT_TOGGLE_DEMOLISH,
                'x' | 'X' => ACT_AUTO_DEMOLISH,
                '1' => ACT_TAB_STATUS,
                '2' => ACT_TAB_MANAGER,
                '3' => ACT_TAB_EVENTS,
                '4' => ACT_TAB_WORLD,
                _ => return false,
            },
        };

        // 撤去モード時のセルクリックを最優先で処理 (DEMOLISH_CELL_BASE..)。
        // demolish_mode が OFF の時に来た場合は無視 (普通は来ない)。
        if action_id >= DEMOLISH_CELL_BASE {
            if !self.state.demolish_mode {
                return false;
            }
            let offset = (action_id - DEMOLISH_CELL_BASE) as usize;
            if offset >= state::GRID_W * state::GRID_H {
                return false;
            }
            let x = offset % state::GRID_W;
            let y = offset / state::GRID_W;
            return logic::demolish_at(&mut self.state, x, y);
        }

        match action_id {
            ACT_STRATEGY_GROWTH => {
                set_strategy(&mut self.state, Strategy::Growth, "📈");
                true
            }
            ACT_STRATEGY_INCOME => {
                set_strategy(&mut self.state, Strategy::Income, "💰");
                true
            }
            ACT_STRATEGY_TECH => {
                set_strategy(&mut self.state, Strategy::Tech, "⚙");
                true
            }
            ACT_STRATEGY_ECO => {
                set_strategy(&mut self.state, Strategy::Eco, "🌳");
                true
            }
            ACT_HIRE_WORKER => logic::hire_worker(&mut self.state),
            ACT_UPGRADE_AI => logic::upgrade_ai(&mut self.state),
            ACT_DISPATCH_OUTPOST => logic::dispatch_outpost(&mut self.state),
            ACT_TOGGLE_DEMOLISH => {
                self.state.demolish_mode = !self.state.demolish_mode;
                let msg = if self.state.demolish_mode {
                    "🗑 撤去モード ON — Built セルをクリックで撤去"
                } else {
                    "✓ 撤去モード OFF"
                };
                self.state.push_event(msg.to_string());
                true
            }
            ACT_AUTO_DEMOLISH => logic::auto_demolish(&mut self.state),
            ACT_TAB_STATUS => {
                self.state.panel_tab = PanelTab::Status;
                true
            }
            ACT_TAB_MANAGER => {
                self.state.panel_tab = PanelTab::Manager;
                true
            }
            ACT_TAB_EVENTS => {
                self.state.panel_tab = PanelTab::Events;
                true
            }
            ACT_TAB_WORLD => {
                self.state.panel_tab = PanelTab::World;
                true
            }
            _ => false,
        }
    }

    fn tick(&mut self, delta_ticks: u32) {
        logic::tick(&mut self.state, delta_ticks);

        // オートセーブ。カウンタ更新自体は常に実行 (フィールドが
        // dead_code にならないよう)、実際の保存は WASM 環境のみ。
        // 30 秒間隔 (= 300 ticks) は cookie/save と揃えている。
        self.save_countdown = self.save_countdown.saturating_sub(delta_ticks);
        if self.save_countdown == 0 {
            #[cfg(target_arch = "wasm32")]
            save::save_game(&self.state);
            self.save_countdown = save::AUTOSAVE_INTERVAL;
        }
    }

    fn render(&self, f: &mut Frame, area: Rect, click_state: &Rc<RefCell<ClickState>>) {
        render::render(&self.state, f, area, click_state);
    }
}

/// Strategy 切替時の共通処理。`logic::strategy_info` を引いて
/// 「ラベル + 副作用 (建設速度・収入修正)」を 1 行のイベントログにまとめる。
/// 切替時に「何が変わったか」が即座にログに見えるようにするのが目的。
fn set_strategy(city: &mut City, s: Strategy, icon: &str) {
    city.strategy = s;
    let info = logic::strategy_info(s);
    let mut suffix = String::new();
    if info.speed_bonus_pct != 0 {
        suffix.push_str(&format!(" / 建設{:+}%", info.speed_bonus_pct));
    }
    if info.income_penalty_pct != 0 {
        suffix.push_str(&format!(" / 収入{:+}%", info.income_penalty_pct));
    }
    city.push_event(format!(
        "{} 戦略: {} — {}{}",
        icon, info.label, info.tagline, suffix
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::ClickScope;

    fn click(id: u16) -> InputEvent {
        InputEvent::Click(ClickScope::Game(GameChoice::Metropolis), id)
    }

    #[test]
    fn strategy_keys_change_state() {
        let mut g = MetropolisGame::new();
        g.handle_input(&InputEvent::Key('g'));
        assert_eq!(g.state.strategy, Strategy::Growth);
        g.handle_input(&InputEvent::Key('i'));
        assert_eq!(g.state.strategy, Strategy::Income);
        g.handle_input(&click(ACT_STRATEGY_TECH));
        assert_eq!(g.state.strategy, Strategy::Tech);
    }

    #[test]
    fn unknown_input_returns_false() {
        let mut g = MetropolisGame::new();
        assert!(!g.handle_input(&InputEvent::Key('q')));
        assert!(!g.handle_input(&click(9999)));
    }

    #[test]
    fn hiring_costs_money() {
        let mut g = MetropolisGame::new();
        g.state.cash = 1000;
        let before = g.state.workers;
        assert!(g.handle_input(&InputEvent::Key('w')));
        assert_eq!(g.state.workers, before + 1);
        assert!(g.state.cash < 1000);
    }

    #[test]
    fn tick_advances_simulation() {
        let mut g = MetropolisGame::new();
        let before = g.state.tick;
        g.tick(50);
        assert_eq!(g.state.tick, before + 50);
    }

    /// 'D' キーで撤去モードがトグルする。
    #[test]
    fn d_key_toggles_demolish_mode() {
        let mut g = MetropolisGame::new();
        assert!(!g.state.demolish_mode);
        assert!(g.handle_input(&InputEvent::Key('d')));
        assert!(g.state.demolish_mode);
        assert!(g.handle_input(&InputEvent::Key('D')));
        assert!(!g.state.demolish_mode);
    }

    /// 撤去モード ON でセルクリックすると撤去される。
    #[test]
    fn demolish_cell_click_removes_building() {
        use state::{Building, Tile, GRID_W};
        let mut g = MetropolisGame::new();
        g.state.cash = 10_000;
        g.state.demolish_mode = true;
        let cx = 16;
        let cy = 8;
        g.state.set_tile(cx, cy, Tile::Built(Building::House));
        // Click action ID for (cx, cy) = DEMOLISH_CELL_BASE + cy * GRID_W + cx
        let action = DEMOLISH_CELL_BASE + (cy as u16) * (GRID_W as u16) + (cx as u16);
        let consumed = g.handle_input(&click(action));
        assert!(consumed);
        assert!(matches!(g.state.tile(cx, cy), Tile::Empty));
    }

    /// 'X' キーで AI 撤去判断が発火する (候補があれば成功、無ければ false 返し)。
    #[test]
    fn x_key_triggers_auto_demolish() {
        use state::{Building, Tile, GRID_W, GRID_H};
        let mut g = MetropolisGame::new();
        g.state.cash = 10_000;
        let cx = GRID_W / 2;
        let cy = GRID_H / 2;
        // inactive Shop を中央に置く。
        g.state.set_tile(cx, cy, Tile::Built(Building::Shop));
        assert!(g.handle_input(&InputEvent::Key('x')));
        assert!(matches!(g.state.tile(cx, cy), Tile::Empty));
    }

    /// 撤去モード OFF 時のセルクリックは無視される。
    #[test]
    fn demolish_cell_click_ignored_when_off() {
        use state::{Building, Tile, GRID_W};
        let mut g = MetropolisGame::new();
        g.state.cash = 10_000;
        g.state.demolish_mode = false;
        let cx = 16;
        let cy = 8;
        g.state.set_tile(cx, cy, Tile::Built(Building::House));
        let action = DEMOLISH_CELL_BASE + (cy as u16) * (GRID_W as u16) + (cx as u16);
        let consumed = g.handle_input(&click(action));
        assert!(!consumed);
        // House は残ったまま。
        assert!(matches!(
            g.state.tile(cx, cy),
            Tile::Built(Building::House)
        ));
    }
}
