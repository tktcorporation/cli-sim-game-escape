//! 玉響 (Tamayura) — 釘を読んで台を選び、一発の行方に祈るパチンコホール。
//!
//! コアループ:
//!   1. ホールで台を選ぶ。台ごとに釘の開き・傾きが違うので、盤面の見た目
//!      から「回りそうな台」を読む
//!   2. 着席してハンドル強度を決め、打ち出す。玉は釘に弾かれて散らばり、
//!      ヘソ (スタートチャッカー) に入るとデジタルが回る
//!   3. 当たればアタッカーが開いて出玉が増える。確変・時短の間はヘソが
//!      広がり、次の当たりへ繋がりやすくなる
//!   4. 軍資金は有限。実測した回転率と収支を見て、粘るか台を替えるか
//!      席を立つかを決める

pub mod actions;
pub mod board;
pub mod logic;
pub mod nails;
mod physics;
pub mod render;
mod rng;
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

use actions::{
    decode_machine_select, BOARD_TAP, BUY_BALLS, HALL_CURSOR_DOWN, HALL_CURSOR_UP,
    HALL_SCROLL_DOWN, HALL_SCROLL_UP, INFO_SCROLL_DOWN, INFO_SCROLL_UP, LEAVE_SEAT, POWER_DOWN,
    POWER_UP, TAB_BOARD, TAB_HISTORY, TAB_RECORD, TOGGLE_FIRE,
};
use state::{InfoTab, Mode, PachinkoState, Phase};

/// スクロールキー / 矢印タップ1回で動かす行数。
const SCROLL_STEP: i32 = 3;
/// 強弱キー1回あたりのハンドル強度の変化量。適正値は台の釘配置ごとに
/// 違い、探り当てるまで何度も刻む操作になるので、端から端まで数回で
/// 飛んでしまわない細かさにする。
const POWER_STEP: i16 = 2;

pub struct PachinkoGame {
    pub state: PachinkoState,
    save_countdown: u32,
    /// 保存済みの `state.jackpot_end_seq`。差が出た tick で即座に保存し、
    /// 大当たりで積んだ出玉と記録が次の定期保存までのリロードで消えるのを防ぐ。
    saved_jackpot_end_seq: u32,
}

impl Default for PachinkoGame {
    fn default() -> Self {
        Self::new()
    }
}

impl PachinkoGame {
    pub fn new() -> Self {
        let mut state = PachinkoState::new();
        #[cfg(target_arch = "wasm32")]
        save::load_game(&mut state);
        // ホールの並びは来店ごとに作る。`PachinkoState::new` は台を持たない
        // ので、これを呼ばないと着席できる台が1つも無い画面になる。
        //
        // 生成はセーブの読み込みより後に置く。`rng_state` を読み込む前に
        // 台を引くと、毎回 `PachinkoState::new` の固定 seed から始まり、
        // 来店のたびに同じ4台・同じ釘が並ぶ。
        // 読み込んだ現金と持ち玉を、この来店の開始時点として記録する。収支は
        // ここからの増減で見せる (`PachinkoState::opening_assets` 参照)。
        state.opening_assets = state.assets_yen();
        logic::generate_hall(&mut state);
        // 台を引いて進んだ seed をその場で書き戻す。次の保存を待つ間に
        // 再読み込みされると、保存済みの古い seed から同じ4台を引き直して
        // しまい、来店ごとに並びが変わらなくなる。
        #[cfg(target_arch = "wasm32")]
        save::save_game(&state);
        let saved_jackpot_end_seq = state.jackpot_end_seq;
        Self {
            state,
            save_countdown: save::AUTOSAVE_INTERVAL,
            saved_jackpot_end_seq,
        }
    }

    fn flush_save(&mut self) {
        #[cfg(target_arch = "wasm32")]
        save::save_game(&self.state);
        self.saved_jackpot_end_seq = self.state.jackpot_end_seq;
        self.save_countdown = save::AUTOSAVE_INTERVAL;
    }

    /// 情報パネルのタブを切り替える。タブごとに行数が大きく違うため、
    /// スクロールを引き継ぐと切り替え直後に中途半端な位置が表示される。
    fn switch_tab(&mut self, tab: InfoTab) -> bool {
        if self.state.tab != tab {
            self.state.info_scroll.set(0);
        }
        self.state.tab = tab;
        true
    }

    fn handle_key(&mut self, key: char) -> bool {
        match self.state.phase {
            Phase::Hall => self.handle_hall_key(key),
            Phase::Playing => self.handle_playing_key(key),
        }
    }

    fn handle_hall_key(&mut self, key: char) -> bool {
        match key {
            // 選択中の台にそのまま座る。プレビューで釘を読んでから座る
            // 流れが、カーソル移動と同じ手だけで閉じる。
            ' ' => match self.state.clamped_hall_cursor() {
                Some(index) => logic::sit_at(&mut self.state, index),
                None => false,
            },
            // 台の並び順そのものがショートカットになる。ホールの台数を
            // 超える数字は `sit_at` が弾く。
            '1'..='9' => {
                let index = key as usize - '1' as usize;
                logic::sit_at(&mut self.state, index)
            }
            // 大文字も受けるのは、index.html の高速スワイプが大文字キーを
            // 送るため。
            'k' | 'K' => self.state.move_hall_cursor(-1),
            'j' | 'J' => self.state.move_hall_cursor(1),
            '!' => logic::buy_balls(&mut self.state),
            // 'q' はここへ落ちる。消費しないことで main.rs がメニューへ戻す。
            _ => false,
        }
    }

    fn handle_playing_key(&mut self, key: char) -> bool {
        match key {
            ' ' | 'a' | 'A' => logic::toggle_fire(&mut self.state),
            'h' => logic::adjust_power(&mut self.state, -POWER_STEP),
            'l' => logic::adjust_power(&mut self.state, POWER_STEP),
            'k' | 'K' => {
                self.state.scroll_info(-SCROLL_STEP);
                true
            }
            'j' | 'J' => {
                self.state.scroll_info(SCROLL_STEP);
                true
            }
            '!' => logic::buy_balls(&mut self.state),
            '{' => self.switch_tab(InfoTab::Board),
            '|' => self.switch_tab(InfoTab::History),
            '}' => self.switch_tab(InfoTab::Record),
            'q' => {
                // 大当たり中は `leave_seat` が断るが、それでもキーは消費する。
                // false を返すと main.rs がメニューへ戻してこのインスタンスを
                // 捨て、開いているアタッカーの出玉ごと失わせてしまう。
                logic::leave_seat(&mut self.state);
                true
            }
            _ => false,
        }
    }

    fn handle_click(&mut self, action_id: u16) -> bool {
        match action_id {
            // main.rs が画面左上へ重ねる「◀戻る」。大当たり中だけは消費して
            // 席に留まる。消費しないと main.rs がこのインスタンスごと捨て、
            // 開いているアタッカーで取れるはずだった出玉を失わせる —
            // `handle_playing_key` の 'q' と同じ扱いに揃える。
            crate::BACK_TO_MENU
                if self.state.phase == Phase::Playing
                    && matches!(self.state.mode, state::Mode::Jackpot(_)) =>
            {
                logic::leave_seat(&mut self.state);
                true
            }
            // 盤面全面のタップも打ち出しのトグルに繋ぐ。画面で最も面積の
            // 広い領域を主操作に当てることで、指でも狙わずに押せる。
            TOGGLE_FIRE | BOARD_TAP => logic::toggle_fire(&mut self.state),
            POWER_DOWN => logic::adjust_power(&mut self.state, -POWER_STEP),
            POWER_UP => logic::adjust_power(&mut self.state, POWER_STEP),
            BUY_BALLS => logic::buy_balls(&mut self.state),
            LEAVE_SEAT => logic::leave_seat(&mut self.state),
            TAB_BOARD => self.switch_tab(InfoTab::Board),
            TAB_HISTORY => self.switch_tab(InfoTab::History),
            TAB_RECORD => self.switch_tab(InfoTab::Record),
            HALL_CURSOR_UP => self.state.move_hall_cursor(-1),
            HALL_CURSOR_DOWN => self.state.move_hall_cursor(1),
            HALL_SCROLL_UP => {
                self.state.scroll_hall(-SCROLL_STEP);
                true
            }
            HALL_SCROLL_DOWN => {
                self.state.scroll_hall(SCROLL_STEP);
                true
            }
            INFO_SCROLL_UP => {
                self.state.scroll_info(-SCROLL_STEP);
                true
            }
            INFO_SCROLL_DOWN => {
                self.state.scroll_info(SCROLL_STEP);
                true
            }
            id => match decode_machine_select(id) {
                Some(index) => logic::sit_at(&mut self.state, index),
                None => false,
            },
        }
    }
}

impl Game for PachinkoGame {
    fn choice(&self) -> GameChoice {
        GameChoice::Pachinko
    }

    fn handle_input(&mut self, event: &InputEvent) -> bool {
        match event {
            InputEvent::Key(c) => self.handle_key(*c),
            InputEvent::Click(_, id) => self.handle_click(*id),
        }
    }

    fn tick(&mut self, delta_ticks: u32) {
        logic::tick_n(&mut self.state, delta_ticks);
        if self.state.jackpot_end_seq != self.saved_jackpot_end_seq {
            // 大当たりの終了は、アタッカーで取れる出玉と自己記録 (`record`) が
            // 揃って確定する瞬間。定期保存を待つと、その間にリロードした分の
            // 出玉と記録が静かに消える。
            //
            // 契機を大当たりの確定側に置かないのは、確定時点では出玉がまだ
            // 1玉も増えていないため。`save.rs` は遊技中の `mode` を保存せず、
            // 読み込みは必ず通常状態から始まるので、確定だけを保存すると
            // 「記録には残っているのに出玉が無い」食い違いだけが残る。
            self.flush_save();
            return;
        }
        if matches!(self.state.mode, Mode::Jackpot(_)) {
            // 大当たりの最中に保存すると、加算済みの記録と取りかけの出玉だけが
            // 残る。`save.rs` は `mode` も残りラウンドも保存しないので、その
            // 途中の状態から読み込むと通常時で再開し、残りのラウンドと確変・
            // 時短が消える。終了時に必ず確定させるので、その間は見送る。
            return;
        }
        if self.save_countdown > delta_ticks {
            self.save_countdown -= delta_ticks;
        } else {
            self.flush_save();
        }
    }

    fn on_leave(&mut self) {
        // 離脱はこのインスタンスごと破棄される (main.rs が AppState を差し
        // 替える) ため、持ち玉のまま抜けると出玉が財布にも記録にも残らない。
        // 席を立つのと同じ扱いで換金してから保存する。
        logic::cash_out(&mut self.state);
        self.flush_save();
    }

    fn render(&self, f: &mut Frame, area: Rect, click_state: &Rc<RefCell<ClickState>>) {
        render::render(&self.state, f, area, click_state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::ClickScope;
    use state::HALL_SIZE;

    fn click(id: u16) -> InputEvent {
        InputEvent::Click(ClickScope::Game(GameChoice::Pachinko), id)
    }

    /// ホールの先頭の台に座る。遊技中の操作を試すテストの共通の前置き。
    fn seated() -> PachinkoGame {
        let mut game = PachinkoGame::new();
        assert!(game.handle_input(&InputEvent::Key('1')));
        assert_eq!(game.state.phase, Phase::Playing);
        game
    }

    #[test]
    fn new_game_opens_a_hall_with_machines() {
        let game = PachinkoGame::new();
        assert_eq!(game.state.machines.len(), HALL_SIZE);
        assert_eq!(game.state.phase, Phase::Hall);
    }

    #[test]
    fn quit_key_is_consumed_only_while_playing() {
        let mut game = PachinkoGame::new();
        assert!(
            !game.handle_input(&InputEvent::Key('q')),
            "ホールの q はメニューへ戻す操作なので、ゲーム側で消費してはいけない"
        );

        let mut game = seated();
        assert!(
            game.handle_input(&InputEvent::Key('q')),
            "遊技中の q は席を立つ操作として消費する"
        );
        assert_eq!(game.state.phase, Phase::Hall);
        assert!(!game.handle_input(&InputEvent::Key('q')));
    }

    #[test]
    fn quit_key_stays_in_the_seat_during_a_jackpot() {
        // 大当たり中に離脱させると、開いているアタッカーの出玉を捨てさせる。
        let mut game = seated();
        game.state.mode = state::Mode::Jackpot(state::JackpotState {
            round: 1,
            total_rounds: 10,
            count: 0,
            ticks_left: state::ROUND_LIMIT_TICKS,
            kakuhen: true,
            payout: 0,
        });
        assert!(game.handle_input(&InputEvent::Key('q')));
        assert_eq!(game.state.phase, Phase::Playing);
    }

    #[test]
    fn back_button_click_stays_in_the_seat_during_a_jackpot() {
        // 戻るボタンは main.rs が毎フレーム盤面へ重ねる。消費しないと
        // インスタンスごと捨てられ、大当たりの残りラウンドの出玉が消える。
        let mut game = seated();
        assert!(
            !game.handle_input(&click(crate::BACK_TO_MENU)),
            "通常時の戻るボタンはメニューへ戻す操作なので、ゲーム側で消費してはいけない"
        );

        game.state.mode = state::Mode::Jackpot(state::JackpotState {
            round: 1,
            total_rounds: 10,
            count: 0,
            ticks_left: state::ROUND_LIMIT_TICKS,
            kakuhen: true,
            payout: 0,
        });
        assert!(game.handle_input(&click(crate::BACK_TO_MENU)));
        assert_eq!(game.state.phase, Phase::Playing);
        assert_eq!(
            game.state.log.first().map(String::as_str),
            Some("大当たり中は席を立てない"),
            "キー操作と同じく、席を立てない理由をログで伝える"
        );
    }

    #[test]
    fn firing_toggles_via_key_and_board_tap() {
        let mut game = seated();
        assert!(!game.state.firing);
        assert!(game.handle_input(&InputEvent::Key('a')));
        assert!(game.state.firing);
        assert!(game.handle_input(&click(BOARD_TAP)));
        assert!(!game.state.firing);
        assert!(game.handle_input(&InputEvent::Key(' ')));
        assert!(game.state.firing);
        assert!(game.handle_input(&click(TOGGLE_FIRE)));
        assert!(!game.state.firing);
    }

    #[test]
    fn machine_select_click_seats_the_player() {
        let mut game = PachinkoGame::new();
        let index = HALL_SIZE - 1;
        assert!(game.handle_input(&click(actions::machine_select_id(index))));
        assert_eq!(game.state.phase, Phase::Playing);
        assert_eq!(game.state.seat, index);
    }

    #[test]
    fn hall_cursor_moves_and_seats_the_selected_machine() {
        // 釘のプレビューは選択中の台を描く。選択を動かして座るまでが
        // 繋がっていないと、読み比べた台とは別の台に座ることになる。
        let mut game = PachinkoGame::new();
        assert_eq!(game.state.hall_cursor, 0);
        assert!(game.handle_input(&InputEvent::Key('j')));
        assert_eq!(game.state.hall_cursor, 1);
        assert!(game.handle_input(&click(HALL_CURSOR_DOWN)));
        assert_eq!(game.state.hall_cursor, 2);
        assert!(game.handle_input(&InputEvent::Key('K')));
        assert_eq!(game.state.hall_cursor, 1);

        assert!(game.handle_input(&InputEvent::Key(' ')));
        assert_eq!(game.state.phase, Phase::Playing);
        assert_eq!(game.state.seat, 1);
    }

    #[test]
    fn hall_cursor_stops_at_both_ends() {
        let mut game = PachinkoGame::new();
        for _ in 0..HALL_SIZE + 3 {
            assert!(game.handle_input(&click(HALL_CURSOR_UP)));
        }
        assert_eq!(game.state.hall_cursor, 0);
        for _ in 0..HALL_SIZE + 3 {
            assert!(game.handle_input(&click(HALL_CURSOR_DOWN)));
        }
        assert_eq!(game.state.hall_cursor, HALL_SIZE - 1);
    }

    #[test]
    fn hall_cursor_follows_the_machine_the_player_sat_at() {
        // 席を立った直後のホールで、今まで打っていた台とは別の台の釘が
        // 出ていると、次にどこへ座るかの比較の起点がずれる。
        let mut game = PachinkoGame::new();
        assert!(game.handle_input(&click(actions::machine_select_id(HALL_SIZE - 1))));
        assert!(game.handle_input(&InputEvent::Key('q')));
        assert_eq!(game.state.phase, Phase::Hall);
        assert_eq!(game.state.hall_cursor, HALL_SIZE - 1);
    }

    #[test]
    fn hall_cursor_keys_are_inert_without_machines() {
        // ホール生成前でもキーは届く。台を前提に添え字を引くと落ちる。
        let mut game = PachinkoGame::new();
        game.state.machines.clear();
        assert!(!game.handle_input(&InputEvent::Key('j')));
        assert!(!game.handle_input(&InputEvent::Key(' ')));
        assert_eq!(game.state.phase, Phase::Hall);
    }

    #[test]
    fn power_moves_in_both_directions() {
        let mut game = seated();
        let base = game.state.power;
        assert!(game.handle_input(&InputEvent::Key('l')));
        assert_eq!(game.state.power, base + POWER_STEP as u8);
        assert!(game.handle_input(&click(POWER_DOWN)));
        assert_eq!(game.state.power, base);
        assert!(game.handle_input(&click(POWER_UP)));
        assert_eq!(game.state.power, base + POWER_STEP as u8);
        assert!(game.handle_input(&InputEvent::Key('h')));
        assert_eq!(game.state.power, base);
    }

    #[test]
    fn buying_balls_works_from_both_screens() {
        let mut game = PachinkoGame::new();
        assert!(game.handle_input(&InputEvent::Key('!')));
        assert_eq!(game.state.balls_held, state::BALL_LOAN_COUNT);

        let mut game = seated();
        assert!(game.handle_input(&click(BUY_BALLS)));
        assert_eq!(game.state.balls_held, state::BALL_LOAN_COUNT);
    }

    #[test]
    fn info_tab_switches_via_key_and_click() {
        let mut game = seated();
        assert_eq!(game.state.tab, InfoTab::Board);
        assert!(game.handle_input(&InputEvent::Key('|')));
        assert_eq!(game.state.tab, InfoTab::History);
        assert!(game.handle_input(&InputEvent::Key('}')));
        assert_eq!(game.state.tab, InfoTab::Record);
        assert!(game.handle_input(&InputEvent::Key('{')));
        assert_eq!(game.state.tab, InfoTab::Board);

        assert!(game.handle_input(&click(TAB_HISTORY)));
        assert_eq!(game.state.tab, InfoTab::History);
        assert!(game.handle_input(&click(TAB_RECORD)));
        assert_eq!(game.state.tab, InfoTab::Record);
        assert!(game.handle_input(&click(TAB_BOARD)));
        assert_eq!(game.state.tab, InfoTab::Board);
    }

    #[test]
    fn switching_tab_rewinds_the_scroll() {
        let mut game = seated();
        game.state.info_scroll.set(12);
        assert!(game.handle_input(&click(TAB_RECORD)));
        assert_eq!(game.state.info_scroll.get(), 0);
    }

    #[test]
    fn scroll_actions_move_the_hall_list_and_the_info_panel() {
        let step = SCROLL_STEP as u16;
        let mut game = PachinkoGame::new();
        assert!(game.handle_input(&click(HALL_SCROLL_DOWN)));
        assert_eq!(game.state.hall_scroll.get(), step);
        assert!(game.handle_input(&click(HALL_SCROLL_UP)));
        assert_eq!(game.state.hall_scroll.get(), 0);

        let mut game = seated();
        assert!(game.handle_input(&click(INFO_SCROLL_DOWN)));
        assert_eq!(game.state.info_scroll.get(), step);
        assert!(game.handle_input(&click(INFO_SCROLL_UP)));
        assert_eq!(game.state.info_scroll.get(), 0);
        assert!(game.handle_input(&InputEvent::Key('j')));
        assert_eq!(game.state.info_scroll.get(), step);
        assert!(game.handle_input(&InputEvent::Key('k')));
        assert_eq!(game.state.info_scroll.get(), 0);
    }

    #[test]
    fn tick_advances_only_while_seated() {
        let mut game = PachinkoGame::new();
        game.state.balls_held = 100;
        game.state.firing = true;
        game.tick(30);
        assert!(game.state.balls.is_empty(), "ホールでは台の物理を進めない");

        let mut game = seated();
        game.state.balls_held = 100;
        game.state.firing = true;
        game.tick(30);
        assert!(!game.state.balls.is_empty(), "着席中は玉が打ち出される");
    }

    /// 最終ラウンドを残り1tickまで進めた大当たり。次の tick で終了する。
    fn last_round_of_a_jackpot() -> state::Mode {
        state::Mode::Jackpot(state::JackpotState {
            round: 1,
            total_rounds: 1,
            count: 0,
            ticks_left: 1,
            kakuhen: false,
            payout: 0,
        })
    }

    #[test]
    fn finishing_a_jackpot_flushes_the_autosave_countdown() {
        let mut game = seated();
        game.tick(10);
        assert!(
            game.save_countdown < save::AUTOSAVE_INTERVAL,
            "何も起きていない tick では定期保存までのカウントダウンが進む"
        );

        game.state.mode = last_round_of_a_jackpot();
        game.tick(1);
        assert!(
            !matches!(game.state.mode, state::Mode::Jackpot(_)),
            "最終ラウンドの時間切れで大当たりが終わっていない"
        );
        assert_eq!(
            game.save_countdown,
            save::AUTOSAVE_INTERVAL,
            "大当たりで確定した出玉と記録は、次の定期保存を待たずに保存する"
        );
        assert_eq!(game.saved_jackpot_end_seq, game.state.jackpot_end_seq);
    }

    #[test]
    fn a_confirmed_jackpot_alone_does_not_flush_the_autosave_countdown() {
        // 大当たりが確定した時点では出玉がまだ1玉も増えていない。ここで
        // 保存すると、リロード後は通常状態から始まるので「記録には残って
        // いるのに出玉が無い」食い違いだけが残る。
        let mut game = seated();
        game.state.jackpot_seq += 1;
        game.state.mode = state::Mode::Jackpot(state::JackpotState {
            round: 1,
            total_rounds: 10,
            count: 0,
            ticks_left: state::ROUND_LIMIT_TICKS,
            kakuhen: true,
            payout: 0,
        });
        let countdown_before = game.save_countdown;
        game.tick(1);

        assert!(
            matches!(game.state.mode, state::Mode::Jackpot(_)),
            "大当たりの途中を試すテストなのに大当たりが終わっている"
        );
        assert_eq!(
            game.save_countdown, countdown_before,
            "大当たりの確定だけで保存を確定させてはいけない"
        );
    }

    #[test]
    fn a_jackpot_in_progress_holds_off_the_periodic_save() {
        // 定期保存は大当たりの途中でも回ってくる。そこで保存すると、加算済みの
        // 記録と取りかけの出玉だけが残り、読み込んだ側は通常時で再開して残りの
        // ラウンドと確変・時短を失う。30秒を超える大当たりでは必ず通る経路。
        let mut game = seated();
        game.state.mode = state::Mode::Jackpot(state::JackpotState {
            round: 1,
            total_rounds: 16,
            count: 0,
            ticks_left: state::ROUND_LIMIT_TICKS,
            kakuhen: true,
            payout: 0,
        });
        game.save_countdown = 1;
        game.tick(5);

        assert!(
            matches!(game.state.mode, state::Mode::Jackpot(_)),
            "大当たりの途中を試すテストなのに大当たりが終わっている"
        );
        assert_eq!(
            game.save_countdown, 1,
            "大当たり中に定期保存が走っている (走ると countdown が間隔まで戻る)"
        );
    }

    #[test]
    fn leaving_the_game_cashes_out_the_held_balls() {
        let mut game = seated();
        game.state.balls_held = 500;
        let cash_before = game.state.cash;
        game.on_leave();
        assert_eq!(game.state.balls_held, 0);
        assert!(game.state.cash > cash_before);
        assert!(game.state.record.total_returned > 0);
    }
}
