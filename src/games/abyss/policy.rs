//! 深淵潜行 — プレイヤー行動の表現。
//!
//! 本体ゲームでは「キー/クリック → PlayerAction → logic::apply_action」と
//! 流れる。シミュレータでも同じ `PlayerAction` を Policy が生成して
//! 同じ `logic::apply_action` を通す。これにより本体・sim の動作は構造的に
//! 一致する (DI のキモ)。

use super::state::{EquipmentId, SoulPerk, Tab};

/// プレイヤー (または AI Policy) が起こせる行動。tick とは独立して適用される。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayerAction {
    /// 装備を 1 個購入する (gold + 前装備の prerequisite を満たすときのみ成功)。
    /// 購入直後は **そのまま自動装着** する (空スロットなら即座に、埋まっていれば置換)。
    /// 購入だけしても装着しない idle UX を避けるための明示的な「購入で装着」。
    BuyEquipment(EquipmentId),
    /// 既に所持している装備を装着する (lane の現スロットを置換)。
    /// 装着切替は無料、いつでも変えられる。
    EquipItem(EquipmentId),
    /// 指定装備を 1 段階強化する (gold で支払う)。所持していなくても良い ─
    /// が、**所持していない装備を強化する意味は薄い** ので UI 側でフィルタする。
    EnhanceEquipment(EquipmentId),
    /// 魂強化を 1 段階購入する。
    BuySoulPerk(SoulPerk),
    ToggleAutoDescend,
    Retreat,
    SetTab(Tab),
    /// ガチャを `count` 回引く。
    GachaPull(u32),
    /// タブ本体を上方向にスクロール。**UI only**。
    ScrollUp,
    /// タブ本体を下方向にスクロール。**UI only**。
    ScrollDown,
}
