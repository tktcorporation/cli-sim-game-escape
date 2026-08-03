//! 常夜灯 click target 用のセマンティックアクションID。

pub const CAMP_UPGRADE_LIGHT: u16 = 1;
pub const CAMP_UPGRADE_POWER: u16 = 2;
pub const CAMP_UPGRADE_EXTRA_SLOT: u16 = 3;
pub const CAMP_START_VIGIL: u16 = 4;
pub const CAMP_SCROLL_UP: u16 = 5;
pub const CAMP_SCROLL_DOWN: u16 = 6;

/// 挑戦ランクの選択 (◀/▶)。`max_unlocked_rank` を超えて選ぶことはできない。
pub const CAMP_RANK_DOWN: u16 = 8;
pub const CAMP_RANK_UP: u16 = 9;

/// 武器スロット拡張 (5枠目解放)。`BOON_OPTION_BASE` (10) の範囲と衝突しない
/// よう、レベルアップ選択肢の帯 (10..13) の外側に置く。
pub const CAMP_UPGRADE_EXTRA_WEAPON_SLOT: u16 = 13;

/// 夜番中に自ら拠点へ撤退する。灯が0になった時と同じ後処理を共有する。
pub const RETREAT_TO_CAMP: u16 = 7;

/// レベルアップモーダルの選択肢: action_id = BOON_OPTION_BASE + index (0..3)
pub const BOON_OPTION_BASE: u16 = 10;

/// 戦場タップ移動 (ClickableGrid, COLUMNS×1): action_id = LANE_CLICK_BASE + lane
pub const LANE_CLICK_BASE: u16 = 100;
