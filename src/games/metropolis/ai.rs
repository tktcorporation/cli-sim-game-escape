//! AI brain — 単一ブレイン設計。
//!
//! 全プレイヤーが常に同じ最強の判断 (1 手読み + フル評価
//! `logic::rank_actions_full`、ノイズなし) で動く。
//!
//! 探索深さは 1 に固定する。Beam Search の知見 (heuristic が悪ければ深く
//! 探しても無駄、広く正確に評価する方が深さより効く) に従い、巧拙は
//! 「評価する候補数の広さ」で表現する設計。

use super::state::*;

/// AI が選ぶ 1 アクション。Build/Demolish/Replace/Idle が同じ評価軸
/// (`action_value`) で比較される — 「建てる/壊す/壊して建て替える/待つ」を同じ
/// 天秤で max 選択する。
///
/// `Replace` は「同セルで Demolish + Build」を 1 単位として扱う複合アクション。
/// 探索の depth=1 で「撤去後に再建」のシーケンスが直接比較対象になり、
/// beam pruning で Demolish 単体が切り落とされて 2 手目の再建が見えなくなる
/// 問題を解消する。production 経路では `apply_ai_action` が Demolish 部分のみを
/// 即時実行し、Build 部分は次 tick の `decide` が空きセルとして再評価して拾う。
#[derive(Clone, Debug, PartialEq)]
pub enum AiAction {
    Build {
        x: usize,
        y: usize,
        kind: Building,
    },
    Demolish {
        x: usize,
        y: usize,
    },
    Replace {
        x: usize,
        y: usize,
        kind: Building,
    },
    Idle,
}

/// 唯一の AI 入口 — 常に最強ブレインで 1 手を返す。
///
/// 1 手読み + フル評価 (`rank_actions_full`)、ノイズなしで最善手を取る。
/// `top_k` は出力の truncate 数なだけで、full evaluate は pre-rank で
/// `PRE_RANK_LIMIT` 件に制限済み → `top_k=1` でも探索コストは最広候補と同じ。
/// top-1 だけ使うので 1 を渡す。
pub fn decide(city: &mut City) -> AiAction {
    super::logic::rank_actions_full(city, 1)
        .into_iter()
        .next()
        .map(|(action, _)| action)
        .unwrap_or(AiAction::Idle)
}
