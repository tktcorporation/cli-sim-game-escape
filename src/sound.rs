//! 効果音 (Web Audio API 経由の合成音) の薄いラッパー。
//!
//! 実体は `index.html` の `window.__playSound(name)` (Web Audio API で
//! oscillator + envelope を組む)。Rust 側はキー名を渡すだけで、音色定義や
//! AudioContext のライフサイクルは JS 側が握る。
//!
//! WASM ビルドでのみ実音を鳴らし、native (cargo test) では no-op。
//! `play("...")` を呼ぶ箇所はゲームロジックの好きな場所に置いてよい。
//!
//! ## 音色一覧 (JS 側 `SOUNDS` テーブルと同期させる)
//!
//! - 汎用: `click`, `select`, `error`
//! - 購入系: `purchase`, `enhance`
//! - 進化系: `level_up`
//! - metropolis: `build_complete`
//! - abyss: `gacha`, `hit_enemy`, `hit_hero`, `boss_appear`, `floor_clear`, `critical`

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
pub const BUILD_COMPLETE: &str = "build_complete";
pub const GACHA: &str = "gacha";
pub const HIT_ENEMY: &str = "hit_enemy";
pub const HIT_HERO: &str = "hit_hero";
pub const BOSS_APPEAR: &str = "boss_appear";
pub const FLOOR_CLEAR: &str = "floor_clear";
pub const CRITICAL: &str = "critical";
