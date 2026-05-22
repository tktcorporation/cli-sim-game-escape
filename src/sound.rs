//! 操作フィードバック (効果音 + ハプティクス) の薄いラッパー。
//!
//! 実体は `index.html` の `window.__playSound(name)`。同じ name で 2 系統の
//! フィードバックを駆動する:
//! - 効果音: Web Audio API で oscillator + envelope を合成 (`SOUNDS` テーブル)
//! - 振動:   Vibration API (`navigator.vibrate`) で触覚フィードバック
//!           (`VIBRATION` テーブル、対応端末のみ)
//!
//! Rust 側はキー名を渡すだけで、音色・振動パターンの定義や AudioContext の
//! ライフサイクルは JS 側が握る。
//!
//! WASM ビルドでのみ実フィードバックを出し、native (cargo test) では no-op。
//! `play("...")` を呼ぶ箇所はゲームロジックの好きな場所に置いてよい。
//!
//! ## イベント名一覧 (JS 側 `SOUNDS` / `VIBRATION` テーブルと同期させる)
//!
//! - 汎用: `click`, `select`, `error`
//! - 購入系: `purchase`, `enhance`
//! - 進化系: `level_up`
//! - abyss: `gacha`, `hit_hero`, `boss_appear`, `floor_clear`, `critical`

#[cfg(target_arch = "wasm32")]
mod imp {
    use wasm_bindgen::prelude::*;

    #[wasm_bindgen]
    extern "C" {
        // index.html の `<script>` 内で `window.__playSound` として定義。
        // catch (_e) で握りつぶしているので `js_name` 解決失敗以外は throw しない。
        #[wasm_bindgen(js_namespace = window, js_name = __playSound, catch)]
        fn js_play_sound(name: &str) -> Result<(), JsValue>;
    }

    pub fn play(name: &str) {
        // 失敗 (関数が未定義 / AudioContext 未対応) は無視。音が出ないだけで
        // ゲーム本体は止めない。
        let _ = js_play_sound(name);
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod imp {
    pub fn play(_name: &str) {
        // native (cargo test) では何もしない。テストから sound::play() を
        // 呼ぶコード経路を素通しさせるための stub。
    }
}

pub use imp::play;

// 音色名は文字列 1 箇所に集約する。typo を防ぐ意図と、JS 側の `SOUNDS`
// テーブルとの対応関係を Rust 側から grep 一発で辿れるようにするため。
pub const CLICK: &str = "click";
pub const SELECT: &str = "select";
pub const ERROR: &str = "error";
pub const PURCHASE: &str = "purchase";
pub const ENHANCE: &str = "enhance";
pub const LEVEL_UP: &str = "level_up";
pub const GACHA: &str = "gacha";
pub const HIT_HERO: &str = "hit_hero";
pub const BOSS_APPEAR: &str = "boss_appear";
pub const FLOOR_CLEAR: &str = "floor_clear";
pub const CRITICAL: &str = "critical";
