//! Read-only IPC command handlers.
//!
//! Phase 1.6 surfaces what the React UI needs to populate Inventory,
//! Editor preview, the Map view's node summaries, the search box, and
//! the Health view's totals. Mutations live in Phase 1.7.
//!
//! Each `#[tauri::command]` is a thin wrapper around a plain Rust
//! function (`*_inner`) so the same code path is exercised by the
//! integration tests without standing up a Tauri runtime.

// Tauri commands accept `State<'_, T>` and `String`/struct parameters
// by value as part of the macro's contract; we then pass them by
// reference to the inner pure functions. Clippy reads that as
// "unnecessary by-value" but the signature is dictated by Tauri.
#![allow(clippy::needless_pass_by_value)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use rusqlite::params;
use tauri::State;

use super::types::{
    ComponentDetail, ComponentDetailWithRaw, ComponentFilter, ComponentFindingsCount,
    ComponentSummary, FindingSummary, HealthSummary, IpcError, SaveOutcome, SearchQuery,
    SearchResult, SecurityCategoryCounts, SecurityFilter, SecuritySummary, SeverityCounts,
    ToolHealthCount,
};
use crate::fs::safe_atomic_write_with_options;
use crate::index::upsert::{parse_component_type, parse_scope, parse_tool_id};
use crate::index::{upsert_component, IndexHandle};
use crate::parser::{hash, parse_bytes};
use crate::pipeline::{ScanContext, ScanReport};
use crate::registry::detect::expand_home;
use crate::registry::registry as registry_descriptors;
use crate::registry::types::{ComponentRoot, ComponentType, Format, Scope, ToolDescriptor, ToolId};
use crate::security::{Category as SecurityCategory, Severity};
use crate::validator::{schema_for_tuple, validate, ValidationError};

/// Server-side cap for `list_components` to protect the IPC channel
/// from accidentally fetching the entire index in one call.
const LIST_COMPONENTS_HARD_LIMIT: u32 = 1000;
/// Default page size when the caller doesn't specify one.
const LIST_COMPONENTS_DEFAULT_LIMIT: u32 = 200;
/// Server-side cap for `search`.
const SEARCH_HARD_LIMIT: u32 = 200;
/// Default `search` page size.
const SEARCH_DEFAULT_LIMIT: u32 = 50;
/// Maximum on-disk size we will read into the editor pane. Mirrors the
/// parser's `MAX_PARSE_SIZE` so the editor never holds bytes the parser
/// would refuse to process. 5 MiB.
const READ_RAW_HARD_LIMIT: u64 = crate::parser::MAX_PARSE_SIZE;
/// Server-side cap for `list_security_findings`. Mirrors
/// `LIST_COMPONENTS_HARD_LIMIT` so a runaway query can't drag the entire
/// finding table across the IPC boundary.
const LIST_FINDINGS_HARD_LIMIT: u32 = 1000;
/// Default page size for `list_security_findings`.
const LIST_FINDINGS_DEFAULT_LIMIT: u32 = 200;

#[tauri::command]
pub fn list_tools() -> Vec<crate::registry::DetectedTool> {
    crate::registry::detect_all()
}

#[tauri::command]
pub fn list_components(
    state: State<'_, Arc<IndexHandle>>,
    filter: ComponentFilter,
) -> Result<Vec<ComponentSummary>, String> {
    list_components_inner(state.inner().as_ref(), &filter).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_component(
    state: State<'_, Arc<IndexHandle>>,
    id: String,
) -> Result<Option<ComponentDetail>, String> {
    get_component_inner(state.inner().as_ref(), &id).map_err(|e| e.to_string())
}

/// Phase 3.1 - return the raw on-disk text for a component so the
/// Monaco pane can edit it directly. The command goes through the
/// index to resolve `id -> path`, then reads the file synchronously.
///
/// Refuses to return more than [`READ_RAW_HARD_LIMIT`] bytes, matching
/// the parser cap so we don't ship partial / unparseable content into
/// the editor.
///
/// Returns a typed [`IpcError`] (rather than `String`) so the React
/// layer can pattern-match the failure - "not found" renders an empty
/// state, "payload too large" renders a dedicated warning, etc.
#[tauri::command]
pub fn read_component_raw(
    state: State<'_, Arc<IndexHandle>>,
    id: String,
) -> Result<String, IpcError> {
    read_component_raw_inner(state.inner().as_ref(), &id)
}

#[tauri::command]
pub fn search(
    state: State<'_, Arc<IndexHandle>>,
    query: SearchQuery,
) -> Result<Vec<SearchResult>, String> {
    search_inner(state.inner().as_ref(), &query).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_health_summary(state: State<'_, Arc<IndexHandle>>) -> Result<HealthSummary, String> {
    health_summary_inner(state.inner().as_ref()).map_err(|e| e.to_string())
}

// ─── Phase 7.3 - Security view IPC ──────────────────────────────────────

#[tauri::command]
pub fn list_security_findings(
    state: State<'_, Arc<IndexHandle>>,
    filter: SecurityFilter,
) -> Result<Vec<FindingSummary>, String> {
    list_security_findings_inner(state.inner().as_ref(), &filter).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn suppress_finding(
    state: State<'_, Arc<IndexHandle>>,
    component_id: String,
    pattern: String,
    reason: Option<String>,
    ttl_days: Option<u32>,
) -> Result<(), String> {
    suppress_finding_inner(
        state.inner().as_ref(),
        &component_id,
        &pattern,
        reason.as_deref(),
        ttl_days,
        now_unix_millis(),
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn unsuppress_finding(
    state: State<'_, Arc<IndexHandle>>,
    component_id: String,
    pattern: String,
) -> Result<(), String> {
    unsuppress_finding_inner(state.inner().as_ref(), &component_id, &pattern)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_findings_count_per_component(
    state: State<'_, Arc<IndexHandle>>,
) -> Result<Vec<ComponentFindingsCount>, String> {
    findings_count_per_component_inner(state.inner().as_ref()).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_security_summary(state: State<'_, Arc<IndexHandle>>) -> Result<SecuritySummary, String> {
    security_summary_inner(state.inner().as_ref()).map_err(|e| e.to_string())
}

// ─── Phase 3.2 - per-tool schema validator IPC ──────────────────────────

/// Re-run validation for a component identified by URI.
///
/// The Editor's form pane (Phase 3.3) calls this after every save and
/// on demand via a "Re-validate" button. The command goes through
/// [`crate::validator::validate_by_id`], which reuses the cached
/// `parsed_json` rather than re-reading the file off disk - the upsert
/// layer already validated on write, this is purely for surfaces that
/// need a fresh outcome without an upsert cycle.
///
/// Failure modes are stringified for symmetry with the rest of the
/// command surface; the failure shape is narrow enough
/// (`ValidatorError::NotFound` / `Sqlite` / ...) that the React layer
/// can pattern-match the message text without losing fidelity.
#[tauri::command]
pub fn validate_component(
    state: State<'_, Arc<IndexHandle>>,
    id: String,
) -> Result<crate::validator::ValidationOutcome, String> {
    crate::validator::validate_by_id(state.inner().as_ref(), &id).map_err(|e| e.to_string())
}

// ─── Phase 3.3 - Editor save flow ───────────────────────────────────────

/// Persist the editor's buffer back to disk through the atomic writer
/// + path-safety guards, then re-index the component so the rest of
/// the UI converges on the new state.
///
/// The caller passes the SHA-256 it received when it opened the file.
/// If the on-disk hash diverged in the meantime (an external editor
/// or another process touched the file), we return
/// [`SaveOutcome::ExternalChange`] with the current bytes so the UI
/// can render a diff banner without a second IPC round-trip.
///
/// Validation runs against the same per-tool schema as
/// [`crate::index::upsert::upsert_component`] uses - the upsert hot
/// path validates on write; this command validates **before** we write
/// so a malformed save never even reaches the atomic writer.
///
/// The path-safety guard rejects writes outside the matching
/// descriptor's `watch_paths`. Forbidden segments (`.git`,
/// `node_modules`, `target`, ...) and symlink escapes both surface as
/// [`SaveOutcome::Forbidden`] with a human-readable reason.
#[tauri::command]
pub fn save_component(
    state: State<'_, Arc<IndexHandle>>,
    id: String,
    content: String,
    original_hash: String,
) -> Result<SaveOutcome, String> {
    save_component_inner(state.inner().as_ref(), &id, &content, &original_hash, None)
        .map_err(|e| e.to_string())
}

/// One-trip detail + raw bytes payload for the Editor view.
///
/// Equivalent to `get_component` followed by `read_component_raw`,
/// bundled so the Editor can populate Monaco + the form pane in a
/// single IPC call. Earlier surfaces (Quick Look, Inventory previewer)
/// still use the two split commands because they only need one half.
#[tauri::command]
pub fn get_component_with_raw(
    state: State<'_, Arc<IndexHandle>>,
    id: String,
) -> Result<Option<ComponentDetailWithRaw>, IpcError> {
    get_component_with_raw_inner(state.inner().as_ref(), &id)
}

/// Return the bundled JSON Schema string for a `(tool, kind)` tuple,
/// or `None` when no schema is bundled.
///
/// The Editor's form pane (Phase 3.3) parses the schema once on the JS
/// side and uses it to map fields → input controls (`type: string` →
/// `<input>`, `enum` → `<select>`, ...). Returning the schema as a raw
/// string keeps the React side decoupled from `serde_json::Value` and
/// matches the bundled-string shape inside the validator.
#[tauri::command]
pub fn get_validation_schema(tool: ToolId, kind: ComponentType) -> Option<String> {
    schema_for_tuple(tool, kind).map(str::to_owned)
}

#[tauri::command]
pub async fn start_full_scan(scan_ctx: State<'_, ScanContext>) -> Result<ScanReport, String> {
    // The scan walks the filesystem and writes to SQLite synchronously.
    // We hand it off to `spawn_blocking` so the Tauri command runtime
    // doesn't stall.
    let ctx = scan_ctx.inner().clone();
    tauri::async_runtime::spawn_blocking(move || ctx.full_scan().map_err(|e| e.to_string()))
        .await
        .map_err(|e| format!("scan task panicked: {e}"))?
}

// ─── Phase 14C - token usage analytics ─────────────────────────────────

/// Read-side IPC for the Cost view. Dispatches one of four query
/// shapes against the `token_usage` rollup table. When `refresh = true`
/// the handler runs an aggregation pass first, so the returned rows
/// reflect the latest on-disk session transcripts.
///
/// The aggregation IO is dispatched via `spawn_blocking` so the Tauri
/// command runtime never blocks waiting for `SQLite`. Even on the
/// developer's home (around 3000 session files) the pass should finish
/// well inside the 5s budget; further-out users may see longer
/// first-mount times, hence the explicit "refresh" affordance the UI
/// exposes.
#[tauri::command]
pub async fn usage_query(
    state: State<'_, Arc<IndexHandle>>,
    kind: crate::usage::CostQuery,
    refresh: Option<bool>,
) -> Result<crate::usage::CostResponse, String> {
    let index = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        if refresh.unwrap_or(false) {
            crate::usage::refresh(&index).map_err(|e| e.to_string())?;
        }
        crate::usage::query::dispatch(&index, kind).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("usage_query task panicked: {e}"))?
}

/// Imperatively re-run the aggregation pass and return the new
/// `refreshed_at` epoch (unix seconds). The Cost view calls this on
/// the user clicking "refresh" and on the 30-min background timer.
///
/// This is a separate command from `usage_query` so the UI can drive
/// the refresh without committing to which view shape it's about to
/// render next.
#[tauri::command]
pub async fn usage_refresh(state: State<'_, Arc<IndexHandle>>) -> Result<i64, String> {
    let index = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::usage::refresh(&index)
            .map(|outcome| outcome.refreshed_at)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("usage_refresh task panicked: {e}"))?
}

// ─── Pure functions exercised by tests ──────────────────────────────────

/// Fetch a paginated, filtered list of `ComponentSummary` rows.
pub fn list_components_inner(
    handle: &IndexHandle,
    filter: &ComponentFilter,
) -> crate::index::Result<Vec<ComponentSummary>> {
    let limit = filter
        .limit
        .unwrap_or(LIST_COMPONENTS_DEFAULT_LIMIT)
        .min(LIST_COMPONENTS_HARD_LIMIT);
    let offset = filter.offset.unwrap_or(0);

    let mut sql = String::from(
        "SELECT c.id, c.name, c.display_name, c.description, c.type, c.tool, c.scope,
                c.format, c.path, c.size, c.mtime, c.hash, c.parse_errors,
                c.last_used_at, c.use_count
         FROM component c",
    );
    if filter.tag.is_some() {
        sql.push_str(" INNER JOIN tag t ON t.component_id = c.id");
    }
    sql.push_str(" WHERE 1=1");

    let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if let Some(tool) = filter.tool_id {
        sql.push_str(" AND c.tool = ?");
        params_vec.push(Box::new(
            crate::index::upsert::tool_id_to_str(tool).to_owned(),
        ));
    }
    if let Some(kind) = filter.kind {
        sql.push_str(" AND c.type = ?");
        params_vec.push(Box::new(
            crate::index::upsert::component_type_to_str(kind).to_owned(),
        ));
    }
    if let Some(scope) = filter.scope {
        sql.push_str(" AND c.scope = ?");
        params_vec.push(Box::new(
            crate::index::upsert::scope_to_str(scope).to_owned(),
        ));
    }
    if let Some(query) = &filter.query {
        sql.push_str(" AND (c.name LIKE ? OR IFNULL(c.description, '') LIKE ?)");
        let pattern = format!("%{query}%");
        params_vec.push(Box::new(pattern.clone()));
        params_vec.push(Box::new(pattern));
    }
    if let Some(tag) = &filter.tag {
        sql.push_str(" AND t.tag = ?");
        params_vec.push(Box::new(tag.clone()));
    }

    sql.push_str(" ORDER BY c.mtime DESC LIMIT ? OFFSET ?");
    params_vec.push(Box::new(i64::from(limit)));
    params_vec.push(Box::new(i64::from(offset)));

    handle.read(|conn| {
        let mut stmt = conn.prepare(&sql)?;
        let param_refs: Vec<&dyn rusqlite::ToSql> =
            params_vec.iter().map(std::convert::AsRef::as_ref).collect();
        let rows = stmt.query_map(param_refs.as_slice(), row_to_summary)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    })
}

/// Fetch the full detail for a single component by URI.
pub fn get_component_inner(
    handle: &IndexHandle,
    id: &str,
) -> crate::index::Result<Option<ComponentDetail>> {
    handle.read(|conn| {
        let row: Option<ComponentDetail> = conn
            .query_row(
                "SELECT c.id, c.name, c.display_name, c.description, c.type, c.tool, c.scope,
                        c.format, c.path, c.size, c.mtime, c.hash, c.parse_errors,
                        c.last_used_at, c.use_count, c.parsed_json, c.origin, c.plugin_id
                 FROM component c WHERE c.id = ?1",
                params![id],
                row_to_detail,
            )
            .map(Some)
            .or_else(|err| match err {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                other => Err(other),
            })?;
        Ok(row)
    })
}

/// Pure-function counterpart to [`read_component_raw`].
///
/// Looks up the component path in the index, reads the file off disk,
/// and returns the UTF-8 text. The cap matches the parser's
/// [`crate::parser::MAX_PARSE_SIZE`] - bigger files are rejected with
/// `IpcError::PayloadTooLarge` so the editor never opens partial
/// content. Missing ids yield `IpcError::NotFound`.
pub fn read_component_raw_inner(handle: &IndexHandle, id: &str) -> Result<String, IpcError> {
    let path: Option<String> = handle
        .read(|conn| {
            let lookup = conn
                .query_row(
                    "SELECT path FROM component WHERE id = ?1",
                    params![id],
                    |row| row.get::<_, String>(0),
                )
                .map(Some)
                .or_else(|err| match err {
                    rusqlite::Error::QueryReturnedNoRows => Ok(None),
                    other => Err(other),
                })?;
            Ok(lookup)
        })
        .map_err(|err| IpcError::Index {
            message: err.to_string(),
        })?;

    let Some(path) = path else {
        return Err(IpcError::NotFound { id: id.to_owned() });
    };

    // Stat first so we can refuse oversized files before reading their
    // contents into RAM. The Tauri IPC channel serialises the entire
    // payload, so a multi-MB read both wastes memory and stalls the UI.
    let meta = std::fs::metadata(&path).map_err(|err| IpcError::Io {
        message: format!("stat {path}: {err}"),
    })?;
    let size = meta.len();
    if size > READ_RAW_HARD_LIMIT {
        return Err(IpcError::PayloadTooLarge {
            size,
            cap: READ_RAW_HARD_LIMIT,
        });
    }

    let bytes = std::fs::read(&path).map_err(|err| IpcError::Io {
        message: format!("read {path}: {err}"),
    })?;
    String::from_utf8(bytes).map_err(|_| IpcError::InvalidUtf8)
}

/// Run an FTS5 query and return ranked matches with snippets.
pub fn search_inner(
    handle: &IndexHandle,
    query: &SearchQuery,
) -> crate::index::Result<Vec<SearchResult>> {
    let limit = query
        .limit
        .unwrap_or(SEARCH_DEFAULT_LIMIT)
        .min(SEARCH_HARD_LIMIT);

    if query.text.trim().is_empty() {
        return Ok(Vec::new());
    }

    // We escape any FTS5 metacharacters by wrapping each whitespace-
    // separated term in double quotes. This converts user input into a
    // safe phrase query rather than letting the user accidentally type
    // operators that throw.
    let safe_match = sanitize_fts_query(&query.text);

    let mut sql = String::from(
        "SELECT c.id, c.name, c.display_name, c.description, c.type, c.tool, c.scope,
                c.format, c.path, c.size, c.mtime, c.hash, c.parse_errors,
                c.last_used_at, c.use_count,
                snippet(component_fts, 3, '<mark>', '</mark>', '…', 16) AS snip
         FROM component_fts
         INNER JOIN component c ON c.id = component_fts.id
         WHERE component_fts MATCH ?1",
    );
    let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(safe_match)];

    if let Some(tool) = query.tool_id {
        sql.push_str(" AND c.tool = ?");
        params_vec.push(Box::new(
            crate::index::upsert::tool_id_to_str(tool).to_owned(),
        ));
    }
    if let Some(kind) = query.kind {
        sql.push_str(" AND c.type = ?");
        params_vec.push(Box::new(
            crate::index::upsert::component_type_to_str(kind).to_owned(),
        ));
    }
    if let Some(scope) = query.scope {
        sql.push_str(" AND c.scope = ?");
        params_vec.push(Box::new(
            crate::index::upsert::scope_to_str(scope).to_owned(),
        ));
    }

    sql.push_str(" ORDER BY rank LIMIT ?");
    params_vec.push(Box::new(i64::from(limit)));

    handle.read(|conn| {
        let mut stmt = conn.prepare(&sql)?;
        let param_refs: Vec<&dyn rusqlite::ToSql> =
            params_vec.iter().map(std::convert::AsRef::as_ref).collect();
        let rows = stmt.query_map(param_refs.as_slice(), |row| {
            let summary = row_to_summary(row)?;
            let snippet: String = row.get("snip")?;
            Ok(SearchResult { summary, snippet })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    })
}

/// Build the `HealthSummary` aggregate.
pub fn health_summary_inner(handle: &IndexHandle) -> crate::index::Result<HealthSummary> {
    handle.read(|conn| {
        let total_components: u32 = conn
            .query_row("SELECT COUNT(*) FROM component", [], |r| r.get::<_, i64>(0))
            .map(|v| u32::try_from(v).unwrap_or(u32::MAX))?;
        let total_parse_errors: u32 = conn
            .query_row(
                "SELECT COUNT(*) FROM component WHERE parse_errors IS NOT NULL",
                [],
                |r| r.get::<_, i64>(0),
            )
            .map(|v| u32::try_from(v).unwrap_or(u32::MAX))?;

        let mut stmt = conn.prepare(
            "SELECT tool, type, COUNT(*) FROM component GROUP BY tool, type ORDER BY tool, type",
        )?;
        let rows = stmt.query_map([], |row| {
            let tool: String = row.get(0)?;
            let kind: String = row.get(1)?;
            let count: i64 = row.get(2)?;
            Ok((tool, kind, count))
        })?;

        let mut by_tool_kind = Vec::new();
        for row in rows {
            let (tool_str, kind_str, count) = row?;
            let Some(tool) = parse_tool_id(&tool_str) else {
                continue;
            };
            let Some(kind) = parse_component_type(&kind_str) else {
                continue;
            };
            by_tool_kind.push(ToolHealthCount {
                tool,
                kind,
                count: u32::try_from(count).unwrap_or(u32::MAX),
            });
        }

        Ok(HealthSummary {
            total_components,
            total_parse_errors,
            by_tool_kind,
        })
    })
}

// ─── Phase 7.3 inner functions ──────────────────────────────────────────

/// Parse the on-disk severity column back into `Severity`. Unknown
/// values are mapped to `Severity::Low` so a malformed row can't poison
/// a list response - the row still surfaces, just at the lowest bucket.
fn parse_severity(s: &str) -> Severity {
    match s {
        "critical" => Severity::Critical,
        "high" => Severity::High,
        "medium" => Severity::Medium,
        _ => Severity::Low,
    }
}

/// Parse the on-disk category column back into `Category`. Unknown
/// values fall back to `Category::Secret` so future rows added by a
/// newer build still render rather than disappearing - the worst case
/// is a slightly mis-labelled row in the Security view.
fn parse_security_category(s: &str) -> SecurityCategory {
    match s {
        "mcp-permission" => SecurityCategory::McpPermission,
        _ => SecurityCategory::Secret,
    }
}

/// Order severity strings DESC so SQL `ORDER BY` puts critical first.
/// `SQLite` has no native enum ordering; we synthesise a `CASE`
/// expression that maps each label to its `Severity::rank()` value.
const SEVERITY_RANK_CASE: &str =
    "CASE severity WHEN 'critical' THEN 3 WHEN 'high' THEN 2 WHEN 'medium' THEN 1 ELSE 0 END";

#[must_use]
fn now_unix_millis() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// Implementation for `list_security_findings`.
///
/// Joins `security_finding` against `component` so the UI can render
/// the row's owning component without a second IPC hop. Filters
/// (`component_id`, `severity`, `category`, `suppressed`) build a
/// dynamic WHERE clause; pagination follows the same conventions as
/// `list_components`.
pub fn list_security_findings_inner(
    handle: &IndexHandle,
    filter: &SecurityFilter,
) -> crate::index::Result<Vec<FindingSummary>> {
    let limit = filter
        .limit
        .unwrap_or(LIST_FINDINGS_DEFAULT_LIMIT)
        .min(LIST_FINDINGS_HARD_LIMIT);
    let offset = filter.offset.unwrap_or(0);

    let mut sql = String::from(
        "SELECT f.id, f.component_id, c.name, c.path, f.category, f.pattern,
                f.severity, f.source_label, f.redacted_preview, f.detected_at,
                f.suppressed
         FROM security_finding f
         INNER JOIN component c ON c.id = f.component_id
         WHERE 1=1",
    );

    let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if let Some(component_id) = &filter.component_id {
        sql.push_str(" AND f.component_id = ?");
        params_vec.push(Box::new(component_id.clone()));
    }
    if let Some(severity) = filter.severity {
        sql.push_str(" AND f.severity = ?");
        params_vec.push(Box::new(severity.as_str().to_owned()));
    }
    if let Some(category) = filter.category {
        sql.push_str(" AND f.category = ?");
        params_vec.push(Box::new(category.as_str().to_owned()));
    }
    if let Some(suppressed) = filter.suppressed {
        sql.push_str(" AND f.suppressed = ?");
        params_vec.push(Box::new(i64::from(suppressed)));
    }

    sql.push_str(" ORDER BY ");
    sql.push_str(SEVERITY_RANK_CASE);
    sql.push_str(" DESC, f.detected_at DESC LIMIT ? OFFSET ?");
    params_vec.push(Box::new(i64::from(limit)));
    params_vec.push(Box::new(i64::from(offset)));

    handle.read(|conn| {
        let mut stmt = conn.prepare(&sql)?;
        let param_refs: Vec<&dyn rusqlite::ToSql> =
            params_vec.iter().map(std::convert::AsRef::as_ref).collect();
        let rows = stmt.query_map(param_refs.as_slice(), |row| {
            let suppressed_int: i64 = row.get(10)?;
            Ok(FindingSummary {
                id: row.get(0)?,
                component_id: row.get(1)?,
                component_name: row.get(2)?,
                component_path: row.get(3)?,
                category: parse_security_category(&row.get::<_, String>(4)?),
                pattern: row.get(5)?,
                severity: parse_severity(&row.get::<_, String>(6)?),
                source_label: row.get(7)?,
                redacted_preview: row.get(8)?,
                detected_at: row.get(9)?,
                suppressed: suppressed_int != 0,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    })
}

/// Implementation for `suppress_finding`. Uses an explicit `now_ms`
/// rather than `SystemTime::now()` directly so tests can pin time.
pub fn suppress_finding_inner(
    handle: &IndexHandle,
    component_id: &str,
    pattern: &str,
    reason: Option<&str>,
    ttl_days: Option<u32>,
    now_ms: i64,
) -> crate::index::Result<()> {
    let suppress_until: Option<i64> = ttl_days.map(|days| {
        // Multiply with i64 widths to prevent silent overflow when the
        // caller sends a huge TTL. `saturating_*` keeps the upper bound
        // at i64::MAX rather than wrapping.
        let day_ms: i64 = 86_400_000;
        let delta = i64::from(days).saturating_mul(day_ms);
        now_ms.saturating_add(delta)
    });

    handle.write(|conn| {
        // Upsert into the suppression table - existing entry's reason and
        // suppressed_at are refreshed.
        conn.execute(
            "INSERT INTO security_finding_suppression (component_id, pattern, suppressed_at, reason)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(component_id, pattern) DO UPDATE SET
               suppressed_at = excluded.suppressed_at,
               reason = excluded.reason",
            params![component_id, pattern, now_ms, reason],
        )?;
        // Flip the `suppressed` flag on every matching finding row so
        // the Security view's "suppressed" filter sees them immediately.
        // We also stash the reason and the suppress-until timestamp so a
        // restart picks them back up without consulting the suppression
        // table.
        conn.execute(
            "UPDATE security_finding
             SET suppressed = 1,
                 suppress_reason = ?3,
                 suppress_until = ?4
             WHERE component_id = ?1 AND pattern = ?2",
            params![component_id, pattern, reason, suppress_until],
        )?;
        Ok(())
    })
}

/// Implementation for `unsuppress_finding`. Drops the suppression row
/// and clears the matching findings' `suppressed` / `suppress_*`
/// columns so the active findings re-surface.
pub fn unsuppress_finding_inner(
    handle: &IndexHandle,
    component_id: &str,
    pattern: &str,
) -> crate::index::Result<()> {
    handle.write(|conn| {
        conn.execute(
            "DELETE FROM security_finding_suppression
             WHERE component_id = ?1 AND pattern = ?2",
            params![component_id, pattern],
        )?;
        conn.execute(
            "UPDATE security_finding
             SET suppressed = 0,
                 suppress_reason = NULL,
                 suppress_until = NULL
             WHERE component_id = ?1 AND pattern = ?2",
            params![component_id, pattern],
        )?;
        Ok(())
    })
}

/// Implementation for `get_findings_count_per_component`. One GROUP BY
/// pass per component, with a per-severity sub-aggregation so the UI
/// can pick the badge colour without a second round-trip.
pub fn findings_count_per_component_inner(
    handle: &IndexHandle,
) -> crate::index::Result<Vec<ComponentFindingsCount>> {
    handle.read(|conn| {
        let mut stmt = conn.prepare(
            "SELECT component_id, severity, COUNT(*)
             FROM security_finding
             WHERE suppressed = 0
             GROUP BY component_id, severity",
        )?;
        let rows = stmt.query_map([], |row| {
            let component_id: String = row.get(0)?;
            let severity_str: String = row.get(1)?;
            let count: i64 = row.get(2)?;
            Ok((component_id, severity_str, count))
        })?;

        // Build per-component map. We resolve to `Vec` ordered by
        // component_id ASC so the response is deterministic - tests can
        // assert on indices, and clients that render the same data need
        // not re-sort.
        let mut by_id: std::collections::BTreeMap<String, SeverityCounts> =
            std::collections::BTreeMap::new();
        for row in rows {
            let (component_id, severity_str, count) = row?;
            let entry = by_id.entry(component_id).or_default();
            let count_u32 = u32::try_from(count).unwrap_or(u32::MAX);
            match parse_severity(&severity_str) {
                Severity::Low => entry.low = entry.low.saturating_add(count_u32),
                Severity::Medium => entry.medium = entry.medium.saturating_add(count_u32),
                Severity::High => entry.high = entry.high.saturating_add(count_u32),
                Severity::Critical => entry.critical = entry.critical.saturating_add(count_u32),
            }
        }

        let mut out = Vec::with_capacity(by_id.len());
        for (component_id, by_severity) in by_id {
            let total = by_severity
                .low
                .saturating_add(by_severity.medium)
                .saturating_add(by_severity.high)
                .saturating_add(by_severity.critical);
            out.push(ComponentFindingsCount {
                component_id,
                total,
                by_severity,
            });
        }
        Ok(out)
    })
}

/// Implementation for `get_security_summary`. Drives the Sidebar Health
/// group's "Security issues" entry + the Security view header.
pub fn security_summary_inner(handle: &IndexHandle) -> crate::index::Result<SecuritySummary> {
    handle.read(|conn| {
        let mut by_severity = SeverityCounts::default();
        let mut by_category = SecurityCategoryCounts::default();
        let mut total: u32 = 0;
        let suppressed: u32;

        // Severity aggregation - ignores suppressed rows because
        // suppressed counts are surfaced separately.
        {
            let mut stmt = conn.prepare(
                "SELECT severity, COUNT(*) FROM security_finding
                 WHERE suppressed = 0 GROUP BY severity",
            )?;
            let rows = stmt.query_map([], |row| {
                let severity: String = row.get(0)?;
                let count: i64 = row.get(1)?;
                Ok((severity, count))
            })?;
            for row in rows {
                let (severity_str, count) = row?;
                let n = u32::try_from(count).unwrap_or(u32::MAX);
                total = total.saturating_add(n);
                match parse_severity(&severity_str) {
                    Severity::Low => by_severity.low = by_severity.low.saturating_add(n),
                    Severity::Medium => by_severity.medium = by_severity.medium.saturating_add(n),
                    Severity::High => by_severity.high = by_severity.high.saturating_add(n),
                    Severity::Critical => {
                        by_severity.critical = by_severity.critical.saturating_add(n);
                    }
                }
            }
        }

        // Category aggregation - same exclusion rule.
        {
            let mut stmt = conn.prepare(
                "SELECT category, COUNT(*) FROM security_finding
                 WHERE suppressed = 0 GROUP BY category",
            )?;
            let rows = stmt.query_map([], |row| {
                let category: String = row.get(0)?;
                let count: i64 = row.get(1)?;
                Ok((category, count))
            })?;
            for row in rows {
                let (category_str, count) = row?;
                let n = u32::try_from(count).unwrap_or(u32::MAX);
                match parse_security_category(&category_str) {
                    SecurityCategory::Secret => {
                        by_category.secret = by_category.secret.saturating_add(n);
                    }
                    SecurityCategory::McpPermission => {
                        by_category.mcp_permission = by_category.mcp_permission.saturating_add(n);
                    }
                }
            }
        }

        // Suppressed count - separate query so the totals stay clean.
        {
            let n: i64 = conn.query_row(
                "SELECT COUNT(*) FROM security_finding WHERE suppressed = 1",
                [],
                |row| row.get(0),
            )?;
            suppressed = u32::try_from(n).unwrap_or(u32::MAX);
        }

        Ok(SecuritySummary {
            total,
            by_severity,
            by_category,
            suppressed,
        })
    })
}

// ─── Phase 3.3 - Editor save flow internals ─────────────────────────────

/// Pure-function counterpart to [`save_component`]. Tests bypass the
/// Tauri runtime by calling this directly; the override hook on the
/// last argument lets them point the home-dir guard at a tmpdir.
///
/// `home_override` is a test seam - production code passes `None` and
/// the safety guards consult the system HOME. Tests pass
/// `Some(tmpdir)` so a fixture under `/tmp` doesn't trip the
/// `OutsideHome` rule baked into `assert_safe_target`.
pub fn save_component_inner(
    handle: &IndexHandle,
    id: &str,
    content: &str,
    original_hash: &str,
    home_override: Option<&Path>,
) -> crate::index::Result<SaveOutcome> {
    // 1. Look up + classify the row. Resolve every variant (path,
    //    tool, kind, format, descriptor) before doing any IO so we
    //    can surface `Forbidden` early without partial work.
    let resolved = match resolve_save_target(handle, id)? {
        Ok(r) => r,
        Err(outcome) => return Ok(outcome),
    };

    // 2. External-change check: if the bytes on disk no longer match
    //    the snapshot the editor opened, return them so the UI can
    //    surface a diff banner.
    if let Some(outcome) = detect_external_change(&resolved.disk_path, original_hash)? {
        return Ok(outcome);
    }

    // 3. Parse + validate the new content. A parse failure becomes a
    //    single synthetic validation error so the React side has one
    //    response shape to handle. Validation errors short-circuit
    //    BEFORE we touch the atomic writer.
    let parsed = match parse_bytes(content.as_bytes(), resolved.format) {
        Ok(p) => p,
        Err(err) => {
            return Ok(SaveOutcome::ValidationFailed {
                errors: vec![ValidationError {
                    path: String::new(),
                    message: err.to_string(),
                    schema_keyword: "parse".to_owned(),
                }],
            });
        }
    };
    let outcome = validate(&parsed, resolved.tool_id, resolved.component_type);
    if !outcome.errors.is_empty() {
        return Ok(SaveOutcome::ValidationFailed {
            errors: outcome.errors,
        });
    }

    // 4. Path safety + atomic write. The guard rejects forbidden
    //    segments, symlink escapes, and writes outside any of the
    //    descriptor's `watch_paths`.
    let roots = resolve_descriptor_roots(resolved.descriptor, home_override);
    let root_refs: Vec<&Path> = roots.iter().map(PathBuf::as_path).collect();
    let allow_outside_home = home_override.is_some();
    if let Err(err) = safe_atomic_write_with_options(
        &resolved.disk_path,
        content.as_bytes(),
        &root_refs,
        allow_outside_home,
    ) {
        return Ok(SaveOutcome::Forbidden {
            reason: err.to_string(),
        });
    }

    // 5. Re-index through the same upsert path the watcher uses, so
    //    secret detection + validator + FTS body all converge on the
    //    new bytes the same way they would for an external edit.
    let Some(component_root) =
        find_root_for_path(resolved.descriptor, &resolved.disk_path, home_override)
    else {
        return Ok(SaveOutcome::Forbidden {
            reason: format!("no component root matches {}", resolved.disk_path.display()),
        });
    };
    let component_name = component_name_for_path(&resolved.disk_path, &component_root);
    upsert_component(
        handle,
        resolved.descriptor,
        &component_root,
        &resolved.disk_path,
        &component_name,
    )?;

    Ok(SaveOutcome::Saved {
        new_hash: parsed.hash.clone(),
        mtime: file_mtime_secs(&resolved.disk_path),
    })
}

/// Resolved save target: every typed value the rest of the pipeline
/// needs after we've finished the index lookup + classification.
struct ResolvedSaveTarget<'a> {
    disk_path: PathBuf,
    tool_id: ToolId,
    component_type: ComponentType,
    format: Format,
    descriptor: &'a ToolDescriptor,
}

/// Look up the row + classify the on-wire enums. Returns either a
/// [`ResolvedSaveTarget`] (Ok variant) or a short-circuit
/// [`SaveOutcome::Forbidden`] (Err variant) - the inner Result is the
/// `Result<crate::index::Result<...>>` that propagates `SQLite` errors.
fn resolve_save_target<'a>(
    handle: &IndexHandle,
    id: &'a str,
) -> crate::index::Result<std::result::Result<ResolvedSaveTarget<'a>, SaveOutcome>> {
    let lookup: Option<SaveLookup> = handle.read(|conn| {
        let row: Option<SaveLookup> = conn
            .query_row(
                "SELECT path, tool, type, format FROM component WHERE id = ?1",
                params![id],
                |row| {
                    Ok(SaveLookup {
                        path: row.get::<_, String>(0)?,
                        tool: row.get::<_, String>(1)?,
                        kind: row.get::<_, String>(2)?,
                        format: row.get::<_, String>(3)?,
                    })
                },
            )
            .map(Some)
            .or_else(|err| match err {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                other => Err(other),
            })?;
        Ok(row)
    })?;

    let Some(lookup) = lookup else {
        return Ok(Err(SaveOutcome::Forbidden {
            reason: format!("component {id} not found"),
        }));
    };
    let Some(tool_id) = parse_tool_id(&lookup.tool) else {
        return Ok(Err(SaveOutcome::Forbidden {
            reason: format!("unknown tool `{}` for {id}", lookup.tool),
        }));
    };
    let Some(component_type) = parse_component_type(&lookup.kind) else {
        return Ok(Err(SaveOutcome::Forbidden {
            reason: format!("unknown component type `{}` for {id}", lookup.kind),
        }));
    };
    let Some(format) = parse_format(&lookup.format) else {
        return Ok(Err(SaveOutcome::Forbidden {
            reason: format!("unknown format `{}` for {id}", lookup.format),
        }));
    };
    let Some(descriptor) = registry_descriptors().iter().find(|d| d.id == tool_id) else {
        return Ok(Err(SaveOutcome::Forbidden {
            reason: format!("no descriptor for tool {tool_id:?}"),
        }));
    };

    Ok(Ok(ResolvedSaveTarget {
        disk_path: PathBuf::from(&lookup.path),
        tool_id,
        component_type,
        format,
        descriptor,
    }))
}

/// Compare the on-disk hash with the editor's snapshot. Returns
/// `Some(SaveOutcome::ExternalChange)` when they diverge, `None`
/// otherwise. Files that don't exist yet (brand-new component) skip
/// this check entirely; the safety guard handles unsafe targets later.
fn detect_external_change(
    disk_path: &Path,
    original_hash: &str,
) -> crate::index::Result<Option<SaveOutcome>> {
    if !disk_path.exists() {
        return Ok(None);
    }
    let disk_bytes = std::fs::read(disk_path).map_err(|err| {
        crate::index::IndexError::Io(std::io::Error::new(
            err.kind(),
            format!("read {}: {err}", disk_path.display()),
        ))
    })?;
    let disk_hash = hash::sha256_hex(&disk_bytes);
    if disk_hash == original_hash {
        return Ok(None);
    }
    // `from_utf8_lossy` keeps the diff banner useful even for
    // non-UTF-8 files; lossy replacement chars are honest about what
    // shipped to the user and prevent a `from_utf8` panic on binary
    // payloads.
    let current_content = String::from_utf8_lossy(&disk_bytes).into_owned();
    Ok(Some(SaveOutcome::ExternalChange {
        current_hash: disk_hash,
        current_content,
    }))
}

/// Stat `path` and return its mtime as a Unix-epoch seconds value, or
/// `0` when the metadata is unavailable. Mirrors the rule used by
/// `index::upsert::file_mtime`.
fn file_mtime_secs(path: &Path) -> i64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .and_then(|d| i64::try_from(d.as_secs()).ok())
        .unwrap_or(0)
}

/// One-trip combined-detail lookup for the Editor open path.
///
/// Reads the component row, then reads the file off disk. Failures
/// from the read path bubble out as the same typed `IpcError` that
/// `read_component_raw` returns; failures on the index path collapse
/// into `IpcError::Index` so the React side has a single error shape.
pub fn get_component_with_raw_inner(
    handle: &IndexHandle,
    id: &str,
) -> Result<Option<ComponentDetailWithRaw>, IpcError> {
    let detail = get_component_inner(handle, id).map_err(|err| IpcError::Index {
        message: err.to_string(),
    })?;
    let Some(detail) = detail else {
        return Ok(None);
    };

    // Reuse the existing typed-error read path so size caps, NotFound,
    // InvalidUtf8 etc. all surface verbatim.
    let raw = read_component_raw_inner(handle, id)?;
    let hash = hash::sha256_hex(raw.as_bytes());
    Ok(Some(ComponentDetailWithRaw { detail, raw, hash }))
}

/// Row payload for the save-time component lookup. Tiny struct because
/// the columns we need are few and we want named fields rather than a
/// 4-tuple.
struct SaveLookup {
    path: String,
    tool: String,
    kind: String,
    format: String,
}

/// Resolve a tool descriptor's `watch_paths` into absolute `PathBuf`s
/// against the current (or overridden) HOME. Empty results just yield
/// an empty roots slice - the safety guard then refuses the write
/// because no root contains the target.
fn resolve_descriptor_roots(
    descriptor: &ToolDescriptor,
    home_override: Option<&Path>,
) -> Vec<PathBuf> {
    let home = home_override.map(Path::to_path_buf).or_else(dirs::home_dir);
    descriptor
        .watch_paths
        .iter()
        .map(|raw| expand_home(raw, home.as_deref()))
        .collect()
}

/// Find the `ComponentRoot` whose glob pattern covers `path`. Mirrors
/// the classification logic but scoped to a single descriptor (we
/// already know the tool from the index row).
fn find_root_for_path(
    descriptor: &ToolDescriptor,
    path: &Path,
    home_override: Option<&Path>,
) -> Option<ComponentRoot> {
    use globset::Glob;
    let home = home_override.map(Path::to_path_buf).or_else(dirs::home_dir);
    for root in &descriptor.component_roots {
        let pattern = expand_home(&root.path_pattern, home.as_deref());
        let Some(pattern_str) = pattern.to_str() else {
            continue;
        };
        let Ok(glob) = Glob::new(pattern_str) else {
            continue;
        };
        if glob.compile_matcher().is_match(path) {
            return Some(root.clone());
        }
    }
    None
}

/// Compute the component identity name for a path under a known root.
/// Folder roots (e.g. `.../skills/foo/SKILL.md`) yield the parent
/// directory name; file roots use the file stem.
fn component_name_for_path(path: &Path, root: &ComponentRoot) -> String {
    if root.is_folder {
        if let Some(parent) = path.parent() {
            if let Some(name) = parent.file_name() {
                return name.to_string_lossy().into_owned();
            }
        }
    }
    path.file_stem().map_or_else(
        || path.to_string_lossy().into_owned(),
        |s| s.to_string_lossy().into_owned(),
    )
}

// ─── Helpers ────────────────────────────────────────────────────────────

fn row_to_summary(row: &rusqlite::Row<'_>) -> rusqlite::Result<ComponentSummary> {
    let id: String = row.get(0)?;
    let name: String = row.get(1)?;
    let display_name: Option<String> = row.get(2)?;
    let description: Option<String> = row.get(3)?;
    let type_str: String = row.get(4)?;
    let tool_str: String = row.get(5)?;
    let scope_str: String = row.get(6)?;
    let format_str: String = row.get(7)?;
    let path: String = row.get(8)?;
    let size: i64 = row.get(9)?;
    let mtime: i64 = row.get(10)?;
    let hash: String = row.get(11)?;
    let parse_errors: Option<String> = row.get(12)?;
    let last_used_at: Option<i64> = row.get(13)?;
    let use_count: i64 = row.get(14)?;

    Ok(ComponentSummary {
        id,
        name,
        display_name,
        description,
        kind: parse_component_type(&type_str).unwrap_or(ComponentType::Settings),
        tool: parse_tool_id(&tool_str).unwrap_or(ToolId::ClaudeCode),
        scope: parse_scope(&scope_str).unwrap_or(Scope::User),
        format: parse_format(&format_str).unwrap_or(Format::Json),
        path,
        size: u64::try_from(size).unwrap_or(0),
        mtime,
        hash,
        has_parse_errors: parse_errors.is_some(),
        last_used_at,
        use_count: u32::try_from(use_count).unwrap_or(0),
    })
}

fn row_to_detail(row: &rusqlite::Row<'_>) -> rusqlite::Result<ComponentDetail> {
    let summary = row_to_summary(row)?;
    let parsed_json: Option<String> = row.get(15)?;
    let origin: String = row.get(16)?;
    let plugin_id: Option<String> = row.get(17)?;
    // `parse_errors` already lives on the summary as a bool; surface
    // the JSON string here for the detail consumer.
    let parse_errors: Option<String> = row.get(12)?;
    Ok(ComponentDetail {
        summary,
        parsed_json,
        parse_errors,
        origin,
        plugin_id,
    })
}

fn parse_format(s: &str) -> Option<Format> {
    match s {
        "json" => Some(Format::Json),
        "toml" => Some(Format::Toml),
        "yaml" => Some(Format::Yaml),
        "markdown" => Some(Format::Markdown),
        "markdownFrontmatter" => Some(Format::MarkdownFrontmatter),
        "mdc" => Some(Format::Mdc),
        "jsonl" => Some(Format::Jsonl),
        "sqlite" => Some(Format::Sqlite),
        "binary" => Some(Format::Binary),
        _ => None,
    }
}

/// Convert user-supplied search text into a safe FTS5 MATCH expression.
///
/// FTS5 has a small DSL with `AND`, `OR`, `NEAR`, column qualifiers,
/// and double-quote phrase grouping. Naively passing user text would
/// fail on any input containing `:`, `*`, `(`, or operators. We split
/// on whitespace, double-quote each non-empty term (escaping internal
/// double quotes by doubling them per FTS5 syntax), and join with
/// space - giving us an implicit AND of phrase tokens that always
/// parses.
fn sanitize_fts_query(text: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    for token in text.split_whitespace() {
        if token.is_empty() {
            continue;
        }
        // FTS5 double-quote escape: `""` inside a quoted phrase.
        let escaped = token.replace('"', "\"\"");
        parts.push(format!("\"{escaped}\""));
    }
    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::upsert::upsert_component;
    use crate::registry::tools;
    use std::fs;
    use tempfile::tempdir;

    fn seed_skill(handle: &IndexHandle, home: &std::path::Path, name: &str, body: &str) {
        let dir = home.join(".claude").join("skills").join(name);
        fs::create_dir_all(&dir).expect("mkdir");
        let path = dir.join("SKILL.md");
        fs::write(
            &path,
            format!("---\nname: {name}\ndescription: {name} skill\n---\n{body}"),
        )
        .expect("write");
        let descriptor = tools::claude_code();
        let root = descriptor
            .component_roots
            .iter()
            .find(|r| r.component_type == ComponentType::Skill)
            .expect("skill root");
        upsert_component(handle, &descriptor, root, &path, name).expect("upsert");
    }

    fn seed_codex_settings(handle: &IndexHandle, home: &std::path::Path) {
        let dir = home.join(".codex");
        fs::create_dir_all(&dir).expect("mkdir");
        let path = dir.join("config.toml");
        fs::write(&path, b"key = \"value\"\n").expect("write");
        let descriptor = tools::codex();
        let root = descriptor
            .component_roots
            .iter()
            .find(|r| r.component_type == ComponentType::Settings)
            .expect("settings root");
        upsert_component(handle, &descriptor, root, &path, "config").expect("upsert");
    }

    #[test]
    fn list_components_filters_by_tool() {
        let home = tempdir().expect("tempdir");
        let handle = IndexHandle::open_in_memory().expect("open");
        seed_skill(&handle, home.path(), "alpha", "the cat sat on the mat\n");
        seed_codex_settings(&handle, home.path());

        let claude_only = list_components_inner(
            &handle,
            &ComponentFilter {
                tool_id: Some(ToolId::ClaudeCode),
                ..ComponentFilter::default()
            },
        )
        .expect("list");
        assert_eq!(claude_only.len(), 1);
        assert_eq!(claude_only[0].tool, ToolId::ClaudeCode);

        let everything = list_components_inner(&handle, &ComponentFilter::default()).unwrap();
        assert_eq!(everything.len(), 2);
    }

    #[test]
    fn search_returns_matches() {
        let home = tempdir().expect("tempdir");
        let handle = IndexHandle::open_in_memory().expect("open");
        seed_skill(&handle, home.path(), "alpha", "the cat sat on the mat\n");

        let hits = search_inner(
            &handle,
            &SearchQuery {
                text: "cat".to_owned(),
                limit: None,
                tool_id: None,
                kind: None,
                scope: None,
            },
        )
        .expect("search");
        assert_eq!(hits.len(), 1);
        assert!(hits[0].snippet.contains("cat"));
    }

    #[test]
    fn search_empty_query_returns_empty() {
        let handle = IndexHandle::open_in_memory().expect("open");
        let hits = search_inner(
            &handle,
            &SearchQuery {
                text: "   ".to_owned(),
                limit: None,
                tool_id: None,
                kind: None,
                scope: None,
            },
        )
        .unwrap();
        assert!(hits.is_empty());
    }

    #[test]
    fn get_component_returns_detail() {
        let home = tempdir().expect("tempdir");
        let handle = IndexHandle::open_in_memory().expect("open");
        seed_skill(&handle, home.path(), "foo", "body\n");
        let detail = get_component_inner(&handle, "aseye://claude-code/user/skill/foo")
            .unwrap()
            .expect("must exist");
        assert_eq!(detail.summary.name, "foo");
        assert_eq!(detail.summary.kind, ComponentType::Skill);
        assert_eq!(detail.origin, "userCreated");
        assert!(detail.parsed_json.is_some());
    }

    #[test]
    fn get_component_returns_none_for_missing_id() {
        let handle = IndexHandle::open_in_memory().expect("open");
        let result = get_component_inner(&handle, "aseye://nope/x/y/z").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn health_summary_counts_by_tool_kind() {
        let home = tempdir().expect("tempdir");
        let handle = IndexHandle::open_in_memory().expect("open");
        seed_skill(&handle, home.path(), "foo", "body\n");
        seed_skill(&handle, home.path(), "bar", "body\n");
        seed_codex_settings(&handle, home.path());

        let summary = health_summary_inner(&handle).expect("health");
        assert_eq!(summary.total_components, 3);
        assert_eq!(summary.total_parse_errors, 0);
        let claude_skills = summary
            .by_tool_kind
            .iter()
            .find(|h| h.tool == ToolId::ClaudeCode && h.kind == ComponentType::Skill)
            .expect("claude/skill row");
        assert_eq!(claude_skills.count, 2);
    }

    #[test]
    fn list_components_query_filter_uses_like() {
        let home = tempdir().expect("tempdir");
        let handle = IndexHandle::open_in_memory().expect("open");
        seed_skill(&handle, home.path(), "alpha", "body\n");
        seed_skill(&handle, home.path(), "beta", "body\n");

        let only_alpha = list_components_inner(
            &handle,
            &ComponentFilter {
                query: Some("alpha".to_owned()),
                ..ComponentFilter::default()
            },
        )
        .unwrap();
        assert_eq!(only_alpha.len(), 1);
        assert_eq!(only_alpha[0].name, "alpha");
    }

    #[test]
    fn sanitize_fts_query_quotes_terms() {
        assert_eq!(sanitize_fts_query("cat"), "\"cat\"");
        assert_eq!(sanitize_fts_query("the cat"), "\"the\" \"cat\"");
        // Embedded double quote becomes `""`.
        assert_eq!(sanitize_fts_query("a\"b"), "\"a\"\"b\"");
        assert_eq!(sanitize_fts_query("   "), "");
    }

    #[test]
    fn read_component_raw_returns_file_text() {
        let home = tempdir().expect("tempdir");
        let handle = IndexHandle::open_in_memory().expect("open");
        seed_skill(&handle, home.path(), "spec", "the spec body\n");

        let raw =
            read_component_raw_inner(&handle, "aseye://claude-code/user/skill/spec").expect("read");
        // The seeded SKILL.md carries frontmatter then the body the
        // helper passed in - both round-trip through the on-disk read.
        assert!(raw.contains("name: spec"));
        assert!(raw.contains("the spec body"));
    }

    #[test]
    fn read_component_raw_rejects_missing_id() {
        let handle = IndexHandle::open_in_memory().expect("open");
        let err = read_component_raw_inner(&handle, "aseye://nope/x/y/z")
            .expect_err("missing id must error");
        assert!(matches!(err, IpcError::NotFound { .. }));
    }

    #[test]
    fn read_component_raw_rejects_oversized_files() {
        let home = tempdir().expect("tempdir");
        let handle = IndexHandle::open_in_memory().expect("open");
        // Seed a normal-size component first so the index has a row
        // we can address by id.
        seed_skill(&handle, home.path(), "fat", "small\n");

        // Then overwrite the file on disk with > 5 MiB of content. The
        // index still points to the same path; the read command should
        // refuse based on the on-disk size, not the cached size.
        let path = home
            .path()
            .join(".claude")
            .join("skills")
            .join("fat")
            .join("SKILL.md");
        let huge = vec![b'a'; usize::try_from(READ_RAW_HARD_LIMIT).expect("cap fits usize") + 1024];
        fs::write(&path, &huge).expect("inflate");

        let err = read_component_raw_inner(&handle, "aseye://claude-code/user/skill/fat")
            .expect_err("oversized must error");
        match err {
            IpcError::PayloadTooLarge { size, cap } => {
                assert!(size > READ_RAW_HARD_LIMIT, "size = {size}");
                assert_eq!(cap, READ_RAW_HARD_LIMIT);
            }
            other => panic!("expected PayloadTooLarge, got {other:?}"),
        }
    }

    // ─── Phase 7.3 - security IPC tests ─────────────────────────────────

    /// Seed a finding row directly into the index. Bypasses
    /// `persist_findings` so a test can stage a precise mix of severity
    /// / category / suppression states without driving the scanner.
    ///
    /// The component row is created on demand if it doesn't exist.
    #[allow(clippy::too_many_arguments)]
    fn seed_finding(
        handle: &IndexHandle,
        component_id: &str,
        component_name: &str,
        component_path: &str,
        finding_id: &str,
        category: SecurityCategory,
        pattern: &str,
        severity: Severity,
        detected_at: i64,
        suppressed: bool,
    ) {
        handle
            .write(|c| {
                // Idempotently insert the owning component row.
                c.execute(
                    "INSERT OR IGNORE INTO component (
                        id, type, tool, scope, origin, name, path, format,
                        hash, updated_at
                     ) VALUES (?1, 'settings', 'claude-code', 'user',
                              'userCreated', ?2, ?3, 'json', 'h', 0)",
                    params![component_id, component_name, component_path],
                )?;
                c.execute(
                    "INSERT INTO security_finding (
                        id, component_id, category, pattern, severity,
                        file_path, line, source_label, redacted_preview,
                        detected_at, suppressed, suppress_reason,
                        suppress_until, evidence_json
                     ) VALUES (
                        ?1, ?2, ?3, ?4, ?5, ?6, NULL, 'body', 'abcd…wxyz',
                        ?7, ?8, NULL, NULL, NULL
                     )",
                    params![
                        finding_id,
                        component_id,
                        category.as_str(),
                        pattern,
                        severity.as_str(),
                        component_path,
                        detected_at,
                        i64::from(suppressed),
                    ],
                )?;
                Ok(())
            })
            .expect("seed finding");
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn list_security_findings_filters_and_sorts() {
        let handle = IndexHandle::open_in_memory().expect("open");
        seed_finding(
            &handle,
            "aseye://t/c1",
            "c1",
            "/tmp/c1",
            "f-1",
            SecurityCategory::Secret,
            "anthropic-key",
            Severity::Critical,
            300,
            false,
        );
        seed_finding(
            &handle,
            "aseye://t/c1",
            "c1",
            "/tmp/c1",
            "f-2",
            SecurityCategory::Secret,
            "openai-key",
            Severity::High,
            200,
            false,
        );
        seed_finding(
            &handle,
            "aseye://t/c2",
            "c2",
            "/tmp/c2",
            "f-3",
            SecurityCategory::McpPermission,
            "postgres-mcp-write",
            Severity::Medium,
            100,
            false,
        );
        seed_finding(
            &handle,
            "aseye://t/c3",
            "c3",
            "/tmp/c3",
            "f-4",
            SecurityCategory::Secret,
            "generic-secret",
            Severity::Low,
            50,
            true,
        );

        // Default filter: all four findings, severity DESC then
        // detected_at DESC.
        let all = list_security_findings_inner(&handle, &SecurityFilter::default()).unwrap();
        assert_eq!(all.len(), 4);
        assert_eq!(all[0].pattern, "anthropic-key");
        assert_eq!(all[0].severity, Severity::Critical);
        assert_eq!(all[1].pattern, "openai-key");

        // Severity filter.
        let high_only = list_security_findings_inner(
            &handle,
            &SecurityFilter {
                severity: Some(Severity::High),
                ..SecurityFilter::default()
            },
        )
        .unwrap();
        assert_eq!(high_only.len(), 1);
        assert_eq!(high_only[0].pattern, "openai-key");

        // Category filter.
        let mcp_only = list_security_findings_inner(
            &handle,
            &SecurityFilter {
                category: Some(SecurityCategory::McpPermission),
                ..SecurityFilter::default()
            },
        )
        .unwrap();
        assert_eq!(mcp_only.len(), 1);
        assert_eq!(mcp_only[0].pattern, "postgres-mcp-write");

        // Component filter.
        let only_c1 = list_security_findings_inner(
            &handle,
            &SecurityFilter {
                component_id: Some("aseye://t/c1".to_owned()),
                ..SecurityFilter::default()
            },
        )
        .unwrap();
        assert_eq!(only_c1.len(), 2);

        // Suppressed-only.
        let suppressed_only = list_security_findings_inner(
            &handle,
            &SecurityFilter {
                suppressed: Some(true),
                ..SecurityFilter::default()
            },
        )
        .unwrap();
        assert_eq!(suppressed_only.len(), 1);
        assert_eq!(suppressed_only[0].pattern, "generic-secret");
        assert!(suppressed_only[0].suppressed);

        // Active-only.
        let active_only = list_security_findings_inner(
            &handle,
            &SecurityFilter {
                suppressed: Some(false),
                ..SecurityFilter::default()
            },
        )
        .unwrap();
        assert_eq!(active_only.len(), 3);
    }

    #[test]
    fn suppress_finding_flips_active_rows_and_persists_until() {
        let handle = IndexHandle::open_in_memory().expect("open");
        seed_finding(
            &handle,
            "aseye://t/c1",
            "c1",
            "/tmp/c1",
            "f-1",
            SecurityCategory::Secret,
            "anthropic-key",
            Severity::Critical,
            0,
            false,
        );

        let now_ms: i64 = 1_000_000;
        suppress_finding_inner(
            &handle,
            "aseye://t/c1",
            "anthropic-key",
            Some("ack"),
            Some(30),
            now_ms,
        )
        .expect("suppress");

        // Suppression row exists.
        let count: i64 = handle
            .read(|c| {
                Ok(c.query_row(
                    "SELECT COUNT(*) FROM security_finding_suppression
                     WHERE component_id = ?1 AND pattern = ?2",
                    params!["aseye://t/c1", "anthropic-key"],
                    |row| row.get(0),
                )?)
            })
            .unwrap();
        assert_eq!(count, 1);

        // Finding row's suppressed flag flipped to 1; suppress_until
        // matches now + 30 days in ms.
        let (flag, until): (i64, Option<i64>) = handle
            .read(|c| {
                Ok(c.query_row(
                    "SELECT suppressed, suppress_until FROM security_finding
                     WHERE id = 'f-1'",
                    [],
                    |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Option<i64>>(1)?)),
                )?)
            })
            .unwrap();
        assert_eq!(flag, 1);
        let expected_until = now_ms + 30 * 86_400_000;
        assert_eq!(until, Some(expected_until));

        // Re-suppress with no TTL clears suppress_until back to NULL.
        suppress_finding_inner(
            &handle,
            "aseye://t/c1",
            "anthropic-key",
            Some("perma"),
            None,
            now_ms,
        )
        .expect("re-suppress");
        let until2: Option<i64> = handle
            .read(|c| {
                Ok(c.query_row(
                    "SELECT suppress_until FROM security_finding WHERE id = 'f-1'",
                    [],
                    |row| row.get(0),
                )?)
            })
            .unwrap();
        assert_eq!(until2, None);
    }

    #[test]
    fn unsuppress_finding_removes_row_and_clears_flag() {
        let handle = IndexHandle::open_in_memory().expect("open");
        seed_finding(
            &handle,
            "aseye://t/c1",
            "c1",
            "/tmp/c1",
            "f-1",
            SecurityCategory::Secret,
            "anthropic-key",
            Severity::Critical,
            0,
            false,
        );
        suppress_finding_inner(&handle, "aseye://t/c1", "anthropic-key", None, None, 0)
            .expect("suppress");
        unsuppress_finding_inner(&handle, "aseye://t/c1", "anthropic-key").expect("unsuppress");

        let supp_count: i64 = handle
            .read(|c| {
                Ok(c.query_row(
                    "SELECT COUNT(*) FROM security_finding_suppression",
                    [],
                    |row| row.get(0),
                )?)
            })
            .unwrap();
        assert_eq!(supp_count, 0);
        let flag: i64 = handle
            .read(|c| {
                Ok(c.query_row(
                    "SELECT suppressed FROM security_finding WHERE id = 'f-1'",
                    [],
                    |row| row.get(0),
                )?)
            })
            .unwrap();
        assert_eq!(flag, 0);
    }

    #[test]
    fn findings_count_per_component_aggregates_severity() {
        let handle = IndexHandle::open_in_memory().expect("open");
        seed_finding(
            &handle,
            "aseye://t/c1",
            "c1",
            "/tmp/c1",
            "f-1",
            SecurityCategory::Secret,
            "anthropic-key",
            Severity::Critical,
            0,
            false,
        );
        seed_finding(
            &handle,
            "aseye://t/c1",
            "c1",
            "/tmp/c1",
            "f-2",
            SecurityCategory::Secret,
            "openai-key",
            Severity::High,
            0,
            false,
        );
        seed_finding(
            &handle,
            "aseye://t/c2",
            "c2",
            "/tmp/c2",
            "f-3",
            SecurityCategory::Secret,
            "generic-secret",
            Severity::Medium,
            0,
            false,
        );
        // Suppressed row must NOT contribute to the per-component count.
        seed_finding(
            &handle,
            "aseye://t/c2",
            "c2",
            "/tmp/c2",
            "f-4",
            SecurityCategory::Secret,
            "low-secret",
            Severity::Low,
            0,
            true,
        );

        let counts = findings_count_per_component_inner(&handle).unwrap();
        assert_eq!(counts.len(), 2);
        let c1 = &counts[0];
        assert_eq!(c1.component_id, "aseye://t/c1");
        assert_eq!(c1.total, 2);
        assert_eq!(c1.by_severity.critical, 1);
        assert_eq!(c1.by_severity.high, 1);
        let c2 = &counts[1];
        assert_eq!(c2.total, 1);
        assert_eq!(c2.by_severity.medium, 1);
        assert_eq!(c2.by_severity.low, 0);
    }

    #[test]
    fn security_summary_aggregates_active_and_suppressed() {
        let handle = IndexHandle::open_in_memory().expect("open");
        seed_finding(
            &handle,
            "aseye://t/c1",
            "c1",
            "/tmp/c1",
            "f-1",
            SecurityCategory::Secret,
            "anthropic-key",
            Severity::Critical,
            0,
            false,
        );
        seed_finding(
            &handle,
            "aseye://t/c2",
            "c2",
            "/tmp/c2",
            "f-2",
            SecurityCategory::McpPermission,
            "postgres-mcp-write",
            Severity::High,
            0,
            false,
        );
        seed_finding(
            &handle,
            "aseye://t/c3",
            "c3",
            "/tmp/c3",
            "f-3",
            SecurityCategory::Secret,
            "low-secret",
            Severity::Low,
            0,
            true,
        );

        let summary = security_summary_inner(&handle).unwrap();
        assert_eq!(
            summary.total, 2,
            "suppressed row should not count toward total"
        );
        assert_eq!(summary.by_severity.critical, 1);
        assert_eq!(summary.by_severity.high, 1);
        assert_eq!(summary.by_severity.low, 0);
        assert_eq!(summary.by_category.secret, 1);
        assert_eq!(summary.by_category.mcp_permission, 1);
        assert_eq!(summary.suppressed, 1);
    }

    // ─── Phase 3.3 - save_component / get_component_with_raw ────────────

    /// Build a Claude Code descriptor whose `watch_paths` and skill
    /// root are rooted under `home` rather than `~/`. Tests use this
    /// so the in-memory descriptor matches what the safety guard
    /// canonicalises against (a real tmpdir).
    fn home_rooted_claude_descriptor(home: &std::path::Path) -> ToolDescriptor {
        let mut descriptor = crate::registry::tools::claude_code();
        let claude = home.join(".claude");
        descriptor.watch_paths = vec![
            claude.to_string_lossy().into_owned(),
            home.join(".claude.json").to_string_lossy().into_owned(),
        ];
        for root in &mut descriptor.component_roots {
            root.path_pattern =
                root.path_pattern
                    .replacen("~/", &format!("{}/", home.display()), 1);
        }
        descriptor
    }

    /// Seed a Claude Code skill on disk + index, returning the
    /// component id and absolute path. `body` controls the post-
    /// frontmatter text so tests can drive the parser through valid
    /// and invalid shapes.
    fn seed_skill_for_save(
        handle: &IndexHandle,
        home: &std::path::Path,
        name: &str,
        body: &str,
    ) -> (String, std::path::PathBuf) {
        let descriptor = home_rooted_claude_descriptor(home);
        let dir = home.join(".claude").join("skills").join(name);
        fs::create_dir_all(&dir).expect("mkdir");
        let path = dir.join("SKILL.md");
        fs::write(
            &path,
            format!("---\nname: {name}\ndescription: {name} skill\n---\n{body}"),
        )
        .expect("write");
        let root = descriptor
            .component_roots
            .iter()
            .find(|r| r.component_type == ComponentType::Skill)
            .expect("skill root");
        let outcome = upsert_component(handle, &descriptor, root, &path, name).expect("upsert");
        (outcome.id, path)
    }

    #[test]
    fn save_component_writes_atomically_and_reindexes() {
        let home = tempdir().expect("tempdir");
        let handle = IndexHandle::open_in_memory().expect("open");
        let (id, path) = seed_skill_for_save(&handle, home.path(), "alpha", "first body\n");

        // Capture the original on-disk hash so the save guards pass.
        let original_bytes = fs::read(&path).expect("read original");
        let original_hash = crate::parser::hash::sha256_hex(&original_bytes);

        let new_content = "---\nname: alpha\ndescription: updated skill\n---\nrewritten body\n";
        let outcome =
            save_component_inner(&handle, &id, new_content, &original_hash, Some(home.path()))
                .expect("save");

        match outcome {
            SaveOutcome::Saved { new_hash, .. } => {
                let on_disk = fs::read_to_string(&path).expect("read after save");
                assert_eq!(on_disk, new_content);
                assert_eq!(
                    new_hash,
                    crate::parser::hash::sha256_hex(new_content.as_bytes())
                );
            }
            other => panic!("expected Saved, got {other:?}"),
        }

        // Index reflects the rewritten description.
        let detail = get_component_inner(&handle, &id)
            .expect("detail")
            .expect("must exist");
        assert_eq!(detail.summary.description.as_deref(), Some("updated skill"));
    }

    #[test]
    fn save_component_rejects_external_change() {
        let home = tempdir().expect("tempdir");
        let handle = IndexHandle::open_in_memory().expect("open");
        let (id, path) = seed_skill_for_save(&handle, home.path(), "beta", "original body\n");

        // Pretend the user opened the editor on the original
        // contents but a third party modified the file between open
        // and save.
        let original_bytes = fs::read(&path).expect("read original");
        let stale_hash = crate::parser::hash::sha256_hex(&original_bytes);
        fs::write(
            &path,
            "---\nname: beta\ndescription: external edit\n---\nthird-party body\n",
        )
        .expect("external write");

        let new_content = "---\nname: beta\ndescription: my edit\n---\nmine\n";
        let outcome =
            save_component_inner(&handle, &id, new_content, &stale_hash, Some(home.path()))
                .expect("save");

        match outcome {
            SaveOutcome::ExternalChange {
                current_hash,
                current_content,
            } => {
                let on_disk = fs::read(&path).expect("read");
                assert_eq!(current_hash, crate::parser::hash::sha256_hex(&on_disk));
                assert!(
                    current_content.contains("third-party body"),
                    "current_content should reflect the external edit, got: {current_content}"
                );
                // The original file was NOT overwritten by our save
                // attempt - we surfaced the conflict instead.
                assert!(!on_disk.starts_with(b"---\nname: beta\ndescription: my edit"));
            }
            other => panic!("expected ExternalChange, got {other:?}"),
        }
    }

    #[test]
    fn save_component_returns_validation_errors() {
        let home = tempdir().expect("tempdir");
        let handle = IndexHandle::open_in_memory().expect("open");
        let (id, path) = seed_skill_for_save(&handle, home.path(), "gamma", "good body\n");

        let original_bytes = fs::read(&path).expect("read original");
        let original_hash = crate::parser::hash::sha256_hex(&original_bytes);

        // Drop the required `description` from the frontmatter; the
        // bundled Claude Code skill schema marks it required.
        let bad_content = "---\nname: gamma\n---\nstill has body\n";
        let outcome =
            save_component_inner(&handle, &id, bad_content, &original_hash, Some(home.path()))
                .expect("save");

        match outcome {
            SaveOutcome::ValidationFailed { errors } => {
                assert!(
                    errors.iter().any(|e| e.schema_keyword == "required"),
                    "expected required error, got {errors:?}"
                );
                // File must be unchanged on disk - validation failures
                // never write.
                let on_disk = fs::read(&path).expect("read");
                assert_eq!(on_disk, original_bytes);
            }
            other => panic!("expected ValidationFailed, got {other:?}"),
        }
    }

    #[test]
    fn save_component_blocks_path_escape() {
        // Stage a component whose indexed path lives OUTSIDE the
        // descriptor's watch_paths. The save must refuse with
        // Forbidden rather than silently writing into a stranger's
        // directory.
        let home = tempdir().expect("tempdir");
        let elsewhere = tempdir().expect("elsewhere");
        let handle = IndexHandle::open_in_memory().expect("open");

        // Seed a real component first so the index has the row.
        let (id, real_path) = seed_skill_for_save(&handle, home.path(), "delta", "body\n");

        // Now relocate the row's `path` column to a file outside any
        // tool root. We manipulate the index directly because no
        // legitimate code path produces this state - it's a synthetic
        // adversarial case to exercise the safety guard.
        let bogus = elsewhere.path().join("evil.md");
        fs::create_dir_all(elsewhere.path()).ok();
        fs::write(&bogus, "---\nname: delta\ndescription: x\n---\noriginal\n").expect("seed bogus");
        let bogus_str = bogus.to_string_lossy().into_owned();
        handle
            .write(|conn| {
                conn.execute(
                    "UPDATE component SET path = ?1 WHERE id = ?2",
                    rusqlite::params![bogus_str, id],
                )?;
                Ok(())
            })
            .expect("relocate");

        let original_hash = crate::parser::hash::sha256_hex(&fs::read(&bogus).expect("read bogus"));
        let new_content = "---\nname: delta\ndescription: y\n---\nrewritten\n";
        let outcome =
            save_component_inner(&handle, &id, new_content, &original_hash, Some(home.path()))
                .expect("save");

        match outcome {
            SaveOutcome::Forbidden { reason } => {
                assert!(!reason.is_empty(), "reason must be human-readable");
            }
            other => panic!("expected Forbidden, got {other:?}"),
        }

        // The legitimate skill file under `home` must NOT have been
        // touched by the rejected save.
        let untouched = fs::read_to_string(&real_path).expect("read real");
        assert!(untouched.contains("body"));
    }

    #[test]
    fn get_component_with_raw_bundles_detail_and_bytes() {
        let home = tempdir().expect("tempdir");
        let handle = IndexHandle::open_in_memory().expect("open");
        let (id, path) = seed_skill_for_save(&handle, home.path(), "epsilon", "body!\n");
        let on_disk = fs::read_to_string(&path).expect("read");

        let bundle = get_component_with_raw_inner(&handle, &id)
            .expect("ok")
            .expect("must exist");
        assert_eq!(bundle.detail.summary.name, "epsilon");
        assert_eq!(bundle.raw, on_disk);
        assert_eq!(
            bundle.hash,
            crate::parser::hash::sha256_hex(on_disk.as_bytes())
        );

        // Missing id yields Ok(None) rather than an error.
        let missing = get_component_with_raw_inner(&handle, "aseye://nope/x/y/z").expect("ok");
        assert!(missing.is_none());
    }

    #[test]
    fn save_outcome_serialises_camel_case_on_the_wire() {
        // Regression guard for the `kind: ...` discriminator + camelCase
        // field names. ts-rs generates the TS bindings, but the runtime
        // wire format is decided by serde - which means the binding and
        // the actual JSON have to agree. Pin both in one assertion so a
        // future serde rename breaks loudly here rather than silently
        // in the React layer.
        let saved = SaveOutcome::Saved {
            new_hash: "abc".to_owned(),
            mtime: 42,
        };
        let s = serde_json::to_string(&saved).expect("serialise");
        assert!(s.contains("\"kind\":\"saved\""), "got: {s}");
        assert!(s.contains("\"newHash\":\"abc\""), "got: {s}");
        assert!(s.contains("\"mtime\":42"), "got: {s}");

        let ext = SaveOutcome::ExternalChange {
            current_hash: "def".to_owned(),
            current_content: "x".to_owned(),
        };
        let s = serde_json::to_string(&ext).expect("serialise");
        assert!(s.contains("\"kind\":\"externalChange\""), "got: {s}");
        assert!(s.contains("\"currentHash\":\"def\""), "got: {s}");
        assert!(s.contains("\"currentContent\":\"x\""), "got: {s}");
    }

    #[test]
    fn save_component_recovers_when_external_change_is_acked() {
        // Round-trip the conflict resolution: first attempt returns
        // ExternalChange, the caller re-issues with the new hash, the
        // second attempt saves.
        let home = tempdir().expect("tempdir");
        let handle = IndexHandle::open_in_memory().expect("open");
        let (id, path) = seed_skill_for_save(&handle, home.path(), "zeta", "body\n");

        let stale_bytes = fs::read(&path).expect("read");
        let stale_hash = crate::parser::hash::sha256_hex(&stale_bytes);
        let external = "---\nname: zeta\ndescription: ext\n---\next body\n";
        fs::write(&path, external).expect("ext write");

        let new_content = "---\nname: zeta\ndescription: mine\n---\nfinal\n";
        let first = save_component_inner(&handle, &id, new_content, &stale_hash, Some(home.path()))
            .expect("first");
        let SaveOutcome::ExternalChange { current_hash, .. } = first else {
            panic!("expected ExternalChange");
        };

        let second =
            save_component_inner(&handle, &id, new_content, &current_hash, Some(home.path()))
                .expect("second");
        match second {
            SaveOutcome::Saved { .. } => {
                let on_disk = fs::read_to_string(&path).expect("read");
                assert_eq!(on_disk, new_content);
            }
            other => panic!("expected Saved on retry, got {other:?}"),
        }
    }
}
