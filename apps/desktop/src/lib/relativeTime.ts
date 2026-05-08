/**
 * Render a UNIX timestamp (seconds, the wire format used by the index)
 * as a short human label suitable for an Inventory row.
 *
 * Buckets:
 *   - within 24h           "today"
 *   - 1..30 days           "Nd ago"
 *   - 30+ days             "Nmo ago"
 *   - null / undefined     "-"
 */

const SECONDS_PER_DAY = 86_400;
const DAYS_PER_MONTH = 30;

/**
 * Convert a possibly-bigint UNIX timestamp (seconds) to a relative
 * label. `bigint` because `ts-rs` emits `i64` as `bigint` in the
 * generated TS bindings; we accept both for resilience.
 */
export function formatRelativeTime(
  timestampSec: bigint | number | null,
  now: Date = new Date(),
): string {
  if (timestampSec === null) return "-";

  const value = typeof timestampSec === "bigint" ? Number(timestampSec) : timestampSec;
  if (!Number.isFinite(value) || value <= 0) return "-";

  const nowSec = Math.floor(now.getTime() / 1000);
  const ageSec = nowSec - value;

  if (ageSec < 0) return "today"; // future timestamp - clamp.
  if (ageSec < SECONDS_PER_DAY) return "today";

  const days = Math.floor(ageSec / SECONDS_PER_DAY);
  if (days < DAYS_PER_MONTH) return `${days}d ago`;

  const months = Math.floor(days / DAYS_PER_MONTH);
  return `${months}mo ago`;
}
