//! 深淵潜行 — クリックアクション ID。
//!
//! 各定数はUIの 1 アクションを表す。render 時に登録され、
//! `InputEvent::Click` 経由で受け取って dispatch する。

// ── サブタブ (各 Tab の直接切替: グループ内サブタブバー / 旧キー継承で使用) ──
pub const TAB_UPGRADES: u16 = 10;
/// 進捗サブタブ。旧 Souls タブの id (11) を継承 (save 互換維持)。
pub const TAB_ROADMAP: u16 = 11;
pub const TAB_STATS: u16 = 12;
pub const TAB_GACHA: u16 = 13;
pub const TAB_SETTINGS: u16 = 14;
/// 装備ショップサブタブ。
pub const TAB_SHOP: u16 = 15;
/// 魂サブタブ (旧強化タブ末尾の魂セクションを独立分離)。
pub const TAB_SOULS: u16 = 16;

// ── トップグループ (メインメニュー 4 つ) ────────────────────
// グループをクリックするとそのグループの default_tab() に切替。
// 値域は既存の TAB_* (10-15) と被らない 50 番台に置く。
pub const TAB_GROUP_GROWTH: u16 = 50;
pub const TAB_GROUP_INFO: u16 = 51;
pub const TAB_GROUP_GACHA: u16 = 52;
pub const TAB_GROUP_SETTINGS: u16 = 53;

// ── トグル / 操作 ─────────────────────────────────────────
pub const TOGGLE_AUTO_DESCEND: u16 = 20;
pub const RETREAT_TO_SURFACE: u16 = 21;

// ── 強化購入 (base + UpgradeKind::index, 0..7) ────────────
pub const BUY_UPGRADE_BASE: u16 = 100;

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
