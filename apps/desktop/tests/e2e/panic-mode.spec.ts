/**
 * Panic mode smoke test.
 *
 * Cmd-Shift-. toggles panic mode. The store flips `panicMode`,
 * `App.tsx` mirrors it onto `body.panic`, and the panic badge
 * appears in the corner. The test asserts the body class flips
 * on, then back off, on a second invocation.
 */
import { test, expect } from "@playwright/test";
import { buildInitScript } from "./fixtures/mockTauri";

test.beforeEach(async ({ page }) => {
  await page.addInitScript(buildInitScript());
});

test("panic mode shortcut toggles body.panic", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByRole("table", { name: "components" })).toBeVisible();

  const body = page.locator("body");
  await expect(body).not.toHaveClass(/panic/);

  // The handler in `lib/keyboard.ts` checks `event.key === "."` AND
  // `event.metaKey/ctrlKey` AND `event.shiftKey`. Synthetic
  // Playwright presses of Meta+Shift+Period emit `event.key === ">"`
  // because Chromium applies the OS keyboard map; macOS WebKit (the
  // production runtime) doesn't transform `.` under Cmd+Shift, so
  // production fires `event.key === "."`. To exercise the same code
  // path the production runtime hits, we dispatch a synthetic
  // KeyboardEvent that reproduces the macOS shape exactly.
  async function fireCmdShiftDot(): Promise<void> {
    await page.evaluate(() => {
      document.dispatchEvent(
        new KeyboardEvent("keydown", {
          key: ".",
          code: "Period",
          metaKey: true,
          shiftKey: true,
          bubbles: true,
        }),
      );
    });
  }

  await fireCmdShiftDot();
  await expect(body).toHaveClass(/panic/);

  await fireCmdShiftDot();
  await expect(body).not.toHaveClass(/panic/);
});
