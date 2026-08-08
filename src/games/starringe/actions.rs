//! 星環のセマンティックアクション ID。

use super::state::UpgradeKind;

/// タブ: 強化
pub const TAB_UPGRADES: u16 = 1;
/// タブ: 図鑑
pub const TAB_CODEX: u16 = 2;
/// コア / 情景タップ (手動火力ブースト)
pub const TAP_STRIKE: u16 = 3;
/// 強化購入ベース + UpgradeKind::index()
pub const BUY_UPGRADE_BASE: u16 = 100;

pub fn buy_upgrade_id(kind: UpgradeKind) -> u16 {
    BUY_UPGRADE_BASE + kind.index() as u16
}

pub fn upgrade_for_buy_id(action_id: u16) -> Option<UpgradeKind> {
    if (BUY_UPGRADE_BASE..BUY_UPGRADE_BASE + 6).contains(&action_id) {
        UpgradeKind::from_index((action_id - BUY_UPGRADE_BASE) as usize)
    } else {
        None
    }
}
