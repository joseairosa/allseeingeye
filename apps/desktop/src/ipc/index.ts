/**
 * Typed wrappers around the Tauri `invoke()` IPC bridge.
 *
 * Each function maps 1:1 onto a `#[tauri::command]` declared in
 * `apps/desktop/src-tauri/src/ipc/commands.rs`. The wrappers exist so
 * components do not need to know command names or worry about generic
 * inference at every call site - they just import a typed function.
 *
 * Command-name mapping (kept in sync with `lib.rs::generate_handler!`):
 *   listTools         → list_tools
 *   listComponents    → list_components
 *   getComponent      → get_component
 *   search            → search
 *   startFullScan     → start_full_scan
 *   getHealthSummary  → get_health_summary
 *
 * `@aseye/shared-types` re-exports the wire types so the React layer
 * never imports directly from `src-tauri/bindings/`.
 */
import { invoke } from "@tauri-apps/api/core";
import type {
  ComponentDetail,
  ComponentDetailWithRaw,
  ComponentFilter,
  ComponentFindingsCount,
  ComponentSummary,
  ComponentType,
  CostQuery,
  CostResponse,
  DetectedTool,
  FindingSummary,
  HealthSummary,
  IpcError,
  SaveOutcome,
  ScanReport,
  SearchQuery,
  SearchResult,
  SecurityFilter,
  SecuritySummary,
  ToolId,
} from "@aseye/shared-types";

export type {
  ComponentDetail,
  ComponentDetailWithRaw,
  ComponentFilter,
  ComponentFindingsCount,
  ComponentSummary,
  ComponentType,
  CostQuery,
  CostResponse,
  DetectedTool,
  FindingSummary,
  HealthSummary,
  IpcError,
  SaveOutcome,
  ScanReport,
  SearchQuery,
  SearchResult,
  SecurityFilter,
  SecuritySummary,
  ToolId,
};

/** Probe the host system for the tools we know about. */
export async function listTools(): Promise<DetectedTool[]> {
  return invoke<DetectedTool[]>("list_tools");
}

/** Paginated, filtered list of component summaries (mtime DESC). */
export async function listComponents(
  filter: ComponentFilter,
): Promise<ComponentSummary[]> {
  return invoke<ComponentSummary[]>("list_components", { filter });
}

/**
 * Full detail for a single component by `aseye://` URI. Returns `null`
 * when the id is unknown so callers can render an empty state instead
 * of catching an error.
 */
export async function getComponent(
  id: string,
): Promise<ComponentDetail | null> {
  return invoke<ComponentDetail | null>("get_component", { id });
}

/**
 * Phase 3.1 - load the raw on-disk text for a component into the
 * Monaco pane. Rejects with the typed `IpcError` envelope when the
 * file is missing, oversized (>5 MiB), or not valid UTF-8.
 */
export async function readComponentRaw(id: string): Promise<string> {
  return invoke<string>("read_component_raw", { id });
}

/** FTS5-backed search over component name/description/parsed text. */
export async function search(query: SearchQuery): Promise<SearchResult[]> {
  return invoke<SearchResult[]>("search", { query });
}

/**
 * Trigger a synchronous full scan of every detected tool root. The
 * Tauri command spawns a blocking task internally, so the awaited
 * promise resolves with the final `ScanReport`.
 */
export async function startFullScan(): Promise<ScanReport> {
  return invoke<ScanReport>("start_full_scan");
}

/** Aggregate counts (totals + per-(tool, kind) breakdown). */
export async function getHealthSummary(): Promise<HealthSummary> {
  return invoke<HealthSummary>("get_health_summary");
}

// ─── Phase 7.3 - Security view IPC ────────────────────────────────────

/** Filtered, paginated list of security findings (severity DESC). */
export async function listSecurityFindings(
  filter: SecurityFilter,
): Promise<FindingSummary[]> {
  return invoke<FindingSummary[]>("list_security_findings", { filter });
}

/** Suppress a finding for the (component, pattern) pair. */
export async function suppressFinding(
  componentId: string,
  pattern: string,
  reason?: string,
  ttlDays?: number,
): Promise<void> {
  return invoke<void>("suppress_finding", {
    componentId,
    pattern,
    reason: reason ?? null,
    ttlDays: ttlDays ?? null,
  });
}

/** Drop a previously-applied suppression. */
export async function unsuppressFinding(
  componentId: string,
  pattern: string,
): Promise<void> {
  return invoke<void>("unsuppress_finding", { componentId, pattern });
}

/** Per-component finding totals + per-severity breakdown for the inventory shield badge. */
export async function getFindingsCountPerComponent(): Promise<
  ComponentFindingsCount[]
> {
  return invoke<ComponentFindingsCount[]>(
    "get_findings_count_per_component",
  );
}

/** Aggregate counts (severity, category, suppressed) for the Sidebar Health row + Security view header. */
export async function getSecuritySummary(): Promise<SecuritySummary> {
  return invoke<SecuritySummary>("get_security_summary");
}

// ─── Phase 3.3 - Editor save flow ─────────────────────────────────────

/**
 * One-trip Editor open: returns the parsed detail and the raw on-disk
 * bytes plus a SHA-256 snapshot the editor hands back on save to gate
 * external-change detection.
 */
export async function getComponentWithRaw(
  id: string,
): Promise<ComponentDetailWithRaw | null> {
  return invoke<ComponentDetailWithRaw | null>("get_component_with_raw", { id });
}

/**
 * Save the editor's buffer back to disk through the atomic writer.
 * `originalHash` is the snapshot the editor opened with; the backend
 * compares it against the on-disk hash and returns
 * `SaveOutcome::ExternalChange` when they diverge.
 */
export async function saveComponent(
  id: string,
  content: string,
  originalHash: string,
): Promise<SaveOutcome> {
  return invoke<SaveOutcome>("save_component", {
    id,
    content,
    originalHash,
  });
}

/**
 * Bundled JSON Schema text for a `(tool, kind)` tuple, or `null` when
 * no schema is bundled. The Editor form pane parses this once on
 * mount and uses it to drive input rendering.
 */
export async function getValidationSchema(
  tool: ToolId,
  kind: ComponentType,
): Promise<string | null> {
  return invoke<string | null>("get_validation_schema", { tool, kind });
}

// ─── Phase 14C - Cost / token usage ───────────────────────────────────

/**
 * Run a typed query against the `token_usage` rollup table. `kind`
 * picks the response shape (summary, byProject, byDay, recommendations).
 * Pass `refresh = true` to force a re-aggregation pass before reading.
 */
export async function usageQuery(
  kind: CostQuery,
  refresh?: boolean,
): Promise<CostResponse> {
  return invoke<CostResponse>("usage_query", {
    kind,
    refresh: refresh ?? null,
  });
}

/**
 * Imperatively re-run the aggregation pass and return the new
 * `refreshed_at` epoch (unix seconds). The Cost view calls this when
 * the user clicks "refresh".
 */
export async function usageRefresh(): Promise<bigint> {
  // The Rust handler returns `i64`; the Tauri JSON bridge surfaces it
  // as `number`. We coerce to `bigint` so the rest of the UI uses a
  // single timestamp type with `formatRelativeTime`.
  const raw = await invoke<number | bigint>("usage_refresh");
  return typeof raw === "bigint" ? raw : BigInt(raw);
}
