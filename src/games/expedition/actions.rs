//! 遠征団のクリック / キーアクション ID。

pub const START_FORMING: u16 = 1;
pub const CANCEL_FORMING: u16 = 2;
pub const LAUNCH: u16 = 3;
pub const LAUNCH_WITH_SCOUT: u16 = 4;
pub const USE_AID: u16 = 5;
pub const ACK_RESULT: u16 = 7;
pub const OPEN_FORMING: u16 = 8;
pub const TAB_CAMP: u16 = 10;
pub const TAB_ROSTER: u16 = 11;
pub const TOGGLE_HERO_BASE: u16 = 20;

pub fn toggle_hero_id(hero_id: u8) -> u16 {
    TOGGLE_HERO_BASE + u16::from(hero_id)
}

pub fn hero_id_from_toggle(action_id: u16) -> Option<u8> {
    if (TOGGLE_HERO_BASE..TOGGLE_HERO_BASE + 8).contains(&action_id) {
        Some((action_id - TOGGLE_HERO_BASE) as u8)
    } else {
        None
    }
}
