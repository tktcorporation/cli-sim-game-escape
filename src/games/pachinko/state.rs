//! 玉響 (Tamayura) — ゲーム状態。
//!
//! 純粋なデータ定義とパラメータ関数のみ。物理・抽選・状態遷移は logic.rs、
//! 描画は render.rs に置く (Pure Logic Pattern)。
//!
//! ## 盤面座標系
//! 盤面は連続座標 (`f64`) の縦長の矩形で、`x` は [0, BOARD_W]、`y` は
//! [0, BOARD_H]。**`y` は下向きが正** で、`y=0` が天井、`y=BOARD_H` が
//! アウト口にあたる。重力の符号をそのまま `vy` に足せる向きを優先した
//! 結果で、上下が反転して見える Canvas へは render 側で反転して描く。
//!
//! ## 台の個性の見せ方
//! 台ごとの回りやすさは `Machine::nail_spread` / `rail_bias` が持つが、
//! これらは数値として UI に出さない。`logic::generate_nails` が釘の座標へ
//! 反映し、プレイヤーは盤面の見た目から読む。実際の回転率は打って計測して
//! 初めて分かる (`logic::spin_rate`)。

use std::cell::Cell;

use ratzilla::ratatui::style::Color;

// ── 盤面レイアウト ─────────────────────────────────────────────

/// 盤面の幅 (Canvas x_bounds)。
pub const BOARD_W: f64 = 64.0;
/// 盤面の高さ (Canvas y_bounds)。
pub const BOARD_H: f64 = 96.0;

/// 玉の半径。釘との衝突判定 `BALL_R + NAIL_R` に使う。
pub const BALL_R: f64 = 0.9;
/// 釘の半径。
pub const NAIL_R: f64 = 0.7;

/// 発射レールの出口 (右上)。ここから初速を与えて打ち出す。
pub const LAUNCH_X: f64 = BOARD_W - 3.0;
pub const LAUNCH_Y: f64 = 10.0;

/// ヘソ (スタートチャッカー) の中心。
pub const START_POCKET_X: f64 = BOARD_W / 2.0;
pub const START_POCKET_Y: f64 = 54.0;
/// ヘソの基本の受け口半幅。台ごとの `nail_spread` と電サポの有無を加えた値が
/// 実効幅になる (`logic::effective_pocket_half_w`)。
pub const START_POCKET_BASE_HALF_W: f64 = 1.0;

/// アタッカー (大当たり中のみ開放)。
pub const ATTACKER_X: f64 = BOARD_W / 2.0;
pub const ATTACKER_Y: f64 = 78.0;
pub const ATTACKER_HALF_W: f64 = 6.0;

/// 一般入賞口 (常時開放)。左右対称に2つ置き、ヘソを外した玉にも
/// わずかな戻りを与えて「全部飲まれる」感覚を薄める。
pub const SIDE_POCKET_Y: f64 = 70.0;
pub const SIDE_POCKET_HALF_W: f64 = 2.4;
pub const SIDE_POCKET_LEFT_X: f64 = 12.0;
pub const SIDE_POCKET_RIGHT_X: f64 = BOARD_W - 12.0;

/// 釘に弾かれた玉を光らせる長さ (tick)。`delta_ticks` は最大5までまとめて
/// 来るため、1 tick ずつ減らすカウンタは 5 未満だと一度も描画されずに
/// 消える可能性がある。
pub const HIT_GLOW_TICKS: u8 = 5;
/// ヘソに玉が入った瞬間にヘソを光らせる長さ (tick)。下限の理由は
/// `HIT_GLOW_TICKS` と同じ。
pub const START_FLASH_TICKS: u8 = 6;
/// リーチに入った瞬間に盤面を光らせる長さ (tick)。1回転につき一度しか
/// 起きない事象なので、何度も起きるヘソ入賞より長く残す。
pub const REACH_FLASH_TICKS: u8 = 10;

// ── フェーズ ───────────────────────────────────────────────────

/// 遊技の大枠。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// ホール。台を選ぶ画面。
    Hall,
    /// 台に着席して打っている。
    Playing,
}

// ── 台 ─────────────────────────────────────────────────────────

/// 台のスペック。実機のような 1/319 は 10 ticks/sec のテンポに合わないため、
/// 1回転 = 12 tick (1.2 秒) を前提に「数分で当たりに手が届く」値へ寄せている。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MachineSpec {
    /// 通常時の大当たり分母。`1 / normal_odds` で当たる。
    pub normal_odds: u32,
    /// 確変中の大当たり分母。
    pub kakuhen_odds: u32,
    /// 大当たりのラウンド数の候補と重み。`(rounds, weight)`。
    pub round_table: &'static [(u32, u32)],
    /// 大当たり後に確変へ入る確率 (%)。
    pub kakuhen_rate: u32,
    /// 時短の回転数 (確変に入らなかった場合)。
    pub jitan_spins: u32,
}

/// ホールに並べる台の原型 (台名とスペック)。
///
/// 甘い台ほど当たりが軽く出玉が少なく、荒い台ほど当たりが重く出玉が多い、
/// という一直線のトレードオフにしてある。どれが「得か」ではなく、
/// 手持ちの軍資金でどこまで粘れるかで選ぶ対象にするため。
pub const MACHINE_SPECS: [(&str, MachineSpec); 3] = [
    (
        "海凪",
        MachineSpec {
            normal_odds: 45,
            kakuhen_odds: 16,
            round_table: &[(4, 60), (8, 40)],
            kakuhen_rate: 35,
            jitan_spins: 18,
        },
    ),
    (
        "花火繚乱",
        MachineSpec {
            normal_odds: 80,
            kakuhen_odds: 22,
            round_table: &[(8, 50), (14, 50)],
            kakuhen_rate: 38,
            jitan_spins: 24,
        },
    ),
    (
        "極楽轟音",
        MachineSpec {
            normal_odds: 105,
            kakuhen_odds: 42,
            round_table: &[(16, 100)],
            kakuhen_rate: 37,
            jitan_spins: 30,
        },
    ),
];

/// ホールに並ぶ台数。`MACHINE_SPECS` の種類数より多いので、同じスペックで
/// 釘だけが違う台が並ぶ — これが釘読みという判断軸を成立させる。
pub const HALL_SIZE: usize = 4;

/// 盤面上の1本の釘。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Nail {
    pub x: f64,
    pub y: f64,
}

/// 1台の遊技機。釘配置とスペックを持つ。
#[derive(Clone, Debug)]
pub struct Machine {
    pub name: &'static str,
    pub spec: MachineSpec,
    /// ヘソ釘の開き。0.0〜1.0。大きいほどヘソが広く、回りやすい。
    /// 描画上はヘソ左右の釘の x 間隔として現れる。
    pub nail_spread: f64,
    /// 寄り釘の傾き。-1.0 (外へ逃がす) 〜 1.0 (中央へ寄せる)。
    /// 描画上は上部釘の x オフセットの傾きとして現れる。
    pub rail_bias: f64,
    /// 釘の配置。`logic::generate_nails` が seed から生成する。
    pub nails: Vec<Nail>,
    /// この台で打った累計。回転率の実測に使うので、台を離れても持ち越す。
    pub balls_spent: u32,
    pub spins_seen: u32,
}

// ── 玉 ─────────────────────────────────────────────────────────

/// 盤面を転がっている玉。
#[derive(Clone, Copy, Debug)]
pub struct Ball {
    pub x: f64,
    pub y: f64,
    pub vx: f64,
    pub vy: f64,
    /// 釘に弾かれた瞬間を光らせるための残り tick。
    pub hit_glow: u8,
}

// ── デジタル抽選 ───────────────────────────────────────────────

/// リーチ (演出) の格。格が上がるほど当たりの割合が高いが、その対応は
/// プレイヤーが観察して掴むもので、UI では信頼度を明示しない。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReachKind {
    /// リーチにならない通常ハズレ。
    None,
    /// ノーマルリーチ。
    Normal,
    /// スーパーリーチ。
    Super,
    /// プレミア。出れば当たり。
    Premium,
}

impl ReachKind {
    /// 描画側の演出リストが全種を網羅しているかをテストで突き合わせるために持つ。
    /// 新しい格を足したとき、描画側の更新を忘れると「回っているのに何も
    /// 起きない」演出になる。
    pub const ALL: [ReachKind; 4] = [
        ReachKind::None,
        ReachKind::Normal,
        ReachKind::Super,
        ReachKind::Premium,
    ];

    /// デジタルが回り続ける長さ (tick)。格が上がるほど長く回るのは、
    /// 「長い＝期待できる」という体感を数値表示なしで作るため。
    pub fn spin_ticks(self) -> u32 {
        match self {
            ReachKind::None => 12,
            ReachKind::Normal => 22,
            ReachKind::Super => 38,
            ReachKind::Premium => 48,
        }
    }

    /// 格の名前。履歴・記録のような「起きた事実」の表示に使う。
    pub fn label(self) -> &'static str {
        match self {
            ReachKind::None => "通常",
            ReachKind::Normal => "リーチ",
            ReachKind::Super => "スーパーリーチ",
            ReachKind::Premium => "プレミア",
        }
    }

    /// 回転中に盤面へ出す一言。プレミアだけ名前を伏せるのは、
    /// 何が起きたのかをプレイヤー自身に確かめさせるため。
    pub fn shout(self) -> &'static str {
        match self {
            ReachKind::None => "",
            ReachKind::Normal => "リーチ！",
            ReachKind::Super => "スーパーリーチ！！",
            ReachKind::Premium => "!?",
        }
    }

    pub fn color(self) -> Color {
        match self {
            ReachKind::None => Color::Gray,
            ReachKind::Normal => Color::White,
            ReachKind::Super => Color::LightYellow,
            ReachKind::Premium => Color::LightMagenta,
        }
    }
}

/// 1回転の結果。ヘソ入賞の時点で確定させ、演出はこの結果に沿って分岐する
/// (実機と同じく「先に当落が決まり、演出が後から説明する」構造)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpinOutcome {
    pub hit: bool,
    /// 当たった場合のラウンド数。ハズレなら 0。
    pub rounds: u32,
    /// 当たった場合に確変へ入るか。
    pub kakuhen: bool,
    /// 演出の格。ハズレでもリーチにはなる。
    pub reach: ReachKind,
    /// 停止出目 (3桁)。当たりならゾロ目、リーチならリーチ目になる。
    pub reels: [u8; 3],
}

/// 台に着いた直後に表示しておく出目。ゾロ目にすると、まだ一度も回して
/// いない台が当たり済みに見えてしまうため、揃わない目にする。
pub const INITIAL_REELS: [u8; 3] = [1, 2, 3];

/// デジタル (液晶) の状態。
#[derive(Clone, Debug, PartialEq)]
pub enum Digit {
    /// 停止中。前回の出目 (`PachinkoState::last_reels`) を表示している。
    Idle,
    /// 回転中。`ticks_left` が 0 になると結果が確定する。
    Spinning { ticks_left: u32, outcome: SpinOutcome },
}

// ── 遊技モード ─────────────────────────────────────────────────

/// 1ラウンドの規定カウント。アタッカーは盤面幅の2割ほどしかないので、
/// 実機と同じ10カウントにすると規定数に届く前に必ず時間切れになり、
/// ラウンドの進捗表示が一度も満たされないまま流れていく。
pub const ROUND_COUNT: u32 = 2;
/// 1ラウンドの制限時間。玉が入らなくてもここで打ち切ることで、
/// 打ち出しを止めたまま大当たりが永久に終わらない状態を防ぐ。
pub const ROUND_LIMIT_TICKS: u32 = 80;
/// アタッカー1入賞あたりの賞球。大当たり中の出玉はほぼこの値だけで決まるので、
/// 「当たった瞬間に持ち玉がドンと増える」手応えはここが持っている。
pub const ATTACKER_PAYOUT: u32 = 16;
/// 一般入賞口1入賞あたりの賞球。
pub const SIDE_PAYOUT: u32 = 1;
/// ヘソ1入賞あたりの賞球。打った玉の1割弱がヘソへ入るので、ここは賞球全体の
/// 4割前後を占める。厚くすると通常時に削られる感覚が消え、大当たりで取り返す
/// という起伏そのものが平坦になる。
pub const START_PAYOUT: u32 = 2;

/// 大当たりラウンドの進行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JackpotState {
    pub round: u32,
    pub total_rounds: u32,
    /// このラウンドでアタッカーに入った玉数。`ROUND_COUNT` で次ラウンドへ。
    pub count: u32,
    /// このラウンドの残り時間。
    pub ticks_left: u32,
    /// 大当たり終了後に確変へ入るか。
    pub kakuhen: bool,
}

/// 現在の遊技状態。確変・時短は「ヘソが広がる / 確率が上がる」形で盤面に効く。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// 通常時。
    Normal,
    /// 確変 (高確率＋電サポ)。`spins_left` は 0 なら次回当たりまで継続。
    Kakuhen { spins_left: u32 },
    /// 時短 (通常確率＋電サポ)。
    Jitan { spins_left: u32 },
    /// 大当たり中。アタッカーが開いている。
    Jackpot(JackpotState),
}

impl Mode {
    pub fn label(self) -> &'static str {
        match self {
            Mode::Normal => "通常",
            Mode::Kakuhen { .. } => "確変",
            Mode::Jitan { .. } => "時短",
            Mode::Jackpot(_) => "大当たり中",
        }
    }

    /// 電サポ (ヘソの受け口が広がる状態) か。
    pub fn is_assisted(self) -> bool {
        matches!(self, Mode::Kakuhen { .. } | Mode::Jitan { .. })
    }
}

// ── 記録 ───────────────────────────────────────────────────────

/// 大当たり1回分の記録。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HistoryEntry {
    pub rounds: u32,
    pub kakuhen: bool,
    /// この当たりまでに回した回転数 (ハマり回数)。
    pub spins_before: u32,
}

/// 来店をまたいで残る自己記録。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Record {
    pub best_balls: u32,
    pub total_jackpots: u32,
    pub best_chain: u32,
    pub total_invested: u32,
    pub total_returned: u32,
}

/// 情報パネルのタブ。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InfoTab {
    Board,
    History,
    Record,
}

impl InfoTab {
    /// 描画側のタブ列が全タブを含むかをテストで突き合わせるために持つ。
    pub const ALL: [InfoTab; 3] = [InfoTab::Board, InfoTab::History, InfoTab::Record];

    pub fn label(self) -> &'static str {
        match self {
            InfoTab::Board => "盤面",
            InfoTab::History => "履歴",
            InfoTab::Record => "記録",
        }
    }
}

// ── トップレベル state ─────────────────────────────────────────

/// 1000 円で借りられる玉数。
pub const BALL_LOAN_YEN: u32 = 1000;
pub const BALL_LOAN_COUNT: u32 = 250;
/// 打ち出し間隔。`try_fire` は残り tick を減らす tick では撃たないので、
/// 実際の周期はこの値 +1 tick = 0.4 秒 (毎分150発) になる。実機の毎分100発
/// より速いのは、盤面に常時数個の玉が流れている絵を作るため — 実機の間隔
/// では玉が1個ずつ落ちるだけの寂しい盤面になり、玉数の上限 (`MAX_BALLS`) も
/// 遊んだままになる。
pub const FIRE_INTERVAL_TICKS: u32 = 3;
/// 保留の上限。
pub const MAX_PENDING: usize = 4;
/// 履歴に残す大当たりの件数。
pub const HISTORY_LEN: usize = 12;
/// 盤面に同時に存在できる玉の上限。釘との距離判定は玉数×釘数で効くため、
/// 描画とシミュレーションの負荷をここで抑える。
pub const MAX_BALLS: usize = 24;
/// ログの保持件数。
pub const LOG_LEN: usize = 40;

pub struct PachinkoState {
    pub phase: Phase,
    /// ホールに並ぶ台。
    pub machines: Vec<Machine>,
    /// 着席中の台の index。`Phase::Hall` では直前に座っていた台を保持する。
    pub seat: usize,
    /// 一度でも着席したか。`seat` は初期値 0 を持つので、この印を見ずに
    /// 着席中の台を強調すると、まだ座っていないプレイヤーにもホールの
    /// 先頭の台が着席中に見える。
    pub has_seated: bool,
    /// ホールで選択中の台の index。盤面プレビューはこの台を描くので、
    /// 着席しなくても釘を読める。台の並びは来店ごとに引き直され、選択も
    /// その場限りの見ている位置でしかないため保存しない。
    ///
    /// 台数より大きい値になっていても落とさず末尾へ丸める
    /// (`clamped_hall_cursor`) — 台数はホールの生成でしか変わらないので、
    /// 書き手側で毎回突き合わせるより読み手側で丸める方が漏れがない。
    pub hall_cursor: usize,
    /// 盤面の玉。
    pub balls: Vec<Ball>,
    /// 打ち出し中か。
    pub firing: bool,
    /// ハンドル強度 0〜100。
    pub power: u8,
    /// 次に玉が出るまでの残り tick。
    pub fire_cooldown: u32,
    /// 手持ちの玉。0 になると打てない。
    pub balls_held: u32,
    /// 財布の現金 (円)。`BALL_LOAN_YEN` 単位で玉に替える。
    pub cash: u32,
    /// この来店での総投資額 (円)。収支表示に使う。
    pub invested: u32,
    pub digit: Digit,
    /// 最後に停止した出目。`Digit::Idle` は「回っていない液晶」なので、
    /// 直前の結果を出し続けることで大当たりの後にゾロ目が残る — 実機で
    /// 台の当たり状況が液晶から読めるのと同じ見え方になる。
    pub last_reels: [u8; 3],
    /// 保留 (最大 `MAX_PENDING`)。ヘソ入賞のたびに積まれ、順に消化される。
    pub pending: Vec<SpinOutcome>,
    pub mode: Mode,
    /// 直近の大当たり履歴 (新しい順、最大 `HISTORY_LEN` 件)。
    pub history: Vec<HistoryEntry>,
    /// 連チャン数 (電サポ中に引いた当たりの連続回数)。
    pub chain: u32,
    pub log: Vec<String>,
    pub rng_state: u32,
    /// 大当たりが確定するたびに増える単調増加カウンタ。読み手は前回見た値との
    /// 差分の有無だけを見るので、logic 側は当たったら増やすだけでよい。
    pub jackpot_seq: u32,
    /// 大当たりが終わって電サポへ移るたびに増える単調増加カウンタ。出玉が
    /// 確定するのは終了時なので、保存の契機はこちらを見る。`jackpot_seq` とは
    /// 別に持つ — 1つのフィールドで確定と終了を兼ねると、差分を見た側が
    /// どちらの瞬間なのかを区別できない。
    pub jackpot_end_seq: u32,
    /// ヘソに玉が入った瞬間を光らせる残り tick。
    pub start_flash: u8,
    /// リーチに入った瞬間を光らせる残り tick。
    pub reach_flash: u8,
    /// `reach_flash` で光らせる色を決める格。光の有無と色を1組で持つことで、
    /// 描画側は `Digit` の中身を辿らずに枠を塗れる。
    pub reach_flash_kind: ReachKind,
    /// 前回の大当たりから消化したデジタル回転数 (ハマり回数)。大当たりの
    /// たびに 0 へ戻す。`jackpot_seq` とは別に持つ — あちらは「増えたか」
    /// しか見ない約束なので、0 へ戻す値を兼ねさせられない。
    pub spins_since_jackpot: u32,
    /// セーブ対象の自己記録。
    pub record: Record,
    /// タブのスクロール位置。`&self` しか持たない render から書き戻すため Cell。
    pub hall_scroll: Cell<u16>,
    pub info_scroll: Cell<u16>,
    /// 情報パネルの選択タブ。
    pub tab: InfoTab,
}

impl PachinkoState {
    pub fn new() -> Self {
        Self {
            phase: Phase::Hall,
            // ホールの並びは `logic::generate_hall` が来店ごとに作る。
            machines: Vec::new(),
            seat: 0,
            has_seated: false,
            hall_cursor: 0,
            balls: Vec::new(),
            firing: false,
            // 中庸な強さから始める。適正値は台の釘配置ごとに違うので、
            // ここから探る余地を残す。
            power: 62,
            fire_cooldown: 0,
            balls_held: 0,
            // 軍資金は 1万円 = `BALL_LOAN_YEN` 10回分。
            cash: 10_000,
            invested: 0,
            digit: Digit::Idle,
            last_reels: INITIAL_REELS,
            pending: Vec::new(),
            mode: Mode::Normal,
            history: Vec::new(),
            chain: 0,
            log: Vec::new(),
            // xorshift32 は 0 が不動点なので非ゼロで始める。
            rng_state: 0x7AC1_2E5B,
            jackpot_seq: 0,
            jackpot_end_seq: 0,
            start_flash: 0,
            reach_flash: 0,
            reach_flash_kind: ReachKind::None,
            spins_since_jackpot: 0,
            record: Record::default(),
            hall_scroll: Cell::new(0),
            info_scroll: Cell::new(0),
            tab: InfoTab::Board,
        }
    }

    /// ログを新しい順に積む。表示側は先頭から読めばよい。
    pub fn add_log(&mut self, text: impl Into<String>) {
        self.log.insert(0, text.into());
        self.log.truncate(LOG_LEN);
    }

    pub fn scroll_hall(&self, delta: i32) {
        adjust_scroll(&self.hall_scroll, delta);
    }

    pub fn scroll_info(&self, delta: i32) {
        adjust_scroll(&self.info_scroll, delta);
    }

    /// ホールで選択中の台の index を、実際に並んでいる台数へ丸めて返す。
    /// 台が1台も無い間は `None`。
    pub fn clamped_hall_cursor(&self) -> Option<usize> {
        let last = self.machines.len().checked_sub(1)?;
        Some(self.hall_cursor.min(last))
    }

    /// ホールで選択中の台。
    pub fn hall_cursor_machine(&self) -> Option<&Machine> {
        self.machines.get(self.clamped_hall_cursor()?)
    }

    /// ホールの選択を上下に動かす。動かせた (台が並んでいる) なら true。
    pub fn move_hall_cursor(&mut self, delta: i32) -> bool {
        let Some(current) = self.clamped_hall_cursor() else {
            return false;
        };
        let last = self.machines.len() - 1;
        let next = (current as i32 + delta).clamp(0, last as i32) as usize;
        self.hall_cursor = next;
        true
    }

    /// 着席中の台。`machines` が空 (ホール生成前) や seat が範囲外でも
    /// 落ちないよう Option で返す。
    pub fn seated_machine(&self) -> Option<&Machine> {
        self.machines.get(self.seat)
    }

    pub fn seated_machine_mut(&mut self) -> Option<&mut Machine> {
        self.machines.get_mut(self.seat)
    }
}

impl Default for PachinkoState {
    fn default() -> Self {
        Self::new()
    }
}

fn adjust_scroll(cell: &Cell<u16>, delta: i32) {
    let cur = cell.get() as i32;
    cell.set(cur.saturating_add(delta).max(0) as u16);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_log_keeps_newest_first_and_bounded() {
        let mut s = PachinkoState::new();
        for i in 0..(LOG_LEN + 10) {
            s.add_log(format!("msg {i}"));
        }
        assert_eq!(s.log.len(), LOG_LEN);
        assert_eq!(s.log[0], format!("msg {}", LOG_LEN + 9));
    }

    #[test]
    fn higher_reach_kind_spins_longer() {
        // 「長く回る＝期待できる」を信頼度の数値表示なしで伝える手段なので、
        // 格と回転時間の順序が崩れると演出の意味自体が失われる。
        for pair in ReachKind::ALL.windows(2) {
            assert!(
                pair[0].spin_ticks() < pair[1].spin_ticks(),
                "{} より {} の方が短く回っている",
                pair[1].label(),
                pair[0].label()
            );
        }
    }

    #[test]
    fn kakuhen_is_easier_than_normal_on_every_spec() {
        for (name, spec) in MACHINE_SPECS {
            assert!(
                spec.kakuhen_odds < spec.normal_odds,
                "{name} の確変が通常より当たりにくい"
            );
            assert!(
                spec.round_table.iter().any(|&(_, weight)| weight > 0),
                "{name} のラウンド抽選の重みが全て 0 で、ラウンド数を選べない"
            );
        }
    }
}
