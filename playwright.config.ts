import { defineConfig, devices } from "@playwright/test";

/**
 * Playwright config for the metropolis AI worker E2E.
 *
 * 本番と同じ multi-bundle 構成 (`trunk build` の `dist/` 配下に main bundle と
 * worker bundle が同居) を `python3 -m http.server` で serve した状態を chromium で
 * 開いて、AI Web Worker が正しく駆動して建物が建ち始めることを検証する。
 */
export default defineConfig({
  testDir: "./tests/e2e",
  fullyParallel: false,
  forbidOnly: !!process.env.CI,
  // Worker bundle の cold start (WASM fetch + instantiate) は環境次第で 1〜3 秒
  // ぶれる。flake 防衛のため 1 リトライを許容する。
  retries: process.env.CI ? 1 : 0,
  workers: 1,
  reporter: process.env.CI ? "github" : "list",

  use: {
    baseURL: "http://127.0.0.1:8181",
    trace: "retain-on-failure",
    actionTimeout: 10_000,
  },

  projects: [
    {
      name: "chromium",
      use: { ...devices["Desktop Chrome"] },
    },
  ],

  webServer: {
    // Trunk が emit した dist/ を静的に配信。port は CI とローカルの衝突を避けて 8181 を採用。
    // python3 は ubuntu-latest / 一般的な開発環境に常駐しており追加 install 不要。
    command: "python3 -m http.server -d dist 8181",
    url: "http://127.0.0.1:8181/index.html",
    timeout: 30_000,
    reuseExistingServer: !process.env.CI,
  },
});
