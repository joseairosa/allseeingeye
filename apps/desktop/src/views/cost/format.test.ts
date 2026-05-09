/**
 * Pure-function unit tests for the Cost view formatters.
 *
 * No RTL / jsdom: this project's vitest harness runs in node, which is
 * exactly what we need for these helpers. Rendering the React shell
 * would not catch what we want to verify - that strings line up with
 * the headline KPI contract spelt out in docs/14.
 */
import { describe, expect, it } from "vitest";
import {
  buildSparklinePoints,
  formatRefreshedAgo,
  formatTokenCount,
  formatUsd,
  shortenProjectPath,
  totalTokens,
} from "./format";

describe("formatUsd", () => {
  it("renders two decimal places with the dollar sign", () => {
    expect(formatUsd(42.18)).toBe("$42.18");
    expect(formatUsd(0)).toBe("$0.00");
  });

  it("always produces exactly two decimal digits", () => {
    expect(formatUsd(1.5).split(".")[1]).toHaveLength(2);
    expect(formatUsd(0.1).split(".")[1]).toHaveLength(2);
    expect(formatUsd(1234.5).split(".")[1]).toHaveLength(2);
  });

  it("collapses null / undefined / NaN to a literal dash", () => {
    expect(formatUsd(null)).toBe("-");
    expect(formatUsd(undefined)).toBe("-");
    expect(formatUsd(Number.NaN)).toBe("-");
  });
});

describe("formatTokenCount", () => {
  it("renders small counts with thousands separators", () => {
    expect(formatTokenCount(0)).toBe("0");
    expect(formatTokenCount(1234)).toBe("1,234");
  });

  it("switches to compact notation above the threshold", () => {
    expect(formatTokenCount(12_345)).toBe("12.3K");
    expect(formatTokenCount(5_400_000)).toBe("5.4M");
    expect(formatTokenCount(1_000_000_000)).toBe("1B");
  });

  it("accepts bigint inputs (`ts-rs` emits `i64` as `bigint`)", () => {
    expect(formatTokenCount(BigInt(2_500_000))).toBe("2.5M");
  });

  it("guards against negative or non-finite values", () => {
    expect(formatTokenCount(-5)).toBe("-");
    expect(formatTokenCount(Number.NaN)).toBe("-");
  });
});

describe("totalTokens", () => {
  it("sums all four buckets across bigint inputs", () => {
    const totals = {
      input: BigInt(100),
      output: BigInt(200),
      cacheRead: BigInt(50),
      cacheCreate: BigInt(25),
    };
    expect(totalTokens(totals)).toBe(375);
  });
});

describe("shortenProjectPath", () => {
  it("keeps the last two path segments", () => {
    expect(
      shortenProjectPath("/Users/jose/Development/projectfinish"),
    ).toBe("Development/projectfinish");
  });

  it("returns shorter inputs unchanged", () => {
    expect(shortenProjectPath("repo")).toBe("repo");
    expect(shortenProjectPath("a/b")).toBe("a/b");
  });

  it("survives a trailing slash", () => {
    expect(shortenProjectPath("/foo/bar/baz/")).toBe("bar/baz");
  });

  it("collapses empty / whitespace-only paths to a placeholder", () => {
    expect(shortenProjectPath("")).toBe("(unknown)");
    expect(shortenProjectPath("/")).toBe("(unknown)");
  });
});

describe("formatRefreshedAgo", () => {
  const NOW = new Date("2026-05-09T12:00:00Z");
  const NOW_SEC = Math.floor(NOW.getTime() / 1000);

  it("returns 'never' for null / 0 / negative", () => {
    expect(formatRefreshedAgo(null, NOW)).toBe("never");
    expect(formatRefreshedAgo(0, NOW)).toBe("never");
    expect(formatRefreshedAgo(-5, NOW)).toBe("never");
  });

  it("returns 'just now' under five seconds", () => {
    expect(formatRefreshedAgo(NOW_SEC - 2, NOW)).toBe("just now");
  });

  it("expresses seconds, minutes, hours and days at the right thresholds", () => {
    expect(formatRefreshedAgo(NOW_SEC - 30, NOW)).toBe("30s ago");
    expect(formatRefreshedAgo(NOW_SEC - 300, NOW)).toBe("5m ago");
    expect(formatRefreshedAgo(NOW_SEC - 7200, NOW)).toBe("2h ago");
    expect(formatRefreshedAgo(NOW_SEC - 3 * 86_400, NOW)).toBe("3d ago");
  });

  it("accepts bigint inputs the IPC layer produces", () => {
    expect(formatRefreshedAgo(BigInt(NOW_SEC - 600), NOW)).toBe("10m ago");
  });
});

describe("buildSparklinePoints", () => {
  it("returns an empty string for an empty series", () => {
    expect(buildSparklinePoints([], 300, 40)).toBe("");
  });

  it("draws a baseline when every cost is zero", () => {
    const points = buildSparklinePoints(
      [
        { day: "2026-05-01", costUsd: 0 },
        { day: "2026-05-02", costUsd: 0 },
        { day: "2026-05-03", costUsd: 0 },
      ],
      300,
      40,
    );
    // Three points along the baseline (y=39).
    expect(points.split(" ")).toHaveLength(3);
    expect(points).toContain("39.00");
  });

  it("scales the peak to the inner top of the viewBox", () => {
    const points = buildSparklinePoints(
      [
        { day: "2026-05-01", costUsd: 0 },
        { day: "2026-05-02", costUsd: 10 },
      ],
      300,
      40,
    );
    // First point at the baseline; second at 1px from the top.
    const [a, b] = points.split(" ");
    expect(a).toBe("0.00,39.00");
    expect(b).toBe("300.00,1.00");
  });
});
