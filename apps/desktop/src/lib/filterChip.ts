/**
 * Filter chip prefix toggling.
 *
 * The Inventory filter strip renders chips like `tool:claude-code`,
 * `type:skill`, `scope:user`, `last:7d`, `has:relations`. Clicking a chip
 * is the inverse of typing the prefix in the search box: if the prefix
 * is already in the search string, remove it; otherwise append it.
 *
 * Toggling preserves any free-text or unrelated prefixes the user has
 * typed - we only touch the exact token whose value matches.
 */

/**
 * Toggle a filter prefix (e.g. `tool:claude-code`) in a search string.
 *
 * Idempotent across two clicks: `toggleFilterPrefix(s, p)` followed by
 * `toggleFilterPrefix(toggleFilterPrefix(s, p), p)` returns the input
 * (modulo whitespace normalisation around the touched token).
 *
 * Whitespace normalisation: tokens are split on any run of whitespace,
 * filtered, and rejoined with a single space. Leading/trailing
 * whitespace is trimmed.
 */
export function toggleFilterPrefix(search: string, prefix: string): string {
  const normalisedPrefix = prefix.trim();
  if (normalisedPrefix.length === 0) return search.trim();

  // Tokenise on whitespace runs. Empty tokens drop out via `filter`.
  const tokens = search.split(/\s+/).filter((token) => token.length > 0);

  // Case-insensitive comparison so `Tool:Claude-Code` round-trips with
  // `tool:claude-code`. The canonical prefix wins on append.
  const target = normalisedPrefix.toLowerCase();
  const without = tokens.filter((token) => token.toLowerCase() !== target);

  if (without.length !== tokens.length) {
    // Prefix was present at least once. Remove all occurrences so a
    // double-typed prefix collapses to none on first toggle.
    return without.join(" ");
  }

  // Prefix absent: append.
  return [...tokens, normalisedPrefix].join(" ");
}
