//! 深淵潜行 — プレイヤー行動の表現。
//!
//! 本体ゲームでは「キー/クリック → PlayerAction → logic::apply_action」と
//! 流れる。シミュレータでも同じ `PlayerAction` を Policy が生成して
//! 同じ `logic::apply_action` を通す。これにより本体・sim の動作は構造的に
//! 一致する (DI のキモ)。

use super::state::{EquipmentId, SoulPerk, Tab, UpgradeKind};

/// プレイヤー (または AI Policy) が起こせる行動。tick とは独立して適用される。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayerAction {
    BuyUpgrade(UpgradeKind),
    BuySoulPerk(SoulPerk),
    /// 装備を 1 個解放する (gold + 強化 Lv + 前装備の条件を全部満たす場合のみ成功)。
    /// 解放したら永続装備、付け替え無し。
    BuyEquipment(EquipmentId),
    ToggleAutoDescend,
    Retreat,
    SetTab(Tab),
    /// ガチャを `count` 回引く (鍵が足りなければ引ける分だけ)。
    GachaPull(u32),
    /// タブ本体を上方向にスクロール。
    ///
    /// **UI only**: simulator policy は絶対に生成しない (純粋に表示位置の制御で
    /// ゲーム進行に影響しないため)。ベンチマーク的にも UI action はゼロにする。
    ScrollUp,
    /// タブ本体を下方向にスクロール。同上、UI only。
    ScrollDown,
}
