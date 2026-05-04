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
pub mod save;
pub mod state;

#[cfg(test)]
mod simulator;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use ratzilla::ratatui::layout::Rect;
use ratzilla::ratatui::Frame;
// tachyonfx::Duration は wasm feature 有効時に独自型 (milliseconds: u32)。
// `wasm` と `std-duration` は排他なので std::time::Duration ではなくこちらを使う。
use tachyonfx::Duration;

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
    /// 定期オートセーブまでの残り tick 数 (イベントセーブ発火時にもリセットされる)。
    save_countdown: u32,
}

/// この PlayerAction を適用したらセーブを発火させるか。
/// SetTab / Scroll は純粋な UI 状態なので除外し、書き込みノイズを抑える。
fn is_save_worthy(action: PlayerAction) -> bool {
    matches!(
        action,
        PlayerAction::BuyUpgrade(_)
            | PlayerAction::BuySoulPerk(_)
            | PlayerAction::GachaPull(_)
            | PlayerAction::Retreat
            | PlayerAction::ToggleAutoDescend
    )
}

/// effect トリガ判定用の軽量 state スナップショット。Copy なフィールドだけ。
///
/// 「rising edge を検知したい」フィールドはここに入れる。フィールド型は state 側と
/// 完全一致させる必要はなく、判定に必要な最小限 (例: bool, u32) で OK。
#[derive(Clone, Copy, Default)]
struct PrevSnapshot {
    floor: u32,
    enemy_hurt_flash: u32,
    hero_hurt_flash: u32,
    enemy_is_boss: bool,
    /// last_enemy_damage の (amount, is_crit) 部分だけ抜いた指紋。
    /// life_ticks は毎 tick 減るので含めない (含めると毎フレーム edge が立ってしまう)。
    last_enemy_dmg: Option<(u64, bool)>,
    /// gacha の累計引き回数。増えた瞬間に「新規ガチャが回った」と判定する。
    gacha_total_pulls: u64,
}

impl AbyssGame {
    pub fn new() -> Self {
        #[allow(unused_mut)]
        let mut state = AbyssState::new();

        #[cfg(target_arch = "wasm32")]
        if save::load_game(&mut state) {
            state.add_log("セーブデータをロードしました");
        }

        let prev = Self::snapshot(&state);
        Self {
            state,
            effects: RefCell::new(AbyssEffects::new()),
            prev: Cell::new(prev),
            last_render_ms: Cell::new(0.0),
            save_countdown: save::AUTOSAVE_INTERVAL,
        }
    }

    /// 現在 state から PrevSnapshot を作る。new() と detect_transitions の末尾の
    /// 両方で使う共通ヘルパ。新フィールドを足したらここにも追加するだけで済む。
    fn snapshot(s: &AbyssState) -> PrevSnapshot {
        PrevSnapshot {
            floor: s.floor,
            enemy_hurt_flash: s.enemy_hurt_flash,
            hero_hurt_flash: s.hero_hurt_flash,
            enemy_is_boss: s.current_enemy.is_boss,
            last_enemy_dmg: s.last_enemy_damage.map(|(a, _, c)| (a, c)),
            gacha_total_pulls: s.total_pulls,
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
        let s = &self.state;

        // ── 階層遷移 (floor の増減で別演出) ──
        if s.floor > prev.floor {
            effects.push_descend(area);
        } else if s.floor < prev.floor {
            effects.push_ascend_or_death(area);
        }

        // ── 敵被弾 (enemy_hurt_flash の rising edge 0 → N) ──
        if prev.enemy_hurt_flash == 0 && s.enemy_hurt_flash > 0 {
            effects.push_enemy_hit(layout.enemy_panel);
        }

        // ── 勇者被弾 (hero_hurt_flash の rising edge) ──
        if prev.hero_hurt_flash == 0 && s.hero_hurt_flash > 0 {
            effects.push_hero_hit(layout.hero_panel);
        }

        // ── ボス出現 (current_enemy.is_boss が false → true) ──
        if !prev.enemy_is_boss && s.current_enemy.is_boss {
            effects.push_boss_appearance(layout.combat);
        }

        // ── ボス撃破 ──
        // 「is_boss が true → false」だけでは撤退/死亡でも発火してしまう
        // (retreat / on_hero_died はどちらも非ボス敵を即座に再生成する)。
        // 真の撃破は floor が +1 されている時だけなので、それで gate する。
        if prev.enemy_is_boss && !s.current_enemy.is_boss && s.floor > prev.floor {
            effects.push_boss_defeated(layout.enemy_panel);
        }

        // ── クリティカル (新しい damage event で is_crit=true) ──
        let cur_dmg = s.last_enemy_damage.map(|(a, _, c)| (a, c));
        if cur_dmg != prev.last_enemy_dmg {
            if let Some((_, true)) = cur_dmg {
                effects.push_critical(layout.combat);
            }
        }

        // ── ガチャ Legendary (新規ガチャで Legendary が含まれた) ──
        // by_tier[3] = Legendary 等級の当選数
        if s.total_pulls > prev.gacha_total_pulls {
            if let Some(g) = &s.last_gacha {
                if g.by_tier[3] > 0 {
                    effects.push_gacha_legendary(layout.body);
                }
            }
        }

        // 次の snapshot に更新
        self.prev.set(Self::snapshot(s));
    }

    /// 前回 render からの経過時間を計算する。初回は 0。
    fn compute_elapsed(&self) -> Duration {
        let now = now_ms().unwrap_or(0.0);
        let prev = self.last_render_ms.get();
        self.last_render_ms.set(now);
        if prev == 0.0 {
            Duration::ZERO
        } else {
            // tab backgrounded 等で巨大な値になった場合は 100ms に clamp。
            // NaN ガード: now / prev が NaN だと比較・clamp が NaN を返し、
            // `as u32` で 0 → effect が永久停止するので明示的に弾く。
            let delta_ms = (now - prev).clamp(0.0, 100.0);
            if !delta_ms.is_finite() {
                return Duration::ZERO;
            }
            Duration::from_millis(delta_ms as u32)
        }
    }

    /// クリック ID を `PlayerAction` に変換する。シミュレータ Policy も同じ
    /// `PlayerAction` を返すので、本体・sim どちらも `logic::apply_action`
    /// 1 本道で処理される (動作の乖離はここで構造的に防ぐ)。
    fn click_to_action(&self, action_id: u16) -> Option<PlayerAction> {
        match action_id {
            TAB_UPGRADES => Some(PlayerAction::SetTab(Tab::Upgrades)),
            TAB_ROADMAP => Some(PlayerAction::SetTab(Tab::Roadmap)),
            TAB_STATS => Some(PlayerAction::SetTab(Tab::Stats)),
            TAB_GACHA => Some(PlayerAction::SetTab(Tab::Gacha)),
            TAB_SETTINGS => Some(PlayerAction::SetTab(Tab::Settings)),
            TOGGLE_AUTO_DESCEND => Some(PlayerAction::ToggleAutoDescend),
            RETREAT_TO_SURFACE => Some(PlayerAction::Retreat),
            GACHA_PULL_1 => Some(PlayerAction::GachaPull(1)),
            GACHA_PULL_10 => Some(PlayerAction::GachaPull(10)),
            SCROLL_UP => Some(PlayerAction::ScrollUp),
            SCROLL_DOWN => Some(PlayerAction::ScrollDown),
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

    /// localStorage に書き込み、定期セーブのカウントダウンをリセットする。
    /// イベントセーブと時間セーブを同じ経路に通すことで、両方が短時間に
    /// 重複発火するのを防ぐ。
    fn flush_save(&mut self) {
        #[cfg(target_arch = "wasm32")]
        save::save_game(&self.state);
        self.save_countdown = save::AUTOSAVE_INTERVAL;
    }

    fn key_to_action(&self, ch: char) -> Option<PlayerAction> {
        match ch {
            '{' => Some(PlayerAction::SetTab(Tab::Upgrades)),
            // 旧 Souls タブ位置 (`|`) を Roadmap に継承。
            // 魂パーク購入は強化タブ統合後 `q/w/e/r` で Tab::Upgrades 内から行う。
            '|' => Some(PlayerAction::SetTab(Tab::Roadmap)),
            '}' => Some(PlayerAction::SetTab(Tab::Stats)),
            '~' => Some(PlayerAction::SetTab(Tab::Gacha)),
            '\\' => Some(PlayerAction::SetTab(Tab::Settings)),
            'a' | 'A' => Some(PlayerAction::ToggleAutoDescend),
            'p' | 'P' => Some(PlayerAction::Retreat),
            '1'..='7' if matches!(self.state.tab, Tab::Upgrades) => {
                let idx = (ch as u8 - b'1') as usize;
                UpgradeKind::from_index(idx).map(PlayerAction::BuyUpgrade)
            }
            // 魂パーク購入 (旧 Souls タブの `q/w/e/r` を踏襲)。
            // **大文字** に変えているのは、小文字 `q` が main.rs のグローバル
            // back-to-menu キー (Esc も `q` にマップ) と衝突するため。
            // Upgrades が default タブになった #78 以降、小文字 q を魂購入に
            // 使うとメニュー戻りが効かなくなる UX 退行が発生する (Codex review #78 参照)。
            'Q' | 'W' | 'E' | 'R' if matches!(self.state.tab, Tab::Upgrades) => {
                let idx = match ch {
                    'Q' => 0,
                    'W' => 1,
                    'E' => 2,
                    'R' => 3,
                    _ => unreachable!(),
                };
                SoulPerk::from_index(idx).map(PlayerAction::BuySoulPerk)
            }
            's' | 'S' if matches!(self.state.tab, Tab::Gacha) => Some(PlayerAction::GachaPull(1)),
            'x' | 'X' if matches!(self.state.tab, Tab::Gacha) => Some(PlayerAction::GachaPull(10)),
            // タブ本体スクロール。タブ非依存で動作 (どのタブでも上下できる)。
            // main.rs:362-366 で矢印キー → h/j/k/l に既に map されているため
            // ↑/↓ も自動的に動く。h/l は abyss では未使用なので競合なし。
            'j' | 'J' => Some(PlayerAction::ScrollDown),
            'k' | 'K' => Some(PlayerAction::ScrollUp),
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
            let save_after = is_save_worthy(a);
            logic::apply_action(&mut self.state, a);
            if save_after {
                self.flush_save();
            }
            true
        } else {
            false
        }
    }

    fn tick(&mut self, delta_ticks: u32) {
        let prev_floor = self.state.floor;
        let prev_deaths = self.state.deaths;
        logic::tick(&mut self.state, delta_ticks);

        // tick 内発生の進捗節目: 階層クリア (floor++) と死亡 (deaths++)。
        let event_save = self.state.floor != prev_floor || self.state.deaths != prev_deaths;
        // 保険の定期セーブ: ミルストーンが起きない長時間プレイ
        // (auto_descend OFF で同フロア周回 etc) でも進捗を落とさないため。
        self.save_countdown = self.save_countdown.saturating_sub(delta_ticks);
        let timer_save = self.save_countdown == 0;

        if event_save || timer_save {
            self.flush_save();
        }
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
        g.handle_input(&click(TAB_ROADMAP));
        assert_eq!(g.state.tab, Tab::Roadmap);
        g.handle_input(&click(TAB_STATS));
        assert_eq!(g.state.tab, Tab::Stats);
        g.handle_input(&click(TAB_UPGRADES));
        assert_eq!(g.state.tab, Tab::Upgrades);
    }

    #[test]
    fn key_buy_upgrade_only_in_upgrades_tab() {
        let mut g = AbyssGame::new();
        g.state.gold = 1000;
        // タブ Roadmap なら反応しない
        g.state.tab = Tab::Roadmap;
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
        // タブが Roadmap でもクリックなら反応
        g.state.tab = Tab::Roadmap;
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
        // 魂強化購入は強化タブ統合後 Tab::Upgrades 内から行う。
        // 小文字 `q` は back-to-menu と衝突するため**大文字** Q/W/E/R を使う。
        g.state.tab = Tab::Upgrades;
        g.state.souls = 100;
        g.handle_input(&InputEvent::Key('Q'));
        assert_eq!(g.state.soul_perks[SoulPerk::Might.index()], 1);
    }

    /// 小文字 `q` は main.rs の back-to-menu キー (Esc 同等)。Upgrades タブ
    /// 統合 (#78) の前は 'q' を魂購入に bind していたが、デフォルトタブが
    /// Upgrades になった現在は q をゲーム側で消費すると最も滞在時間の長い
    /// 画面でメニュー戻りが効かなくなる。Codex P1 レビューで指摘されたバグ
    /// の回帰防止: **`q` は handle_input() で消費されない (= false が返る)**。
    #[test]
    fn lowercase_q_does_not_consume_input_on_upgrades_tab() {
        let mut g = AbyssGame::new();
        g.state.tab = Tab::Upgrades;
        g.state.souls = 999_999; // 購入余地あっても q を bind してはいけない。
        let consumed = g.handle_input(&InputEvent::Key('q'));
        assert!(
            !consumed,
            "小文字 'q' を消費するとメニュー戻りが効かなくなる"
        );
        assert_eq!(
            g.state.soul_perks[SoulPerk::Might.index()],
            0,
            "小文字 'q' で誤って魂パークが買われてはいけない"
        );
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

    #[test]
    fn timer_save_fires_after_autosave_interval() {
        let mut g = AbyssGame::new();
        // 初期状態では満タン。
        assert_eq!(g.save_countdown, save::AUTOSAVE_INTERVAL);
        // インターバル分まで進めると 0 に到達 → tick 内で flush されてリセット。
        g.tick(save::AUTOSAVE_INTERVAL);
        assert_eq!(g.save_countdown, save::AUTOSAVE_INTERVAL);
    }

    #[test]
    fn event_save_resets_timer_to_avoid_double_write() {
        let mut g = AbyssGame::new();
        g.state.gold = 10_000;
        // タイマーを少しだけ進めた状態にする (中途半端な残り時間)。
        g.tick(100);
        assert_eq!(g.save_countdown, save::AUTOSAVE_INTERVAL - 100);
        // upgrade 購入 = イベントセーブ発火 → タイマーは満タンに戻るべき。
        g.handle_input(&InputEvent::Key('1')); // タブが Upgrades なら Sword を購入
        assert_eq!(g.save_countdown, save::AUTOSAVE_INTERVAL);
    }

    #[test]
    fn save_worthy_actions_classified_correctly() {
        assert!(is_save_worthy(PlayerAction::BuyUpgrade(UpgradeKind::Sword)));
        assert!(is_save_worthy(PlayerAction::BuySoulPerk(SoulPerk::Might)));
        assert!(is_save_worthy(PlayerAction::GachaPull(1)));
        assert!(is_save_worthy(PlayerAction::Retreat));
        assert!(is_save_worthy(PlayerAction::ToggleAutoDescend));
        // SetTab は UI 状態のみ変化させるためセーブを発火しない。
        assert!(!is_save_worthy(PlayerAction::SetTab(Tab::Roadmap)));
    }
}
