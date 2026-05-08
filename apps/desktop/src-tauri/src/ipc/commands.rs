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
    ComponentDetail, ComponentFilter, ComponentSummary, HealthSummary, SearchQuery, SearchResult,
    ToolHealthCount,
};
use crate::index::upsert::{parse_component_type, parse_scope, parse_tool_id};
use crate::index::IndexHandle;
use crate::pipeline::{ScanContext, ScanReport};
use crate::registry::types::{ComponentType, Format, Scope, ToolId};

/// Server-side cap for `list_components` to protect the IPC channel
/// from accidentally fetching the entire index in one call.
const LIST_COMPONENTS_HARD_LIMIT: u32 = 1000;
/// Default page size when the caller doesn't specify one.
const LIST_COMPONENTS_DEFAULT_LIMIT: u32 = 200;
/// Server-side cap for `search`.
const SEARCH_HARD_LIMIT: u32 = 200;
/// Default `search` page size.
const SEARCH_DEFAULT_LIMIT: u32 = 50;

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
}
