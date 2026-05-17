//! Library root.
//!
//! `main.rs` (game UI) と `bin/metropolis_worker.rs` (AI を別 WASM
//! インスタンスで動かす Web Worker エントリ) の両方から共有するために、
//! ゲームロジック・入力ハンドリング・widget・time tick を lib として公開する。
//!
//! Worker が必要とするのは現状 `games::metropolis::{ai, logic, state, save}`
//! のみ。残りのモジュールも同居させているのは、追加の worker 化や統合
//! テストから再利用できる余地を残すため。

pub mod games;
pub mod input;
pub mod sound;
pub mod time;
pub mod widgets;

/// 「メニューに戻る」共通アクション ID。
/// 各ゲームの `Clickable::new(back, BACK_TO_MENU)` から参照されるため、
/// lib のルートに置いてクレート全域から `crate::BACK_TO_MENU` で引けるようにする。
pub const BACK_TO_MENU: u16 = 65535;
