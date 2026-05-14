//! マージゲームのクリック ID。

use super::state::{GRID_H, GRID_W};

/// 盤面セルの base id。`(col, row)` のセルは `GRID_CLICK_BASE + row*GRID_W + col`。
pub const GRID_CLICK_BASE: u16 = 100;

/// クエスト納品ボタン (3 スロット)。
pub const ACT_QUEST_DELIVER_BASE: u16 = 200;

/// クエスト破棄 (リロール) ボタン。
pub const ACT_QUEST_REROLL_BASE: u16 = 210;

/// アップグレード購入。
pub const ACT_UPGRADE_GENERATORS: u16 = 220;

/// 選択解除 (盤面外タップ用)。
pub const ACT_CLEAR_SELECTION: u16 = 221;

/// `action_id` が盤面セル範囲 (`GRID_CLICK_BASE` から `GRID_W * GRID_H` 個)
/// に収まっていれば `(col, row)` を返す。`ClickableGrid::decode` は範囲外で
/// も `Some` を返してしまうので、ここで明示的に弾いて他のボタン (クエスト
/// など) と被らないようにする。
pub fn decode_grid(action_id: u16) -> Option<(usize, usize)> {
    let max = GRID_CLICK_BASE as u32 + (GRID_W * GRID_H) as u32;
    if action_id as u32 >= max {
        return None;
    }
    crate::widgets::ClickableGrid::decode(GRID_CLICK_BASE, GRID_W, action_id)
}
