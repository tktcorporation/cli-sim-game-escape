//! 周回討伐 click target 用のセマンティックアクションID。

pub const CAMP_UPGRADE_MAX_HP: u16 = 1;
pub const CAMP_UPGRADE_ATTACK: u16 = 2;
pub const CAMP_UPGRADE_EXTRA_CARD: u16 = 3;
pub const CAMP_START_OR_RESUME: u16 = 4;
pub const REFILL_HAND: u16 = 5;
pub const GO_TO_CAMP: u16 = 6;
pub const CAMP_SCROLL_UP: u16 = 7;
pub const CAMP_SCROLL_DOWN: u16 = 8;

/// 手札クリック: action_id = HAND_CLICK_BASE + hand_index
pub const HAND_CLICK_BASE: u16 = 10;

/// 道クリック (ClickableGrid): action_id = PATH_CLICK_BASE + gy * RING_W + gx
pub const PATH_CLICK_BASE: u16 = 100;
