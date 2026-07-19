import { test, expect } from "@playwright/test";

/**
 * Metropolis AI Web Worker の E2E 検証。
 *
 * 検出したい主要シナリオは「worker が起動するが応答せず AI が完全停止する
 * 沈黙系の不具合」。`metropolis_worker_entry.js` で raw wasm export を直叩き
 * していた回帰など、Rust の単体テストでは捕まらないクラスを CI で防衛する。
 *
 * ## 観測戦略
 *
 * `<pre>` 内に毎フレーム描画される grid タイトルに `WIP <N>` (現在進行中の
 * 建設数) が出る。AI が 1 度でも `Build` action を成功させれば
 * `start_construction` が active_constructions を 1 以上にし、`WIP 1+` が
 * 表示される。autosave (30 秒) を待つより遥かに早い (= AI 1 dispatch 分の
 * round trip + 1 tick = 数百 ms 〜 数秒) ので、CI のレイテンシに優しい。
 */
test("AI worker dispatches actions and at least one construction starts", async ({
  page,
}) => {
  const consoleErrors: string[] = [];
  const allConsoleMessages: string[] = [];
  page.on("console", (msg) => {
    allConsoleMessages.push(`[${msg.type()}] ${msg.text()}`);
    if (msg.type() === "error") {
      consoleErrors.push(msg.text());
    }
  });
  page.on("pageerror", (err) => {
    consoleErrors.push(`pageerror: ${err.message}`);
  });

  await page.goto("/index.html");

  // メニュー画面の `<pre>` がマウントされるのを待つ。
  await page.waitForSelector("pre");
  // 1 フレーム分余裕を持たせて keyboard handler が登録されるのを待つ。
  await page.waitForTimeout(200);

  // '6' = MENU_SELECT_METROPOLIS。`main.rs` の InputEvent::Key('6') が
  // GameChoice::Metropolis に直接遷移するため、SPACE での確認操作は不要。
  await page.keyboard.press("6");

  // ratzilla は terminal 行ごとに `<pre>` を分割するので、ページ全体テキストで
  // 探す。Metropolis のグリッド title (`▟▙ City — POP ...`) が描画されたら遷移完了。
  await expect
    .poll(
      async () => (await page.locator("body").textContent()) ?? "",
      { timeout: 5_000, intervals: [100, 200] },
    )
    .toContain("City");

  // AI の最初の Build action が反映されるまでポーリング。
  // - worker init: 〜1 秒 (cold start)
  // - request 1 往復: 50〜500 ms
  // - tick で apply: 100 ms
  // 余裕を見て 15 秒。`WIP <N>` は grid title に毎フレーム描画される
  // active_constructions の表示で、AI の Build が成功した瞬間に 1 以上になる。
  try {
    await expect
      .poll(
        async () => {
          const text = (await page.locator("body").textContent()) ?? "";
          const match = text.match(/WIP\s+(\d+)/);
          if (!match) return -1;
          return parseInt(match[1], 10);
        },
        {
          timeout: 15_000,
          intervals: [200, 500, 1_000],
        },
      )
      .toBeGreaterThan(0);
  } catch (e) {
    // 失敗時は console ログを全部吐いて diagnostic に使えるようにする。
    console.log("--- captured console messages ---");
    for (const m of allConsoleMessages) console.log(m);
    console.log("--- end ---");
    throw e;
  }

  // worker は console.error で init/decode 失敗を残す設計。fail がゼロであること。
  const workerErrors = consoleErrors.filter((e) =>
    e.includes("[metropolis_worker]"),
  );
  expect(
    workerErrors,
    "metropolis_worker から console.error が出ている",
  ).toEqual([]);
});
