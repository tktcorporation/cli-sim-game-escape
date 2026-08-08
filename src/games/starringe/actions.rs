//! 星環のセマンティックアクション ID。

use super::state::{RingUpgrade, WeaponKind, WeaponStat};

/// タブ: 武装
pub const TAB_ARMORY: u16 = 1;
/// タブ: 環
pub const TAB_RING: u16 = 2;
/// タブ: 図鑑
pub const TAB_CODEX: u16 = 3;
/// コア / 情景タップ (手動火力ブースト)
pub const TAP_STRIKE: u16 = 4;
/// 武装選択: 前へ / 次へ
pub const WEAPON_PREV: u16 = 10;
pub const WEAPON_NEXT: u16 = 11;
/// 武装直接選択ベース + WeaponKind::index()
pub const SELECT_WEAPON_BASE: u16 = 20;
/// 武器ステ強化ベース: + weapon_index * 3 + stat_index
pub const BUY_WEAPON_STAT_BASE: u16 = 100;
/// 環強化ベース + RingUpgrade::index()
pub const BUY_RING_BASE: u16 = 200;

pub fn select_weapon_id(kind: WeaponKind) -> u16 {
    SELECT_WEAPON_BASE + kind.index() as u16
}

pub fn weapon_for_select_id(action_id: u16) -> Option<WeaponKind> {
    if (SELECT_WEAPON_BASE..SELECT_WEAPON_BASE + 5).contains(&action_id) {
        WeaponKind::from_index((action_id - SELECT_WEAPON_BASE) as usize)
    } else {
        None
    }
}

pub fn buy_weapon_stat_id(weapon: WeaponKind, stat: WeaponStat) -> u16 {
    BUY_WEAPON_STAT_BASE + (weapon.index() * 3 + stat.index()) as u16
}

pub fn weapon_stat_for_buy_id(action_id: u16) -> Option<(WeaponKind, WeaponStat)> {
    if !(BUY_WEAPON_STAT_BASE..BUY_WEAPON_STAT_BASE + 15).contains(&action_id) {
        return None;
    }
    let offset = (action_id - BUY_WEAPON_STAT_BASE) as usize;
    let weapon = WeaponKind::from_index(offset / 3)?;
    let stat = WeaponStat::from_index(offset % 3)?;
    Some((weapon, stat))
}

pub fn buy_ring_id(kind: RingUpgrade) -> u16 {
    BUY_RING_BASE + kind.index() as u16
}

pub fn ring_for_buy_id(action_id: u16) -> Option<RingUpgrade> {
    if (BUY_RING_BASE..BUY_RING_BASE + RingUpgrade::ALL.len() as u16).contains(&action_id) {
        RingUpgrade::from_index((action_id - BUY_RING_BASE) as usize)
    } else {
        None
    }
}
