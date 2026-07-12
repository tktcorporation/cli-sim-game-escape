//! 穴掘り長屋のクリック ID。

use super::state::{YARD_H, YARD_W};

/// タブ切り替え。
pub const ACT_TAB_YARD: u16 = 1;
pub const ACT_TAB_NEIGHBORS: u16 = 2;
pub const ACT_TAB_COLLECTION: u16 = 3;

/// 庭グリッドセルの base id。`(col, row)` のセルは `GRID_CLICK_BASE + row*YARD_W + col`。
pub const GRID_CLICK_BASE: u16 = 100;

/// ご近所さんのお福分け穴。`ACT_NEIGHBOR_DIG_BASE + neighbor_index`。
pub const ACT_NEIGHBOR_DIG_BASE: u16 = 200;

/// シャベル強化購入。
pub const ACT_UPGRADE_SHOVEL: u16 = 210;

/// `action_id` が庭グリッド範囲 (`GRID_CLICK_BASE` から `YARD_W * YARD_H` 個)
/// に収まっていればセル index (`row * YARD_W + col`) を返す。
pub fn decode_grid(action_id: u16) -> Option<usize> {
    if action_id < GRID_CLICK_BASE {
        return None;
    }
    let offset = (action_id - GRID_CLICK_BASE) as usize;
    if offset >= YARD_W * YARD_H {
        return None;
    }
    Some(offset)
}

/// `action_id` がご近所さんのお福分け穴クリックであれば neighbor index を返す。
pub fn decode_neighbor(action_id: u16, neighbor_count: usize) -> Option<usize> {
    if action_id < ACT_NEIGHBOR_DIG_BASE {
        return None;
    }
    let offset = (action_id - ACT_NEIGHBOR_DIG_BASE) as usize;
    if offset >= neighbor_count {
        return None;
    }
    Some(offset)
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::state::NEIGHBOR_COUNT;

    #[test]
    fn decode_gridは範囲内のみsomeを返す() {
        assert_eq!(decode_grid(GRID_CLICK_BASE), Some(0));
        assert_eq!(decode_grid(GRID_CLICK_BASE + (YARD_W * YARD_H) as u16 - 1), Some(YARD_W * YARD_H - 1));
        assert_eq!(decode_grid(GRID_CLICK_BASE + (YARD_W * YARD_H) as u16), None);
        assert_eq!(decode_grid(GRID_CLICK_BASE - 1), None);
    }

    #[test]
    fn decode_neighborは範囲内のみsomeを返す() {
        assert_eq!(decode_neighbor(ACT_NEIGHBOR_DIG_BASE, NEIGHBOR_COUNT), Some(0));
        assert_eq!(
            decode_neighbor(ACT_NEIGHBOR_DIG_BASE + NEIGHBOR_COUNT as u16 - 1, NEIGHBOR_COUNT),
            Some(NEIGHBOR_COUNT - 1)
        );
        assert_eq!(
            decode_neighbor(ACT_NEIGHBOR_DIG_BASE + NEIGHBOR_COUNT as u16, NEIGHBOR_COUNT),
            None
        );
        assert_eq!(decode_neighbor(ACT_NEIGHBOR_DIG_BASE - 1, NEIGHBOR_COUNT), None);
    }
}
