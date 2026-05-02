//! 深淵潜行 — プレイヤー行動の表現。
//!
//! 本体ゲームでは「キー/クリック → PlayerAction → logic::apply_action」と
//! 流れる。シミュレータでも同じ `PlayerAction` を Policy が生成して
//! 同じ `logic::apply_action` を通す。これにより本体・sim の動作は構造的に
//! 一致する (DI のキモ)。

use super::state::{SoulPerk, Tab, UpgradeKind};

/// プレイヤー (または AI Policy) が起こせる行動。tick とは独立して適用される。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayerAction {
    BuyUpgrade(UpgradeKind),
    BuySoulPerk(SoulPerk),
    ToggleAutoDescend,
    Retreat,
    SetTab(Tab),
    /// ガチャを `count` 回引く (鍵が足りなければ引ける分だけ)。
    GachaPull(u32),
}
