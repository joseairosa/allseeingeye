/**
 * Cmd-K palette action registry (Phase 2.4).
 *
 * Actions are the second section of the command palette - non-component
 * commands like "Open Settings" or "Toggle theme". The registry itself is
 * built inside `CommandPalette.tsx` (it needs the Zustand store + IPC
 * functions), but the type and the fuzzy-filter helper live here so they
 * can be unit-tested without mounting React.
 *
 * Design choices:
 *   - Each action is a plain object so adding a new entry is a one-liner.
 *   - `keywords` lets us match against synonyms ("dark mode" → toggle theme)
 *     without polluting the visible label.
 *   - The filter is a forgiving subsequence match (every query char must
 *     appear in order, anywhere in label/keywords). It is NOT a fuzzy
 *     scorer with ranking - the registry is small (<20 items) and the
 *     palette already shows them in author-defined order.
 */

export interface PaletteAction {
  /** Stable id, used as React key and for test identity assertions. */
  id: string;
  /** Visible label rendered in the palette row. */
  label: string;
  /** Optional synonyms / search aliases. Not rendered. */
  keywords?: readonly string[];
  /**
   * Side effect to run when the action is fired. May return a promise; the
   * palette runs it fire-and-forget so a slow `start_full_scan` doesn't
   * block keyboard handling.
   */
  run: () => void | Promise<void>;
}

/**
 * Forgiving subsequence match: returns `true` if every character of
 * `query` (lowercased) appears in order somewhere in the haystack.
 *
 * Examples:
 *   matches("opn st", "Open Settings")  → true
 *   matches("toggle theme", "Toggle theme") → true
 *   matches("dark", action with keyword "dark mode") → true
 */
function subsequenceMatch(haystack: string, query: string): boolean {
  if (query.length === 0) return true;
  let i = 0;
  for (const ch of haystack) {
    if (ch === query[i]) {
      i += 1;
      if (i === query.length) return true;
    }
  }
  return i === query.length;
}

/**
 * Filter the action list against a free-form query.
 *
 * - Empty / whitespace-only query → returns the whole list unchanged.
 * - Non-empty → keeps actions whose label OR any keyword contains the
 *   query as a case-insensitive subsequence.
 *
 * Order is preserved (no relevance ranking) so the registry's authoring
 * order is what the user sees.
 */
export function filterPaletteActions(
  actions: readonly PaletteAction[],
  query: string,
): PaletteAction[] {
  const trimmed = query.trim().toLowerCase();
  if (trimmed.length === 0) return [...actions];

  return actions.filter((action) => {
    const label = action.label.toLowerCase();
    if (subsequenceMatch(label, trimmed)) return true;
    const keywords = action.keywords ?? [];
    return keywords.some((kw) => subsequenceMatch(kw.toLowerCase(), trimmed));
  });
}
