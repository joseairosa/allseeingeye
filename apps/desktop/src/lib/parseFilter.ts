/**
 * Search-bar query parser.
 *
 * The Inventory search box accepts a small expression language documented
 * in `docs/06-ux-design.md`:
 *
 *   type:skill tool:claude-code scope:user tag:pinned   the cat sat
 *
 * Recognised prefixes split off into a typed `ComponentFilter`; everything
 * else is collapsed into a free-text run that goes into `filter.query`
 * (substring match server-side via `LIKE`). The dedicated `search`
 * command is FTS-backed - this parser is purely for the live filter
 * grid in Inventory.
 */
import type {
  ComponentFilter,
  ComponentType,
  Scope,
  ToolId,
} from "@aseye/shared-types";

const TOOL_IDS: readonly ToolId[] = [
  "claude-code",
  "codex",
  "cursor",
  "antigravity",
] as const;

const COMPONENT_TYPES: readonly ComponentType[] = [
  "tool",
  "settings",
  "memory",
  "rule",
  "skill",
  "command",
  "agent",
  "mcp",
  "hook",
  "plugin",
  "marketplace",
  "session",
  "task",
  "outputStyle",
  "statusline",
  "permission",
] as const;

const SCOPES: readonly Scope[] = ["user", "project", "enterprise", "plugin"] as const;

function isToolId(value: string): value is ToolId {
  return (TOOL_IDS as readonly string[]).includes(value);
}

function isComponentType(value: string): value is ComponentType {
  return (COMPONENT_TYPES as readonly string[]).includes(value);
}

function isScope(value: string): value is Scope {
  return (SCOPES as readonly string[]).includes(value);
}

export interface ParsedSearch {
  filter: ComponentFilter;
  /** The free-text remainder, for highlighting and aria-live announcements. */
  freeText: string;
}

/**
 * Audit issue #8: convert a `last:` value like `7d`, `48h`, `2w` into
 * a unix-seconds cutoff. Returns `null` for malformed values so the
 * caller can drop the token to free-text rather than guess.
 *
 * Accepted units: `s` seconds, `m` minutes, `h` hours, `d` days,
 * `w` weeks. The integer must be positive. `last:0d` falls through to
 * free-text because "modified after now" is never what the user meant.
 *
 * The function takes `nowSec` so tests can pin a deterministic clock.
 */
export function parseLastValue(value: string, nowSec: number): number | null {
  const match = /^(\d+)(s|m|h|d|w)$/i.exec(value.trim());
  if (!match) return null;
  const nRaw = match[1];
  const unit = match[2];
  if (!nRaw || !unit) return null;
  const n = Number.parseInt(nRaw, 10);
  if (!Number.isFinite(n) || n <= 0) return null;
  const SECONDS_BY_UNIT: Record<string, number> = {
    s: 1,
    m: 60,
    h: 3600,
    d: 86_400,
    w: 7 * 86_400,
  };
  const factor = SECONDS_BY_UNIT[unit.toLowerCase()];
  if (factor === undefined) return null;
  return nowSec - n * factor;
}

/**
 * Parse a raw search box value into a structured filter + leftover
 * free-text. Unknown values for known prefixes (e.g. `type:bogus`) fall
 * through to free-text rather than producing an error - the user sees
 * an empty grid and self-corrects.
 *
 * Whitespace tolerance: `tool: claude-code` (space after colon) is
 * folded into `tool:claude-code` before tokenising, so the chip
 * round-trips even when the user types loose syntax. Multiple spaces
 * are collapsed.
 */
export function parseSearchQuery(
  input: string,
  nowSec: number = Math.floor(Date.now() / 1000),
): ParsedSearch {
  const filter: ComponentFilter = {
    toolId: null,
    kind: null,
    scope: null,
    query: null,
    tag: null,
    limit: null,
    offset: null,
    modifiedAfterUnix: null,
  };
  const freeTokens: string[] = [];

  // Collapse `prefix: value` -> `prefix:value` so a stray space after
  // the colon does not orphan the prefix from its value. Only the
  // immediate next whitespace run is consumed.
  const normalised = input.replace(/([A-Za-z]+):\s+(\S)/g, "$1:$2");

  for (const raw of normalised.split(/\s+/)) {
    const token = raw.trim();
    if (!token) continue;

    const colon = token.indexOf(":");
    if (colon <= 0 || colon === token.length - 1) {
      freeTokens.push(token);
      continue;
    }

    const prefix = token.slice(0, colon).toLowerCase();
    const value = token.slice(colon + 1);

    switch (prefix) {
      case "tool":
        if (isToolId(value)) {
          filter.toolId = value;
        } else {
          freeTokens.push(token);
        }
        break;
      case "type":
        if (isComponentType(value)) {
          filter.kind = value;
        } else {
          freeTokens.push(token);
        }
        break;
      case "scope":
        if (isScope(value)) {
          filter.scope = value;
        } else {
          freeTokens.push(token);
        }
        break;
      case "tag":
        filter.tag = value;
        break;
      case "last": {
        // Audit issue #8: recognise `last:7d`, `last:48h`, `last:2w` etc.
        // Convert to a unix-second cutoff that the backend SQL applies
        // as `c.mtime >= cutoff`. Malformed values (e.g. `last:7days`,
        // `last:0d`) fall through to free-text so the user sees no
        // results and self-corrects rather than getting a silently
        // misapplied filter.
        // ts-rs emits Rust's `i64` as `bigint` in the TS binding so
        // the filter field is `bigint | null`; we coerce here to keep
        // the helper API simple.
        const cutoff = parseLastValue(value, nowSec);
        if (cutoff === null) {
          freeTokens.push(token);
        } else {
          filter.modifiedAfterUnix = BigInt(cutoff);
        }
        break;
      }
      default:
        freeTokens.push(token);
    }
  }

  const freeText = freeTokens.join(" ");
  filter.query = freeText.length > 0 ? freeText : null;

  return { filter, freeText };
}
