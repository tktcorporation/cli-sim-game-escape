//! Semantic action IDs for Dungeon Dive click targets.

// ── Scene choices (1-based index) ──────────────────────────────
/// Choice base: +index (0-based). So choice [1] = CHOICE_BASE+0, etc.
pub const CHOICE_BASE: u16 = 10;

// ── Battle sub-menus ───────────────────────────────────────────
/// Skill select base: + index into available skills.
pub const SKILL_BASE: u16 = 30;
/// Item select base (in battle): + index into consumable items.
pub const BATTLE_ITEM_BASE: u16 = 40;
/// Back from sub-menu in battle.
pub const BATTLE_BACK: u16 = 50;

// ── Overlay: Inventory ─────────────────────────────────────────
pub const INV_USE_BASE: u16 = 60;

// ── Overlay: Shop ──────────────────────────────────────────────
pub const SHOP_BUY_BASE: u16 = 80;

// ── Overlay open ──────────────────────────────────────────────
pub const OPEN_INVENTORY: u16 = 101;
pub const OPEN_STATUS: u16 = 102;

// ── Overlay close ──────────────────────────────────────────────
pub const CLOSE_OVERLAY: u16 = 100;

// ── Dungeon movement ──────────────────────────────────────────
pub const MOVE_FORWARD: u16 = 110;
pub const TURN_LEFT: u16 = 111;
pub const TURN_RIGHT: u16 = 112;
pub const TURN_AROUND: u16 = 113;

// ── Event choices ─────────────────────────────────────────────
pub const EVENT_CHOICE_BASE: u16 = 120;
