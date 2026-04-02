//! Semantic action ID constants for the Café game.

// ── Story mode ────────────────────────────────────────────
/// Advance text in story mode (tap anywhere / press space)
pub const STORY_ADVANCE: u16 = 100;

// ── Business phase: menu selection ─────────────────────────
pub const MENU_ITEM_BASE: u16 = 200; // +0..19 for menu items
pub const SERVE_CONFIRM: u16 = 220;

// ── Business phase: tabs (Phase 2+) ───────────────────────
// pub const TAB_CAFE: u16 = 300;
// pub const TAB_RECIPE: u16 = 301;
// pub const TAB_CUSTOMERS: u16 = 302;

// ── Recipe discovery (Phase 2+) ───────────────────────────
// pub const INGREDIENT_BASE: u16 = 400;
// pub const TRY_RECIPE: u16 = 420;
