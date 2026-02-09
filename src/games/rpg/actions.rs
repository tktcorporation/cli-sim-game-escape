//! Semantic action IDs for RPG Quest click targets.
//!
//! Simplified for the scene-based system.
//! Choices always use [1]-[5], overlays use [I]/[S]/[Q].

// ── Scene choices (1-based index) ──────────────────────────────
/// Choice base: +index (0-based). So choice [1] = CHOICE_BASE+0, [2] = CHOICE_BASE+1, etc.
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

// ── Overlay close ──────────────────────────────────────────────
pub const CLOSE_OVERLAY: u16 = 100;
