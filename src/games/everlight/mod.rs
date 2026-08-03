//! 常夜灯 — 降り注ぐ魔物から最後の灯を守り抜く、縦画面バレットヘヴン×
//! タワーディフェンス。
//!
//! コアループ:
//!   1. 拠点で残光を払って恒久強化を整え、夜番へ出る
//!   2. 灯を左右のレーンへ動かしながら、自動発砲する武器で敵を迎撃する
//!   3. 精鋭/魔王を倒すと宝箱が落ちる。受け止めるとレベルアップ選択肢が
//!      開き、新しい武器/効果を得るか既存を強化するかを選ぶ
//!   4. 灯が0になる、または自ら撤退すると拠点へ戻る。波数と生存時間の
//!      自己ベストが記録される (残光は失われない)

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

use crate::effects::FrameClock;
use crate::games::{Game, GameChoice};
use crate::input::{ClickState, InputEvent};
use crate::sound;
use crate::time;
use crate::widgets::ClickableGrid;

use actions::*;
use effects::EverlightEffects;
use state::{Phase, COLUMNS};

pub struct EverlightGame {
    pub state: state::EverlightState,
    effects: RefCell<EverlightEffects>,
    prev: Cell<PrevSnapshot>,
    frame_clock: FrameClock,
    save_countdown: u32,
}

/// 前フレームのカウンタのスナップショット。render 毎に差分を見て演出を
/// トリガする (`detect_transitions`)。値そのものではなく単調増加カウンタの
/// 差分を見るのは、1回のrenderに複数tickがまとまった時に演出の発火を
/// 取りこぼさないため (loopmarchと同じ設計、state.rsのコメント参照)。
#[derive(Clone, Copy)]
struct PrevSnapshot {
    phase: Phase,
    wave: u32,
    breach_count: u32,
    light_hit_count: u32,
    chest_caught_count: u32,
    boss_spawn_count: u32,
}

impl PrevSnapshot {
    fn capture(state: &state::EverlightState) -> Self {
        Self {
            phase: state.phase,
            wave: state.wave,
            breach_count: state.breach_count,
            light_hit_count: state.light_hit_count,
            chest_caught_count: state.chest_caught_count,
            boss_spawn_count: state.boss_spawn_count,
        }
    }
}

impl Default for EverlightGame {
    fn default() -> Self {
        Self::new()
    }
}

impl EverlightGame {
    pub fn new() -> Self {
        #[allow(unused_mut)]
        let mut state = state::EverlightState::new();
        #[cfg(target_arch = "wasm32")]
        save::load_game(&mut state);
        let prev = PrevSnapshot::capture(&state);
        Self {
            state,
            effects: RefCell::new(EverlightEffects::new()),
            prev: Cell::new(prev),
            frame_clock: FrameClock::new(),
            save_countdown: save::AUTOSAVE_INTERVAL,
        }
    }

    fn handle_key(&mut self, ch: char) -> bool {
        if self.state.pending_boons.is_some() {
            return match ch {
                '1' => self.choose_boon_and_notify(0),
                '2' => self.choose_boon_and_notify(1),
                '3' => self.choose_boon_and_notify(2),
                _ => false,
            };
        }
        match self.state.phase {
            Phase::Camp => match ch {
                '1' => self.buy_and_notify(logic::purchase_light),
                '2' => self.buy_and_notify(logic::purchase_power),
                '3' => self.buy_and_notify(logic::purchase_extra_slot),
                ' ' => {
                    logic::start_vigil(&mut self.state);
                    sound::play(sound::SELECT);
                    true
                }
                'k' => {
                    adjust_scroll(&self.state.camp_scroll, -3);
                    true
                }
                'j' => {
                    adjust_scroll(&self.state.camp_scroll, 3);
                    true
                }
                _ => false,
            },
            Phase::Vigil => match ch {
                'h' => {
                    logic::nudge_lantern(&mut self.state, -1);
                    true
                }
                'l' => {
                    logic::nudge_lantern(&mut self.state, 1);
                    true
                }
                'r' => {
                    logic::retreat_to_camp(&mut self.state);
                    sound::play(sound::CLICK);
                    true
                }
                _ => false,
            },
        }
    }

    fn handle_click(&mut self, action_id: u16) -> bool {
        if self.state.pending_boons.is_some() {
            if (BOON_OPTION_BASE..BOON_OPTION_BASE + 3).contains(&action_id) {
                return self.choose_boon_and_notify((action_id - BOON_OPTION_BASE) as usize);
            }
            return false;
        }
        match self.state.phase {
            Phase::Camp => match action_id {
                CAMP_UPGRADE_LIGHT => self.buy_and_notify(logic::purchase_light),
                CAMP_UPGRADE_POWER => self.buy_and_notify(logic::purchase_power),
                CAMP_UPGRADE_EXTRA_SLOT => self.buy_and_notify(logic::purchase_extra_slot),
                CAMP_START_VIGIL => {
                    logic::start_vigil(&mut self.state);
                    sound::play(sound::SELECT);
                    true
                }
                CAMP_SCROLL_UP => {
                    adjust_scroll(&self.state.camp_scroll, -3);
                    true
                }
                CAMP_SCROLL_DOWN => {
                    adjust_scroll(&self.state.camp_scroll, 3);
                    true
                }
                _ => false,
            },
            Phase::Vigil => match action_id {
                RETREAT_TO_CAMP => {
                    logic::retreat_to_camp(&mut self.state);
                    sound::play(sound::CLICK);
                    true
                }
                id if (LANE_CLICK_BASE..LANE_CLICK_BASE + COLUMNS as u16).contains(&id) => {
                    if let Some((lane, _row)) = ClickableGrid::decode(LANE_CLICK_BASE, COLUMNS, id) {
                        logic::set_lantern_target_lane(&mut self.state, lane);
                    }
                    true
                }
                _ => false,
            },
        }
    }

    fn buy_and_notify(&mut self, purchase: impl FnOnce(&mut state::EverlightState) -> bool) -> bool {
        if purchase(&mut self.state) {
            sound::play(sound::PURCHASE);
            // 次の定期autosaveを待つと、直後にリロードされた場合に
            // 恒久強化の購入が消えてしまうため即座に保存する。
            self.flush_save();
        } else {
            sound::play(sound::ERROR);
        }
        true
    }

    fn choose_boon_and_notify(&mut self, index: usize) -> bool {
        if logic::choose_boon(&mut self.state, index) {
            sound::play(sound::LEVEL_UP);
        }
        true
    }

    fn detect_transitions(&self, area: Rect) {
        let prev = self.prev.get();
        let mut effects = self.effects.borrow_mut();
        let header = Rect::new(area.x, area.y, area.width, 4.min(area.height));

        if self.state.light_hit_count != prev.light_hit_count {
            effects.push_light_hit(header);
        }
        if self.state.breach_count != prev.breach_count {
            effects.push_breach(area);
        }
        if self.state.chest_caught_count != prev.chest_caught_count {
            effects.push_chest_caught(area);
        }
        if self.state.boss_spawn_count != prev.boss_spawn_count {
            effects.push_boss_appear(area);
        }
        // `!=` だと、夜番終了後 `start_vigil` が wave を1へ戻した時にも
        // 「波が変わった」と誤検知して無関係な進行演出が出てしまう
        // (wave は breach_count 等と違って表示上リセットが必要なので、
        // 値を単調にする代わりにここで増加方向のみを見る)。
        if self.state.wave > prev.wave {
            effects.push_wave_advance(header);
        }
        if prev.phase == Phase::Vigil && self.state.phase == Phase::Camp {
            effects.push_vigil_end(area);
        }

        drop(effects);
        self.prev.set(PrevSnapshot::capture(&self.state));
    }

    fn flush_save(&mut self) {
        #[cfg(target_arch = "wasm32")]
        save::save_game(&self.state);
        self.save_countdown = save::AUTOSAVE_INTERVAL;
    }
}

impl Game for EverlightGame {
    fn choice(&self) -> GameChoice {
        GameChoice::Everlight
    }

    fn handle_input(&mut self, event: &InputEvent) -> bool {
        match event {
            InputEvent::Key(c) => self.handle_key(*c),
            InputEvent::Click(_, id) => self.handle_click(*id),
        }
    }

    fn tick(&mut self, delta_ticks: u32) {
        let was_vigil = self.state.phase == Phase::Vigil;
        logic::tick_n(&mut self.state, delta_ticks);

        if was_vigil && self.state.phase == Phase::Camp {
            // 夜番終了でember/自己ベストが確定した直後 — 次の定期autosave
            // (最大30秒後) を待つとリロードで記録が失われうるため即座に保存する。
            self.flush_save();
        } else if self.save_countdown > delta_ticks {
            self.save_countdown -= delta_ticks;
        } else {
            self.flush_save();
        }
    }

    fn on_leave(&mut self) {
        self.flush_save();
    }

    fn render(&self, f: &mut Frame, area: Rect, click_state: &Rc<RefCell<ClickState>>) {
        self.detect_transitions(area);
        render::render(&self.state, f, area, click_state);
        let elapsed = self.frame_clock.elapsed(time::now_ms().unwrap_or(0.0));
        self.effects.borrow_mut().process(elapsed, f.buffer_mut(), area);
    }
}

/// `Cell<u16>` スクロール値を負にならないよう飽和加算/減算で更新する。
/// 上限側のクランプは描画側 (`ScrollableTab`) がコンテンツ高さに合わせて行う。
fn adjust_scroll(cell: &Cell<u16>, delta: i32) {
    let cur = cell.get() as i32;
    let next = (cur + delta).clamp(0, u16::MAX as i32) as u16;
    cell.set(next);
}

#[cfg(test)]
mod tests {
    use super::*;
    use state::{BoonKind, BoonOption, WeaponKind};

    fn click(id: u16) -> InputEvent {
        InputEvent::Click(crate::input::ClickScope::Game(GameChoice::Everlight), id)
    }

    #[test]
    fn choice_reports_everlight() {
        let game = EverlightGame::new();
        assert_eq!(game.choice(), GameChoice::Everlight);
    }

    #[test]
    fn start_vigil_via_click_transitions_phase() {
        let mut game = EverlightGame::new();
        assert!(game.handle_input(&click(CAMP_START_VIGIL)));
        assert_eq!(game.state.phase, Phase::Vigil);
    }

    #[test]
    fn tick_advances_vigil_and_autosave_countdown() {
        let mut game = EverlightGame::new();
        logic::start_vigil(&mut game.state);
        let before = game.save_countdown;
        game.tick(5);
        assert_eq!(game.state.elapsed_ticks, 5);
        assert_eq!(game.save_countdown, before - 5);
    }

    #[test]
    fn purchase_flushes_save_immediately_instead_of_waiting_for_autosave() {
        // 定期autosave (最大30秒後) を待つ間にリロードされると恒久強化の
        // 購入が失われるため、購入成功時は即座に保存されるはず。
        // `flush_save` は必ず `save_countdown` をリセットするので、遠い
        // 値に設定してから購入し、リセットされたことでflushを検証する。
        let mut game = EverlightGame::new();
        game.state.ember = 999_999;
        game.save_countdown = save::AUTOSAVE_INTERVAL + 1000;
        assert!(game.buy_and_notify(logic::purchase_light));
        assert_eq!(
            game.save_countdown,
            save::AUTOSAVE_INTERVAL,
            "購入成功時にflush_saveが呼ばれてsave_countdownがリセットされるはず"
        );
    }

    #[test]
    fn vigil_end_flushes_save_immediately_instead_of_waiting_for_autosave() {
        // 夜番終了でember/自己ベストが確定した直後にリロードされても
        // 記録が失われないよう、定期autosaveを待たず即座に保存されるはず。
        let mut game = EverlightGame::new();
        logic::start_vigil(&mut game.state);
        game.state.lantern.light = 0;
        game.save_countdown = save::AUTOSAVE_INTERVAL + 1000;

        game.tick(1);

        assert_eq!(game.state.phase, Phase::Camp, "灯が尽きて夜番が終了しているはず");
        assert_eq!(
            game.save_countdown,
            save::AUTOSAVE_INTERVAL,
            "夜番終了時にflush_saveが呼ばれてsave_countdownがリセットされるはず"
        );
    }

    #[test]
    fn lane_click_moves_lantern_target() {
        let mut game = EverlightGame::new();
        logic::start_vigil(&mut game.state);
        assert!(game.handle_input(&click(LANE_CLICK_BASE)));
        assert_eq!(game.state.lantern.target_lane, 0);
        assert!(game.handle_input(&click(LANE_CLICK_BASE + 3)));
        assert_eq!(game.state.lantern.target_lane, 3);
    }

    #[test]
    fn retreat_click_returns_to_camp() {
        let mut game = EverlightGame::new();
        logic::start_vigil(&mut game.state);
        assert!(game.handle_input(&click(RETREAT_TO_CAMP)));
        assert_eq!(game.state.phase, Phase::Camp);
    }

    #[test]
    fn boon_modal_click_is_consumed_and_ignores_lane_taps() {
        let mut game = EverlightGame::new();
        logic::start_vigil(&mut game.state);
        game.state.pending_boons = Some([
            BoonOption { kind: BoonKind::NewWeapon(WeaponKind::Spray) },
            BoonOption { kind: BoonKind::NewWeapon(WeaponKind::Aurora) },
            BoonOption { kind: BoonKind::NewWeapon(WeaponKind::Halo) },
        ]);
        // モーダル表示中はレーンタップを無視する (誤操作防止)。
        assert!(!game.handle_input(&click(LANE_CLICK_BASE)));
        assert!(game.handle_input(&click(BOON_OPTION_BASE + 1)));
        assert!(game.state.loadout.weapons.iter().any(|w| w.kind == WeaponKind::Aurora));
    }

    #[test]
    fn detect_transitions_triggers_effect_on_light_hit() {
        let mut game = EverlightGame::new();
        logic::start_vigil(&mut game.state);
        let area = Rect::new(0, 0, 40, 30);
        // 初回呼び出しでprevスナップショットを確定させておく。
        game.detect_transitions(area);
        assert!(!game.effects.borrow().is_running(), "変化が無ければ演出は起きないはず");

        game.state.light_hit_count += 1;
        game.detect_transitions(area);
        assert!(
            game.effects.borrow().is_running(),
            "light_hit_count の増加でdetect_transitionsが演出を積むはず"
        );
    }

    #[test]
    fn detect_transitions_does_not_flash_when_wave_decreases() {
        // waveは夜番終了→start_vigilで1に戻る (breach_count等と違って
        // HUD表示のためにリセットが必要)。`!=` で比較すると、この減少も
        // 「波が変わった」と誤検知して無関係な進行演出が誤発火してしまう。
        let mut game = EverlightGame::new();
        logic::start_vigil(&mut game.state);
        game.state.wave = 5;
        // detect_transitions経由だと1→5の増加そのもので演出を積んでしまうため、
        // 演出を発火させずにprevスナップショットだけ5に合わせる。
        game.prev.set(PrevSnapshot::capture(&game.state));
        let area = Rect::new(0, 0, 40, 30);

        game.state.wave = 1; // 夜番の再スタート等でwaveが減った状況を模す
        game.detect_transitions(area);
        assert!(
            !game.effects.borrow().is_running(),
            "waveが減った時に進行演出が誤発火してはいけない"
        );
    }

    #[test]
    fn render_does_not_panic_across_phases() {
        // `Game::render` (トレイトメソッド) は内部で `time::now_ms()` を
        // 呼ぶが、これは web_sys 経由のため non-wasm ネイティブテストでは
        // panic する (他ゲームのmod.rsテストも同じ理由でトレイト経由の
        // render は呼ばず、`render::render` を直接叩く)。ここでは
        // tick→render のロジック的な連なりだけを検証する。
        use ratzilla::ratatui::backend::TestBackend;
        use ratzilla::ratatui::Terminal;

        let mut game = EverlightGame::new();
        let click_state = Rc::new(RefCell::new(ClickState::new()));
        let mut terminal = Terminal::new(TestBackend::new(40, 30)).unwrap();
        terminal.draw(|f| render::render(&game.state, f, f.area(), &click_state)).unwrap();

        game.tick(1);
        logic::start_vigil(&mut game.state);
        game.tick(80);
        terminal.draw(|f| render::render(&game.state, f, f.area(), &click_state)).unwrap();
    }
}
