import { describe, expect, it } from "vitest";
import { parseLastValue, parseSearchQuery } from "./parseFilter";

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

  it("converts last:Nd into a modifiedAfterUnix cutoff", () => {
    const NOW = 2_000_000_000;
    const { filter } = parseSearchQuery("last:7d", NOW);
    expect(filter.modifiedAfterUnix).toBe(BigInt(NOW - 7 * 86_400));
  });

  it("supports h, m, s, w units", () => {
    const NOW = 2_000_000_000;
    expect(parseSearchQuery("last:24h", NOW).filter.modifiedAfterUnix).toBe(
      BigInt(NOW - 24 * 3_600),
    );
    expect(parseSearchQuery("last:90m", NOW).filter.modifiedAfterUnix).toBe(
      BigInt(NOW - 90 * 60),
    );
    expect(parseSearchQuery("last:2w", NOW).filter.modifiedAfterUnix).toBe(
      BigInt(NOW - 14 * 86_400),
    );
  });

  it("falls through to free text for malformed last: values", () => {
    const { filter, freeText } = parseSearchQuery("last:7days");
    expect(filter.modifiedAfterUnix).toBeNull();
    expect(freeText).toBe("last:7days");
  });

  it("rejects last:0d (zero-window is never the intent)", () => {
    const { filter, freeText } = parseSearchQuery("last:0d");
    expect(filter.modifiedAfterUnix).toBeNull();
    expect(freeText).toBe("last:0d");
  });
});

describe("parseLastValue", () => {
  it("returns null for empty / non-numeric input", () => {
    expect(parseLastValue("", 0)).toBeNull();
    expect(parseLastValue("abc", 0)).toBeNull();
    expect(parseLastValue("d", 0)).toBeNull();
  });

  it("rejects fractions and negatives", () => {
    expect(parseLastValue("1.5d", 0)).toBeNull();
    expect(parseLastValue("-3d", 0)).toBeNull();
  });

  it("is case-insensitive for the unit", () => {
    expect(parseLastValue("3D", 1000)).toBe(1000 - 3 * 86_400);
    expect(parseLastValue("12H", 1000)).toBe(1000 - 12 * 3_600);
  });
});
