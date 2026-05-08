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

use crate::widgets::ClickableGrid;

use state::{City, PanelTab, Strategy};

// ── Action IDs scoped to MetropolisGame ─────────────────────────
//
// These are click/key actions on the manager panel.  Keep them stable —
// they're persisted through Click events keyed by `ClickScope::Game(...)`.
//
// Phase A (撤去・開拓の完全自動化) で ID 7/8/9 (旧 DISPATCH_OUTPOST /
// TOGGLE_DEMOLISH / AUTO_DEMOLISH) と DEMOLISH_CELL_BASE (1000+) を撤去。
// 戦略を選んだ後の挙動はすべて `logic::auto_strategy_actions` が tick から
// 自動実行する。プレイヤー操作は戦略 / 雇用 / CPU 進化 / タブだけ。
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

// タブ切替アクション (10-13 を予約; 戦略の隣だが衝突しない)。
pub const ACT_TAB_STATUS: u16 = 10;
pub const ACT_TAB_MANAGER: u16 = 11;
pub const ACT_TAB_EVENTS: u16 = 12;
pub const ACT_TAB_WORLD: u16 = 13;

// ビューポートスクロール (Phase 3)。マップ 64×32 を 32×16 の viewport で覗く。
// h/j/k/l (Vim 流) または矢印キー風のキーで動かす。1 回 4 セル送り (= 視野の 1/8)。
pub const ACT_SCROLL_LEFT: u16 = 20;
pub const ACT_SCROLL_RIGHT: u16 = 21;
pub const ACT_SCROLL_UP: u16 = 22;
pub const ACT_SCROLL_DOWN: u16 = 23;

/// 右パネル (タブ内コンテンツ) の縦スクロール。スマホ等の浅い縦幅で
/// Manager / Status の下端が見切れる問題への対応。▲/▼ ボタンと大文字 J/K で発火。
/// ビューポートスクロール (小文字 j/k = ACT_SCROLL_*) とは別系統。
pub const ACT_PANEL_SCROLL_UP: u16 = 24;
pub const ACT_PANEL_SCROLL_DOWN: u16 = 25;

/// 1 回のスクロールで動かすセル数。視野の 1/8 (32/8 = 4) で「ちょっとずつ
/// 動かす」感じ。短すぎると到達まで連打、長すぎると見落とすバランス。
const SCROLL_STEP: i32 = 4;

/// 右パネル縦スクロールの 1 回ぶん (visual rows)。短すぎると連打が必要、
/// 長すぎると行き過ぎる。Manager の最大行数 (~10) に対して 2 行送りが妥当。
const PANEL_SCROLL_STEP: i32 = 2;

/// マップセルのクリック識別子の起点。`base + row * VIEW_W + col` で
/// ビューポート相対座標を u16 に詰め込む。`ClickableGrid::decode` で逆引き。
/// 1000 番台はかつて DEMOLISH_CELL_BASE で使っていたが Phase A の自動撤去で
/// 廃止済みなので再利用可能。VIEW_W * VIEW_H = 32*16 = 512 < 1000 なので
/// 既存 ID 1-23 とも衝突しない。
pub const ACT_GRID_CELL_BASE: u16 = 1000;

pub struct MetropolisGame {
    pub state: City,
    /// オートセーブまでの残り tick 数。`save::AUTOSAVE_INTERVAL` から減算。
    save_countdown: u32,
}

impl MetropolisGame {
    pub fn new() -> Self {
        // wasm ビルドでは下の `let state = { ... }` で shadow するため
        // 外側の `mut` が unused 扱いになる。non-wasm ブランチが
        // `state.push_event` で `mut` を要求するので、warning は許容。
        #[allow(unused_mut)]
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
        // マップセルのクリック (action_id >= ACT_GRID_CELL_BASE) は
        // `selected_cell` を更新する。Status タブでその施設の詳細を表示する。
        if let InputEvent::Click(_, id) = event {
            if *id >= ACT_GRID_CELL_BASE {
                if let Some((col, row)) = ClickableGrid::decode(
                    ACT_GRID_CELL_BASE,
                    state::VIEW_W,
                    *id,
                ) {
                    let abs_x = self.state.cam_x + col;
                    let abs_y = self.state.cam_y + row;
                    if abs_x < state::GRID_W && abs_y < state::GRID_H {
                        // 既に選択中なら deselect (= 同じセルを再タップで閉じる)。
                        if self.state.selected_cell == Some((abs_x, abs_y)) {
                            self.state.selected_cell = None;
                        } else {
                            self.state.selected_cell = Some((abs_x, abs_y));
                            // 選択時は Status タブにフォーカス (= 詳細を見せる)。
                            self.state.panel_tab = PanelTab::Status;
                        }
                        return true;
                    }
                }
                return false;
            }
        }
        let action_id = match event {
            InputEvent::Click(_, id) => *id,
            InputEvent::Key(c) => match c {
                'g' | 'G' => ACT_STRATEGY_GROWTH,
                'i' | 'I' => ACT_STRATEGY_INCOME,
                't' | 'T' => ACT_STRATEGY_TECH,
                'e' | 'E' => ACT_STRATEGY_ECO,
                'w' | 'W' => ACT_HIRE_WORKER,
                'u' | 'U' => ACT_UPGRADE_AI,
                '1' => ACT_TAB_STATUS,
                '2' => ACT_TAB_MANAGER,
                '3' => ACT_TAB_EVENTS,
                '4' => ACT_TAB_WORLD,
                // Phase 3: Vim 流 hjkl でビューポートをスクロール (64×32 マップ)。
                'h' => ACT_SCROLL_LEFT,
                'j' => ACT_SCROLL_DOWN,
                'k' => ACT_SCROLL_UP,
                'l' => ACT_SCROLL_RIGHT,
                // 大文字 J/K で右パネル縦スクロール (タブ内容を上下に動かす)。
                'J' => ACT_PANEL_SCROLL_DOWN,
                'K' => ACT_PANEL_SCROLL_UP,
                _ => return false,
            },
        };

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
            ACT_TAB_STATUS => {
                switch_tab(&mut self.state, PanelTab::Status);
                true
            }
            ACT_TAB_MANAGER => {
                switch_tab(&mut self.state, PanelTab::Manager);
                true
            }
            ACT_TAB_EVENTS => {
                switch_tab(&mut self.state, PanelTab::Events);
                true
            }
            ACT_TAB_WORLD => {
                switch_tab(&mut self.state, PanelTab::World);
                true
            }
            ACT_SCROLL_LEFT => {
                self.state.scroll_camera(-SCROLL_STEP, 0);
                true
            }
            ACT_SCROLL_RIGHT => {
                self.state.scroll_camera(SCROLL_STEP, 0);
                true
            }
            ACT_SCROLL_UP => {
                self.state.scroll_camera(0, -SCROLL_STEP);
                true
            }
            ACT_SCROLL_DOWN => {
                self.state.scroll_camera(0, SCROLL_STEP);
                true
            }
            ACT_PANEL_SCROLL_UP => {
                self.state.scroll_panel(-PANEL_SCROLL_STEP);
                true
            }
            ACT_PANEL_SCROLL_DOWN => {
                self.state.scroll_panel(PANEL_SCROLL_STEP);
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

/// タブ切替時にパネルの縦スクロールを先頭にリセットする。
/// タブごとにコンテンツの長さが大きく異なるため、前のタブで深くスクロール
/// していると新しいタブで「いきなり下端」が表示されて違和感が出る。
fn switch_tab(city: &mut City, tab: PanelTab) {
    if city.panel_tab != tab {
        city.panel_scroll.set(0);
    }
    city.panel_tab = tab;
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

    /// 大文字 J / K で右パネルだけが動き、ビューポート (cam_x/cam_y) には
    /// 影響しない (= 小文字 j/k と完全に独立した系統)。
    #[test]
    fn panel_scroll_keys_move_panel_not_camera() {
        let mut g = MetropolisGame::new();
        let cam_before = (g.state.cam_x, g.state.cam_y);
        assert_eq!(g.state.panel_scroll.get(), 0);

        assert!(g.handle_input(&InputEvent::Key('J')));
        assert!(g.state.panel_scroll.get() > 0, "J should scroll panel down");
        assert_eq!(
            (g.state.cam_x, g.state.cam_y),
            cam_before,
            "J should not move viewport camera"
        );

        let after_down = g.state.panel_scroll.get();
        assert!(g.handle_input(&InputEvent::Key('K')));
        assert!(
            g.state.panel_scroll.get() < after_down,
            "K should scroll panel up"
        );
    }

    /// タブを切替えるとパネル縦スクロールが先頭に戻る。長い Status タブで
    /// 下までスクロールしてから Manager に切替えた時に、いきなり下端が
    /// 表示される違和感を避けるため。
    #[test]
    fn tab_switch_resets_panel_scroll() {
        let mut g = MetropolisGame::new();
        g.state.panel_scroll.set(5);
        // 同じタブへの切替は no-op (= scroll 維持)。
        g.handle_input(&InputEvent::Key('2')); // Manager (default)
        assert_eq!(g.state.panel_scroll.get(), 5);
        // 別タブへ切り替えるとリセット。
        g.handle_input(&InputEvent::Key('1')); // Status
        assert_eq!(g.state.panel_scroll.get(), 0);
    }

    /// スクロール位置は 0 未満にならない (= 上端で止まる)。
    #[test]
    fn panel_scroll_clamps_to_zero_at_top() {
        let mut g = MetropolisGame::new();
        // すでに 0 から上に動かそうとしても 0 のまま。
        assert!(g.handle_input(&InputEvent::Key('K')));
        assert_eq!(g.state.panel_scroll.get(), 0);
    }

    /// AI が中央の inactive Shop を自分で撤去対象に選ぶ (= drive_ai が
    /// `AiAction::Demolish` を生成して `demolish_at` を呼ぶ経路の sanity check)。
    /// Tier 4 (`DemandAware`) は `placement_value` と `demolish_value` を比較し、
    /// 機能不全建物は build より高評価 → 即撤去される決定論的経路。
    #[test]
    fn drive_ai_demolishes_inactive_shop() {
        use state::{AiTier, Building, Tile, GRID_H, GRID_W};
        let mut g = MetropolisGame::new();
        g.state.cash = 50_000;
        g.state.workers = 4;
        g.state.strategy = Strategy::Income;
        g.state.ai_tier = AiTier::DemandAware;
        let cx = GRID_W / 2;
        let cy = GRID_H / 2;
        g.state.set_tile(cx, cy, Tile::Built(Building::Shop));
        // 数 tick で Demolish action が選ばれて発火する (周期発火ではないので
        // 大きな tick 数は不要)。
        g.tick(20);
        assert!(
            matches!(g.state.tile(cx, cy), Tile::Empty),
            "AI (Tier 4) should select the inactive Shop for Demolish"
        );
    }
}
