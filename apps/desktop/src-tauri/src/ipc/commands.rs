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

use std::sync::Arc;

use rusqlite::params;
use tauri::State;

use super::types::{
    ComponentDetail, ComponentFilter, ComponentFindingsCount, ComponentSummary, FindingSummary,
    HealthSummary, IpcError, SearchQuery, SearchResult, SecurityCategoryCounts, SecurityFilter,
    SecuritySummary, SeverityCounts, ToolHealthCount,
};
use crate::index::upsert::{parse_component_type, parse_scope, parse_tool_id};
use crate::index::IndexHandle;
use crate::pipeline::{ScanContext, ScanReport};
use crate::registry::types::{ComponentType, Format, Scope, ToolId};
use crate::security::{Category as SecurityCategory, Severity};

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
}
