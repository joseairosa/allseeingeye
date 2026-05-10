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
  BackupReport,
  BackupStatusReport,
  ComponentDetail,
  ComponentDetailWithRaw,
  ComponentFilter,
  ComponentFindingsCount,
  ComponentSummary,
  ComponentType,
  CostResponse,
  DetectedTool,
  FindingSummary,
  HealthSummary,
  PipelineEvent,
  RestoreReport,
  VerifyReport,
  SaveOutcome,
  SearchQuery,
  SearchResult,
  SecurityFilter,
  SecuritySummary,
  ToolId,
} from "@aseye/shared-types";
import {
  backupNow,
  backupSetAuto,
  backupStatus,
  backupVerify,
  getComponent,
  getComponentWithRaw,
  getExcludedToolIds,
  getFindingsCountPerComponent,
  getHealthSummary,
  getSecuritySummary,
  getValidationSchema,
  listComponents,
  listSecurityFindings,
  listTools,
  readComponentRaw,
  restoreNow,
  saveComponent,
  search,
  setToolIndexed,
  suppressFinding,
  unsuppressFinding,
  usageQuery,
  usageRefresh,
  getProjectMemoryRoots,
  setProjectMemoryRoots,
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
  /** Phase 3.3 - one-trip detail+raw payload for the Editor view. */
  componentWithRaw: ["componentWithRaw"] as const,
  /** Phase 3.3 - bundled JSON Schema text for the form pane. */
  validationSchema: ["validationSchema"] as const,
  search: ["search"] as const,
  health: ["health"] as const,
  /** Phase 7.3 - security findings, summary, per-component counts. */
  securityFindings: ["securityFindings"] as const,
  securitySummary: ["securitySummary"] as const,
  componentFindingsCounts: ["componentFindingsCounts"] as const,
  /** Phase 14C - per-`CostQuery` payloads driving the Cost view. */
  cost: ["cost"] as const,
  /** Phase 14B - app settings reads (project memory roots, ...). */
  settings: ["settings"] as const,
  /** Phase 15 - backup status (manifest count, last run, auto flag). */
  backup: ["backup"] as const,
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
 * Phase 3.3 - one-trip detail + raw bytes for the Editor open path.
 * Disabled when no component is selected. The pipeline-event
 * invalidator drives refetch on external file changes.
 */
export function useComponentWithRaw(
  id: string | null,
): UseQueryResult<ComponentDetailWithRaw | null, Error> {
  return useQuery({
    queryKey: [...QUERY_KEYS.componentWithRaw, id] as const,
    queryFn: () => getComponentWithRaw(id as string),
    enabled: id !== null,
    staleTime: STALE_COMPONENT_RAW_MS,
  });
}

/**
 * Phase 3.3 - bundled JSON Schema text for a (tool, kind) tuple.
 * Returns `null` when no schema is bundled. Schemas never change at
 * runtime, so a long stale time is fine.
 */
export function useValidationSchema(
  tool: ToolId | null,
  kind: ComponentType | null,
): UseQueryResult<string | null, Error> {
  const enabled = tool !== null && kind !== null;
  return useQuery({
    queryKey: [...QUERY_KEYS.validationSchema, tool, kind] as const,
    queryFn: () => getValidationSchema(tool as ToolId, kind as ComponentType),
    enabled,
    // Schemas are bundled into the binary; only a new build can
    // change them.
    staleTime: Number.POSITIVE_INFINITY,
  });
}

/** Variables for `useSaveComponent`. */
export interface SaveComponentVariables {
  id: string;
  content: string;
  originalHash: string;
}

/**
 * Save a component through the atomic writer + re-index path. The
 * mutation invalidates every downstream cache that could change as
 * a result (`component`, `componentRaw`, `componentWithRaw`,
 * `components`, `health`, security caches) so the rest of the UI
 * converges without polling.
 */
export function useSaveComponent(): UseMutationResult<
  SaveOutcome,
  Error,
  SaveComponentVariables
> {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ id, content, originalHash }) =>
      saveComponent(id, content, originalHash),
    onSuccess: (outcome) => {
      // Only invalidate when the save actually landed. ExternalChange
      // and ValidationFailed leave the index untouched; refreshing
      // would be wasted work.
      if (outcome.kind === "saved") {
        void qc.invalidateQueries({ queryKey: QUERY_KEYS.component });
        void qc.invalidateQueries({ queryKey: QUERY_KEYS.componentRaw });
        void qc.invalidateQueries({ queryKey: QUERY_KEYS.componentWithRaw });
        void qc.invalidateQueries({ queryKey: QUERY_KEYS.components });
        void qc.invalidateQueries({ queryKey: QUERY_KEYS.health });
        void qc.invalidateQueries({ queryKey: QUERY_KEYS.securityFindings });
        void qc.invalidateQueries({ queryKey: QUERY_KEYS.securitySummary });
        void qc.invalidateQueries({
          queryKey: QUERY_KEYS.componentFindingsCounts,
        });
      }
    },
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

// ─── Phase 14C - Cost view hooks ──────────────────────────────────────

/** 5 minutes - the user controls refresh timing via the explicit button. */
const STALE_COST_MS = 5 * 60_000;

/**
 * Per-`CostQuery` payload narrows. The wire type is a discriminated
 * union; each hook narrows it to the matching variant so view-side
 * components don't have to re-discriminate after every fetch.
 */
type CostSummaryPayload = Extract<CostResponse, { kind: "summary" }>;
type CostByProjectPayload = Extract<CostResponse, { kind: "byProject" }>;
type CostByDayPayload = Extract<CostResponse, { kind: "byDay" }>;
type CostRecsPayload = Extract<CostResponse, { kind: "recommendations" }>;

async function queryCostSummary(): Promise<CostSummaryPayload> {
  const res = await usageQuery("summary");
  if (res.kind !== "summary") {
    throw new Error(`expected summary, got ${res.kind}`);
  }
  return res;
}

async function queryCostByProject(): Promise<CostByProjectPayload> {
  const res = await usageQuery("byProject");
  if (res.kind !== "byProject") {
    throw new Error(`expected byProject, got ${res.kind}`);
  }
  return res;
}

async function queryCostByDay(): Promise<CostByDayPayload> {
  const res = await usageQuery("byDay");
  if (res.kind !== "byDay") {
    throw new Error(`expected byDay, got ${res.kind}`);
  }
  return res;
}

async function queryCostRecommendations(): Promise<CostRecsPayload> {
  const res = await usageQuery("recommendations");
  if (res.kind !== "recommendations") {
    throw new Error(`expected recommendations, got ${res.kind}`);
  }
  return res;
}

/** Headline KPIs (tokens 30d, $ 30d, top project). */
export function useCostSummary(): UseQueryResult<CostSummaryPayload, Error> {
  return useQuery({
    queryKey: [...QUERY_KEYS.cost, "summary"] as const,
    queryFn: queryCostSummary,
    staleTime: STALE_COST_MS,
    refetchOnWindowFocus: false,
  });
}

/** Per-project rollup driving the bar chart. */
export function useCostByProject(): UseQueryResult<
  CostByProjectPayload,
  Error
> {
  return useQuery({
    queryKey: [...QUERY_KEYS.cost, "byProject"] as const,
    queryFn: queryCostByProject,
    staleTime: STALE_COST_MS,
    refetchOnWindowFocus: false,
  });
}

/** Per-day rollup driving the sparkline. */
export function useCostByDay(): UseQueryResult<CostByDayPayload, Error> {
  return useQuery({
    queryKey: [...QUERY_KEYS.cost, "byDay"] as const,
    queryFn: queryCostByDay,
    staleTime: STALE_COST_MS,
    refetchOnWindowFocus: false,
  });
}

/** Up to 5 ordered recommendations powering the Cost view's right panel. */
export function useCostRecommendations(): UseQueryResult<
  CostRecsPayload,
  Error
> {
  return useQuery({
    queryKey: [...QUERY_KEYS.cost, "recommendations"] as const,
    queryFn: queryCostRecommendations,
    staleTime: STALE_COST_MS,
    refetchOnWindowFocus: false,
  });
}

/**
 * Trigger an aggregation pass on the backend; resolves with the new
 * `refreshed_at` epoch and invalidates every Cost-related cache so the
 * view re-renders with the latest rollup.
 */
export function useCostRefresh(): UseMutationResult<bigint, Error, void> {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: () => usageRefresh(),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: QUERY_KEYS.cost });
    },
  });
}

// ─── Phase 14B - app settings ──────────────────────────────────────────

/**
 * Read the configured project-memory walker roots. Backend always
 * returns a non-empty list (defaults applied server-side), so callers
 * do not need to handle empty.
 */
export function useProjectMemoryRoots(): UseQueryResult<string[], Error> {
  return useQuery({
    queryKey: [...QUERY_KEYS.settings, "projectMemoryRoots"] as const,
    queryFn: () => getProjectMemoryRoots(),
    staleTime: 60_000,
  });
}

/**
 * Persist project-memory roots and invalidate the read query so the
 * Settings textarea reflects the durable state on success.
 */
export function useSetProjectMemoryRoots(): UseMutationResult<
  void,
  Error,
  string[]
> {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (roots: string[]) => setProjectMemoryRoots(roots),
    onSuccess: () => {
      void qc.invalidateQueries({
        queryKey: [...QUERY_KEYS.settings, "projectMemoryRoots"] as const,
      });
    },
  });
}

// ─── Phase 15 - backup + restore ──────────────────────────────────────

/**
 * Read the backup status payload (manifest count, last run, auto
 * flag, backup directory). Refetches on a 30s stale window plus
 * after every backup / restore mutation invalidation.
 */
export function useBackupStatus(): UseQueryResult<BackupStatusReport, Error> {
  return useQuery({
    queryKey: [...QUERY_KEYS.backup, "status"] as const,
    queryFn: backupStatus,
    staleTime: 30_000,
  });
}

/**
 * Trigger a backup pass. The Settings pane shows a busy state while
 * the mutation is in flight; on settle the status query is
 * invalidated so the pane reflects the new manifest count.
 */
export function useBackupNow(): UseMutationResult<BackupReport, Error, void> {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: () => backupNow(),
    onSettled: () => {
      void qc.invalidateQueries({ queryKey: QUERY_KEYS.backup });
    },
  });
}

/**
 * Trigger a restore pass. `dryRun = true` returns the report without
 * touching disk; `false` performs the actual restore. Both paths
 * invalidate the backup status (the manifest doesn't change but the
 * "last seen" should update if the call succeeded).
 */
export function useRestoreNow(): UseMutationResult<
  RestoreReport,
  Error,
  boolean
> {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (dryRun: boolean) => restoreNow(dryRun),
    onSettled: () => {
      void qc.invalidateQueries({ queryKey: QUERY_KEYS.backup });
    },
  });
}

/**
 * Toggle the auto-after-edit backup behaviour.
 */
export function useBackupSetAuto(): UseMutationResult<void, Error, boolean> {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (enabled: boolean) => backupSetAuto(enabled),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: QUERY_KEYS.backup });
    },
  });
}

/**
 * Run the backup integrity verify sweep. Read-only with respect to
 * both storage and the component table - no invalidations needed
 * because nothing changed; the report is consumed in-place.
 */
export function useBackupVerify(): UseMutationResult<VerifyReport, Error, void> {
  return useMutation({
    mutationFn: () => backupVerify(),
  });
}

/**
 * Read the persisted excluded-tool-id set. Empty list means every
 * detected tool is indexed.
 */
export function useExcludedToolIds(): UseQueryResult<string[], Error> {
  return useQuery({
    queryKey: [...QUERY_KEYS.settings, "excludedToolIds"] as const,
    queryFn: getExcludedToolIds,
    staleTime: 60_000,
  });
}

/** Argument shape for `useSetToolIndexed`. */
export interface SetToolIndexedVariables {
  toolId: string;
  indexed: boolean;
}

/**
 * Toggle a tool's indexing state. The mutation invalidates the
 * excluded-id query so the Settings list reflects the new state, and
 * the components / health caches because the next scan honours the
 * new exclusion immediately.
 */
export function useSetToolIndexed(): UseMutationResult<
  string[],
  Error,
  SetToolIndexedVariables
> {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ toolId, indexed }) => setToolIndexed(toolId, indexed),
    onSuccess: () => {
      void qc.invalidateQueries({
        queryKey: [...QUERY_KEYS.settings, "excludedToolIds"] as const,
      });
      // The watcher dispatch reads the same row at runtime, but
      // existing rows for an excluded tool stay in the index until
      // the user runs a re-scan or rebuild. The user controls that;
      // we do not auto-trigger because the surface is destructive.
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
          // Phase 3.3 - bundled detail+raw payload tracks the same
          // refresh cadence as the split commands above.
          void qc.invalidateQueries({ queryKey: QUERY_KEYS.componentWithRaw });
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
