/**
 * Playwright configuration for the desktop app.
 *
 * The Tauri webview is not launchable in CI (no headless WKWebView /
 * WebView2 build). Instead we run the React shell against the Vite
 * dev server in Chromium and stub `window.__TAURI_INTERNALS__` so
 * `invoke()` calls resolve against deterministic in-memory data.
 * See `tests/e2e/fixtures/mockTauri.ts` for the stub registry.
 *
 * Why Chromium and not WebKit? Chromium is the closest CI-runnable
 * proxy for the React app's behaviour. The components we exercise
 * here (CommandPalette, InventoryView, QuickLook, body-class theme
 * toggles, panic mode) do not touch any WebKit-specific quirks; they
 * use plain DOM events, ARIA, and CSS class toggles. WebKit-only
 * regressions are caught at packaged-Tauri smoke time, not here.
 */
import { defineConfig, devices } from "@playwright/test";

const PORT = 1420;
const BASE_URL = `http://localhost:${PORT}`;

export default defineConfig({
  testDir: "./tests/e2e",
  // Per-test budget. 30s is generous for the kinds of synthetic UI
  // interactions we run (no real network, no Tauri build); a real
  // failure shows up well before the timeout.
  timeout: 30_000,
  // Suite-wide budget kept small intentionally. If the whole grid
  // blows past 60s something is hung, not slow.
  globalTimeout: 60_000,
  fullyParallel: true,
  forbidOnly: Boolean(process.env["CI"]),
  retries: process.env["CI"] ? 1 : 0,
  workers: process.env["CI"] ? 2 : undefined,
  reporter: process.env["CI"] ? "line" : "list",
  use: {
    baseURL: BASE_URL,
    // Trace once on first retry so flake analysis has a snapshot.
    trace: "on-first-retry",
    screenshot: "only-on-failure",
    video: "off",
  },
  projects: [
    {
      name: "chromium",
      use: { ...devices["Desktop Chrome"] },
    },
  ],
  webServer: {
    // `pnpm --filter @aseye/desktop dev` is the same script developers
    // run locally. Vite logs go to playwright's output for triage.
    command: "pnpm --filter @aseye/desktop dev",
    url: BASE_URL,
    reuseExistingServer: !process.env["CI"],
    // Vite cold-start budget. Empirically <10s on local hardware; we
    // leave headroom for slower CI runners.
    timeout: 60_000,
    stdout: "pipe",
    stderr: "pipe",
  },
});
