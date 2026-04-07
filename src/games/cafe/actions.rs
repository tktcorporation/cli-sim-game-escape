//! Semantic action ID constants for the Café game.

// ── Story mode ────────────────────────────────────────────
pub const STORY_ADVANCE: u16 = 100;

// ── Hub tabs ──────────────────────────────────────────────
pub const TAB_HOME: u16 = 110;
pub const TAB_CHARACTERS: u16 = 111;
pub const TAB_CARDS: u16 = 112;
pub const TAB_PRODUCE: u16 = 113;
pub const TAB_MISSIONS: u16 = 114;

// ── Hub actions ───────────────────────────────────────────
pub const HUB_STORY: u16 = 120;
pub const HUB_BUSINESS: u16 = 121;
pub const HUB_PRODUCE: u16 = 122;

// ── Character select ──────────────────────────────────────
pub const CHARACTER_BASE: u16 = 130; // +0..4
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
pub const CARD_EQUIP_BASE: u16 = 170; // +0..19
pub const CARD_BACK: u16 = 199;

// ── Gacha result ──────────────────────────────────────────
pub const GACHA_RESULT_OK: u16 = 200;

// ── Character detail ──────────────────────────────────────
pub const DETAIL_EPISODE_BASE: u16 = 210; // +0..9
pub const DETAIL_PROMOTE: u16 = 215;
pub const DETAIL_BACK: u16 = 219;

// ── Business / Day result ─────────────────────────────────
pub const SERVE_CONFIRM: u16 = 240;
pub const DAY_RESULT_OK: u16 = 250;

// ── Produce mode ──────────────────────────────────────────
pub const PRODUCE_CHAR_BASE: u16 = 300; // +0..4
pub const PRODUCE_BACK: u16 = 309;
pub const PRODUCE_TRAIN_SERVICE: u16 = 310;
pub const PRODUCE_TRAIN_COOKING: u16 = 311;
pub const PRODUCE_TRAIN_ATMOSPHERE: u16 = 312;
pub const PRODUCE_TRAIN_REST: u16 = 313;
pub const PRODUCE_CONTINUE: u16 = 320;
pub const PRODUCE_FINISH: u16 = 330;
