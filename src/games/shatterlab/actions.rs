//! 破壊VFXラボの操作。スタイル切替のみ。

use super::state::DemoStyle;

pub const TAB_ORE: u16 = 1;
pub const TAB_PRESS: u16 = 2;
pub const TAB_PLANET: u16 = 3;
pub const TAB_CITY: u16 = 4;

pub fn style_for_tab(action_id: u16) -> Option<DemoStyle> {
    match action_id {
        TAB_ORE => Some(DemoStyle::OreBomb),
        TAB_PRESS => Some(DemoStyle::PressCrush),
        TAB_PLANET => Some(DemoStyle::PlanetPeel),
        TAB_CITY => Some(DemoStyle::CityCollapse),
        _ => None,
    }
}

pub fn tab_for_style(style: DemoStyle) -> u16 {
    match style {
        DemoStyle::OreBomb => TAB_ORE,
        DemoStyle::PressCrush => TAB_PRESS,
        DemoStyle::PlanetPeel => TAB_PLANET,
        DemoStyle::CityCollapse => TAB_CITY,
    }
}
