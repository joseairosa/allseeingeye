import { describe, expect, it } from "vitest";
import { toggleFilterPrefix } from "./filterChip";

describe("toggleFilterPrefix", () => {
  it("appends a prefix that is not present", () => {
    expect(toggleFilterPrefix("", "tool:claude-code")).toBe("tool:claude-code");
  });

  it("appends with a single space separator preserving free text", () => {
    expect(toggleFilterPrefix("cool feature", "type:skill")).toBe(
      "cool feature type:skill",
    );
  });

  it("removes an existing prefix and preserves the remainder", () => {
    expect(
      toggleFilterPrefix("tool:claude-code type:skill cool", "tool:claude-code"),
    ).toBe("type:skill cool");
  });

  it("is idempotent: toggle twice returns the original tokens", () => {
    const start = "type:skill cool";
    const once = toggleFilterPrefix(start, "scope:user");
    const twice = toggleFilterPrefix(once, "scope:user");
    expect(twice).toBe("type:skill cool");
  });

  it("collapses repeated occurrences of the same prefix on remove", () => {
    expect(
      toggleFilterPrefix("tool:claude-code tool:claude-code foo", "tool:claude-code"),
    ).toBe("foo");
  });

  it("normalises whitespace and is case-insensitive on the prefix match", () => {
    expect(
      toggleFilterPrefix("  Tool:Claude-Code   foo  ", "tool:claude-code"),
    ).toBe("foo");
  });
});
