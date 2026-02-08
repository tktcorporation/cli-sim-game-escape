//! Semantic action IDs for RPG Quest click targets.

// ── World screen ────────────────────────────────────────────
pub const EXPLORE: u16 = 10;
pub const TALK_NPC: u16 = 11;
pub const GO_SHOP: u16 = 12;
pub const GO_INVENTORY: u16 = 13;
pub const GO_QUEST_LOG: u16 = 14;
pub const GO_STATUS: u16 = 15;
pub const REST: u16 = 16;

/// Travel destination base: + index into connections array.
pub const TRAVEL_BASE: u16 = 20;

// ── Battle screen ───────────────────────────────────────────
pub const BATTLE_ATTACK: u16 = 40;
pub const BATTLE_SKILL: u16 = 41;
pub const BATTLE_ITEM: u16 = 42;
pub const BATTLE_FLEE: u16 = 43;
/// Skill select base: + index into available skills.
pub const SKILL_SELECT_BASE: u16 = 50;
/// Item select base (in battle): + index into consumable items.
pub const BATTLE_ITEM_BASE: u16 = 60;
pub const BATTLE_CONTINUE: u16 = 70;
pub const BACK_FROM_SKILL: u16 = 71;
pub const BACK_FROM_BATTLE_ITEM: u16 = 72;

// ── Inventory screen ────────────────────────────────────────
/// Use/equip item base: + index into inventory.
pub const INV_USE_BASE: u16 = 100;
pub const BACK_FROM_INVENTORY: u16 = 130;

// ── Quest log screen ────────────────────────────────────────
pub const BACK_FROM_QUEST_LOG: u16 = 140;

// ── Shop screen ─────────────────────────────────────────────
/// Buy item base: + index into shop inventory.
pub const SHOP_BUY_BASE: u16 = 150;
pub const BACK_FROM_SHOP: u16 = 170;

// ── Status screen ───────────────────────────────────────────
pub const BACK_FROM_STATUS: u16 = 180;

// ── Dialogue screen ─────────────────────────────────────────
pub const DIALOGUE_NEXT: u16 = 190;

// ── Game clear screen ───────────────────────────────────────
pub const GAME_CLEAR_CONTINUE: u16 = 200;
