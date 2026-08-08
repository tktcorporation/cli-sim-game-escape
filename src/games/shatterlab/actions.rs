//! 破壊VFXラボの操作。コンセプト切替と威力Lv切替。

use super::state::{DemoStyle, PowerLevel};

pub const TAB_CRUISE: u16 = 1;
pub const TAB_MINE: u16 = 2;
pub const TAB_RAIL: u16 = 3;
pub const TAB_SAT: u16 = 4;

pub const TAB_POWER_LOW: u16 = 10;
pub const TAB_POWER_MID: u16 = 11;
pub const TAB_POWER_HIGH: u16 = 12;
pub const TAB_POWER_AUTO: u16 = 13;

pub fn style_for_tab(action_id: u16) -> Option<DemoStyle> {
    match action_id {
        TAB_CRUISE => Some(DemoStyle::SpaceCruise),
        TAB_MINE => Some(DemoStyle::OrbitMine),
        TAB_RAIL => Some(DemoStyle::RailBreak),
        TAB_SAT => Some(DemoStyle::SatDefense),
        _ => None,
    }
}

pub fn tab_for_style(style: DemoStyle) -> u16 {
    match style {
        DemoStyle::SpaceCruise => TAB_CRUISE,
        DemoStyle::OrbitMine => TAB_MINE,
        DemoStyle::RailBreak => TAB_RAIL,
        DemoStyle::SatDefense => TAB_SAT,
    }
}

pub fn power_for_tab(action_id: u16) -> Option<PowerLevel> {
    match action_id {
        TAB_POWER_LOW => Some(PowerLevel::Low),
        TAB_POWER_MID => Some(PowerLevel::Mid),
        TAB_POWER_HIGH => Some(PowerLevel::High),
        _ => None,
    }
}

pub fn tab_for_power(power: PowerLevel) -> u16 {
    match power {
        PowerLevel::Low => TAB_POWER_LOW,
        PowerLevel::Mid => TAB_POWER_MID,
        PowerLevel::High => TAB_POWER_HIGH,
    }
}
