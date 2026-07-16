//! 穴掘り長屋のクリック ID。

use super::state::SITE_LEN;

/// タブ切り替え。
pub const ACT_TAB_SITE: u16 = 1;
pub const ACT_TAB_MUSEUM: u16 = 2;

/// 羅盤モードの切り替え (次のグリッドタップが「調べる」になる)。
pub const ACT_RADAR: u16 = 3;

/// 図鑑タブのスクロール。
pub const ACT_MUSEUM_SCROLL_UP: u16 = 4;
pub const ACT_MUSEUM_SCROLL_DOWN: u16 = 5;

/// 現場グリッドセルの base id。`(col, row)` のセルは `GRID_CLICK_BASE + row*SITE_W + col`。
pub const GRID_CLICK_BASE: u16 = 100;

/// `action_id` が現場グリッド範囲に収まっていればセル index を返す。
pub fn decode_grid(action_id: u16) -> Option<usize> {
    if action_id < GRID_CLICK_BASE {
        return None;
    }
    let offset = (action_id - GRID_CLICK_BASE) as usize;
    if offset >= SITE_LEN {
        return None;
    }
    Some(offset)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_gridは範囲内のみsomeを返す() {
        assert_eq!(decode_grid(GRID_CLICK_BASE), Some(0));
        assert_eq!(
            decode_grid(GRID_CLICK_BASE + SITE_LEN as u16 - 1),
            Some(SITE_LEN - 1)
        );
        assert_eq!(decode_grid(GRID_CLICK_BASE + SITE_LEN as u16), None);
        assert_eq!(decode_grid(GRID_CLICK_BASE - 1), None);
    }
}
