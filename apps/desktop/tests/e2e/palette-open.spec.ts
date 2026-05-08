/**
 * Command palette smoke test.
 *
 * Opens the palette via Cmd-K, types a query against the static
 * action registry, and asserts that hitting Enter switches the
 * active view. The actions list is hard-coded in `CommandPalette.tsx`,
 * so this test does not require any IPC fixtures beyond what the
 * mock provides for component results.
 */
import { test, expect } from "@playwright/test";
import { buildInitScript } from "./fixtures/mockTauri";

test.beforeEach(async ({ page }) => {
  await page.addInitScript(buildInitScript());
});

test("palette opens, filters actions, switches view", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByRole("table", { name: "components" })).toBeVisible();

  // Cmd-K on macOS / Ctrl-K elsewhere both flow through the same
  // global handler in `lib/keyboard.ts`. Playwright's `Meta` is the
  // platform-correct mod key on the runner.
  await page.keyboard.press("Meta+k");

  const dialog = page.getByRole("dialog", { name: "command palette" });
  await expect(dialog).toBeVisible();

  // Filter the actions list down to the inventory entry. The action
  // registry contains "Open Inventory" verbatim; the filter is case-
  // insensitive subsequence match (see `lib/paletteActions.ts`).
  const palInput = page.getByLabel("command search");
  await palInput.fill("open inventory");

  // The first matching action is the keyboard cursor's default,
  // so Enter activates "Open Inventory" and closes the palette.
  await palInput.press("Enter");

  await expect(dialog).toBeHidden();

  // Inventory view becomes active (carries the `active` class).
  const inventory = page.locator("[data-view-panel='inventory']");
  await expect(inventory).toHaveClass(/active/);
});
