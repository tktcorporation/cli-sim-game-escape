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

/// 拠点画面のタブ切替 (出撃/強化/武器/戦績)。
pub const CAMP_TAB_PREPARE: u16 = 14;
pub const CAMP_TAB_UPGRADES: u16 = 15;
pub const CAMP_TAB_WEAPONS: u16 = 16;
pub const CAMP_TAB_STATS: u16 = 17;

/// 武器解放の購入: action_id = CAMP_UNLOCK_WEAPON_BASE + `WeaponKind::save_id()`。
/// `WeaponKind::all()` は現状8種なので 20..28 を占有する。
pub const CAMP_UNLOCK_WEAPON_BASE: u16 = 20;

/// 初期武器の選択 (◀/▶)。解放済みの武器のみを `WeaponKind::all()` の
/// 順で巡回する。
pub const CAMP_STARTING_WEAPON_PREV: u16 = 30;
pub const CAMP_STARTING_WEAPON_NEXT: u16 = 31;

/// 灯のタイプの選択 (◀/▶)。`LanternType::all()` を巡回する。
pub const CAMP_LANTERN_TYPE_PREV: u16 = 32;
pub const CAMP_LANTERN_TYPE_NEXT: u16 = 33;

/// 武器詳細モーダル (`state.weapon_detail_modal`) の確定/閉じるボタン。
/// モーダル中は常に1件しか開かないため、対象を引数に持たない固定IDでよい。
pub const CAMP_WEAPON_DETAIL_CONFIRM: u16 = 34;
pub const CAMP_WEAPON_DETAIL_CLOSE: u16 = 35;

/// 夜番中に自ら拠点へ撤退する。灯が0になった時と同じ後処理を共有する。
pub const RETREAT_TO_CAMP: u16 = 7;

/// レベルアップモーダルの選択肢: action_id = BOON_OPTION_BASE + index (0..3)
pub const BOON_OPTION_BASE: u16 = 10;

/// 戦場タップ移動 (ClickableGrid, COLUMNS×1): action_id = LANE_CLICK_BASE + lane
pub const LANE_CLICK_BASE: u16 = 100;
