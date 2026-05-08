import { describe, expect, it } from "vitest";
import {
  filterPaletteActions,
  type PaletteAction,
} from "./paletteActions";

const noop = (): void => {};

const ACTIONS: readonly PaletteAction[] = [
  { id: "open-inventory", label: "Open Inventory", run: noop },
  { id: "open-settings", label: "Open Settings", run: noop },
  {
    id: "toggle-theme",
    label: "Toggle theme",
    keywords: ["dark mode", "light mode"],
    run: noop,
  },
  {
    id: "toggle-density",
    label: "Toggle density",
    keywords: ["compact", "comfortable"],
    run: noop,
  },
  { id: "run-full-scan", label: "Run full scan", run: noop },
];

describe("PaletteAction registry", () => {
  it("ids are unique across the registry", () => {
    const ids = ACTIONS.map((a) => a.id);
    expect(new Set(ids).size).toBe(ids.length);
  });
});

describe("filterPaletteActions", () => {
  it("returns the whole list when the input is empty", () => {
    expect(filterPaletteActions(ACTIONS, "").map((a) => a.id)).toEqual(
      ACTIONS.map((a) => a.id),
    );
  });

  it("matches against the visible label (subsequence, case-insensitive)", () => {
    // "opn inv" is a subsequence of "open inventory".
    const ids = filterPaletteActions(ACTIONS, "opn inv").map((a) => a.id);
    expect(ids).toContain("open-inventory");
    // No false positive on actions that don't share the order.
    expect(ids).not.toContain("toggle-theme");
  });

  it("matches against keywords when the label does not", () => {
    // "dark" only lives in the toggle-theme keywords.
    const ids = filterPaletteActions(ACTIONS, "dark").map((a) => a.id);
    expect(ids).toEqual(["toggle-theme"]);
  });

  it("matches partial-word fragments inside the label", () => {
    // "scan" is a substring (and subsequence) of "Run full scan".
    const ids = filterPaletteActions(ACTIONS, "scan").map((a) => a.id);
    expect(ids).toEqual(["run-full-scan"]);
  });
});
