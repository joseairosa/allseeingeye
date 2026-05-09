/**
 * Pure formatting helpers for the Cost view.
 *
 * Centralised so the layout components stay declarative AND so the unit
 * suite can verify the user-facing strings without rendering React.
 *
 * Localisation note: every formatter pins `Intl.NumberFormat` to
 * `en-US`. The desktop app ships English-only in MVP; an explicit
 * locale keeps snapshots stable across host machines that may have
 * exotic defaults (e.g. `de-DE` swapping the decimal separator).
 */

import type { TokenTotals } from "@aseye/shared-types";

const USD_FORMATTER = new Intl.NumberFormat("en-US", {
  style: "currency",
  currency: "USD",
  minimumFractionDigits: 2,
  maximumFractionDigits: 2,
});

const COMPACT_FORMATTER = new Intl.NumberFormat("en-US", {
  notation: "compact",
  maximumFractionDigits: 1,
});

const COMPACT_INT_THRESHOLD = 10_000;

/**
 * Format a USD amount as a currency string. `null`/non-finite values
 * collapse to `-` so the UI never shows `$NaN` if a hook errors before
 * the cache is hot.
 */
export function formatUsd(amount: number | null | undefined): string {
  if (amount === null || amount === undefined) return "-";
  if (!Number.isFinite(amount)) return "-";
  return USD_FORMATTER.format(amount);
}

/**
 * Format a token count. Small numbers stay literal (`9,876`) so users
 * can read them precisely; numbers above 10k switch to compact notation
 * (`1.2M`) because the strip is for at-a-glance KPI scanning, not
 * audit-grade lookup.
 */
export function formatTokenCount(value: bigint | number): string {
  const n = typeof value === "bigint" ? Number(value) : value;
  if (!Number.isFinite(n) || n < 0) return "-";
  if (n < COMPACT_INT_THRESHOLD) {
    return new Intl.NumberFormat("en-US").format(Math.trunc(n));
  }
  return COMPACT_FORMATTER.format(n);
}

/**
 * Sum the four token buckets. We prefer `bigint` on the wire because
 * counts can exceed `Number.MAX_SAFE_INTEGER` for power users; we widen
 * to `bigint` arithmetic before coercing to `number` for display.
 */
export function totalTokens(totals: TokenTotals): number {
  const sum =
    BigInt(totals.input) +
    BigInt(totals.output) +
    BigInt(totals.cacheRead) +
    BigInt(totals.cacheCreate);
  return Number(sum);
}

/**
 * Take the last two segments of a project path so the bar chart label
 * stays readable. `/Users/jose/Development/projectfinish` becomes
 * `Development/projectfinish`. Edge cases:
 *
 *   - Trailing slashes are trimmed.
 *   - Single-segment inputs (e.g. `repo`) return as-is.
 *   - Empty input collapses to `(unknown)` so the row is never blank.
 */
export function shortenProjectPath(project: string): string {
  if (!project) return "(unknown)";
  const trimmed = project.replace(/\/+$/u, "");
  const parts = trimmed.split("/").filter((p) => p.length > 0);
  if (parts.length === 0) return "(unknown)";
  if (parts.length <= 2) return parts.join("/");
  // We picked the last two for clarity over the leaf alone - a
  // pure leaf can collide ("api" lives in many repos) while two
  // segments disambiguate without flooding the row.
  return parts.slice(-2).join("/");
}

/**
 * Render a `refreshed_at` epoch (unix seconds) as "Xm ago" / "Xs ago".
 * A bespoke helper rather than reusing `formatRelativeTime` because
 * the Cost view wants minute-level resolution; "today" is too coarse
 * when the user just clicked refresh.
 */
export function formatRefreshedAgo(
  refreshedAt: bigint | number | null | undefined,
  now: Date = new Date(),
): string {
  if (refreshedAt === null || refreshedAt === undefined) return "never";
  const value =
    typeof refreshedAt === "bigint" ? Number(refreshedAt) : refreshedAt;
  if (!Number.isFinite(value) || value <= 0) return "never";
  const ageSec = Math.max(0, Math.floor(now.getTime() / 1000) - value);
  if (ageSec < 5) return "just now";
  if (ageSec < 60) return `${ageSec}s ago`;
  const mins = Math.floor(ageSec / 60);
  if (mins < 60) return `${mins}m ago`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.floor(hours / 24);
  return `${days}d ago`;
}

/**
 * Build a normalised SVG `polyline points` string for the per-day
 * sparkline. The viewBox is always 300×40 (declared by the consumer);
 * we map `costUsd` onto y so the visual encoding matches the headline
 * KPI. A flat zero series is rendered as a baseline.
 */
export function buildSparklinePoints(
  rows: ReadonlyArray<{ day: string; costUsd: number }>,
  width: number,
  height: number,
): string {
  if (rows.length === 0) return "";
  const max = rows.reduce((acc, r) => Math.max(acc, r.costUsd), 0);
  if (max <= 0) {
    // Degenerate data - draw a baseline so the SVG has visible content
    // instead of an empty `<polyline>` that some screen readers skip.
    return rows
      .map((_, i) => {
        const x = (i / Math.max(1, rows.length - 1)) * width;
        return `${x.toFixed(2)},${(height - 1).toFixed(2)}`;
      })
      .join(" ");
  }
  return rows
    .map((row, i) => {
      const x = (i / Math.max(1, rows.length - 1)) * width;
      // Reserve a 1px top margin so the peak bar isn't clipped by
      // the SVG bounds.
      const y = height - 1 - (row.costUsd / max) * (height - 2);
      return `${x.toFixed(2)},${y.toFixed(2)}`;
    })
    .join(" ");
}
