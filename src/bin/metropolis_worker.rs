//! Metropolis AI Web Worker のエントリ。
//!
//! Trunk が `data-trunk rel="rust" data-bin="metropolis_worker" data-type="worker"`
//! でこの bin を別 WASM バンドルとしてビルドする。Main thread (`cli-sim-game-escape`
//! バイナリ) と **別の WASM インスタンス** がワーカー側で動き、
//! `ai::decide` の重い探索がレンダリングと並列で進む。
//!
//! ## 通信プロトコル
//!
//! - Main → Worker: `postMessage(request_json: string)`
//! - Worker → Main: `postMessage(response_json: string)`
//!
//! 形式は `metropolis::ai_worker` モジュールが規定する JSON。Worker 内部では
//! `handle_request_json` を呼ぶだけで、AI 関連のロジックは全てメインクレート
//! 側 (`metropolis::ai` / `logic` / `state`) に集約されている。
//!
//! ## 注意
//!
//! Worker 側 WASM はメイン WASM の `City` 状態と **メモリ空間を共有しない**。
//! 1 リクエストごとに JSON snapshot を `apply_save` で fresh な `City` に
//! 流し込んで `decide` を回し、結果だけ JSON で返す。stale handling や
//! re-validation は main 側 (MetropolisGame::tick) の責務。

#[cfg(target_arch = "wasm32")]
mod wasm_entry {
    use cli_sim_game_escape::games::metropolis::ai_worker;
    use wasm_bindgen::prelude::*;

    /// Worker から JS が呼ぶ唯一の export。`onmessage(e)` ハンドラが
    /// `ai_decide(e.data)` を呼び、戻り値を `postMessage(...)` で返す。
    ///
    /// エラー時は空文字列を返す (worker.js 側で空文字を弾く実装)。
    /// panic は `console_error_panic_hook` 経由でブラウザ console に出る。
    #[wasm_bindgen]
    pub fn ai_decide(request_json: &str) -> String {
        match ai_worker::handle_request_json(request_json) {
            Ok(s) => s,
            Err(e) => {
                web_sys::console::error_1(
                    &format!("ai_worker handle_request_json: {}", e).into(),
                );
                String::new()
            }
        }
    }
}

/// Worker WASM 起動時のフック。`console_error_panic_hook` を登録するだけで
/// メインスレッドの DOM mount のような副作用は持たない (worker context には
/// `window` も `document` も無い)。
///
/// 非 WASM (cargo test 等) ビルドではこの bin はそもそも起動しないが、
/// `main()` のシグネチャ自体は cargo に必要なので空関数として残す。
fn main() {
    #[cfg(target_arch = "wasm32")]
    console_error_panic_hook::set_once();
}
