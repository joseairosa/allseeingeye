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
 * Parse a raw search box value into a structured filter + leftover
 * free-text. Unknown values for known prefixes (e.g. `type:bogus`) fall
 * through to free-text rather than producing an error - the user sees
 * an empty grid and self-corrects.
 */
export function parseSearchQuery(input: string): ParsedSearch {
  const filter: ComponentFilter = {
    toolId: null,
    kind: null,
    scope: null,
    query: null,
    tag: null,
    limit: null,
    offset: null,
  };
  const freeTokens: string[] = [];

  for (const raw of input.split(/\s+/)) {
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
      default:
        freeTokens.push(token);
    }
  }

  const freeText = freeTokens.join(" ");
  filter.query = freeText.length > 0 ? freeText : null;

  return { filter, freeText };
}
