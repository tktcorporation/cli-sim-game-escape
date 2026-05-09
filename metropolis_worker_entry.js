// Metropolis AI Web Worker のエントリ。
//
// `cli_sim_game_escape::bin::metropolis_worker` (別 WASM バイナリ) を
// この Worker のグローバルにロードし、`ai_decide` を onmessage で呼ぶ。
//
// プロトコル:
//   - Main → Worker: `worker.postMessage(request_json: string)`
//   - Worker → Main: `self.postMessage(response_json: string)`
//
// `metropolis::ai_worker` モジュールが規定する JSON 形式を main 側
// (MetropolisGame) と shared して使う。
//
// ## ローダー形式の選択
//
// Trunk の `data-type="worker"` は wasm-bindgen を `--target no-modules` で
// 走らせるため、出力 (`metropolis_worker.js`) は ESM ではなく IIFE 形式の
// グローバル関数 `wasm_bindgen` を提供する。`import` 文は使えないので
// `importScripts` で同期ロードする。Main 側もクラシック Worker (= 非
// `{ type: "module" }`) として生成する規約。
//
// ## エラーハンドリングの方針
//
// WASM 初期化が失敗した場合や `ai_decide` が throw した場合に何も postMessage
// しないと、main 側 `AiWorkerHandle` の `in_flight` がロックされ AI が永久停止
// する。これを防ぐ二重防御:
//   1. ここで `ready` の reject を catch して onmessage 内で握りつぶす
//      (= unhandled rejection を発生させない)。
//   2. main 側 `try_dispatch` がタイムアウトで強制 dispatch を再開する。

importScripts("./metropolis_worker.js");

// `wasm_bindgen` は IIFE のグローバル名。WASM をフェッチして instantiate し、
// 解決後に `ai_decide` を取り出す。`init()` Promise は再 await しても
// 1 度だけ resolve されるので onmessage 内で `await ready` してよい。
//
// reject した時に unhandled rejection を出さないよう `.catch` で潰し、
// `null` を返すことで onmessage 側が「init 失敗 = 何もできない」と判定できるようにする。
const ready = self
  .wasm_bindgen("./metropolis_worker_bg.wasm")
  .catch((e) => {
    console.error("[metropolis_worker] init failed:", e);
    return null;
  });

self.onmessage = async (event) => {
  let initialized;
  try {
    initialized = await ready;
  } catch (_e) {
    // ready が reject されても上の .catch で null になっているので通常ここには
    // 来ないが、もしランナーが Promise を差し替えた場合の保険。
    return;
  }

  if (!initialized || typeof initialized.ai_decide !== "function") {
    return;
  }

  const request = event.data;
  if (typeof request !== "string" || request.length === 0) {
    return;
  }

  try {
    const response = initialized.ai_decide(request);
    if (typeof response === "string" && response.length > 0) {
      self.postMessage(response);
    }
  } catch (e) {
    // 1 リクエスト分の失敗で worker が壊れても、main 側は次の tick でまた
    // request を投げてくるので回復する。致命的エラーは console に残す。
    console.error("[metropolis_worker] ai_decide threw:", e);
  }
};
