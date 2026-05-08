import { useMemo } from "react";
import { useUi } from "@/store/ui";
import { useComponents } from "@/ipc/hooks";
import { parseSearchQuery } from "@/lib/parseFilter";
import { formatRelativeTime } from "@/lib/relativeTime";
import { FiltersIcon, SearchIcon, TypeIcon, type TypeIconId } from "@/components/icons";
import type {
  ComponentSummary,
  ComponentType,
  Scope,
  ToolId,
} from "@aseye/shared-types";

const TYPE_TO_ICON: Record<string, TypeIconId> = {
  skill: "icon-skill",
  agent: "icon-agent",
  command: "icon-command",
  mcp: "icon-mcp",
  rule: "icon-rule",
  memory: "icon-memory",
  hook: "icon-hook",
};

const TOOL_DISPLAY_NAME: Record<ToolId, string> = {
  "claude-code": "Claude Code",
  codex: "Codex",
  cursor: "Cursor",
  antigravity: "Antigravity",
};

/**
 * Display label for a `ComponentSummary`. Prefers the optional
 * `displayName` (set by parsers that surface a slash-command label or
 * similar) and falls back to `name`.
 */
function displayLabel(row: ComponentSummary): string {
  return row.displayName?.trim() || row.name;
}

const FILTER_CHIPS = [
  { id: "tool:claude-code", label: "Claude Code" },
  { id: "type:skill", label: "Skill" },
  { id: "scope:user", label: "User" },
  { id: "last:7d", label: "Recently used" },
  { id: "has:relations", label: "Has relations" },
] as const;

interface RowProps {
  row: ComponentSummary;
}

function rowFlags(row: ComponentSummary, selected: boolean): string {
  const parts: string[] = [];
  if (selected) parts.push("selected");
  if (row.hasParseErrors) parts.push("issue");
  return parts.join(" ");
}

function Row({ row }: RowProps) {
  const selectedId = useUi((s) => s.selectedComponentId);
  const selectComponent = useUi((s) => s.selectComponent);
  const setView = useUi((s) => s.setView);

  const selected = selectedId === row.id;
  const className = `component-row ${rowFlags(row, selected)}`.trim();
  const iconId: TypeIconId = TYPE_TO_ICON[row.kind] ?? "icon-skill";

  return (
    <button
      type="button"
      className={className}
      role="row"
      aria-selected={selected}
      onClick={() => selectComponent(row.id)}
      onDoubleClick={() => setView("editor")}
    >
      <span role="cell" className="type-cell">
        <TypeIcon id={iconId} />
        <strong>{row.kind}</strong>
      </span>
      <span role="cell" className="name-cell">
        <span>{displayLabel(row)}</span>
        <small>{row.description ?? row.path}</small>
      </span>
      <span role="cell">{TOOL_DISPLAY_NAME[row.tool]}</span>
      <span role="cell">{row.scope}</span>
      <span role="cell">
        <span className={`health-pill ${row.hasParseErrors ? "warn" : "unprobed"}`}>
          {row.hasParseErrors ? "parse error" : "unprobed"}
        </span>
      </span>
      <span role="cell">{formatRelativeTime(row.lastUsedAt)}</span>
    </button>
  );
}

function SkeletonRows({ count }: { count: number }) {
  // Render an explicit skeleton row so the grid keeps its height while
  // the first IPC round-trip is in flight. The visual is a CSS-only
  // shimmer driven by `.component-row.skeleton` (defined in design-system.css).
  const items = Array.from({ length: count }, (_, idx) => idx);
  return (
    <>
      {items.map((idx) => (
        <div
          key={`skeleton-${idx}`}
          className="component-row skeleton"
          role="row"
          aria-hidden="true"
        >
          <span role="cell" className="type-cell">
            <span className="skeleton-block" />
          </span>
          <span role="cell" className="name-cell">
            <span className="skeleton-block" />
          </span>
          <span role="cell">
            <span className="skeleton-block" />
          </span>
          <span role="cell">
            <span className="skeleton-block" />
          </span>
          <span role="cell">
            <span className="skeleton-block" />
          </span>
          <span role="cell">
            <span className="skeleton-block" />
          </span>
        </div>
      ))}
    </>
  );
}

interface ChipState {
  /** True when this chip's filter prefix is currently active in the search. */
  active: boolean;
}

function chipStates(
  filterToolId: ToolId | null,
  filterKind: ComponentType | null,
  filterScope: Scope | null,
): Record<string, ChipState> {
  return {
    "tool:claude-code": { active: filterToolId === "claude-code" },
    "type:skill": { active: filterKind === "skill" },
    "scope:user": { active: filterScope === "user" },
    "last:7d": { active: false },
    "has:relations": { active: false },
  };
}

export function InventoryView() {
  const search = useUi((s) => s.search);
  const setSearch = useUi((s) => s.setSearch);
  const view = useUi((s) => s.view);

  const parsed = useMemo(() => parseSearchQuery(search), [search]);
  const { data, isPending, isError } = useComponents(parsed.filter);

  const isActive = view === "inventory";
  const chips = chipStates(
    parsed.filter.toolId,
    parsed.filter.kind,
    parsed.filter.scope,
  );

  return (
    <section
      className={`view${isActive ? " active" : ""}`}
      data-view-panel="inventory"
      aria-labelledby="inventory-heading"
      hidden={!isActive}
    >
      <h2 className="sr-only" id="inventory-heading">Component inventory</h2>

      <div className="inventory-toolbar">
        <label className="search-field">
          <SearchIcon />
          <input
            type="search"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            aria-label="search components"
            placeholder="search..."
          />
        </label>
        <button type="button" className="text-button">
          <FiltersIcon />
          filters
        </button>
      </div>

      <div className="filter-strip" aria-label="active filters">
        {FILTER_CHIPS.map((chip) => (
          <button
            key={chip.id}
            type="button"
            className={`chip${chips[chip.id]?.active ? " selected" : ""}`}
          >
            {chip.label}
          </button>
        ))}
        <button type="button" className="chip ghost">+ tag</button>
      </div>

      <div className="inventory-grid" role="table" aria-label="components">
        <div className="grid-head" role="row">
          <span role="columnheader">type</span>
          <span role="columnheader">name</span>
          <span role="columnheader">tool</span>
          <span role="columnheader">scope</span>
          <span role="columnheader">state</span>
          <span role="columnheader">used</span>
        </div>

        {isPending ? <SkeletonRows count={6} /> : null}

        {isError ? (
          <div className="component-row" role="row" aria-live="polite">
            <span role="cell" className="name-cell">
              <span>could not load components</span>
              <small>check the index process and retry</small>
            </span>
          </div>
        ) : null}

        {!isPending && !isError && data && data.length === 0 ? (
          <div className="component-row" role="row" aria-live="polite">
            <span role="cell" className="name-cell">
              <span>no matches</span>
              <small>clear filters to see every component</small>
            </span>
          </div>
        ) : null}

        {!isPending && !isError && data
          ? data.map((row) => <Row key={row.id} row={row} />)
          : null}
      </div>
    </section>
  );
}
