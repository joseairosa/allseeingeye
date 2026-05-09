import { describe, expect, it } from "vitest";
import {
  contextWindowPct,
  estimateTokens,
  formatBytes,
  formatTokensExact,
  formatTokensK,
  MAX_CONTEXT_TOKENS,
  OVERSIZED_MEMORY_BYTES,
} from "./tokens";

describe("formatBytes", () => {
  it("renders bytes under 1k as a plain B label", () => {
    expect(formatBytes(0)).toBe("0B");
    expect(formatBytes(987)).toBe("987B");
  });

  it("renders kilobytes with one decimal", () => {
    expect(formatBytes(12_345)).toBe("12.1kB");
    expect(formatBytes(1024)).toBe("1.0kB");
  });

  it("renders megabytes with one decimal", () => {
    // 2.5 MiB exactly rounds to 2.5MB.
    expect(formatBytes(2.5 * 1024 * 1024)).toBe("2.5MB");
  });

  it("accepts bigint inputs (ts-rs i64 bindings)", () => {
    expect(formatBytes(8192n)).toBe("8.0kB");
  });

  it("clamps invalid inputs to 0B rather than throwing", () => {
    expect(formatBytes(-1)).toBe("0B");
    expect(formatBytes(Number.NaN)).toBe("0B");
  });
});

describe("estimateTokens", () => {
  it("divides by the documented 4-chars-per-token heuristic", () => {
    expect(estimateTokens(0)).toBe(0);
    expect(estimateTokens(4)).toBe(1);
    expect(estimateTokens(12_288)).toBe(3072);
  });

  it("accepts bigint", () => {
    expect(estimateTokens(8192n)).toBe(2048);
  });

  it("returns 0 for invalid input", () => {
    expect(estimateTokens(-1)).toBe(0);
    expect(estimateTokens(Number.NaN)).toBe(0);
  });
});

describe("formatTokensK", () => {
  it("uses '<0.1k' for sub-100 token counts so 0.0k never appears", () => {
    expect(formatTokensK(24)).toBe("<0.1k");
    expect(formatTokensK(99)).toBe("<0.1k");
  });

  it("rounds to one decimal", () => {
    expect(formatTokensK(3072)).toBe("3.1k");
    expect(formatTokensK(50_000)).toBe("50.0k");
    expect(formatTokensK(123_456)).toBe("123.5k");
  });

  it("clamps invalid input to 0k", () => {
    expect(formatTokensK(-1)).toBe("0k");
    expect(formatTokensK(Number.NaN)).toBe("0k");
  });
});

describe("formatTokensExact", () => {
  it("uses thousands separators", () => {
    expect(formatTokensExact(3072)).toBe("3,072");
    expect(formatTokensExact(0)).toBe("0");
    expect(formatTokensExact(1_234_567)).toBe("1,234,567");
  });

  it("clamps invalid input", () => {
    expect(formatTokensExact(-5)).toBe("0");
    expect(formatTokensExact(Number.NaN)).toBe("0");
  });
});

describe("contextWindowPct", () => {
  it("computes share of the 200k window with one decimal", () => {
    expect(contextWindowPct(3072)).toBeCloseTo(1.5, 1);
    expect(contextWindowPct(MAX_CONTEXT_TOKENS)).toBe(100);
  });

  it("returns 0 for non-positive token counts", () => {
    expect(contextWindowPct(0)).toBe(0);
    expect(contextWindowPct(-1)).toBe(0);
  });
});

describe("OVERSIZED_MEMORY_BYTES", () => {
  it("matches the backend bloat threshold (~2k tokens)", () => {
    expect(OVERSIZED_MEMORY_BYTES).toBe(8192);
    // The threshold should imply roughly 2k tokens via the heuristic.
    expect(estimateTokens(OVERSIZED_MEMORY_BYTES)).toBe(2048);
  });
});
