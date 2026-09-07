//! 遠征団のクリック / キーアクション ID。

pub const START_FORMING: u16 = 1;
pub const CANCEL_FORMING: u16 = 2;
pub const LAUNCH: u16 = 3;
pub const CONFIRM_PLACEMENT: u16 = 4;
pub const ACK_RESULT: u16 = 7;
pub const OPEN_FORMING: u16 = 8;
pub const TAB_CAMP: u16 = 10;
pub const TAB_ARCADE: u16 = 11;
pub const MEDAL_ROLL: u16 = 12;
pub const PLACE_SLOT_BASE: u16 = 30;
pub const LEVEL_HERO_BASE: u16 = 40;
pub const TOGGLE_HERO_BASE: u16 = 20;

pub fn place_slot_id(slot: u8) -> u16 {
    PLACE_SLOT_BASE + u16::from(slot)
}

pub fn slot_from_place(action_id: u16) -> Option<u8> {
    if (PLACE_SLOT_BASE..PLACE_SLOT_BASE + 8).contains(&action_id) {
        Some((action_id - PLACE_SLOT_BASE) as u8)
    } else {
        None
    }
}

pub fn level_hero_id(hero_id: u8) -> u16 {
    LEVEL_HERO_BASE + u16::from(hero_id)
}

pub fn hero_id_from_level(action_id: u16) -> Option<u8> {
    if (LEVEL_HERO_BASE..LEVEL_HERO_BASE + 8).contains(&action_id) {
        Some((action_id - LEVEL_HERO_BASE) as u8)
    } else {
        None
    }
}

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
