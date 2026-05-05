//! Idle Metropolis — AI-driven city builder.
//!
//! The player buys upgrades and sets strategy; an automated CPU does the
//! actual placement.  Because the lowest AI tier is intentionally dumb,
//! `simulator.rs` is provided up-front to verify that even a bad CPU keeps
//! the game progressing (cash & population trending up over time).
//!
//! Architecture follows the project's "pure logic" pattern:
//!   • `state.rs`  — all data, no behavior.
//!   • `logic.rs`  — pure functions (tick, income, construction).
//!   • `ai.rs`     — strategy brains, one function per tier.
//!   • `simulator.rs` — balance tests (cargo test, no rendering).
//!
//! `render.rs` and the `Game` trait wiring will be added once balance
//! is verified.

// MVP scaffold: nothing in main.rs touches these modules yet, so every
// public function reads as dead code from the binary build's perspective.
// Remove this allow once `impl Game for MetropolisGame` is wired up in
// `games::create_game`.
#![allow(dead_code)]

pub mod ai;
pub mod logic;
pub mod state;
pub mod simulator;
