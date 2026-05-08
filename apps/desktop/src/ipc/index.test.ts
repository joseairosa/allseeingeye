/**
 * Compile-time + runtime sanity check for the IPC wrappers.
 *
 * The runtime `invoke()` call requires a Tauri host, so we only assert
 * each wrapper resolves to a function. The real value of this test is
 * that it forces TypeScript to resolve the generic types in
 * `@aseye/shared-types` - if a binding goes missing, the suite fails
 * to type-check.
 */
import { describe, expect, it } from "vitest";
import {
  getComponent,
  getHealthSummary,
  listComponents,
  listTools,
  readComponentRaw,
  search,
  startFullScan,
} from "./index";

describe("ipc wrappers", () => {
  it("exposes a function for every command", () => {
    expect(typeof listTools).toBe("function");
    expect(typeof listComponents).toBe("function");
    expect(typeof getComponent).toBe("function");
    expect(typeof readComponentRaw).toBe("function");
    expect(typeof search).toBe("function");
    expect(typeof startFullScan).toBe("function");
    expect(typeof getHealthSummary).toBe("function");
  });
});
