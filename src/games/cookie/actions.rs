//! Semantic action IDs for Cookie Factory click targets.
//!
//! Each constant represents a distinct clickable action in the UI.
//! These IDs are registered during render and dispatched via `InputEvent::Click`.

// ── Core actions ────────────────────────────────────────────────
pub const CLICK_COOKIE: u16 = 0;
pub const CLAIM_GOLDEN: u16 = 1;

// ── Tab navigation ──────────────────────────────────────────────
pub const TAB_PRODUCERS: u16 = 10;
pub const TAB_UPGRADES: u16 = 11;
pub const TAB_RESEARCH: u16 = 12;
pub const TAB_MILESTONES: u16 = 13;
pub const TAB_PRESTIGE: u16 = 14;

// ── Producer purchase (base + producer index 0..11) ─────────────
pub const BUY_PRODUCER_BASE: u16 = 100;

// ── Upgrade purchase (base + display index) ─────────────────────
pub const BUY_UPGRADE_BASE: u16 = 200;

// ── Research purchase (base + display index) ────────────────────
pub const BUY_RESEARCH_BASE: u16 = 300;

// ── Milestone actions ───────────────────────────────────────────
pub const CLAIM_MILESTONE_BASE: u16 = 400;
pub const CLAIM_ALL_MILESTONES: u16 = 499;

// ── Prestige actions ────────────────────────────────────────────
pub const PRESTIGE_RESET: u16 = 500;
pub const BUY_PRESTIGE_UPGRADE_BASE: u16 = 600;

// ── Dragon actions (feed producer to dragon, base + producer index) ──
pub const DRAGON_FEED_BASE: u16 = 700;
pub const DRAGON_CYCLE_AURA: u16 = 799;

// ── Prestige sub-section navigation ─────────────────────────────
pub const PRESTIGE_SEC_UPGRADES: u16 = 520;
pub const PRESTIGE_SEC_BOOSTS: u16 = 521;
pub const PRESTIGE_SEC_DRAGON: u16 = 522;
pub const PRESTIGE_SEC_STATS: u16 = 523;
pub const PRESTIGE_SCROLL_UP: u16 = 530;
pub const PRESTIGE_SCROLL_DOWN: u16 = 531;

// ── Sugar boost / auto-clicker ──────────────────────────────────
pub const SUGAR_RUSH: u16 = 800;
pub const SUGAR_FEVER: u16 = 801;
pub const SUGAR_FRENZY: u16 = 802;
pub const TOGGLE_AUTO_CLICKER: u16 = 810;
