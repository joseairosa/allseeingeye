import { memo, useMemo, useRef } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { useUi } from "@/store/ui";
import { useComponentFindingsCounts, useComponents } from "@/ipc/hooks";
import { parseSearchQuery } from "@/lib/parseFilter";
import { toggleFilterPrefix } from "@/lib/filterChip";
import { formatRelativeTime } from "@/lib/relativeTime";
import {
  FiltersIcon,
  SearchIcon,
  ShieldIcon,
  TypeIcon,
  type TypeIconId,
} from "@/components/icons";
import type {
  ComponentFindingsCount,
  ComponentSummary,
  ComponentType,
  Scope,
  SeverityCounts,
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

/** Per-density row heights. Match `--row-height` from design-system.css. */
const ROW_HEIGHT_COMFORTABLE = 56;
const ROW_HEIGHT_COMPACT = 40;

/** Virtualizer overscan: render this many extra rows above/below viewport. */
const OVERSCAN = 6;

/** Skeleton row count rendered above the virtualizer while pending. */
const SKELETON_COUNT = 6;

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
  selected: boolean;
  /** Inline transform produced by the virtualizer for absolute positioning. */
  transform: string;
  /**
   * Per-component finding totals. Drives the inline shield badge -
   * `null` when no findings exist, so the badge stays hidden rather
   * than rendering an empty shell.
   */
  findings: ComponentFindingsCount | null;
  onSelect: (id: string) => void;
  onOpenEditor: () => void;
}

/**
 * Pick the highest severity present in a per-component finding count
 * so the badge picks its colour from the worst offender.
 */
function highestSeverity(counts: SeverityCounts): "critical" | "high" | "medium" | "low" {
  if (counts.critical > 0) return "critical";
  if (counts.high > 0) return "high";
  if (counts.medium > 0) return "medium";
  return "low";
}

function rowFlags(row: ComponentSummary, selected: boolean): string {
  const parts: string[] = [];
  if (selected) parts.push("selected");
  if (row.hasParseErrors) parts.push("issue");
  return parts.join(" ");
}

/**
 * Inventory row. `React.memo` keeps stable rows from re-rendering when
 * the parent re-renders (selection change moves the `selected` flag on
 * exactly one old + one new row, leaving the rest untouched).
 */
const Row = memo(function Row({
  row,
  selected,
  transform,
  findings,
  onSelect,
  onOpenEditor,
}: RowProps) {
  const className = `component-row virtual ${rowFlags(row, selected)}`.trim();
  const iconId: TypeIconId = TYPE_TO_ICON[row.kind] ?? "icon-skill";
  const findingsTotal = findings?.total ?? 0;
  const severity =
    findingsTotal > 0 && findings ? highestSeverity(findings.bySeverity) : null;

  return (
    <button
      type="button"
      className={className}
      role="row"
      aria-selected={selected}
      onClick={() => onSelect(row.id)}
      onDoubleClick={onOpenEditor}
      style={{ transform }}
    >
      <span role="cell" className="type-cell">
        <TypeIcon id={iconId} />
        <strong>{row.kind}</strong>
      </span>
      <span role="cell" className="name-cell">
        <span className="name-cell-title">
          <span>{displayLabel(row)}</span>
          {severity ? (
            <span
              className={`shield-badge ${severity}`}
              aria-label={`${findingsTotal} security findings`}
              title={`${findingsTotal} ${
                findingsTotal === 1 ? "finding" : "findings"
              }`}
            >
              <ShieldIcon />
            </span>
          ) : null}
        </span>
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
});

function SkeletonRows({ count }: { count: number }) {
  // Render an explicit skeleton row so the grid keeps its height while
  // the first IPC round-trip is in flight. The visual is a CSS-only
  // shimmer driven by `.component-row.skeleton` (defined in design-system.css).
  // Skeletons are NOT virtualised - a fixed batch sits at the top of the
  // grid until the first payload lands.
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
    // `last:7d` and `has:relations` are not yet plumbed through to the
    // backend filter, so chip activity reflects raw search-string presence.
    "last:7d": { active: false },
    "has:relations": { active: false },
  };
}

interface VirtualBodyProps {
  rows: ComponentSummary[];
  rowHeight: number;
  selectedId: string | null;
  /** Per-component finding totals indexed by component id. */
  findingsByComponent: ReadonlyMap<string, ComponentFindingsCount>;
  onSelect: (id: string) => void;
  onOpenEditor: () => void;
}

/**
 * Virtualised body of the inventory grid.
 *
 * The hook is split out so `useVirtualizer` only mounts when we actually
 * have data - avoids spinning up a virtualizer while skeletons render.
 * The scroll container (`.inventory-grid-virtual`) is the
 * `getScrollElement`; the inner `<div>` with explicit `height` is the
 * total content surface; rows position absolutely via inline transform.
 */
function VirtualBody({
  rows,
  rowHeight,
  selectedId,
  findingsByComponent,
  onSelect,
  onOpenEditor,
}: VirtualBodyProps) {
  const scrollRef = useRef<HTMLDivElement>(null);

  const virtualizer = useVirtualizer<HTMLDivElement, HTMLButtonElement>({
    count: rows.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => rowHeight,
    overscan: OVERSCAN,
  });

  const totalSize = virtualizer.getTotalSize();
  const items = virtualizer.getVirtualItems();

  return (
    <div
      ref={scrollRef}
      className="inventory-grid-virtual"
      role="rowgroup"
    >
      <div style={{ height: `${totalSize}px`, position: "relative", width: "100%" }}>
        {items.map((vItem) => {
          const row = rows[vItem.index];
          if (!row) return null;
          return (
            <Row
              key={row.id}
              row={row}
              selected={row.id === selectedId}
              transform={`translateY(${vItem.start}px)`}
              findings={findingsByComponent.get(row.id) ?? null}
              onSelect={onSelect}
              onOpenEditor={onOpenEditor}
            />
          );
        })}
      </div>
    </div>
  );
}

export function InventoryView() {
  const search = useUi((s) => s.search);
  const setSearch = useUi((s) => s.setSearch);
  const view = useUi((s) => s.view);
  const density = useUi((s) => s.density);
  const selectedId = useUi((s) => s.selectedComponentId);
  const selectComponent = useUi((s) => s.selectComponent);
  const setView = useUi((s) => s.setView);

  const parsed = useMemo(() => parseSearchQuery(search), [search]);
  const { data, isPending, isError } = useComponents(parsed.filter);
  const { data: findingsCounts } = useComponentFindingsCounts();

  // Index findings by component id once per fetch so the virtualised
  // row render is O(1) per row rather than O(n) per row across N
  // findings entries.
  const findingsByComponent = useMemo(() => {
    const map = new Map<string, ComponentFindingsCount>();
    for (const entry of findingsCounts ?? []) {
      map.set(entry.componentId, entry);
    }
    return map;
  }, [findingsCounts]);

  const isActive = view === "inventory";
  const chips = chipStates(
    parsed.filter.toolId,
    parsed.filter.kind,
    parsed.filter.scope,
  );

  const rowHeight =
    density === "compact" ? ROW_HEIGHT_COMPACT : ROW_HEIGHT_COMFORTABLE;

  const rows = data ?? [];
  const showEmpty = !isPending && !isError && rows.length === 0;
  const showVirtualBody = !isPending && !isError && rows.length > 0;

  const handleOpenEditor = (): void => setView("editor");

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
        {FILTER_CHIPS.map((chip) => {
          const active = chips[chip.id]?.active ?? false;
          return (
            <button
              key={chip.id}
              type="button"
              aria-pressed={active}
              className={`chip${active ? " selected" : ""}`}
              onClick={() => setSearch(toggleFilterPrefix(search, chip.id))}
            >
              {chip.label}
            </button>
          );
        })}
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

        {isPending ? <SkeletonRows count={SKELETON_COUNT} /> : null}

        {isError ? (
          <div className="component-row" role="row" aria-live="polite">
            <span role="cell" className="name-cell">
              <span>could not load components</span>
              <small>check the index process and retry</small>
            </span>
          </div>
        ) : null}

        {showEmpty ? (
          <div className="component-row" role="row" aria-live="polite">
            <span role="cell" className="name-cell">
              <span>no matches</span>
              <small>clear filters to see every component</small>
            </span>
          </div>
        ) : null}

        {showVirtualBody ? (
          <VirtualBody
            rows={rows}
            rowHeight={rowHeight}
            selectedId={selectedId}
            findingsByComponent={findingsByComponent}
            onSelect={selectComponent}
            onOpenEditor={handleOpenEditor}
          />
        ) : null}
      </div>
    </section>
  );
}
