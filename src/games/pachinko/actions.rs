//! 玉響のセマンティックアクション ID。

use super::state::HALL_SIZE;

/// 打ち出しの ON/OFF
pub const TOGGLE_FIRE: u16 = 1;
/// ハンドル強度を弱める / 強める
pub const POWER_DOWN: u16 = 2;
pub const POWER_UP: u16 = 3;
/// 現金を玉に替える
pub const BUY_BALLS: u16 = 4;
/// 台を離れてホールへ戻る
pub const LEAVE_SEAT: u16 = 5;
/// 情報パネルのタブ: 盤面 / 履歴 / 記録
pub const TAB_BOARD: u16 = 6;
pub const TAB_HISTORY: u16 = 7;
pub const TAB_RECORD: u16 = 8;
/// 盤面全面のタップ。打ち出しのトグルに割り当てる。盤面は Canvas 1枚で
/// 占有面積が最も広いので、モバイルで最も押しやすい操作を主操作に当てる。
pub const BOARD_TAP: u16 = 9;
/// 台リスト (ホール) と情報パネル (遊技中) のスクロール矢印。矢印の描画と
/// タップ登録は `ScrollableTab` が行い、押された時の挙動はスクロール位置の
/// 更新だけで閉じる。
pub const HALL_SCROLL_UP: u16 = 10;
pub const HALL_SCROLL_DOWN: u16 = 11;
pub const INFO_SCROLL_UP: u16 = 12;
pub const INFO_SCROLL_DOWN: u16 = 13;
/// ホールの台選択ベース + 台の index。
pub const MACHINE_SELECT_BASE: u16 = 100;

pub fn machine_select_id(index: usize) -> u16 {
    MACHINE_SELECT_BASE + index as u16
}

pub fn decode_machine_select(action_id: u16) -> Option<usize> {
    if (MACHINE_SELECT_BASE..MACHINE_SELECT_BASE + HALL_SIZE as u16).contains(&action_id) {
        Some((action_id - MACHINE_SELECT_BASE) as usize)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn machine_select_id_roundtrips_for_every_seat() {
        for index in 0..HALL_SIZE {
            let id = machine_select_id(index);
            assert_eq!(decode_machine_select(id), Some(index));
        }
    }

    #[test]
    fn decode_machine_select_rejects_out_of_hall_ids() {
        // ホールの台数を超える id を通すと、存在しない台に着席させてしまう。
        assert_eq!(decode_machine_select(machine_select_id(HALL_SIZE)), None);
        assert_eq!(decode_machine_select(MACHINE_SELECT_BASE - 1), None);
        assert_eq!(decode_machine_select(TOGGLE_FIRE), None);
        assert_eq!(decode_machine_select(BOARD_TAP), None);
        assert_eq!(decode_machine_select(INFO_SCROLL_DOWN), None);
    }
}
