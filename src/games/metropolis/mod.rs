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
pub mod ai_worker;
pub mod logic;
pub mod render;
pub mod save;
pub mod simulator;
pub mod state;
pub mod terrain;
#[cfg(target_arch = "wasm32")]
pub mod worker_handle;

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
// 7-9 と 1000 番台 (旧 DEMOLISH_CELL_BASE) は再利用可能な ID 範囲。
// プレイヤー操作は戦略 / 雇用 / CPU 進化 / タブだけで、撤去・開拓判断は
// すべて AI (`ai::decide`) が `evaluate` / `action_value` 経由で行う。
pub const ACT_STRATEGY_GROWTH: u16 = 1;
pub const ACT_STRATEGY_INCOME: u16 = 2;
/// Tech 戦略 (建設速度 +20% / 収入 -20%)。
/// 数値 ID は永続クリックスコープのため変更しない。
pub const ACT_STRATEGY_TECH: u16 = 3;
pub const ACT_HIRE_WORKER: u16 = 4;
pub const ACT_UPGRADE_AI: u16 = 5;
/// Eco 戦略 (建設 -10% / 収入 +5% / Forest を切らない)。
/// 既存 ID 1-5 と重複しないよう新しい番号を取得。
pub const ACT_STRATEGY_ECO: u16 = 6;

// タブ切替アクション (10-14 を予約; 戦略の隣だが衝突しない)。
pub const ACT_TAB_STATUS: u16 = 10;
pub const ACT_TAB_MANAGER: u16 = 11;
pub const ACT_TAB_EVENTS: u16 = 12;
pub const ACT_TAB_WORLD: u16 = 13;
pub const ACT_TAB_CATALOG: u16 = 14;

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

/// オフライン進行ボーナス通知モーダルを閉じる click target ID。
/// モーダル領域全体を `Clickable` で wrap してこの ID を登録する。
pub const ACT_DISMISS_OFFLINE_WELCOME: u16 = 30;

/// 1 回のスクロールで動かすセル数。視野の 1/8 (32/8 = 4) で「ちょっとずつ
/// 動かす」感じ。短すぎると到達まで連打、長すぎると見落とすバランス。
const SCROLL_STEP: i32 = 4;

/// 右パネル縦スクロールの 1 回ぶん (visual rows)。短すぎると連打が必要、
/// 長すぎると行き過ぎる。Manager の最大行数 (~10) に対して 2 行送りが妥当。
const PANEL_SCROLL_STEP: i32 = 2;

/// マップセルのクリック識別子の起点。`base + row * VIEW_W + col` で
/// ビューポート相対座標を u16 に詰め込む。`ClickableGrid::decode` で逆引き。
/// VIEW_W * VIEW_H = 32*16 = 512 < 1000 なので既存 action ID と衝突しない。
pub const ACT_GRID_CELL_BASE: u16 = 1000;

pub struct MetropolisGame {
    pub state: City,
    /// オートセーブまでの残り tick 数。`save::AUTOSAVE_INTERVAL` から減算。
    save_countdown: u32,
    /// 直近の tick で観測した wall-clock (ms since epoch)。タブをバックグラウンド
    /// にして戻ってきた時に、この値と現在時刻の gap を見てオフライン進行ボーナスを
    /// 支給する。in-memory のみで永続化はしない (永続化は autosave 側の
    /// `last_save_wall_ms` でカバー済み)。
    ///
    /// `requestAnimationFrame` はタブ非表示で実質停止するため、tick の delta だけ
    /// では「裏で経過した時間」を取り戻せない (`time.rs` の clamp が 500ms で打ち切る)。
    /// `Date.now()` は連続的に進むので、tick 経由の gap 検出に使う。
    #[cfg(target_arch = "wasm32")]
    last_wall_ms: u64,
    /// AI 探索を別 WASM の Web Worker で動かすためのハンドル。
    /// `Some` の間は `tick` が同期的な `drive_ai` を呼ばず、worker から
    /// 返ってきた `AiAction` を `apply_ai_action` で適用する非同期パスに切替わる。
    /// 生成失敗 (file:// 起動・worker 制限環境等) の場合は `None` で同期パスにフォールバック。
    #[cfg(target_arch = "wasm32")]
    ai_worker: Option<worker_handle::AiWorkerHandle>,
    /// `tick` / `render` の実行時間サンプル (ms)。WASM ブラウザで描画が重い時に
    /// どこがボトルネックかを画面表示するために使う。release ビルドでも有効
    /// (チューニング中の実測用)。 RefCell で `&self` の `render` から書き込む。
    perf: std::cell::RefCell<PerfStats>,
}

/// `tick` / `render` 実行時間の rolling-window 統計。
///
/// 60 FPS の WASM 描画で「重さ」を可視化するために使う:
///   - tick が 16ms 超 → 1 フレーム以上 main thread が block して frame drop
///   - render が 16ms 超 → 描画自体が間に合わない (本来軽量なはず)
///
/// 最近 N サンプルの max / avg を表示することで、瞬間的な spike も逃さない。
pub struct PerfStats {
    tick_samples: std::collections::VecDeque<f32>,
    render_samples: std::collections::VecDeque<f32>,
    /// オーバーレイを描画するかのトグル。Manager タブの設定で切替予定。
    /// 既定 true: チューニング中なので常時表示。
    pub overlay_enabled: bool,
}

impl PerfStats {
    const WINDOW: usize = 30;

    pub fn new() -> Self {
        Self {
            tick_samples: std::collections::VecDeque::with_capacity(Self::WINDOW),
            render_samples: std::collections::VecDeque::with_capacity(Self::WINDOW),
            overlay_enabled: true,
        }
    }

    fn push(buf: &mut std::collections::VecDeque<f32>, ms: f32) {
        if buf.len() == Self::WINDOW {
            buf.pop_front();
        }
        buf.push_back(ms);
    }

    pub fn record_tick(&mut self, ms: f32) {
        Self::push(&mut self.tick_samples, ms);
    }

    pub fn record_render(&mut self, ms: f32) {
        Self::push(&mut self.render_samples, ms);
    }

    fn stat(buf: &std::collections::VecDeque<f32>) -> (f32, f32) {
        if buf.is_empty() {
            return (0.0, 0.0);
        }
        let n = buf.len() as f32;
        let sum: f32 = buf.iter().sum();
        let max = buf.iter().cloned().fold(0.0f32, f32::max);
        (sum / n, max)
    }

    pub fn tick_avg_max(&self) -> (f32, f32) {
        Self::stat(&self.tick_samples)
    }

    pub fn render_avg_max(&self) -> (f32, f32) {
        Self::stat(&self.render_samples)
    }
}

impl Default for PerfStats {
    fn default() -> Self {
        Self::new()
    }
}

/// 高解像度ミリ秒タイマー。WASM では `Performance.now()`、native では
/// `SystemTime` フォールバック (テストで panic しないため)。
fn perf_now_ms() -> Option<f64> {
    #[cfg(target_arch = "wasm32")]
    {
        web_sys::window()
            .and_then(|w| w.performance())
            .map(|p| p.now())
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()
            .map(|d| d.as_secs_f64() * 1000.0)
    }
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

        // Worker のスポーンは best-effort。失敗 (CSP / file:// 起動等) しても
        // ゲームは同期 AI でそのまま動くようフォールバックする。`AI_WORKER_SCRIPT_URL`
        // は `index.html` の `<link data-trunk rel="copy-file">` 経路と一致させる。
        #[cfg(target_arch = "wasm32")]
        let ai_worker = worker_handle::AiWorkerHandle::try_new(AI_WORKER_SCRIPT_URL);

        Self {
            state,
            save_countdown: save::AUTOSAVE_INTERVAL,
            // load_game 直後の wall-clock を起点にする。これにより load_game 内で
            // 既にボーナス済みの期間を tick 側で二重支給することはない。
            #[cfg(target_arch = "wasm32")]
            last_wall_ms: save::wall_clock_now_ms(),
            #[cfg(target_arch = "wasm32")]
            ai_worker,
            perf: std::cell::RefCell::new(PerfStats::new()),
        }
    }
}

/// `metropolis_worker_entry.js` を Trunk が dist 直下にコピーする想定。
/// dist のルート絶対パスを当てにせず、相対パスで参照する。
#[cfg(target_arch = "wasm32")]
const AI_WORKER_SCRIPT_URL: &str = "./metropolis_worker_entry.js";

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
        // オフライン進行ボーナス通知モーダル表示中は通常操作をブロックし、
        // 任意のクリック / キーで閉じる。プレイヤーに「ボーナスを受け取った」
        // 認知を一度だけ強制するための関所。閉じても Events タブに同内容が
        // 残るため、振り返りは可能。
        if self.state.pending_offline_welcome.is_some() {
            self.state.pending_offline_welcome = None;
            return true;
        }

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
                            // `switch_tab` 経由で `panel_scroll` を 0 リセット
                            // しないと、Manager で深くスクロールしていた時に
                            // Status が stale offset で開いて選択セル詳細が
                            // 見切れる。
                            switch_tab(&mut self.state, PanelTab::Status);
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
                '5' => ACT_TAB_CATALOG,
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
            ACT_TAB_CATALOG => {
                switch_tab(&mut self.state, PanelTab::Catalog);
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
        let t_start = perf_now_ms();
        // タブ復帰時のオフライン進行ボーナス。`requestAnimationFrame` がバック
        // グラウンドで停止していた間、`delta_ticks` は `time.rs` の clamp で
        // 最大 5 tick (500ms) しか進まないため、wall-clock の gap で実時間を
        // 検出してボーナス支給する。`OFFLINE_MIN_SECS (60秒)` 未満の gap では
        // `apply_offline_bonus_with_persist` が None を返すので、通常プレイ中は
        // 副作用ゼロ。
        //
        // `last_wall_ms` を毎 tick で更新するのは、tick が連続して呼ばれている
        // = タブが visible である状態を表すため。バックグラウンドで rAF が止まれば
        // tick も呼ばれず last_wall_ms が据え置かれて、復帰時の最初の tick で
        // 正しい gap (= 不在時間) が観測される。
        #[cfg(target_arch = "wasm32")]
        {
            // 戻り値は state に反映済み or ロールバック済みなので捨てる。
            // 計測起点は支給有無 / save 成否いずれの outcome でも `now_ms` に進める:
            // 進めないと、save 失敗ロールバック後にゲームが進行し続けた foreground
            // 時間が、次回 retry で「オフライン」として誤って二重支給される。
            // localStorage の失敗は通常持続的なので、取りこぼしの確率は低く、
            // 二重支給を防ぐ方が体験への悪影響が小さい。
            let _ = save::apply_offline_bonus_with_persist(&mut self.state, self.last_wall_ms);
            self.last_wall_ms = save::wall_clock_now_ms();
        }

        // AI Worker が確保できている時のみ 非同期 AI 経路に分岐する。
        // worker が用意した `AiAction` を tick 開始時にまず適用してから
        // 物理シム (`tick_without_ai`) を進めることで、worker の判断が
        // 「現在の街」より 1 tick 古い前提で計算されたとしても、
        // `apply_ai_action` 内の `start_construction` / `demolish_at` が
        // 再検証して stale なら no-op にする。
        #[cfg(target_arch = "wasm32")]
        {
            if let Some(handle) = self.ai_worker.as_mut() {
                if let Some(action) = handle.take_action() {
                    let _ = logic::apply_ai_action(&mut self.state, action);
                }
                logic::tick_without_ai(&mut self.state, delta_ticks);
                // 物理 tick で grid / cash / tick が進んだ最新スナップショットを
                // worker に投げて、次の判断を仕込む。in-flight 中は no-op。
                handle.try_dispatch(&self.state);
            } else {
                logic::tick(&mut self.state, delta_ticks);
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            logic::tick(&mut self.state, delta_ticks);
        }

        // オートセーブ。カウンタ更新自体は常に実行 (フィールドが
        // dead_code にならないよう)、実際の保存は WASM 環境のみ。
        // 30 秒間隔 (= 300 ticks) は cookie/save と揃えている。
        self.save_countdown = self.save_countdown.saturating_sub(delta_ticks);
        if self.save_countdown == 0 {
            // best-effort autosave: 失敗時の戻り値は無視する (warn は save_game 内で出力済)。
            #[cfg(target_arch = "wasm32")]
            let _ = save::save_game(&self.state);
            self.save_countdown = save::AUTOSAVE_INTERVAL;
        }

        if let (Some(s), Some(e)) = (t_start, perf_now_ms()) {
            self.perf.borrow_mut().record_tick((e - s) as f32);
        }
    }

    fn render(&self, f: &mut Frame, area: Rect, click_state: &Rc<RefCell<ClickState>>) {
        let t_start = perf_now_ms();
        render::render(&self.state, f, area, click_state);
        if let (Some(s), Some(e)) = (t_start, perf_now_ms()) {
            self.perf.borrow_mut().record_render((e - s) as f32);
        }
        // perf overlay: チューニング中の実測用に右上に小さく表示。
        // overlay_enabled = false にすれば消せる (将来 Manager タブの設定で切替予定)。
        let perf = self.perf.borrow();
        if perf.overlay_enabled {
            let (t_avg, t_max) = perf.tick_avg_max();
            let (r_avg, r_max) = perf.render_avg_max();
            let line = format!(
                " ⚡ T:{:.1}/{:.1}ms R:{:.1}/{:.1}ms ",
                t_avg, t_max, r_avg, r_max
            );
            let w = (line.chars().count() as u16).min(area.width);
            if w > 0 {
                let overlay_area = Rect::new(
                    area.x + area.width.saturating_sub(w),
                    area.y,
                    w,
                    1,
                );
                let p = ratatui::widgets::Paragraph::new(ratatui::text::Span::styled(
                    line,
                    ratatui::style::Style::default()
                        .fg(ratatui::style::Color::DarkGray)
                        .bg(ratatui::style::Color::Black),
                ));
                f.render_widget(p, overlay_area);
            }
        }
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
    /// Tier 4 (`Planner`) は `evaluate` と `action_value` を比較し、
    /// 機能不全建物 (= 物理的に救えない位置の inactive Shop) を AI が撤去する。
    ///
    /// **テスト前提**: 街全体に edge-connected Road が存在しない (= seed road を消去)
    /// 状態 + 中央の Shop の 4-近傍を Water 地形で囲んで Road/House の隣接配置を
    /// 不可能にする。これで AI には:
    ///     - Shop を救う手段が無い (= 隣接セルが全部 Water)
    ///     - Build House すると unconnected Cottage で +22.8 action_value
    ///     - Demolish Shop は中央なので cost $50、+39.3 action_value
    ///
    /// となり、Demolish が一意に最高評価。短い tick 数で確実に選ばれる。
    #[test]
    fn drive_ai_demolishes_inactive_shop() {
        use state::{AiTier, Building, Tile, GRID_H, GRID_W};
        let mut g = MetropolisGame::new();
        g.state.cash = 50_000;
        g.state.workers = 4;
        g.state.strategy = Strategy::Income;
        g.state.ai_tier = AiTier::Planner;
        for y in 0..GRID_H {
            for x in 0..GRID_W {
                g.state.terrain[y][x] = crate::games::metropolis::terrain::Terrain::Plain;
                if matches!(g.state.tile(x, y), Tile::Built(Building::Road)) {
                    g.state.set_tile(x, y, Tile::Empty);
                }
            }
        }
        let sx = GRID_W / 2;
        let sy = GRID_H / 2;
        g.state.set_tile(sx, sy, Tile::Built(Building::Shop));
        // 4-近傍を Water (= 建設不可) にして Shop の救済路を断つ。
        for (dx, dy) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
            let nx = (sx as i32 + dx) as usize;
            let ny = (sy as i32 + dy) as usize;
            g.state.terrain[ny][nx] = crate::games::metropolis::terrain::Terrain::Water;
        }

        g.tick(60);
        assert!(
            matches!(g.state.tile(sx, sy), Tile::Empty),
            "AI (Tier 4) should demolish unreachable inactive Shop"
        );
    }
}
