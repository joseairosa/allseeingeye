/**
 * Dedicated vitest config so we can scope unit tests without bleeding
 * vitest types into vite.config.ts (which is consumed by tsc and would
 * complain about an unknown `test` property without ambient `vitest`
 * imports).
 *
 * Why a scope-down: Phase 5.1 added `tests/e2e/*.spec.ts` for
 * Playwright. Vitest's default discovery picks up *.spec files
 * everywhere; including a Playwright spec in vitest registers
 * `test.beforeEach()` outside Playwright's runner and fails. We
 * restrict vitest to `src/**` and explicitly exclude `tests/`.
 */
import { defineConfig } from "vitest/config";
import path from "node:path";

export default defineConfig({
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
      "@aseye/shared-types": path.resolve(__dirname, "../../packages/shared-types/src"),
    },
  },
  test: {
    include: ["src/**/*.{test,spec}.{ts,tsx}"],
    exclude: ["node_modules/**", "dist/**", "tests/**"],
  },
});
