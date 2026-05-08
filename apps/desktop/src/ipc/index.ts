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
  ComponentFilter,
  ComponentSummary,
  DetectedTool,
  HealthSummary,
  ScanReport,
  SearchQuery,
  SearchResult,
} from "@aseye/shared-types";

export type {
  ComponentDetail,
  ComponentFilter,
  ComponentSummary,
  DetectedTool,
  HealthSummary,
  ScanReport,
  SearchQuery,
  SearchResult,
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
