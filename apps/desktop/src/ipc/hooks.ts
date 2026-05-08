/**
 * TanStack Query hooks wrapping the IPC layer.
 *
 * Every read goes through `useQuery` so the rest of the UI gets caching
 * and re-render isolation for free. `usePipelineEventInvalidator` is
 * the bridge that turns Rust-side pipeline events into surgical query
 * invalidations.
 */
import { useEffect, useState } from "react";
import {
  useQuery,
  useQueryClient,
  type UseQueryResult,
} from "@tanstack/react-query";
import type {
  ComponentDetail,
  ComponentFilter,
  ComponentSummary,
  DetectedTool,
  HealthSummary,
  PipelineEvent,
  SearchQuery,
  SearchResult,
} from "@aseye/shared-types";
import {
  getComponent,
  getHealthSummary,
  listComponents,
  listTools,
  search,
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
  search: ["search"] as const,
  health: ["health"] as const,
} as const;

const STALE_TOOLS_MS = 30_000;
const STALE_COMPONENTS_MS = 5_000;
const STALE_COMPONENT_MS = 5_000;
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
