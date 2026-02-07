//! Semantic action IDs for Career Simulator click targets.

// ── Main screen ──────────────────────────────────────────────
pub const GO_TRAINING: u16 = 10;
pub const DO_NETWORKING: u16 = 11;
pub const DO_SIDE_JOB: u16 = 12;
pub const GO_JOB_MARKET: u16 = 20;
pub const GO_INVEST: u16 = 21;
pub const GO_BUDGET: u16 = 22;
pub const GO_LIFESTYLE: u16 = 23;
pub const ADVANCE_MONTH: u16 = 24;

// ── Training screen ────────────────────────────────────────────
pub const TRAINING_BASE: u16 = 100; // +index 0..4
pub const BACK_FROM_TRAINING: u16 = 109;

// ── Job Market screen ────────────────────────────────────────
pub const APPLY_JOB_BASE: u16 = 30; // +index 0..9
pub const BACK_FROM_JOBS: u16 = 50;

// ── Invest screen ────────────────────────────────────────────
pub const INVEST_SAVINGS: u16 = 60;
pub const INVEST_STOCKS: u16 = 61;
pub const INVEST_REAL_ESTATE: u16 = 62;
pub const BACK_FROM_INVEST: u16 = 70;

// ── Budget screen ────────────────────────────────────────────
pub const BACK_FROM_BUDGET: u16 = 80;

// ── Lifestyle screen ─────────────────────────────────────────
pub const LIFESTYLE_BASE: u16 = 90; // +index 0..4
pub const BACK_FROM_LIFESTYLE: u16 = 99;

// ── Report screen ───────────────────────────────────────────
pub const BACK_FROM_REPORT: u16 = 110;
