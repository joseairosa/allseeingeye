/**
 * TanStack Query hooks wrapping the IPC layer.
 *
 * Every read goes through `useQuery` so the rest of the UI gets caching
 * and re-render isolation for free. `usePipelineEventInvalidator` is
 * the bridge that turns Rust-side pipeline events into surgical query
 * invalidations.
 */
import { useEffect, useMemo, useState } from "react";
import {
  useMutation,
  useQuery,
  useQueryClient,
  type UseMutationResult,
  type UseQueryResult,
} from "@tanstack/react-query";
import type {
  ComponentDetail,
  ComponentFilter,
  ComponentFindingsCount,
  ComponentSummary,
  DetectedTool,
  FindingSummary,
  HealthSummary,
  PipelineEvent,
  SearchQuery,
  SearchResult,
  SecurityFilter,
  SecuritySummary,
} from "@aseye/shared-types";
import {
  getComponent,
  getFindingsCountPerComponent,
  getHealthSummary,
  getSecuritySummary,
  listComponents,
  listSecurityFindings,
  listTools,
  readComponentRaw,
  search,
  suppressFinding,
  unsuppressFinding,
} from "./index";
import { subscribeToPipelineEvents } from "./events";

/**
 * Query key roots. Exported so tests / event invalidators reference the
 * same string and refactors stay in one place.
 */
export const QUERY_KEYS = {
  tools: ["tools"] as const,
  components: ["components"] as const,
  component: ["component"] as const,
  componentRaw: ["componentRaw"] as const,
  search: ["search"] as const,
  health: ["health"] as const,
  /** Phase 7.3 - security findings, summary, per-component counts. */
  securityFindings: ["securityFindings"] as const,
  securitySummary: ["securitySummary"] as const,
  componentFindingsCounts: ["componentFindingsCounts"] as const,
} as const;

const STALE_TOOLS_MS = 30_000;
const STALE_COMPONENTS_MS = 5_000;
const STALE_COMPONENT_MS = 5_000;
/** 5s matches the live-watcher debounce; the editor refetches when the
 *  pipeline event invalidator fires. */
const STALE_COMPONENT_RAW_MS = 5_000;
const STALE_SEARCH_MS = 30_000;
const STALE_HEALTH_MS = 60_000;

const SEARCH_DEBOUNCE_MS = 80;

/** Detected tools (sidebar Tools group, Settings tools list). */
export function useTools(): UseQueryResult<DetectedTool[], Error> {
  return useQuery({
    queryKey: QUERY_KEYS.tools,
    queryFn: listTools,
    staleTime: STALE_TOOLS_MS,
  });
}

/** Inventory rows for the currently-selected filter. */
export function useComponents(
  filter: ComponentFilter,
): UseQueryResult<ComponentSummary[], Error> {
  return useQuery({
    queryKey: [...QUERY_KEYS.components, filter] as const,
    queryFn: () => listComponents(filter),
    staleTime: STALE_COMPONENTS_MS,
  });
}

/** Full detail for the selected component (Quick Look, Editor preview). */
export function useComponent(
  id: string | null,
): UseQueryResult<ComponentDetail | null, Error> {
  return useQuery({
    queryKey: [...QUERY_KEYS.component, id] as const,
    queryFn: () => getComponent(id as string),
    enabled: id !== null,
    staleTime: STALE_COMPONENT_MS,
  });
}

/**
 * Phase 3.1 - raw on-disk bytes for the Editor's Monaco pane. Disabled
 * when no component is selected. The pipeline-event invalidator
 * automatically refetches when the file changes underneath.
 */
export function useComponentRaw(
  id: string | null,
): UseQueryResult<string, Error> {
  return useQuery({
    queryKey: [...QUERY_KEYS.componentRaw, id] as const,
    queryFn: () => readComponentRaw(id as string),
    enabled: id !== null,
    staleTime: STALE_COMPONENT_RAW_MS,
  });
}

/**
 * Debounced FTS5 search. The hook holds the latest query in local
 * state so React Query's key changes only after the debounce window.
 */
export function useSearch(
  query: SearchQuery,
): UseQueryResult<SearchResult[], Error> {
  const [debounced, setDebounced] = useState<SearchQuery>(query);

  useEffect(() => {
    const id = window.setTimeout(() => setDebounced(query), SEARCH_DEBOUNCE_MS);
    return () => window.clearTimeout(id);
  }, [query]);

  return useQuery({
    queryKey: [...QUERY_KEYS.search, debounced] as const,
    queryFn: () => search(debounced),
    enabled: debounced.text.length > 0,
    staleTime: STALE_SEARCH_MS,
  });
}

/** Health totals for the sidebar Health group + Health view. */
export function useHealthSummary(): UseQueryResult<HealthSummary, Error> {
  return useQuery({
    queryKey: QUERY_KEYS.health,
    queryFn: getHealthSummary,
    staleTime: STALE_HEALTH_MS,
  });
}

// ─── Phase 7.3 - Security view hooks ──────────────────────────────────

/** 60s matches `STALE_HEALTH_MS` - security findings change as often as
 *  health metrics, never on every keystroke. */
const STALE_SECURITY_MS = 60_000;

/** Filtered security findings (Security view + per-component lookup). */
export function useSecurityFindings(
  filter: SecurityFilter,
): UseQueryResult<FindingSummary[], Error> {
  return useQuery({
    queryKey: [...QUERY_KEYS.securityFindings, filter] as const,
    queryFn: () => listSecurityFindings(filter),
    staleTime: STALE_SECURITY_MS,
  });
}

/** Aggregate security totals (Sidebar Health row + Security view header). */
export function useSecuritySummary(): UseQueryResult<SecuritySummary, Error> {
  return useQuery({
    queryKey: QUERY_KEYS.securitySummary,
    queryFn: getSecuritySummary,
    staleTime: STALE_SECURITY_MS,
  });
}

/** Per-component finding totals - inventory shield badge. */
export function useComponentFindingsCounts(): UseQueryResult<
  ComponentFindingsCount[],
  Error
> {
  return useQuery({
    queryKey: QUERY_KEYS.componentFindingsCounts,
    queryFn: getFindingsCountPerComponent,
    staleTime: STALE_SECURITY_MS,
  });
}

/**
 * Convenience hook deriving the findings list for a single component
 * from `useSecurityFindings({ componentId })`. Quick Look uses this to
 * show the per-component Security section without managing a manual
 * filter object at the call site.
 */
export function useFindingsForComponent(
  id: string | null,
): UseQueryResult<FindingSummary[], Error> {
  // Memoise the filter so the query key is referentially stable when
  // the parent re-renders. Without this every render produces a new
  // filter object and the query refetches unnecessarily.
  const filter = useMemo<SecurityFilter>(
    () => ({
      componentId: id,
      severity: null,
      category: null,
      suppressed: null,
      limit: null,
      offset: null,
    }),
    [id],
  );
  return useQuery({
    queryKey: [...QUERY_KEYS.securityFindings, filter] as const,
    queryFn: () => listSecurityFindings(filter),
    enabled: id !== null,
    staleTime: STALE_SECURITY_MS,
  });
}

/** Argument shape for `useSuppressFinding`. */
export interface SuppressFindingVariables {
  componentId: string;
  pattern: string;
  reason?: string;
  ttlDays?: number;
}

/**
 * Suppress a finding. Invalidates every security query on success so
 * the Sidebar count, Inventory badge, Quick Look section, and Security
 * view all converge on the new state without polling.
 */
export function useSuppressFinding(): UseMutationResult<
  void,
  Error,
  SuppressFindingVariables
> {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ componentId, pattern, reason, ttlDays }) =>
      suppressFinding(componentId, pattern, reason, ttlDays),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: QUERY_KEYS.securityFindings });
      void qc.invalidateQueries({ queryKey: QUERY_KEYS.securitySummary });
      void qc.invalidateQueries({
        queryKey: QUERY_KEYS.componentFindingsCounts,
      });
    },
  });
}

/** Argument shape for `useUnsuppressFinding`. */
export interface UnsuppressFindingVariables {
  componentId: string;
  pattern: string;
}

/** Unsuppress a finding; same invalidation pattern as `useSuppressFinding`. */
export function useUnsuppressFinding(): UseMutationResult<
  void,
  Error,
  UnsuppressFindingVariables
> {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ componentId, pattern }) =>
      unsuppressFinding(componentId, pattern),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: QUERY_KEYS.securityFindings });
      void qc.invalidateQueries({ queryKey: QUERY_KEYS.securitySummary });
      void qc.invalidateQueries({
        queryKey: QUERY_KEYS.componentFindingsCounts,
      });
    },
  });
}

/**
 * Subscribe to `pipeline-event`s and surgically invalidate caches:
 *
 *   componentUpserted / componentDeleted / parseError
 *     → refetch components + health (component lists / counts changed).
 *   scanCompleted (the bulk indexer finished an `IndexRebuilt` pass)
 *     → invalidate everything; tools detection may also have changed.
 *
 * Mount this once near the App root.
 */
export function usePipelineEventInvalidator(): void {
  const qc = useQueryClient();

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let cancelled = false;

    function handle(event: PipelineEvent): void {
      switch (event.event) {
        case "componentUpserted":
        case "componentDeleted":
        case "parseError": {
          void qc.invalidateQueries({ queryKey: QUERY_KEYS.components });
          void qc.invalidateQueries({ queryKey: QUERY_KEYS.health });
          // Also refresh the open detail panel - the row may have
          // changed underfoot.
          void qc.invalidateQueries({ queryKey: QUERY_KEYS.component });
          // Editor pane re-reads on the next tick so external edits
          // surface in Monaco without a manual refresh.
          void qc.invalidateQueries({ queryKey: QUERY_KEYS.componentRaw });
          // Phase 7.3 - upserts can produce new findings; refresh
          // the security caches so the Sidebar count and Inventory
          // shield badge converge on the new state.
          void qc.invalidateQueries({ queryKey: QUERY_KEYS.securityFindings });
          void qc.invalidateQueries({ queryKey: QUERY_KEYS.securitySummary });
          void qc.invalidateQueries({
            queryKey: QUERY_KEYS.componentFindingsCounts,
          });
          break;
        }
        case "scanCompleted": {
          // Treat a completed full scan as IndexRebuilt for the UI:
          // every cached read is stale.
          void qc.invalidateQueries();
          break;
        }
      }
    }

    void subscribeToPipelineEvents(handle).then((un) => {
      if (cancelled) {
        un();
      } else {
        unlisten = un;
      }
    });

    return () => {
      cancelled = true;
      if (unlisten) unlisten();
    };
  }, [qc]);
}
