//! 深淵潜行 — クリックアクション ID。
//!
//! 各定数はUIの 1 アクションを表す。render 時に登録され、
//! `InputEvent::Click` 経由で受け取って dispatch する。

// ── タブ ──────────────────────────────────────────────────
pub const TAB_UPGRADES: u16 = 10;
pub const TAB_SOULS: u16 = 11;
pub const TAB_STATS: u16 = 12;
pub const TAB_GACHA: u16 = 13;
pub const TAB_SETTINGS: u16 = 14;

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
