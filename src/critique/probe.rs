//! 画面の裏にある「今プレイヤーが知るべきこと」の抽象表現。
//!
//! 画面テキストだけだと「この数字は目標なのか装飾なのか」が分からない。
//! 各ゲームの subject がここへ意図を書き、metrics が画面と突き合わせる。

use crate::critique::frame::ScreenSnapshot;

/// 画面に出ているはずの、または出すべき操作。
#[derive(Clone, Debug)]
pub struct ActionFact {
    pub id: u16,
    /// 画面テキストに含まれると期待する短いラベル（部分一致）。
    pub label: String,
    /// `[1]` 形式のキーヒント。無ければ `None`。
    pub hint: Option<char>,
    /// 「今これをやれ」という主操作か。開口の CTA 判定に使う。
    pub primary: bool,
}

/// 1時点の概念状態。シミュレーターのスナップショットより人間向け。
#[derive(Clone, Debug)]
pub struct ProbeFacts {
    pub phase: String,
    /// プレイヤーが今向かうべき目標の文言。画面に同じ（または十分近い）
    /// 文字列が出ているかを GoalVisibility が測る。
    pub next_goal: Option<String>,
    pub actions: Vec<ActionFact>,
    /// 名前付き進捗メーター。フィードバック判定の前後比較に使う。
    pub progress: Vec<(String, f64)>,
    /// 直近のログやフラッシュなど、「何か起きた」手がかり。
    pub recent_feedback: Vec<String>,
}

/// 評価対象ゲーム。`Game::render` は `now_ms` 経由で native テストが
/// panic するので、各実装は `render::render` を直接叩く。
pub trait Subject {
    fn name(&self) -> &'static str;
    fn probe(&self) -> ProbeFacts;
    fn capture(&self, width: u16, height: u16) -> ScreenSnapshot;
    fn tick(&mut self, n: u32);
    /// セマンティック action_id を1つ消費する。未対応なら false。
    fn apply_action(&mut self, action_id: u16) -> bool;
    /// 自動プレイ方針。None ならセッションは開口評価だけで終わる。
    fn suggest_action(&self, facts: &ProbeFacts, screen: &ScreenSnapshot) -> Option<u16>;
}
