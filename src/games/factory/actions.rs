//! Semantic action IDs for Tiny Factory click targets.

pub const SELECT_MINER: u16 = 1;
pub const SELECT_SMELTER: u16 = 2;
pub const SELECT_ASSEMBLER: u16 = 3;
pub const SELECT_EXPORTER: u16 = 4;
pub const SELECT_FABRICATOR: u16 = 5;
pub const SELECT_BELT: u16 = 6;
pub const SELECT_DELETE: u16 = 7;
pub const TOGGLE_MINER_MODE: u16 = 8;

/// Grid click: action_id = GRID_CLICK_BASE + viewport_row * VIEW_W + viewport_col
pub const GRID_CLICK_BASE: u16 = 100;
