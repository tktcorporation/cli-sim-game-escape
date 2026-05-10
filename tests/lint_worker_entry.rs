//! Lint: Web Worker エントリ JS の wasm-bindgen 呼び出し規約を検証する。
//!
//! Trunk の `data-type="worker"` は wasm-bindgen を `--target no-modules` で
//! 走らせるため、出力は IIFE 形式で **2 種類** の `ai_decide` を露出する:
//!
//! - `wasm_bindgen.ai_decide` — 文字列 ↔ ポインタ変換を行う wrapper
//! - `(await wasm_bindgen('./xxx.wasm')).ai_decide` — raw wasm export
//!   `(ptr, len)` を取る。JS 文字列を渡すと NaN coerce → out-of-bounds → panic
//!
//! 後者を呼ぶと worker は沈黙し、main 側はタイムアウトで再 dispatch を永久に
//! 繰り返すだけで AI が完全停止する。`metropolis_worker_entry.js` がこの罠に
//! 落ちていないことを CI で防衛する。
//!
//! このテストは **静的 grep** だけで検証する — Playwright (`tests/e2e/`) で
//! 本番動作を別途検証するが、こちらは `cargo test` で 0.01 秒で回せる即時防衛。

use std::fs;
use std::path::Path;

/// `metropolis_worker_entry.js` を読む。
fn read_worker_entry() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("metropolis_worker_entry.js");
    fs::read_to_string(&path).expect("metropolis_worker_entry.js が読めない")
}

/// JS のコメント (// ... と /* ... */) を空白に置換した版を返す。grep が
/// ドキュメント例文に誤反応するのを防ぐ。**簡易** 実装で文字列リテラル中の
/// `//` までは丁寧に区別しない (今回の検査対象 JS ではバックスラッシュ脱出
/// 入り文字列を扱わないため十分)。
fn strip_comments(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let bytes = src.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // 行コメント
        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'/' {
            while i < bytes.len() && bytes[i] != b'\n' {
                out.push(' ');
                i += 1;
            }
            continue;
        }
        // ブロックコメント
        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            out.push_str("  ");
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                out.push(if bytes[i] == b'\n' { '\n' } else { ' ' });
                i += 1;
            }
            // closing */
            if i + 1 < bytes.len() {
                out.push_str("  ");
                i += 2;
            }
            continue;
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

#[test]
fn worker_entry_calls_wasm_bindgen_wrapper() {
    let raw = read_worker_entry();
    let code = strip_comments(&raw);

    // ai_decide を呼んでいる箇所が必ず `wasm_bindgen.ai_decide` 経由であること。
    let calls: Vec<&str> = code
        .match_indices(".ai_decide(")
        .map(|(idx, _)| {
            let start = idx.saturating_sub(40);
            &code[start..idx + ".ai_decide(".len()]
        })
        .collect();

    assert!(
        !calls.is_empty(),
        "metropolis_worker_entry.js に ai_decide 呼び出しが見当たらない — \
         ファイルが空か、構造が大きく変わった可能性"
    );

    for snippet in &calls {
        assert!(
            snippet.contains("wasm_bindgen.ai_decide"),
            "raw wasm export を直接呼んでいる箇所がある: `{}`\n\
             wasm-bindgen --target no-modules では `wasm_bindgen.ai_decide` (wrapper) を呼ぶ必要がある。\
             `(await ready).ai_decide` や `initialized.ai_decide` は raw export で、\
             文字列を渡すと WASM 側で out-of-bounds → panic → worker 沈黙。",
            snippet
        );
    }
}

#[test]
fn worker_entry_does_not_use_self_wasm_bindgen() {
    // wasm-bindgen --target no-modules は `let wasm_bindgen = (function(){...})()`
    // で **script-scope の let バインディング** を作る。`let` は `var` と違って
    // `self` / `globalThis` のプロパティにならないため、`self.wasm_bindgen` は
    // 常に `undefined` で TypeError になる。bare の `wasm_bindgen(...)` を使うこと。
    let raw = read_worker_entry();
    let code = strip_comments(&raw);
    assert!(
        !code.contains("self.wasm_bindgen"),
        "`self.wasm_bindgen` を参照している。`let wasm_bindgen` は self の \
         プロパティにならないので undefined → TypeError → worker 沈黙。\
         `self.` プレフィックスを外して bare `wasm_bindgen` を使うこと。"
    );
}

#[test]
fn worker_entry_handles_init_rejection() {
    let raw = read_worker_entry();
    let code = strip_comments(&raw);

    // `wasm_bindgen('./xxx.wasm')` の reject を catch していない場合、
    // unhandled rejection で worker がハングする。`.catch(` が付いていることを確認。
    let init_call_idx = code
        .find("wasm_bindgen(")
        .expect("worker_entry が wasm_bindgen('./...wasm') を呼んでいない");
    let after = &code[init_call_idx..init_call_idx + 200.min(code.len() - init_call_idx)];
    assert!(
        after.contains(".catch("),
        "wasm_bindgen('./...wasm') の Promise を `.catch` で握っていない。\
         init reject 時に unhandled rejection になり、worker が無応答ハングする恐れ。\
         `{}` 周辺で `.catch(...)` を付けること。",
        after.lines().next().unwrap_or(after)
    );
}
