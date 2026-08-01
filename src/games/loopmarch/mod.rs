//! 周回討伐 — ループ状の道に地形タイルを配置し、勇者が自動で周回して
//! 戦うローグライト。地形は資源(木材/石材/魂)を生むが敵の湧きや強さも
//! 上げる、諸刃の剣として機能する。
//!
//! コアループ:
//!   1. 拠点で恒久強化 (魂で購入) を整えて遠征へ出発
//!   2. 手札の地形カードを道に配置しながら、勇者が自動周回して戦う
//!   3. 死亡すると拠点へ撤退。木材/石材と道の配置は失うが、魂と拠点強化は残る

pub mod actions;
pub mod effects;
pub mod logic;
pub mod render;
pub mod save;
pub mod state;

#[cfg(test)]
mod simulator;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use ratzilla::ratatui::layout::Rect;
use ratzilla::ratatui::Frame;

use crate::effects::{FlashTimer, FrameClock};
use crate::games::{Game, GameChoice};
use crate::input::{is_narrow_layout, ClickState, InputEvent};
use crate::sound;
use crate::time;
use crate::widgets::ClickableGrid;

use actions::*;
use effects::LoopMarchEffects;
use logic::UpgradeKind;
use state::{Phase, HAND_MAX, RING_H, RING_W};

/// 拠点画面のスクロール1クリックあたりの行数。
const CAMP_SCROLL_STEP: i32 = 2;

pub struct LoopMarchGame {
    pub state: state::LoopMarchState,
    effects: RefCell<LoopMarchEffects>,
    prev: Cell<PrevSnapshot>,
    frame_clock: FrameClock,
    save_countdown: u32,
}

impl Default for LoopMarchGame {
    fn default() -> Self {
        Self::new()
    }
}

/// 前フレームの state スナップショット。render 毎に差分を見て演出をトリガする
/// (`detect_transitions`)。
#[derive(Clone, Copy, Default)]
struct PrevSnapshot {
    lap: u32,
    best_lap: u32,
    run_active: bool,
    hero_hurt_flash: FlashTimer,
    enemy_hurt_flash: FlashTimer,
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

        let prev = Self::snapshot(&state);
        Self {
            state,
            effects: RefCell::new(LoopMarchEffects::new()),
            prev: Cell::new(prev),
            frame_clock: FrameClock::new(),
            save_countdown: save::AUTOSAVE_INTERVAL,
        }
    }

    fn snapshot(s: &state::LoopMarchState) -> PrevSnapshot {
        PrevSnapshot {
            lap: s.lap,
            best_lap: s.best_lap,
            run_active: s.run_active,
            hero_hurt_flash: s.hero_hurt_flash,
            enemy_hurt_flash: s.enemy_hurt_flash,
        }
    }

    fn detect_transitions(&self, area: Rect) {
        let prev = self.prev.get();
        let mut effects = self.effects.borrow_mut();
        let s = &self.state;

        if s.phase == Phase::Expedition {
            let layout = render::compute_expedition_layout(area, is_narrow_layout(area.width));

            if !prev.hero_hurt_flash.is_active() && s.hero_hurt_flash.is_active() {
                effects.push_hero_hit(layout.header);
                sound::play(sound::DAMAGE);
            }
            if !prev.enemy_hurt_flash.is_active() && s.enemy_hurt_flash.is_active() {
                effects.push_enemy_hit(layout.ring);
            }
            if s.lap > prev.lap {
                effects.push_lap_complete(layout.ring);
                sound::play(sound::VICTORY);
            }
        }

        if s.best_lap > prev.best_lap {
            effects.push_best_lap_achievement(area);
        }

        if prev.run_active && !s.run_active {
            effects.push_death(area);
            sound::play(sound::DEFEAT);
        }

        self.prev.set(Self::snapshot(s));
    }

    fn handle_click(&mut self, action_id: u16) -> bool {
        match action_id {
            CAMP_UPGRADE_MAX_HP => {
                self.purchase_and_flush(UpgradeKind::MaxHp);
                true
            }
            CAMP_UPGRADE_ATTACK => {
                self.purchase_and_flush(UpgradeKind::Attack);
                true
            }
            CAMP_UPGRADE_EXTRA_CARD => {
                self.purchase_and_flush(UpgradeKind::ExtraCard);
                true
            }
            CAMP_START_OR_RESUME => {
                self.start_or_resume_and_flush();
                true
            }
            REFILL_HAND => {
                self.refill_and_flush();
                true
            }
            GO_TO_CAMP => {
                logic::go_to_camp(&mut self.state);
                sound::play(sound::CLICK);
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
            HAND_SCROLL_UP => {
                self.state.scroll_hand(-CAMP_SCROLL_STEP);
                true
            }
            HAND_SCROLL_DOWN => {
                self.state.scroll_hand(CAMP_SCROLL_STEP);
                true
            }
            id if (HAND_CLICK_BASE..HAND_CLICK_BASE + HAND_MAX as u16).contains(&id) => {
                logic::select_hand(&mut self.state, (id - HAND_CLICK_BASE) as usize);
                sound::play(sound::CLICK);
                true
            }
            id if (PATH_CLICK_BASE..PATH_CLICK_BASE + (RING_W * RING_H) as u16).contains(&id) => {
                if let Some((gx, gy)) = ClickableGrid::decode(PATH_CLICK_BASE, RING_W, id) {
                    if let Some(path_index) = logic::ring_index_at(gx, gy) {
                        let placed = logic::place_selected(&mut self.state, path_index);
                        sound::play(if placed { sound::CLICK } else { sound::ERROR });
                    }
                }
                true
            }
            _ => false,
        }
    }

    fn flush_save(&mut self) {
        #[cfg(target_arch = "wasm32")]
        save::save_game(&self.state);
        self.save_countdown = save::AUTOSAVE_INTERVAL;
    }

    /// 拠点強化を購入し、成功時は即セーブする。ブラウザのタブを閉じる/
    /// リロードは検知できない (`pagehide` 等のフックが無い) ため、頻度の
    /// 低い永続データの変更はオートセーブのタイマーを待たずその場で
    /// 書き込み、離脱タイミングに関わらず失われないようにする。
    fn purchase_and_flush(&mut self, kind: UpgradeKind) -> bool {
        let bought = logic::purchase_upgrade(&mut self.state, kind);
        sound::play(if bought { sound::PURCHASE } else { sound::ERROR });
        if bought {
            self.flush_save();
        }
        bought
    }

    /// 遠征を開始/再開する。新規開始時は手札を引くために rng_state が
    /// 進む — `Game::tick` の差分検知は tick 間の変化しか見ないため、
    /// クリック/キー操作起因のこの変化は自分で明示的に保存する。
    fn start_or_resume_and_flush(&mut self) {
        let rng_before = self.state.rng_state;
        logic::start_or_resume_expedition(&mut self.state);
        sound::play(sound::CLICK);
        if self.state.rng_state != rng_before {
            self.flush_save();
        }
    }

    /// 手札を補充し、成功時は即セーブする。補充は乱数でカードを引くため
    /// rng_state を進める。
    fn refill_and_flush(&mut self) -> bool {
        let refilled = logic::refill_hand(&mut self.state);
        sound::play(if refilled { sound::PURCHASE } else { sound::ERROR });
        if refilled {
            self.flush_save();
        }
        refilled
    }

    fn handle_key(&mut self, key: char) -> bool {
        match self.state.phase {
            Phase::Camp => match key {
                '1' => {
                    self.purchase_and_flush(UpgradeKind::MaxHp);
                    true
                }
                '2' => {
                    self.purchase_and_flush(UpgradeKind::Attack);
                    true
                }
                '3' => {
                    self.purchase_and_flush(UpgradeKind::ExtraCard);
                    true
                }
                ' ' | 's' => {
                    self.start_or_resume_and_flush();
                    true
                }
                _ => false,
            },
            Phase::Expedition => match key {
                '1' | '2' | '3' | '4' => {
                    let idx = (key as u8 - b'1') as usize;
                    logic::select_hand(&mut self.state, idx);
                    sound::play(sound::CLICK);
                    true
                }
                'r' => {
                    self.refill_and_flush();
                    true
                }
                'c' => {
                    logic::go_to_camp(&mut self.state);
                    sound::play(sound::CLICK);
                    true
                }
                'h' => {
                    logic::move_cursor(&mut self.state, -1);
                    true
                }
                'l' => {
                    logic::move_cursor(&mut self.state, 1);
                    true
                }
                ' ' => {
                    let cursor = self.state.cursor;
                    let placed = logic::place_selected(&mut self.state, cursor);
                    sound::play(if placed { sound::CLICK } else { sound::ERROR });
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
        // 魂・自己ベスト周回数・乱数状態は tick 中 (討伐報酬・草原到達・
        // 周回達成・モンスター湧き判定) に変わりうる永続データ。ブラウザの
        // タブを閉じる/リロードするタイミングは検知できないため、変化した
        // 瞬間にタイマーを待たず保存しておく。
        let persistent_before = (self.state.soul, self.state.best_lap, self.state.rng_state);

        logic::tick_n(&mut self.state, delta_ticks);

        // 死亡直後(run_active: true→false)はタイマーを待たず即セーブする。
        // 「死んでも魂は残る」が核となる約束なので、その直後にリロード/
        // タブを閉じられても失われないようにする。
        let died_this_tick = was_run_active && !self.state.run_active;
        let persistent_changed = (self.state.soul, self.state.best_lap, self.state.rng_state)
            != persistent_before;

        self.save_countdown = self.save_countdown.saturating_sub(delta_ticks);
        if self.save_countdown == 0 || died_this_tick || persistent_changed {
            self.flush_save();
        }
    }

    fn on_leave(&mut self) {
        // メニューに戻る操作でこのインスタンスは破棄されるため、直近の
        // オートセーブ以降に増えた魂・拠点強化・乱数状態を確実に残す。
        self.flush_save();
    }

    fn render(&self, f: &mut Frame, area: Rect, click_state: &Rc<RefCell<ClickState>>) {
        self.detect_transitions(area);
        render::render(&self.state, f, area, click_state);
        let elapsed = self.frame_clock.elapsed(time::now_ms().unwrap_or(0.0));
        self.effects.borrow_mut().process(elapsed, f.buffer_mut(), area);
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
    fn keyboard_only_flow_can_place_terrain() {
        // Codex指摘: キーボードのみでは地形配置ができなかった。
        // 選択(数字キー) → 移動(h/l) → 配置(space) が一通り動くことを確認する。
        let mut game = LoopMarchGame::new();
        game.handle_input(&InputEvent::Key(' ')); // 拠点で遠征開始
        game.handle_input(&InputEvent::Key('1')); // 手札0番を選択
        assert_eq!(game.state.selected_hand, Some(0));

        game.handle_input(&InputEvent::Key('l'));
        game.handle_input(&InputEvent::Key('l'));
        assert_eq!(game.state.cursor, 2);

        game.handle_input(&InputEvent::Key(' ')); // カーソル位置に配置
        assert!(game.state.path[2].terrain.is_some());
        assert_eq!(game.state.selected_hand, None, "配置後は選択解除される");
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
    fn soul_gain_during_tick_forces_immediate_save() {
        // タブを閉じる/リロードは検知できないので、魂が増えた瞬間に
        // タイマーを待たず保存しておかないと、その進捗は失われる。
        let mut game = LoopMarchGame::new();
        game.handle_input(&click(CAMP_START_OR_RESUME));
        game.save_countdown = save::AUTOSAVE_INTERVAL + 1000;
        game.state.path[0].terrain = Some(state::Terrain::Meadow);
        game.state.hero.position = state::PATH_LEN - 1;
        game.state.move_progress = state::MOVE_TICKS - 1;

        game.tick(1); // 草原に到達 → 魂+1

        assert_eq!(game.state.soul, 1);
        assert_eq!(
            game.save_countdown,
            save::AUTOSAVE_INTERVAL,
            "魂が増えた tick は即セーブ扱いになるべき"
        );
    }

    #[test]
    fn starting_new_expedition_flushes_save_immediately() {
        // Codex指摘: 遠征開始は手札を引くためrng_stateを進めるが、
        // tickベースの差分検知はクリック操作起因の変化を捉えられない。
        // 明示的なflushで即座に保存されるべき。
        let mut game = LoopMarchGame::new();
        game.save_countdown = save::AUTOSAVE_INTERVAL + 1000;

        game.handle_input(&click(CAMP_START_OR_RESUME));

        assert_eq!(game.state.phase, Phase::Expedition);
        assert_eq!(
            game.save_countdown,
            save::AUTOSAVE_INTERVAL,
            "新規遠征開始(手札を引く=rng_state進行)は即セーブされるべき"
        );
    }

    #[test]
    fn resuming_active_expedition_does_not_force_flush() {
        // 遠征中に拠点を覗いて戻るだけなら rng_state は変化しないので、
        // 無駄な書き込みを避けるべき。
        let mut game = LoopMarchGame::new();
        game.handle_input(&click(CAMP_START_OR_RESUME));
        logic::go_to_camp(&mut game.state);
        game.save_countdown = save::AUTOSAVE_INTERVAL + 1000;

        game.handle_input(&click(CAMP_START_OR_RESUME));

        assert_eq!(game.state.phase, Phase::Expedition);
        assert_eq!(
            game.save_countdown,
            save::AUTOSAVE_INTERVAL + 1000,
            "拠点⇔遠征の表示切り替えだけでは何も変化していないのでセーブ不要"
        );
    }

    #[test]
    fn refill_hand_click_flushes_save_immediately() {
        // Codex指摘: 手札補充もランダムなカードを引くためrng_stateを
        // 進めるが、tickベースの差分検知では捉えられない。
        let mut game = LoopMarchGame::new();
        game.handle_input(&click(CAMP_START_OR_RESUME));
        game.state.hand = vec![None; HAND_MAX];
        game.state.wood = 10;
        game.state.stone = 10;
        game.save_countdown = save::AUTOSAVE_INTERVAL + 1000;

        game.handle_input(&click(REFILL_HAND));

        assert!(game.state.hand.iter().any(|c| c.is_some()));
        assert_eq!(
            game.save_countdown,
            save::AUTOSAVE_INTERVAL,
            "手札補充成功(rng_state進行)は即セーブされるべき"
        );
    }

    #[test]
    fn purchase_upgrade_click_flushes_save_immediately() {
        let mut game = LoopMarchGame::new();
        game.save_countdown = save::AUTOSAVE_INTERVAL + 1000;
        game.state.soul = 100;

        game.handle_input(&click(CAMP_UPGRADE_MAX_HP));

        assert_eq!(game.state.camp.max_hp_level, 1);
        assert_eq!(
            game.save_countdown,
            save::AUTOSAVE_INTERVAL,
            "拠点強化の購入成功時は即セーブされるべき"
        );
    }

    #[test]
    fn failed_purchase_does_not_reset_save_timer() {
        let mut game = LoopMarchGame::new();
        game.save_countdown = save::AUTOSAVE_INTERVAL + 1000;
        game.state.soul = 0; // 買えない

        game.handle_input(&click(CAMP_UPGRADE_MAX_HP));

        assert_eq!(
            game.save_countdown,
            save::AUTOSAVE_INTERVAL + 1000,
            "何も変化していないのに毎回セーブし直す必要はない"
        );
    }

    #[test]
    fn on_leave_flushes_save_before_returning_to_menu() {
        let mut game = LoopMarchGame::new();
        game.save_countdown = save::AUTOSAVE_INTERVAL + 1000; // オートセーブにはまだ遠い
        game.state.soul = 42;

        game.on_leave();

        assert_eq!(
            game.save_countdown,
            save::AUTOSAVE_INTERVAL,
            "メニューへ戻る直前はオートセーブのタイマーを待たず即セーブする"
        );
    }

    #[test]
    fn unknown_click_id_not_consumed() {
        let mut game = LoopMarchGame::new();
        // 9 は CAMP_*/CAMP_SCROLL_* (1-8) にも HAND_CLICK_BASE.. (10-13) にも
        // PATH_CLICK_BASE.. (100+) にも属さない未使用の隙間。
        assert!(!game.handle_input(&click(9)));
    }

    /// リング範囲内 (PATH_CLICK_BASE..+RING_W*RING_H) の decode 失敗
    /// (=リング内部セル) はクリックイベントとして消費するが無視する。
    #[test]
    fn interior_grid_id_within_range_is_consumed_but_ignored() {
        let mut game = LoopMarchGame::new();
        game.handle_input(&click(CAMP_START_OR_RESUME));
        let before = game.state.selected_hand;
        // (1,1) はリング内部。id = PATH_CLICK_BASE + 1*RING_W + 1 (範囲内)
        assert!(game.handle_input(&click(PATH_CLICK_BASE + RING_W as u16 + 1)));
        assert_eq!(game.state.selected_hand, before);
    }

    /// 範囲外の id (例: 戻るボタンの BACK_TO_MENU=65535) は道グリッドの
    /// クリックとして飲み込んではならない。飲み込むと `handle_input` が
    /// `true` を返し、main.rs の「戻る」処理が握り潰されてしまう
    /// (回帰防止: Codexレビュー指摘)。
    #[test]
    fn out_of_range_id_like_back_button_is_not_consumed() {
        let mut game = LoopMarchGame::new();
        game.handle_input(&click(CAMP_START_OR_RESUME));
        assert!(
            !game.handle_input(&click(crate::BACK_TO_MENU)),
            "戻るボタンのidを道クリックとして飲み込んでしまっている"
        );
    }
}
