/**
 * Command palette (Phase 2.4).
 *
 * Cmd-K opens the palette over any view. Two stacked sections:
 *
 *   1. Components - live FTS5 search results (`useSearch`). When the
 *      input is empty we show the 5 most recently used components from
 *      `useComponents({})` so the palette is useful on first open.
 *   2. Actions - palette-only commands (open a view, toggle a setting,
 *      kick off a full scan, restart onboarding). Filtered by the same
 *      query via a forgiving subsequence match (see `lib/paletteActions`).
 *
 * Keyboard model: Up/Down move a single linear focus cursor across both
 * sections; Enter fires the active item; Tab jumps between sections;
 * Esc closes (handled centrally in `lib/keyboard.ts`).
 *
 * Mouse hover visually highlights but does NOT move the keyboard cursor -
 * the active class always reflects the keyboard position so a user
 * arrowing through the list isn't surprised by their pointer.
 */
import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type ReactNode,
} from "react";
import { useUi, type ViewId } from "@/store/ui";
import { useComponents, useSearch } from "@/ipc/hooks";
import { startFullScan } from "@/ipc";
import { resetOnboarding } from "@/lib/onboarding";
import {
  filterPaletteActions,
  type PaletteAction,
} from "@/lib/paletteActions";
import type { ComponentSummary, SearchResult } from "@aseye/shared-types";
import { SearchIcon } from "./icons";

/** Hard cap on component results shown in the palette. */
const COMPONENT_LIMIT = 8;
/** Number of recently-used components rendered when the query is empty. */
const RECENT_LIMIT = 5;

/**
 * Flat row entry in the palette. Both component hits and palette actions
 * are normalised into this shape so the keyboard cursor can walk a single
 * linear list across the two visible sections.
 */
interface ComponentRow {
  kind: "component";
  section: "components";
  id: string;
  componentId: string;
  /** Localised component kind label ("skill", "command", ...). */
  typeLabel: string;
  /** Display name. */
  label: string;
  /** Tool name + matched-snippet (when present). */
  meta: string;
  /** When set, snippet rendered as a third line (FTS hits). */
  snippet?: string;
}

interface ActionRow {
  kind: "action";
  section: "actions";
  id: string;
  label: string;
  run: () => void | Promise<void>;
}

type PaletteRow = ComponentRow | ActionRow;

/** Render `claude-code` → `claude code` for the palette meta column. */
function prettyToolName(tool: string): string {
  return tool.replace(/-/g, " ");
}

/** Resolve a `ComponentSummary` (or `SearchResult`) display name. */
function pickDisplayName(c: {
  displayName: string | null;
  name: string;
}): string {
  return c.displayName?.trim() || c.name;
}

/** Convert an FTS `SearchResult` into the flat palette row shape. */
function searchResultToRow(r: SearchResult): ComponentRow {
  return {
    kind: "component",
    section: "components",
    id: `c:${r.id}`,
    componentId: r.id,
    typeLabel: r.kind,
    label: pickDisplayName(r),
    meta: prettyToolName(r.tool),
    ...(r.snippet ? { snippet: r.snippet } : {}),
  };
}

/** Convert a `ComponentSummary` into the flat palette row shape. */
function summaryToRow(c: ComponentSummary): ComponentRow {
  return {
    kind: "component",
    section: "components",
    id: `c:${c.id}`,
    componentId: c.id,
    typeLabel: c.kind,
    label: pickDisplayName(c),
    meta: prettyToolName(c.tool),
  };
}

/**
 * Sort recently-used components by `lastUsedAt` DESC, falling back to
 * `mtime` for components that have never been opened. We resort
 * client-side because `list_components` orders by `mtime` only.
 */
function sortByRecency(rows: ComponentSummary[]): ComponentSummary[] {
  const copy = [...rows];
  copy.sort((a, b) => {
    const aKey = a.lastUsedAt ?? a.mtime;
    const bKey = b.lastUsedAt ?? b.mtime;
    if (aKey === bKey) return 0;
    return aKey > bKey ? -1 : 1;
  });
  return copy;
}

interface PaletteRowViewProps {
  row: PaletteRow;
  active: boolean;
  onActivate: () => void;
  /** Mouse-enter sets a "hover" highlight without stealing keyboard focus. */
  onHoverChange: (hovered: boolean) => void;
  hovered: boolean;
}

function PaletteRowView({
  row,
  active,
  onActivate,
  onHoverChange,
  hovered,
}: PaletteRowViewProps) {
  // The visual `active` class drives focus highlight. Hover state is a
  // softer treatment we layer on top via the same class - the existing
  // CSS already maps `.palette-row:hover` to the same look as `.active`.
  const classes = ["palette-row"];
  if (active) classes.push("active");

  const ariaSelected = active ? true : undefined;

  if (row.kind === "action") {
    return (
      <button
        type="button"
        className={classes.join(" ")}
        onClick={onActivate}
        onMouseEnter={() => onHoverChange(true)}
        onMouseLeave={() => onHoverChange(false)}
        role="option"
        aria-selected={ariaSelected}
        data-section={row.section}
        data-row-id={row.id}
        data-hovered={hovered ? "true" : undefined}
      >
        <span className="palette-kind action">action</span>
        <strong>{row.label}</strong>
        <span aria-hidden>↵</span>
      </button>
    );
  }

  return (
    <button
      type="button"
      className={classes.join(" ")}
      onClick={onActivate}
      onMouseEnter={() => onHoverChange(true)}
      onMouseLeave={() => onHoverChange(false)}
      role="option"
      aria-selected={ariaSelected}
      data-section={row.section}
      data-row-id={row.id}
      data-hovered={hovered ? "true" : undefined}
    >
      <span className="palette-kind">{row.typeLabel}</span>
      <span className="palette-row-name">
        <strong>{row.label}</strong>
        {row.snippet ? (
          <small className="palette-row-snippet">{row.snippet}</small>
        ) : null}
      </span>
      <span>{row.meta}</span>
    </button>
  );
}

interface SectionHeadingProps {
  label: string;
  children: ReactNode;
}

function PaletteSection({ label, children }: SectionHeadingProps) {
  return (
    <div data-palette-section={label.toLowerCase()}>
      <div className="palette-section-heading">{label}</div>
      {children}
    </div>
  );
}

/**
 * Build the action list inside the component (the registry needs the
 * Zustand store + IPC functions, so it can't be a top-level constant).
 * Memoised on the underlying setters to keep referential equality stable.
 */
function usePaletteActions(): readonly PaletteAction[] {
  const setView = useUi((s) => s.setView);
  const toggleTheme = useUi((s) => s.toggleTheme);
  const toggleDensity = useUi((s) => s.toggleDensity);
  const togglePanicMode = useUi((s) => s.togglePanicMode);
  const toggleOnboarding = useUi((s) => s.toggleOnboarding);

  return useMemo<readonly PaletteAction[]>(() => {
    const view = (id: ViewId): (() => void) => () => setView(id);
    return [
      { id: "open-inventory", label: "Open Inventory", run: view("inventory") },
      { id: "open-map", label: "Open Map", run: view("map") },
      { id: "open-editor", label: "Open Editor", run: view("editor") },
      { id: "open-health", label: "Open Health", run: view("health") },
      {
        id: "open-settings",
        label: "Open Settings",
        keywords: ["preferences", "config"],
        run: view("settings"),
      },
      {
        id: "toggle-theme",
        label: "Toggle theme",
        keywords: ["dark mode", "light mode"],
        run: toggleTheme,
      },
      {
        id: "toggle-density",
        label: "Toggle density",
        keywords: ["compact", "comfortable"],
        run: toggleDensity,
      },
      {
        id: "toggle-panic-mode",
        label: "Toggle panic mode",
        keywords: ["mask secrets", "hide secrets"],
        run: togglePanicMode,
      },
      {
        id: "restart-onboarding",
        label: "Restart onboarding",
        keywords: ["welcome", "tour"],
        run: () => {
          resetOnboarding();
          toggleOnboarding(true);
        },
      },
      {
        id: "run-full-scan",
        label: "Run full scan",
        keywords: ["index", "rescan", "refresh"],
        run: () => {
          // Fire-and-forget. The pipeline-event subscriber in
          // `usePipelineEventInvalidator` will surface the result.
          void startFullScan();
        },
      },
    ];
  }, [
    setView,
    toggleTheme,
    toggleDensity,
    togglePanicMode,
    toggleOnboarding,
  ]);
}

interface CommandPaletteProps {
  /**
   * Pre-fill the search box when the palette opens. Intended for stories
   * and tests; production callers (App.tsx) leave it unset.
   */
  defaultQuery?: string;
}

export function CommandPalette({ defaultQuery = "" }: CommandPaletteProps = {}) {
  const open = useUi((s) => s.paletteOpen);
  const toggle = useUi((s) => s.togglePalette);
  const selectComponent = useUi((s) => s.selectComponent);
  const inputRef = useRef<HTMLInputElement>(null);

  // Local state - the query lives only as long as the palette is open.
  const [query, setQuery] = useState(defaultQuery);
  const [activeIndex, setActiveIndex] = useState(0);
  const [hoveredId, setHoveredId] = useState<string | null>(null);

  const queryNonEmpty = query.trim().length > 0;

  // Components: search when there's a query, recent list otherwise.
  const searchHook = useSearch({
    text: query,
    limit: COMPONENT_LIMIT,
    toolId: null,
    kind: null,
    scope: null,
  });
  const recentHook = useComponents({
    toolId: null,
    kind: null,
    scope: null,
    query: null,
    tag: null,
    limit: RECENT_LIMIT * 4, // over-fetch then sort by lastUsedAt
    offset: null,
  });

  const componentRows: ComponentRow[] = useMemo(() => {
    if (queryNonEmpty) {
      return (searchHook.data ?? []).slice(0, COMPONENT_LIMIT).map(searchResultToRow);
    }
    return sortByRecency(recentHook.data ?? [])
      .slice(0, RECENT_LIMIT)
      .map(summaryToRow);
  }, [queryNonEmpty, searchHook.data, recentHook.data]);

  // Actions registry, fuzzy-filtered by the same query.
  const actions = usePaletteActions();
  const actionRows: ActionRow[] = useMemo(
    () =>
      filterPaletteActions(actions, query).map((a) => ({
        kind: "action",
        section: "actions",
        id: `a:${a.id}`,
        label: a.label,
        run: a.run,
      })),
    [actions, query],
  );

  /**
   * Single linear list the keyboard cursor walks. Components first,
   * actions second (the section headings are decorative; they don't
   * receive focus).
   */
  const allRows: PaletteRow[] = useMemo(
    () => [...componentRows, ...actionRows],
    [componentRows, actionRows],
  );

  // Reset query + cursor when the palette opens; clear when it closes.
  useEffect(() => {
    if (open) {
      setQuery(defaultQuery);
      setActiveIndex(0);
      // Slight delay lets the dialog finish its CSS transition before
      // we yank focus into it; 0ms is enough on most browsers.
      const id = window.setTimeout(() => inputRef.current?.focus(), 0);
      return () => window.clearTimeout(id);
    }
    return undefined;
  }, [open, defaultQuery]);

  // Clamp the cursor when the visible list shrinks.
  useEffect(() => {
    setActiveIndex((idx) => {
      if (allRows.length === 0) return 0;
      if (idx >= allRows.length) return allRows.length - 1;
      return idx;
    });
  }, [allRows.length]);

  // Reset the cursor whenever the user types - they're starting over.
  useEffect(() => {
    setActiveIndex(0);
  }, [query]);

  function fireRow(row: PaletteRow): void {
    if (row.kind === "component") {
      selectComponent(row.componentId);
      toggle(false);
      return;
    }
    // Run the action; the call may close the palette itself (e.g.
    // `togglePanicMode` clears `paletteOpen`). We close as a default so
    // every action feels final.
    void row.run();
    toggle(false);
  }

  function handleKey(event: ReactKeyboardEvent<HTMLDivElement>): void {
    if (allRows.length === 0) {
      // Even with no rows we still want Enter on the input to be a no-op.
      if (event.key === "Enter") event.preventDefault();
      return;
    }

    if (event.key === "ArrowDown") {
      event.preventDefault();
      setActiveIndex((i) => (i + 1) % allRows.length);
      return;
    }
    if (event.key === "ArrowUp") {
      event.preventDefault();
      setActiveIndex((i) => (i - 1 + allRows.length) % allRows.length);
      return;
    }
    if (event.key === "Enter") {
      event.preventDefault();
      const row = allRows[activeIndex];
      if (row) fireRow(row);
      return;
    }
    if (event.key === "Tab") {
      // Tab jumps between sections (components → actions and back).
      // If only one section is populated, falls through to the browser
      // default - useful when only actions match.
      const componentsCount = componentRows.length;
      if (componentsCount === 0 || actionRows.length === 0) return;
      event.preventDefault();
      const inActions = activeIndex >= componentsCount;
      setActiveIndex(inActions ? 0 : componentsCount);
    }
  }

  const componentsLoading = queryNonEmpty && searchHook.isFetching;
  const showNoMatches =
    queryNonEmpty &&
    !searchHook.isFetching &&
    componentRows.length === 0;

  return (
    <div
      className={`palette-backdrop${open ? " open" : ""}`}
      aria-hidden={!open}
      onClick={(e) => {
        if (e.target === e.currentTarget) toggle(false);
      }}
    >
      <div
        className="command-palette"
        role="dialog"
        aria-modal="true"
        aria-label="command palette"
        onKeyDown={handleKey}
      >
        <label className="palette-search">
          <SearchIcon />
          <input
            ref={inputRef}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            aria-label="command search"
            aria-controls="palette-results"
            aria-activedescendant={
              allRows[activeIndex] ? allRows[activeIndex].id : undefined
            }
            placeholder="search components and actions..."
            type="text"
            autoComplete="off"
            spellCheck={false}
          />
        </label>

        <div
          className="palette-results"
          id="palette-results"
          role="listbox"
        >
          <PaletteSection
            label={queryNonEmpty ? "Components" : "Recent"}
          >
            {componentsLoading ? (
              <div className="palette-row muted" aria-busy="true">
                <span className="palette-kind">…</span>
                <span>searching…</span>
                <span aria-hidden />
              </div>
            ) : null}
            {!componentsLoading && showNoMatches ? (
              <div className="palette-row muted">
                <span className="palette-kind">—</span>
                <span>no matches</span>
                <span aria-hidden />
              </div>
            ) : null}
            {componentRows.map((row) => {
              const idx = allRows.indexOf(row);
              return (
                <PaletteRowView
                  key={row.id}
                  row={row}
                  active={idx === activeIndex}
                  hovered={hoveredId === row.id}
                  onHoverChange={(h) =>
                    setHoveredId(h ? row.id : (cur) => (cur === row.id ? null : cur))
                  }
                  onActivate={() => fireRow(row)}
                />
              );
            })}
          </PaletteSection>

          {actionRows.length > 0 ? (
            <PaletteSection label="Actions">
              {actionRows.map((row) => {
                const idx = allRows.indexOf(row);
                return (
                  <PaletteRowView
                    key={row.id}
                    row={row}
                    active={idx === activeIndex}
                    hovered={hoveredId === row.id}
                    onHoverChange={(h) =>
                      setHoveredId(h ? row.id : (cur) => (cur === row.id ? null : cur))
                    }
                    onActivate={() => fireRow(row)}
                  />
                );
              })}
            </PaletteSection>
          ) : null}
        </div>
      </div>
    </div>
  );
}
