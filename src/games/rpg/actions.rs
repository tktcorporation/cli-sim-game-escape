//! Semantic action IDs for Dungeon Dive click targets.

// ── Scene choices (1-based index) ──────────────────────────────
pub const CHOICE_BASE: u16 = 10;

// ── Skill (overlay during dungeon) ─────────────────────────────
pub const SKILL_BASE: u16 = 30;

// ── Overlay: Inventory ─────────────────────────────────────────
pub const INV_USE_BASE: u16 = 60;

// ── Overlay: Shop ──────────────────────────────────────────────
pub const SHOP_BUY_BASE: u16 = 80;

// ── Overlay open ──────────────────────────────────────────────
pub const OPEN_INVENTORY: u16 = 101;
pub const OPEN_STATUS: u16 = 102;
pub const OPEN_SKILL_MENU: u16 = 103;

// ── Overlay close ──────────────────────────────────────────────
pub const CLOSE_OVERLAY: u16 = 100;

// ── Event choices ─────────────────────────────────────────────
pub const EVENT_CHOICE_BASE: u16 = 120;

// ── Map tap zones (3×3 grid) ─────────────────────────────────
pub const MAP_TAP_BASE: u16 = 140;

// ── D-pad controller (3×3 grid) ─────────────────────────────
pub const DPAD_BASE: u16 = 150;

// ── Quest board ──────────────────────────────────────────────
pub const QUEST_ACCEPT_BASE: u16 = 170;
pub const QUEST_ABANDON: u16 = 175;

// ── Pray ─────────────────────────────────────────────────────
pub const PRAY_CONFIRM: u16 = 180;

// ── A/B buttons (dungeon explore) ────────────────────────────
/// Context-sensitive primary action.
/// Adjacent enemy → open skill menu.
/// Standing on event → confirm primary choice.
/// Otherwise → wait one turn.
pub const AB_A_BUTTON: u16 = 190;
/// Open the unified menu (持ち物 / スキル / ステータス tabs).
pub const AB_B_BUTTON: u16 = 191;

// ── Unified menu tab switch ──────────────────────────────────
pub const MENU_TAB_INVENTORY: u16 = 200;
pub const MENU_TAB_SKILL: u16 = 201;
pub const MENU_TAB_STATUS: u16 = 202;
