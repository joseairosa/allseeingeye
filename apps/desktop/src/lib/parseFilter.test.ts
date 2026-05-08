import { describe, expect, it } from "vitest";
import { parseSearchQuery } from "./parseFilter";

describe("parseSearchQuery", () => {
  it("splits known prefixes into typed filter slots and keeps free text", () => {
    const { filter, freeText } = parseSearchQuery(
      "tool:claude-code type:skill cool feature",
    );
    expect(filter.toolId).toBe("claude-code");
    expect(filter.kind).toBe("skill");
    expect(filter.query).toBe("cool feature");
    expect(filter.scope).toBeNull();
    expect(filter.tag).toBeNull();
    expect(freeText).toBe("cool feature");
  });

  it("handles a free-text-only input with no prefixes", () => {
    const { filter, freeText } = parseSearchQuery("repo walker hello");
    expect(filter.toolId).toBeNull();
    expect(filter.kind).toBeNull();
    expect(filter.query).toBe("repo walker hello");
    expect(freeText).toBe("repo walker hello");
  });

  it("treats an unknown value for a known prefix as free text", () => {
    const { filter, freeText } = parseSearchQuery("type:bogus widget");
    expect(filter.kind).toBeNull();
    expect(filter.query).toBe("type:bogus widget");
    expect(freeText).toBe("type:bogus widget");
  });

  it("captures scope and tag prefixes", () => {
    const { filter, freeText } = parseSearchQuery(
      "scope:project tag:pinned promo",
    );
    expect(filter.scope).toBe("project");
    expect(filter.tag).toBe("pinned");
    expect(filter.query).toBe("promo");
    expect(freeText).toBe("promo");
  });

  it("returns a fully empty filter for empty input", () => {
    const { filter, freeText } = parseSearchQuery("   ");
    expect(filter.toolId).toBeNull();
    expect(filter.kind).toBeNull();
    expect(filter.scope).toBeNull();
    expect(filter.tag).toBeNull();
    expect(filter.query).toBeNull();
    expect(freeText).toBe("");
  });

  it("tolerates whitespace after the colon for known prefixes", () => {
    const { filter } = parseSearchQuery("tool: claude-code type:  skill");
    expect(filter.toolId).toBe("claude-code");
    expect(filter.kind).toBe("skill");
  });
});
