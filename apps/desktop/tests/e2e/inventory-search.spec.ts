/**
 * Inventory search smoke test.
 *
 * Loads the app with the stubbed Tauri runtime, types into the
 * inventory search field, and asserts at least one row matches.
 * Runs against the mock IPC backend (not a live `tauri dev`) — the
 * mock returns a fixed three-component fixture so the assertion is
 * deterministic without coupling to real on-disk content.
 */
import { test, expect } from "@playwright/test";
import { buildInitScript } from "./fixtures/mockTauri";

test.beforeEach(async ({ page }) => {
  await page.addInitScript(buildInitScript());
});

test("inventory search filters rows", async ({ page }) => {
  await page.goto("/");

  // Wait for the inventory grid to mount and at least one row to render.
  const grid = page.getByRole("table", { name: "components" });
  await expect(grid).toBeVisible();

  // The store seeds a default search ("type:skill tool:claude-code"), so
  // we clear it first to start from a known state.
  const search = page.getByLabel("search components");
  await search.fill("");
  await search.fill("spec");

  // The "spec" skill is the canonical match in the fixture; we assert
  // by visible text rather than row count to stay resilient to virtualised
  // overscan rendering more rows than strictly required.
  const specRow = page.getByRole("row").filter({ hasText: /Spec/ });
  await expect(specRow.first()).toBeVisible();
});
