//! 深淵潜行 — クリックアクション ID。
//!
//! 各定数はUIの 1 アクションを表す。render 時に登録され、
//! `InputEvent::Click` 経由で受け取って dispatch する。

// ── サブタブ (各 Tab の直接切替: グループ内サブタブバー / 旧キー継承で使用) ──
pub const TAB_UPGRADES: u16 = 10;
pub const TAB_ROADMAP: u16 = 11;
pub const TAB_STATS: u16 = 12;
pub const TAB_GACHA: u16 = 13;
pub const TAB_SETTINGS: u16 = 14;
pub const TAB_SHOP: u16 = 15;
pub const TAB_SOULS: u16 = 16;

// ── トップグループ (メインメニュー 4 つ) ────────────────────
pub const TAB_GROUP_GROWTH: u16 = 50;
pub const TAB_GROUP_INFO: u16 = 51;
pub const TAB_GROUP_GACHA: u16 = 52;
pub const TAB_GROUP_SETTINGS: u16 = 53;

// ── トグル / 操作 ─────────────────────────────────────────
pub const TOGGLE_AUTO_DESCEND: u16 = 20;
pub const RETREAT_TO_SURFACE: u16 = 21;

// ── 魂強化購入 (base + SoulPerk::index, 0..4) ─────────────
pub const BUY_SOUL_PERK_BASE: u16 = 200;

// ── ガチャ ────────────────────────────────────────────────
pub const GACHA_PULL_1: u16 = 300;
pub const GACHA_PULL_10: u16 = 301;

// ── タブ本体スクロール (▲▼ オーバーレイ用) ───────────────
pub const SCROLL_UP: u16 = 400;
pub const SCROLL_DOWN: u16 = 401;

// ── 装備購入 (base + EquipmentId::index, 0..12) ───────────
pub const BUY_EQUIPMENT_BASE: u16 = 500;

// ── 装備装着切替 (base + EquipmentId::index, 0..12) ───────
/// 既に所持している装備を、その lane に装着する。lane は EquipmentId から導出されるので
/// アクション ID 1 種で「どの装備を装着」が一意に決まる (lane id を別に持たない)。
pub const EQUIP_ITEM_BASE: u16 = 520;

// ── 装備強化 (base + EquipmentId::index, 0..12) ───────────
/// 指定装備の強化 Lv を 1 上げる。所持装備に対してしか UI から呼ばれない想定だが
/// logic 側でも owned ガードはせず、cost と gold だけ見る (装備強化は所持に依らず
/// state.equipment_levels に蓄積する設計 — 将来 prestige 系で使い回せるよう柔軟性確保)。
pub const ENHANCE_EQUIPMENT_BASE: u16 = 540;
