import { memo, useEffect, useMemo, useRef, useState } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { useUi } from "@/store/ui";
import { useComponentFindingsCounts, useComponents } from "@/ipc/hooks";
import { parseSearchQuery } from "@/lib/parseFilter";
import { toggleFilterPrefix } from "@/lib/filterChip";
import { formatRelativeTime } from "@/lib/relativeTime";
import {
  estimateTokens,
  formatBytes,
  formatTokensK,
} from "@/lib/tokens";
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
  { id: "last:7d", label: "Recently modified" },
] as const;

/**
 * Toggle groups exposed by the filters popover (audit issue #6). The
 * popover writes search-string tokens through `toggleFilterPrefix`, the
 * same path the inline chips use - so popover and chip state stay in
 * sync without a parallel store.
 */
const POPOVER_TOOLS: ReadonlyArray<{ id: ToolId; label: string }> = [
  { id: "claude-code", label: "Claude Code" },
  { id: "codex", label: "Codex" },
  { id: "cursor", label: "Cursor" },
  { id: "antigravity", label: "Antigravity" },
];
const POPOVER_TYPES: ReadonlyArray<{ id: ComponentType; label: string }> = [
  { id: "skill", label: "Skill" },
  { id: "agent", label: "Agent" },
  { id: "command", label: "Command" },
  { id: "mcp", label: "MCP" },
  { id: "rule", label: "Rule" },
  { id: "memory", label: "Memory" },
  { id: "hook", label: "Hook" },
];
const POPOVER_SCOPES: ReadonlyArray<{ id: Scope; label: string }> = [
  { id: "user", label: "User" },
  { id: "project", label: "Project" },
  { id: "plugin", label: "Plugin" },
];

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

  // Phase 14B - size / cost chip rendered only for memory components.
  // The walker (14A) populates `size` for every component but token
  // cost only matters for the per-turn memory preamble; surfacing the
  // chip on every row would add noise without informing decisions.
  const showSizeChip = row.kind === "memory";
  const sizeChipLabel = showSizeChip
    ? `${formatBytes(row.size)} · ~${formatTokensK(estimateTokens(row.size))} tok`
    : null;

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
          {sizeChipLabel ? (
            <span
              className="size-chip"
              title="Approximate, based on ~4 chars/token. Real cost varies by tokenizer and content."
              aria-label={`size ${sizeChipLabel}`}
            >
              {sizeChipLabel}
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
  modifiedAfterUnix: bigint | null,
): Record<string, ChipState> {
  return {
    "tool:claude-code": { active: filterToolId === "claude-code" },
    "type:skill": { active: filterKind === "skill" },
    "scope:user": { active: filterScope === "user" },
    // `last:7d` is active whenever the parser produced a non-null
    // cutoff. We do not check the exact value: the chip toggles a
    // 7-day cutoff but the user can also type `last:14d` and the chip
    // still reads as "you have a date filter on" which is the right
    // signal.
    "last:7d": { active: modifiedAfterUnix !== null },
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

interface FilterPopoverProps {
  search: string;
  filterToolId: ToolId | null;
  filterKind: ComponentType | null;
  filterScope: Scope | null;
  anchorRef: React.RefObject<HTMLButtonElement | null>;
  onChange: (next: string) => void;
  onClose: () => void;
}

/**
 * Audit issue #6: the filters button used to be dead. It now toggles a
 * popover with grouped chips for tool / type / scope. Each chip pipes
 * through `toggleFilterPrefix` so popover state mirrors the inline
 * chips and the search bar without a separate store.
 *
 * Outside-click handling: a `pointerdown` on the document is the trip
 * wire. We ignore clicks that originate inside the popover body OR on
 * the anchor button (so the parent's onClick can re-toggle without a
 * close-then-open race).
 */
function FilterPopover({
  search,
  filterToolId,
  filterKind,
  filterScope,
  anchorRef,
  onChange,
  onClose,
}: FilterPopoverProps): React.ReactElement {
  const popoverRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    function onPointerDown(event: PointerEvent): void {
      const target = event.target;
      if (!(target instanceof Node)) return;
      if (popoverRef.current?.contains(target)) return;
      if (anchorRef.current?.contains(target)) return;
      onClose();
    }
    function onKey(event: KeyboardEvent): void {
      if (event.key === "Escape") onClose();
    }
    document.addEventListener("pointerdown", onPointerDown);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown);
      document.removeEventListener("keydown", onKey);
    };
  }, [anchorRef, onClose]);

  function toggle(prefix: string): void {
    onChange(toggleFilterPrefix(search, prefix));
  }

  return (
    <div
      ref={popoverRef}
      className="filter-popover"
      role="dialog"
      aria-label="filters"
    >
      <div className="filter-popover-group">
        <h4>Tool</h4>
        <div className="filter-popover-chips">
          {POPOVER_TOOLS.map((t) => {
            const active = filterToolId === t.id;
            return (
              <button
                key={t.id}
                type="button"
                className={`chip${active ? " selected" : ""}`}
                aria-pressed={active}
                onClick={() => toggle(`tool:${t.id}`)}
              >
                {t.label}
              </button>
            );
          })}
        </div>
      </div>
      <div className="filter-popover-group">
        <h4>Type</h4>
        <div className="filter-popover-chips">
          {POPOVER_TYPES.map((t) => {
            const active = filterKind === t.id;
            return (
              <button
                key={t.id}
                type="button"
                className={`chip${active ? " selected" : ""}`}
                aria-pressed={active}
                onClick={() => toggle(`type:${t.id}`)}
              >
                {t.label}
              </button>
            );
          })}
        </div>
      </div>
      <div className="filter-popover-group">
        <h4>Scope</h4>
        <div className="filter-popover-chips">
          {POPOVER_SCOPES.map((t) => {
            const active = filterScope === t.id;
            return (
              <button
                key={t.id}
                type="button"
                className={`chip${active ? " selected" : ""}`}
                aria-pressed={active}
                onClick={() => toggle(`scope:${t.id}`)}
              >
                {t.label}
              </button>
            );
          })}
        </div>
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
    parsed.filter.modifiedAfterUnix,
  );

  const rowHeight =
    density === "compact" ? ROW_HEIGHT_COMPACT : ROW_HEIGHT_COMFORTABLE;

  const rows = data ?? [];
  const showEmpty = !isPending && !isError && rows.length === 0;
  const showVirtualBody = !isPending && !isError && rows.length > 0;

  const handleOpenEditor = (): void => setView("editor");

  // Audit issue #6: the filters toolbar button now toggles a popover
  // with grouped tool / type / scope chips.
  const [filtersOpen, setFiltersOpen] = useState(false);
  const filtersBtnRef = useRef<HTMLButtonElement>(null);

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
        <div className="filter-anchor">
          <button
            ref={filtersBtnRef}
            type="button"
            className={`text-button${filtersOpen ? " active" : ""}`}
            aria-expanded={filtersOpen}
            aria-haspopup="dialog"
            onClick={() => setFiltersOpen((o) => !o)}
          >
            <FiltersIcon />
            filters
          </button>
          {filtersOpen ? (
            <FilterPopover
              search={search}
              filterToolId={parsed.filter.toolId}
              filterKind={parsed.filter.kind}
              filterScope={parsed.filter.scope}
              anchorRef={filtersBtnRef}
              onChange={setSearch}
              onClose={() => setFiltersOpen(false)}
            />
          ) : null}
        </div>
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
        {/*
          Audit issue #10: a "+ tag" chip used to live here but tags are
          not a thing yet. Removed for consistency with QuickLook (#4).
          Returns when a tag system actually ships.
        */}
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
