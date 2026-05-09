/**
 * Phase 14B - shared helpers for the size / cost diagnostics surface.
 *
 * The numbers we render in the UI are intentionally rough. The walker
 * stores `component.size` in bytes; tokens are estimated using the
 * documented 4-chars-per-token heuristic (see docs/14-cost-and-memory.md
 * § 14B). We never ship `tiktoken` or any vendor tokenizer locally - the
 * binary cost is too high for what is a marketing number, not a
 * billing-accurate one. The tooltip surfaces the caveat.
 *
 * Helpers:
 *   formatBytes        - "12.4kB" / "987B" / "1.2MB"
 *   estimateTokens     - bytes / 4
 *   formatTokensK      - "3.1k" tokens, rounded to 1 decimal
 *   formatTokensExact  - "3,072 tokens" with thousands separators
 *   contextWindowPct   - share of a fixed 200k context budget
 *
 * Pure functions, no React. Safe to call from render paths.
 */

/**
 * Stated input-context window for the Sonnet/Opus 4.x family. The Cost
 * view footer and Quick Look both reference this constant so updating
 * it here propagates everywhere.
 *
 * Verified against the public Anthropic model documentation for the
 * Sonnet/Opus 4.x window. Bump when a new model lands with a different
 * stated window.
 */
export const MAX_CONTEXT_TOKENS = 200_000;

/**
 * Bytes-per-token heuristic. We document the caveat in the tooltip
 * because real tokenisation varies wildly for non-English content,
 * code, and base64 blobs.
 */
const BYTES_PER_TOKEN = 4;

/**
 * Coerce a `bigint | number` to a plain `number` so the formatter
 * helpers don't have to repeat the dance. Components consuming
 * `ts-rs`-generated bindings receive `bigint` for `i64`/`u64` columns.
 */
function toNumber(value: bigint | number): number {
  if (typeof value === "bigint") return Number(value);
  return value;
}

/**
 * Render a byte count as a short human label. Three buckets keep the
 * chip width predictable in the inventory grid.
 *
 * Examples:
 *   987   -> "987B"
 *   12345 -> "12.1kB"
 *   2_500_000 -> "2.4MB"
 */
export function formatBytes(bytes: bigint | number): string {
  const n = toNumber(bytes);
  if (!Number.isFinite(n) || n < 0) return "0B";
  if (n < 1024) return `${Math.round(n)}B`;
  if (n < 1024 * 1024) {
    const kb = n / 1024;
    return `${kb.toFixed(1)}kB`;
  }
  const mb = n / (1024 * 1024);
  return `${mb.toFixed(1)}MB`;
}

/**
 * Estimated token count. Rounded to integer so the value composes
 * nicely with both the kilo formatter (chip) and the exact formatter
 * (Quick Look). The heuristic is `bytes / 4`.
 */
export function estimateTokens(bytes: bigint | number): number {
  const n = toNumber(bytes);
  if (!Number.isFinite(n) || n < 0) return 0;
  return Math.round(n / BYTES_PER_TOKEN);
}

/**
 * Render a token count as a "k tokens" label rounded to one decimal.
 * Anything below 100 tokens collapses to "<0.1k" so the UI never
 * renders a bare "0.0k" that misleads users into thinking the file is
 * empty.
 *
 * Examples:
 *   24    -> "<0.1k"
 *   3072  -> "3.1k"
 *   50000 -> "50.0k"
 */
export function formatTokensK(tokens: number): string {
  if (!Number.isFinite(tokens) || tokens < 0) return "0k";
  if (tokens < 100) return "<0.1k";
  const k = Math.round(tokens / 100) / 10;
  return `${k.toFixed(1)}k`;
}

/**
 * Exact token count with thousands separators - used in the Quick Look
 * footer where horizontal space allows for the long form ("~3,072
 * tokens").
 */
export function formatTokensExact(tokens: number): string {
  if (!Number.isFinite(tokens) || tokens < 0) return "0";
  return tokens.toLocaleString("en-US");
}

/**
 * Share of a 200k context window expressed as a percentage with one
 * decimal. Returned as a number so the caller can compose its own
 * sentence; passing 0 returns 0 (not "0.0%").
 */
export function contextWindowPct(tokens: number): number {
  if (!Number.isFinite(tokens) || tokens <= 0) return 0;
  const pct = (tokens / MAX_CONTEXT_TOKENS) * 100;
  return Math.round(pct * 10) / 10;
}

/**
 * Threshold above which a memory component is treated as "bloated".
 * Mirrors the backend constant in `usage::bloat::OVERSIZED_BYTES` so
 * the Health view filter and the backend recommendation engine agree.
 *
 * 8192 bytes is roughly 2k tokens - the line above which a memory file
 * starts to be a real every-turn cost contributor.
 */
export const OVERSIZED_MEMORY_BYTES = 8192;
