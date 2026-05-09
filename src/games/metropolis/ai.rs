//! AI brains — 将棋AI 風のレベル別設計。
//!
//! **アーキテクチャ**: 各 Tier は **同じ評価関数 + 探索インフラ**
//! (`logic::evaluate` / `logic::action_value` / `logic::search_best_action`) を
//! 使い、差分は次の3つだけ:
//!   - 評価関数: Tier 1 はなし、Tier 2 は簡易、Tier 3+ はフル
//!   - 探索深さ: Tier 2/3 は depth=1、Tier 4 は depth=2、Tier 5 は depth=3
//!   - ノイズ%: Tier 2 は 30%、Tier 3 は 5%、Tier 4/5 は 0%
//!
//! Stockfish Skill Level / ぴよ将棋の段位設計と同じ「自然な弱さ」思想 —
//! 視野 (探索深さ) を狭めることで自然に悪手が出る。明示ブランダーは入れない。

use super::state::*;

/// AI が選ぶ 1 アクション。Build/Demolish/Idle が同じ評価軸 (`action_value`) で
/// 比較されることに注意 — 「建てる/壊す/待つ」を同じ天秤で max 選択する。
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
    Idle,
}

/// Tier dispatcher — 各 Tier の brain を呼ぶ。
pub fn decide(city: &mut City) -> AiAction {
    match city.ai_tier {
        AiTier::Random => tier1_random(city),
        AiTier::Greedy => tier2_greedy(city),
        AiTier::Aware => tier3_aware(city),
        AiTier::Planner => tier4_planner(city),
        AiTier::Master => tier5_master(city),
    }
}

/// Tier 1 (15級) — ランダム指し。
///
/// 評価関数なし。合法手から RNG で引くだけ。**「街を発展させる」は最低限の
/// 自然 (= ゲームの目的)** として保つので、Build を Demolish より優先 (Demolish は
/// 5% 確率)。配置の良し悪し・建物選択は完全にランダム — 道路を中央に大量に
/// 置いて死に道路を作ったり、機能不全 Shop を量産したりする。
///
/// 「合法手からランダム」の最低限ガード: Build 候補が無い時のみ Idle / Demolish。
fn tier1_random(city: &mut City) -> AiAction {
    let actions = super::logic::enumerate_actions(city);
    if actions.is_empty() {
        return AiAction::Idle;
    }
    let demolish_roll = city.next_rand() % 100;
    let want_demolish = demolish_roll < 5;
    let mut filtered: Vec<&AiAction> = actions
        .iter()
        .filter(|a| match a {
            AiAction::Build { .. } => !want_demolish,
            AiAction::Demolish { .. } => want_demolish,
            AiAction::Idle => false,
        })
        .collect();
    if filtered.is_empty() {
        filtered = actions
            .iter()
            .filter(|a| matches!(a, AiAction::Build { .. }))
            .collect();
    }
    if filtered.is_empty() {
        return AiAction::Idle;
    }
    let idx = (city.next_rand() as usize) % filtered.len();
    filtered[idx].clone()
}

/// Tier 2 (5級) — 1手読み + 簡易評価 + 30% ノイズ。
///
/// **評価関数 = `evaluate_simple`**: House 数の和だけ見る = 「目先の家賃しか
/// 見えない」短視眼。Road や Park の長期効果は読めず、Cottage を量産する
/// 傾向が出るのが Tier 2 のキャラクター。
///
/// 30% の確率で次善手を選ぶことで「いつも最適に動かない」自然な弱さ。
fn tier2_greedy(city: &mut City) -> AiAction {
    let rng = city.next_rand();
    let ranked = super::logic::rank_actions(city, &super::logic::evaluate_simple, 6);
    super::logic::pick_with_noise(&ranked, 30, rng).unwrap_or(AiAction::Idle)
}

/// Tier 3 (初段) — 1手読み + フル評価 + 5% ノイズ。
///
/// **評価関数 = `evaluate`**: cents/sec 解像度の income/sec + Strategy bonus。
/// Tier 2 から評価関数だけが進化し、edge connectivity / Tier 昇格 / 需給按分が
/// 全部見える。「ちゃんと考えてるが先は読まない」レベル。
fn tier3_aware(city: &mut City) -> AiAction {
    let rng = city.next_rand();
    let ranked = super::logic::rank_actions(city, &super::logic::evaluate, 8);
    super::logic::pick_with_noise(&ranked, 5, rng).unwrap_or(AiAction::Idle)
}

/// Tier 4 (三段) — 2手読み + フル評価。
///
/// **キャラクター**: 「道路を引いて家を建てる」のような 1手目+2手目の連携を
/// 発見できる。Tier 3 からの差は **探索深さだけ** (評価関数は同じ)。
fn tier4_planner(city: &mut City) -> AiAction {
    super::logic::search_best_action(city, 2, 4, &super::logic::evaluate)
        .unwrap_or(AiAction::Idle)
}

/// Tier 5 (アマ高段) — 3手読み + フル評価 + 狭めの beam。
///
/// **キャラクター**: 「Outpost を派遣 → 道路を伸ばす → Highrise化」のような
/// 3手目までの長期投資が見える。Tier 4 からの差は **探索深さ**。
///
/// beam 幅 K=4 と狭め: depth=3 で計算量が `K × K/2 × N` まで膨らむため、
/// 30 分シミュ (= 18000 ticks × 10 candidates) が現実時間で終わる範囲に絞る。
/// 深さで「先読みできる」を表現し、K で広さは抑える将棋エンジン的バランス。
fn tier5_master(city: &mut City) -> AiAction {
    super::logic::search_best_action(city, 3, 4, &super::logic::evaluate)
        .unwrap_or(AiAction::Idle)
}
