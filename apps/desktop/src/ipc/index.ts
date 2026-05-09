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
  BackupReport,
  BackupStatusReport,
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
  RestoreReport,
  SaveOutcome,
  ScanReport,
  SearchQuery,
  SearchResult,
  SecurityFilter,
  SecuritySummary,
  ToolId,
} from "@aseye/shared-types";

export type {
  BackupReport,
  BackupStatusReport,
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
  RestoreReport,
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

// ─── Phase 14B - app settings ─────────────────────────────────────────

/**
 * Read the configured project-memory walker roots. Falls back to
 * `["~/Development", "~"]` server-side when the row is missing or
 * malformed, so the response is always a non-empty array.
 */
export async function getProjectMemoryRoots(): Promise<string[]> {
  return invoke<string[]>("get_project_memory_roots");
}

/**
 * Persist a new list of project-memory walker roots. Empty / whitespace
 * entries are stripped server-side; the call rejects if the resulting
 * list is empty (writing `[]` would resolve to defaults on read, which
 * would silently undo the user's edit).
 */
export async function setProjectMemoryRoots(roots: string[]): Promise<void> {
  return invoke<void>("set_project_memory_roots", { roots });
}

// ─── Phase 15 - backup + restore ──────────────────────────────────────

/**
 * Run a backup pass. Idempotent: components whose plaintext hash
 * matches the existing manifest entry are skipped. Per-component
 * failures collect in the report rather than aborting the sweep.
 */
export async function backupNow(): Promise<BackupReport> {
  return invoke<BackupReport>("backup_now");
}

/**
 * Run a restore pass. `dryRun = true` reports what would happen
 * without writing any files; `false` performs the actual restore.
 * Files whose local mtime is newer than the backup `encrypted_at`
 * are skipped server-side so the user's recent work is not
 * overwritten.
 */
export async function restoreNow(dryRun: boolean): Promise<RestoreReport> {
  return invoke<RestoreReport>("restore_now", { dryRun });
}

/** Lightweight status payload for the Settings backup pane. */
export async function backupStatus(): Promise<BackupStatusReport> {
  return invoke<BackupStatusReport>("backup_status");
}

/** Toggle the auto-after-edit backup behaviour. */
export async function backupSetAuto(enabled: boolean): Promise<void> {
  return invoke<void>("backup_set_auto", { enabled });
}

// ─── Audit follow-ups - Settings + Onboarding wiring ──────────────────

/**
 * Probe whether a path is readable from the desktop process. The
 * onboarding "Allow read access" step uses this to decide whether to
 * surface the macOS Full Disk Access deep link. Returns `false` for
 * any error (`NotFound`, `PermissionDenied`, ...).
 */
export async function checkPathReadable(path: string): Promise<boolean> {
  return invoke<boolean>("check_path_readable", { path });
}

/**
 * Drop every indexed-content row and re-run a full scan. User
 * preferences (`app_settings`) are preserved across the rebuild so the
 * re-scan reuses the configured project memory roots, excluded tool
 * ids, etc. Resolves with the resulting `ScanReport`.
 */
export async function rebuildIndex(): Promise<ScanReport> {
  return invoke<ScanReport>("rebuild_index");
}

/**
 * Drop every indexed-content row *and* every persisted user preference.
 * Schema is preserved; the database file remains valid. The caller is
 * responsible for triggering a re-scan if one is desired.
 */
export async function resetIndex(): Promise<void> {
  return invoke<void>("reset_index");
}

/**
 * Persist a sanitised diagnostics JSON snapshot to the user-chosen
 * `targetPath`. The caller is responsible for sanitising the report
 * (`sanitiseForClipboard` from `lib/diagnosticsSanitiser`) and for
 * picking the target path through the Tauri dialog plugin.
 */
export async function exportDiagnostics(
  targetPath: string,
  contents: string,
): Promise<void> {
  return invoke<void>("export_diagnostics", { targetPath, contents });
}

/**
 * Read the persisted set of `excludedToolIds`. Empty list means every
 * detected tool is indexed (the docs/03 default).
 */
export async function getExcludedToolIds(): Promise<string[]> {
  return invoke<string[]>("get_excluded_tool_ids");
}

/**
 * Toggle whether a detected tool is indexed. `indexed: true` removes
 * the tool from the excluded set; `indexed: false` adds it. Returns
 * the resulting excluded list so callers can update their cache
 * without a follow-up read.
 */
export async function setToolIndexed(
  toolId: string,
  indexed: boolean,
): Promise<string[]> {
  return invoke<string[]>("set_tool_indexed", { toolId, indexed });
}
