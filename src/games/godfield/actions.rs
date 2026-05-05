//! Click action ID constants for 神の戦場.
//!
//! IDs are local to the game's [`ClickScope`](crate::input::ClickScope), so
//! the global namespace doesn't need to be coordinated with other games.

// ── Top-level actions ──────────────────────────────────────────
pub const ACTION_START: u16 = 1;
pub const ACTION_ATTACK: u16 = 2;
pub const ACTION_HEAL: u16 = 3;
pub const ACTION_SPECIAL: u16 = 4;
pub const ACTION_PASS: u16 = 5;
pub const ACTION_CONFIRM_WEAPONS: u16 = 6;
pub const ACTION_CANCEL: u16 = 7;
pub const ACTION_RESTART: u16 = 8;

// ── Indexed actions ────────────────────────────────────────────
/// Tap a card in the human's hand: `HAND_BASE + hand_idx`.
pub const HAND_BASE: u16 = 100;
/// Tap a player to attack: `TARGET_BASE + player_idx`.
pub const TARGET_BASE: u16 = 200;
