//! Semantic action ID constants for the Café game.

// ── Story mode ────────────────────────────────────────────
/// Advance text in story mode (tap anywhere / press space)
pub const STORY_ADVANCE: u16 = 100;

// ── Hub tabs ──────────────────────────────────────────────
pub const TAB_HOME: u16 = 110;
pub const TAB_CHARACTERS: u16 = 111;
pub const TAB_CARDS: u16 = 112;
pub const TAB_MISSIONS: u16 = 113;

// ── Hub actions ───────────────────────────────────────────
/// Continue story / enter next chapter
pub const HUB_STORY: u16 = 120;
/// Open business (run café day)
pub const HUB_BUSINESS: u16 = 121;

// ── Character select ──────────────────────────────────────
pub const CHARACTER_BASE: u16 = 130; // +0..4 for characters
pub const CHARACTER_BACK: u16 = 139;

// ── Action select ─────────────────────────────────────────
pub const ACTION_EAT: u16 = 140;
pub const ACTION_OBSERVE: u16 = 141;
pub const ACTION_TALK: u16 = 142;
pub const ACTION_SPECIAL: u16 = 143;
pub const ACTION_BACK: u16 = 144;

// ── Action result ─────────────────────────────────────────
pub const RESULT_OK: u16 = 150;

// ── Card screen ───────────────────────────────────────────
pub const CARD_DAILY_DRAW: u16 = 160;
pub const CARD_GACHA_SINGLE: u16 = 161;
pub const CARD_GACHA_TEN: u16 = 162;
pub const CARD_EQUIP_BASE: u16 = 170; // +0..19 for card equip
#[allow(dead_code)] // Phase 2+ card level up UI
pub const CARD_LEVEL_UP_BASE: u16 = 190;
pub const CARD_BACK: u16 = 199;

// ── Gacha result ──────────────────────────────────────────
pub const GACHA_RESULT_OK: u16 = 200;

// ── Character detail ──────────────────────────────────────
pub const DETAIL_EPISODE_BASE: u16 = 210; // +0..9 for episodes
pub const DETAIL_BACK: u16 = 219;

// ── Business phase: menu selection ─────────────────────────
#[allow(dead_code)] // Phase 2+ menu selection
pub const MENU_ITEM_BASE: u16 = 220;
pub const SERVE_CONFIRM: u16 = 240;

// ── Day result ────────────────────────────────────────────
pub const DAY_RESULT_OK: u16 = 250;
