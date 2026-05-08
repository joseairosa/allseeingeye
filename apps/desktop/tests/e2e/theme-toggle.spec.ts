/**
 * Theme toggle smoke test.
 *
 * The TitleBar carries a "toggle theme" button that cycles
 * dark → light → system → dark. The `App.tsx` effect resolves
 * `system` against `prefers-color-scheme: light`; under Chromium's
 * default media query that resolves to dark, so we only assert the
 * `light` class transition (dark → light) which is deterministic.
 */
import { test, expect } from "@playwright/test";
import { buildInitScript } from "./fixtures/mockTauri";

test.beforeEach(async ({ page }) => {
  await page.addInitScript(buildInitScript());
});

test("theme button toggles body.light class", async ({ page }) => {
  // Force the OS-preferred colour scheme to dark so `system` mode
  // resolves to dark and the dark→light transition is observable
  // without ambiguity from a light-preferring runner.
  await page.emulateMedia({ colorScheme: "dark" });

  await page.goto("/");

  const body = page.locator("body");
  await expect(body).not.toHaveClass(/light/);

  await page.getByRole("button", { name: "toggle theme" }).click();
  await expect(body).toHaveClass(/light/);
});
