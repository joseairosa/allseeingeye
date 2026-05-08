/**
 * Quick Look smoke test.
 *
 * Click an inventory row and assert the Quick Look panel slides in;
 * then close it via the close button and assert it goes hidden.
 *
 * The store auto-opens Quick Look whenever a component is selected
 * (`selectComponent` sets `quickLookOpen: id !== null`), so a single
 * click on the row covers the open path.
 */
import { test, expect } from "@playwright/test";
import { buildInitScript } from "./fixtures/mockTauri";

test.beforeEach(async ({ page }) => {
  await page.addInitScript(buildInitScript());
});

test("quick look opens on row click and closes via close button", async ({ page }) => {
  await page.goto("/");

  // Wait for at least one virtualised row before clicking.
  const firstRow = page.getByRole("row").filter({ has: page.locator("strong") }).first();
  await expect(firstRow).toBeVisible();
  await firstRow.click();

  // Quick Look panel is `<aside class="quicklook" aria-label="quick look panel">`,
  // toggled with the `open` class. We address it via the
  // aria-label directly because `aria-hidden` flips with the open
  // state and would hide the element from the accessibility tree
  // when closed.
  const quickLook = page.locator("aside[aria-label='quick look panel']");
  await expect(quickLook).toHaveClass(/open/);

  // Close it via the labelled close button.
  await page.getByRole("button", { name: "close quick look" }).click();
  await expect(quickLook).not.toHaveClass(/open/);
});
