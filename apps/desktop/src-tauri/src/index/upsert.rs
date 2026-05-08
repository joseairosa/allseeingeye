//! Component upsert.
//!
//! Phase 1.6 - the seam between the parser dispatch and the `SQLite`
//! index. Given a registry classification (which tool, which root, which
//! component name) and a filesystem path, we:
//!
//! 1. Read + parse the file via `parser::parse_file`.
//! 2. Compose the `aseye://<tool>/<scope>/<type>/<name>` URI.
//! 3. Compare the new SHA-256 hash with the row already stored.
//! 4. Insert / update the `component`, `component_file`, and FTS rows
//!    inside a single transaction.
//!
//! Parse errors are not fatal: we still record the component row with
//! `parse_errors` populated and an empty FTS body so the UI can surface
//! the broken file. This matches `docs/05-data-architecture.md`
//! ("Failure modes - parse error on a file").

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::error::{IndexError, Result};
use super::IndexHandle;
use crate::parser::{parse_file, ParseError, ParsedComponent};
use crate::registry::types::{ComponentRoot, ComponentType, Scope, ToolDescriptor, ToolId};

/// Outcome of an `upsert_component` call.
///
/// Distinguishes "we wrote new data" from "the file content is identical
/// to what we already had", so the watcher pipeline can short-circuit
/// downstream work (relation recompute, broadcast emission) on no-op
/// modifications.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../bindings/index/UpsertOutcome.ts")]
#[ts(rename_all = "camelCase")]
pub enum UpsertKind {
    Inserted,
    Updated,
    Unchanged,
}

/// Result returned from a successful upsert.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../bindings/index/UpsertResult.ts")]
#[ts(rename_all = "camelCase")]
pub struct UpsertOutcome {
    pub id: String,
    pub kind: UpsertKind,
    pub had_parse_error: bool,
}

/// Read `path`, parse with the format declared by `root`, and write/
/// update the corresponding `component` row.
///
/// The function is intentionally synchronous and blocking - it is meant
/// to be invoked from the index-writer task (or a `spawn_blocking`
/// closure from async code). It does not subscribe to any broadcast or
/// emit any IPC events; that is the pipeline's job.
pub fn upsert_component(
    handle: &IndexHandle,
    descriptor: &ToolDescriptor,
    root: &ComponentRoot,
    path: &Path,
    component_name: &str,
) -> Result<UpsertOutcome> {
    let id = build_uri(
        descriptor.id,
        root.scope,
        root.component_type,
        component_name,
    );

    let parse_result = parse_file(path, root.format);
    let mtime = file_mtime(path);
    let now = unix_now();

    handle.write(|conn| {
        let existing_hash: Option<String> = conn
            .query_row(
                "SELECT hash FROM component WHERE id = ?1",
                params![id],
                |row| row.get::<_, String>(0),
            )
            .optional()?;

        match parse_result {
            Ok(parsed) => write_parsed(
                conn,
                &id,
                descriptor,
                root,
                path,
                component_name,
                &parsed,
                existing_hash.as_deref(),
                mtime,
                now,
            ),
            Err(err) => write_unparseable(
                conn,
                &id,
                descriptor,
                root,
                path,
                component_name,
                &err,
                existing_hash.as_deref(),
                mtime,
                now,
            ),
        }
    })
}

/// Delete the row(s) for a component identified by URI. Returns the
/// number of rows actually removed (0 if no row was indexed for that
/// URI yet).
///
/// `component_file` and FTS rows are scrubbed alongside the parent row
/// so a delete is a true "vanish" rather than leaving FTS hits pointing
/// at a missing component.
pub fn delete_component(handle: &IndexHandle, id: &str) -> Result<usize> {
    handle.write(|conn| {
        // The component table has ON DELETE CASCADE for component_file;
        // FTS5 has no FK so we wipe it explicitly.
        conn.execute("DELETE FROM component_fts WHERE id = ?1", params![id])?;
        let removed = conn.execute("DELETE FROM component WHERE id = ?1", params![id])?;
        Ok(removed)
    })
}

/// Look up the URI for a path, if any `component_file` row references it.
/// Used by the watcher pipeline to translate a `Deleted { path }` event
/// into the right component identity. Returns `Ok(None)` when no row
/// references the path (e.g. the file was deleted before we ever
/// indexed it).
pub fn lookup_component_id_by_path(handle: &IndexHandle, path: &Path) -> Result<Option<String>> {
    let path_str = path.to_string_lossy().into_owned();
    handle.read(|conn| {
        let id: Option<String> = conn
            .query_row(
                "SELECT id FROM component WHERE path = ?1 LIMIT 1",
                params![path_str],
                |row| row.get(0),
            )
            .optional()?;
        Ok(id)
    })
}

#[allow(clippy::too_many_arguments)]
fn write_parsed(
    conn: &Connection,
    id: &str,
    descriptor: &ToolDescriptor,
    root: &ComponentRoot,
    path: &Path,
    name: &str,
    parsed: &ParsedComponent,
    existing_hash: Option<&str>,
    mtime: i64,
    now: i64,
) -> Result<UpsertOutcome> {
    if existing_hash == Some(parsed.hash.as_str()) {
        return Ok(UpsertOutcome {
            id: id.to_owned(),
            kind: UpsertKind::Unchanged,
            had_parse_error: false,
        });
    }

    let kind = if existing_hash.is_some() {
        UpsertKind::Updated
    } else {
        UpsertKind::Inserted
    };

    let path_str = path.to_string_lossy().into_owned();
    let description = extract_description(parsed);
    let display_name = extract_display_name(parsed);
    let parsed_json = render_parsed_json(parsed);
    let body = parsed.body.clone().unwrap_or_default();
    let size_i64 = i64::try_from(parsed.size).unwrap_or(i64::MAX);

    upsert_component_row(
        conn,
        id,
        descriptor.id,
        root,
        name,
        display_name.as_deref(),
        description.as_deref(),
        &path_str,
        size_i64,
        mtime,
        &parsed.hash,
        parsed_json.as_deref(),
        None, // parse_errors
        now,
    )?;

    upsert_component_file(conn, id, &path_str, file_role(root))?;
    upsert_component_fts(conn, id, name, description.as_deref().unwrap_or(""), &body)?;

    Ok(UpsertOutcome {
        id: id.to_owned(),
        kind,
        had_parse_error: false,
    })
}

#[allow(clippy::too_many_arguments)]
fn write_unparseable(
    conn: &Connection,
    id: &str,
    descriptor: &ToolDescriptor,
    root: &ComponentRoot,
    path: &Path,
    name: &str,
    err: &ParseError,
    existing_hash: Option<&str>,
    mtime: i64,
    now: i64,
) -> Result<UpsertOutcome> {
    let path_str = path.to_string_lossy().into_owned();
    let parse_errors_json = render_parse_error_json(err);
    // No usable hash for a file we couldn't parse - synthesise a
    // sentinel so we can still detect when a previously-broken file
    // becomes parseable. Keying on path + mtime is good enough to
    // detect "file changed", which is the only thing this hash drives
    // for the broken case.
    let synthetic_hash = format!("parse-error::{path_str}::{mtime}");
    let kind = if existing_hash.is_some() {
        UpsertKind::Updated
    } else {
        UpsertKind::Inserted
    };

    upsert_component_row(
        conn,
        id,
        descriptor.id,
        root,
        name,
        None,
        None,
        &path_str,
        // Size is unknown post-parse-failure; the row is for surfacing
        // the broken file in the UI, not for byte accounting.
        0,
        mtime,
        &synthetic_hash,
        None,
        Some(&parse_errors_json),
        now,
    )?;

    upsert_component_file(conn, id, &path_str, file_role(root))?;
    // Wipe any prior FTS body so a search doesn't return stale text from
    // before the file broke. The row is removed entirely - FTS5 has no
    // partial-update story and we don't need one for the broken case.
    conn.execute("DELETE FROM component_fts WHERE id = ?1", params![id])?;

    Ok(UpsertOutcome {
        id: id.to_owned(),
        kind,
        had_parse_error: true,
    })
}

#[allow(clippy::too_many_arguments)]
fn upsert_component_row(
    conn: &Connection,
    id: &str,
    tool: ToolId,
    root: &ComponentRoot,
    name: &str,
    display_name: Option<&str>,
    description: Option<&str>,
    path: &str,
    size: i64,
    mtime: i64,
    hash: &str,
    parsed_json: Option<&str>,
    parse_errors: Option<&str>,
    updated_at: i64,
) -> Result<()> {
    // Origin defaults to "userCreated" for everything outside plugins;
    // a plugin scope auto-sets it to "plugin" so we don't lose
    // provenance. This matches the taxonomy in
    // `docs/03-component-model.md`.
    let origin = if matches!(root.scope, Scope::Plugin) {
        "plugin"
    } else {
        "userCreated"
    };

    let format_str = format_to_str(root.format);
    let scope_str = scope_to_str(root.scope);
    let type_str = component_type_to_str(root.component_type);
    let tool_str = tool_id_to_str(tool);

    conn.execute(
        "INSERT INTO component (
            id, type, tool, scope, origin, plugin_id, name, display_name,
            description, path, format, size, mtime, ctime, enabled, health,
            last_used_at, use_count, parsed_json, parse_errors, hash, updated_at
         ) VALUES (
            ?1, ?2, ?3, ?4, ?5, NULL, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?12, 1, NULL,
            NULL, 0, ?13, ?14, ?15, ?16
         ) ON CONFLICT(id) DO UPDATE SET
             type         = excluded.type,
             tool         = excluded.tool,
             scope        = excluded.scope,
             origin       = excluded.origin,
             name         = excluded.name,
             display_name = excluded.display_name,
             description  = excluded.description,
             path         = excluded.path,
             format       = excluded.format,
             size         = excluded.size,
             mtime        = excluded.mtime,
             parsed_json  = excluded.parsed_json,
             parse_errors = excluded.parse_errors,
             hash         = excluded.hash,
             updated_at   = excluded.updated_at",
        params![
            id,
            type_str,
            tool_str,
            scope_str,
            origin,
            name,
            display_name,
            description,
            path,
            format_str,
            size,
            mtime,
            parsed_json,
            parse_errors,
            hash,
            updated_at,
        ],
    )?;
    Ok(())
}

fn upsert_component_file(
    conn: &Connection,
    component_id: &str,
    path: &str,
    role: &str,
) -> Result<()> {
    conn.execute(
        "INSERT INTO component_file (component_id, path, role)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(component_id, path) DO UPDATE SET role = excluded.role",
        params![component_id, path, role],
    )?;
    Ok(())
}

fn upsert_component_fts(
    conn: &Connection,
    id: &str,
    name: &str,
    description: &str,
    body: &str,
) -> Result<()> {
    // FTS5 has no UPSERT; we delete then insert. Cheap because the
    // virtual table is keyed on rowid + the unindexed `id` column.
    conn.execute("DELETE FROM component_fts WHERE id = ?1", params![id])?;
    conn.execute(
        "INSERT INTO component_fts (id, name, description, body)
         VALUES (?1, ?2, ?3, ?4)",
        params![id, name, description, body],
    )?;
    Ok(())
}

fn build_uri(tool: ToolId, scope: Scope, ty: ComponentType, name: &str) -> String {
    format!(
        "aseye://{}/{}/{}/{}",
        tool_id_to_str(tool),
        scope_to_str(scope),
        component_type_to_str(ty),
        name
    )
}

/// Description heuristic shared with the parser layer: prefer a
/// frontmatter `description` field, then the body's first non-empty
/// paragraph, then `None`.
fn extract_description(parsed: &ParsedComponent) -> Option<String> {
    if let Some(fm) = parsed.frontmatter.as_ref() {
        if let Some(d) = fm.get("description").and_then(|v| v.as_str()) {
            return Some(d.to_owned());
        }
    }
    if let Some(structured) = parsed.structured.as_ref() {
        if let Some(d) = structured.get("description").and_then(|v| v.as_str()) {
            return Some(d.to_owned());
        }
    }
    parsed.body.as_deref().and_then(first_paragraph)
}

fn extract_display_name(parsed: &ParsedComponent) -> Option<String> {
    parsed
        .frontmatter
        .as_ref()
        .and_then(|fm| fm.get("displayName").or_else(|| fm.get("name")))
        .and_then(|v| v.as_str().map(str::to_owned))
}

/// First non-empty paragraph of a Markdown body. We look at the first
/// 4 KB only - large bodies don't make better descriptions.
fn first_paragraph(body: &str) -> Option<String> {
    let head = if body.len() > 4096 {
        body.get(..4096).unwrap_or(body)
    } else {
        body
    };
    head.split("\n\n")
        .map(str::trim)
        .find(|p| !p.is_empty())
        .map(str::to_owned)
}

fn render_parsed_json(parsed: &ParsedComponent) -> Option<String> {
    if let Some(value) = parsed.structured.as_ref() {
        return serde_json::to_string(value).ok();
    }
    if let Some(fm) = parsed.frontmatter.as_ref() {
        return serde_json::to_string(fm).ok();
    }
    None
}

fn render_parse_error_json(err: &ParseError) -> String {
    // Compact JSON of `{"message":"..."}`; the parser error type is a
    // typed enum but the UI consumes it as a string. We keep a single
    // shape so the React side can render parse errors uniformly.
    serde_json::json!({ "message": err.to_string() }).to_string()
}

fn file_mtime(path: &Path) -> i64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .and_then(|d| i64::try_from(d.as_secs()).ok())
        .unwrap_or(0)
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_secs()).ok())
        .unwrap_or(0)
}

fn file_role(root: &ComponentRoot) -> &'static str {
    if root.is_folder {
        "folder-member"
    } else {
        "primary"
    }
}

// String conversions kept in this module so the wire format used in
// `component.*` columns stays consistent across reads and writes. The
// canonical mapping mirrors the `serde(rename_all = "...")` attributes
// on each enum.

pub(crate) fn tool_id_to_str(id: ToolId) -> &'static str {
    match id {
        ToolId::ClaudeCode => "claude-code",
        ToolId::Codex => "codex",
        ToolId::Cursor => "cursor",
        ToolId::Antigravity => "antigravity",
    }
}

pub(crate) fn parse_tool_id(s: &str) -> Option<ToolId> {
    match s {
        "claude-code" => Some(ToolId::ClaudeCode),
        "codex" => Some(ToolId::Codex),
        "cursor" => Some(ToolId::Cursor),
        "antigravity" => Some(ToolId::Antigravity),
        _ => None,
    }
}

pub(crate) fn component_type_to_str(ty: ComponentType) -> &'static str {
    match ty {
        ComponentType::Tool => "tool",
        ComponentType::Settings => "settings",
        ComponentType::Memory => "memory",
        ComponentType::Rule => "rule",
        ComponentType::Skill => "skill",
        ComponentType::Command => "command",
        ComponentType::Agent => "agent",
        ComponentType::Mcp => "mcp",
        ComponentType::Hook => "hook",
        ComponentType::Plugin => "plugin",
        ComponentType::Marketplace => "marketplace",
        ComponentType::Session => "session",
        ComponentType::Task => "task",
        ComponentType::OutputStyle => "outputStyle",
        ComponentType::Statusline => "statusline",
        ComponentType::Permission => "permission",
    }
}

pub(crate) fn parse_component_type(s: &str) -> Option<ComponentType> {
    match s {
        "tool" => Some(ComponentType::Tool),
        "settings" => Some(ComponentType::Settings),
        "memory" => Some(ComponentType::Memory),
        "rule" => Some(ComponentType::Rule),
        "skill" => Some(ComponentType::Skill),
        "command" => Some(ComponentType::Command),
        "agent" => Some(ComponentType::Agent),
        "mcp" => Some(ComponentType::Mcp),
        "hook" => Some(ComponentType::Hook),
        "plugin" => Some(ComponentType::Plugin),
        "marketplace" => Some(ComponentType::Marketplace),
        "session" => Some(ComponentType::Session),
        "task" => Some(ComponentType::Task),
        "outputStyle" => Some(ComponentType::OutputStyle),
        "statusline" => Some(ComponentType::Statusline),
        "permission" => Some(ComponentType::Permission),
        _ => None,
    }
}

pub(crate) fn scope_to_str(s: Scope) -> &'static str {
    match s {
        Scope::User => "user",
        Scope::Project => "project",
        Scope::Enterprise => "enterprise",
        Scope::Plugin => "plugin",
    }
}

pub(crate) fn parse_scope(s: &str) -> Option<Scope> {
    match s {
        "user" => Some(Scope::User),
        "project" => Some(Scope::Project),
        "enterprise" => Some(Scope::Enterprise),
        "plugin" => Some(Scope::Plugin),
        _ => None,
    }
}

pub(crate) fn format_to_str(f: crate::registry::types::Format) -> &'static str {
    use crate::registry::types::Format;
    match f {
        Format::Json => "json",
        Format::Toml => "toml",
        Format::Yaml => "yaml",
        Format::Markdown => "markdown",
        Format::MarkdownFrontmatter => "markdownFrontmatter",
        Format::Mdc => "mdc",
        Format::Jsonl => "jsonl",
        Format::Sqlite => "sqlite",
        Format::Binary => "binary",
    }
}

// Re-export internal IndexError under the module's prelude so tests that
// match on it can do so without `use crate::index::IndexError;`.
#[allow(dead_code)]
pub(crate) type _IndexError = IndexError;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::tools;
    use std::fs;
    use tempfile::tempdir;

    fn setup_skill(home: &Path, name: &str, body: &str) -> std::path::PathBuf {
        let dir = home.join(".claude").join("skills").join(name);
        fs::create_dir_all(&dir).expect("mkdir");
        let path = dir.join("SKILL.md");
        let content = format!("---\nname: {name}\ndescription: a skill called {name}\n---\n{body}");
        fs::write(&path, content).expect("write");
        path
    }

    #[test]
    fn upsert_inserts_new_component() {
        let home = tempdir().expect("tempdir");
        let path = setup_skill(home.path(), "foo", "hello world\n");
        let descriptor = tools::claude_code();
        let root = descriptor
            .component_roots
            .iter()
            .find(|r| r.component_type == ComponentType::Skill)
            .expect("skill root");

        let handle = IndexHandle::open_in_memory().expect("open");
        let outcome = upsert_component(&handle, &descriptor, root, &path, "foo").expect("upsert");
        assert_eq!(outcome.kind, UpsertKind::Inserted);
        assert_eq!(outcome.id, "aseye://claude-code/user/skill/foo");
        assert!(!outcome.had_parse_error);

        // The component row is queryable and the FTS index has the body.
        let count: i64 = handle
            .read(|c| Ok(c.query_row("SELECT COUNT(*) FROM component", [], |r| r.get(0))?))
            .unwrap();
        assert_eq!(count, 1);
        let fts_hits: i64 = handle
            .read(|c| {
                Ok(c.query_row(
                    "SELECT COUNT(*) FROM component_fts WHERE component_fts MATCH 'hello'",
                    [],
                    |r| r.get(0),
                )?)
            })
            .unwrap();
        assert_eq!(fts_hits, 1);
    }

    #[test]
    fn upsert_idempotent() {
        let home = tempdir().expect("tempdir");
        let path = setup_skill(home.path(), "foo", "stable body\n");
        let descriptor = tools::claude_code();
        let root = descriptor
            .component_roots
            .iter()
            .find(|r| r.component_type == ComponentType::Skill)
            .expect("skill root");

        let handle = IndexHandle::open_in_memory().expect("open");
        let first = upsert_component(&handle, &descriptor, root, &path, "foo").expect("first");
        assert_eq!(first.kind, UpsertKind::Inserted);
        let second = upsert_component(&handle, &descriptor, root, &path, "foo").expect("second");
        assert_eq!(second.kind, UpsertKind::Unchanged);
    }

    #[test]
    fn upsert_parse_error_still_records_row() {
        let home = tempdir().expect("tempdir");
        let claude_dir = home.path().join(".claude");
        fs::create_dir_all(&claude_dir).expect("mkdir");
        let bad = claude_dir.join("settings.json");
        fs::write(&bad, b"{not json").expect("write");

        let descriptor = tools::claude_code();
        let root = descriptor
            .component_roots
            .iter()
            .find(|r| {
                r.component_type == ComponentType::Settings
                    && r.path_pattern == "~/.claude/settings.json"
            })
            .expect("settings root");

        let handle = IndexHandle::open_in_memory().expect("open");
        let outcome =
            upsert_component(&handle, &descriptor, root, &bad, "settings").expect("upsert");
        assert!(outcome.had_parse_error);
        let pe: Option<String> = handle
            .read(|c| {
                Ok(c.query_row(
                    "SELECT parse_errors FROM component WHERE id = ?1",
                    params![outcome.id],
                    |r| r.get(0),
                )?)
            })
            .unwrap();
        assert!(pe.is_some_and(|s| s.contains("invalid JSON")));

        // FTS body must be empty for unparseable content.
        let fts_count: i64 = handle
            .read(|c| {
                Ok(c.query_row(
                    "SELECT COUNT(*) FROM component_fts WHERE id = ?1",
                    params![outcome.id],
                    |r| r.get(0),
                )?)
            })
            .unwrap();
        assert_eq!(fts_count, 0);
    }

    #[test]
    fn upsert_then_modify_reports_updated() {
        let home = tempdir().expect("tempdir");
        let path = setup_skill(home.path(), "foo", "initial\n");
        let descriptor = tools::claude_code();
        let root = descriptor
            .component_roots
            .iter()
            .find(|r| r.component_type == ComponentType::Skill)
            .expect("skill root");

        let handle = IndexHandle::open_in_memory().expect("open");
        upsert_component(&handle, &descriptor, root, &path, "foo").unwrap();

        // Rewrite with different content.
        fs::write(
            &path,
            b"---\nname: foo\ndescription: changed\n---\nnew body\n",
        )
        .unwrap();

        let outcome =
            upsert_component(&handle, &descriptor, root, &path, "foo").expect("second upsert");
        assert_eq!(outcome.kind, UpsertKind::Updated);
    }

    #[test]
    fn delete_component_removes_rows() {
        let home = tempdir().expect("tempdir");
        let path = setup_skill(home.path(), "foo", "body\n");
        let descriptor = tools::claude_code();
        let root = descriptor
            .component_roots
            .iter()
            .find(|r| r.component_type == ComponentType::Skill)
            .expect("skill root");

        let handle = IndexHandle::open_in_memory().expect("open");
        let outcome = upsert_component(&handle, &descriptor, root, &path, "foo").unwrap();
        let removed = delete_component(&handle, &outcome.id).expect("delete");
        assert_eq!(removed, 1);

        let count: i64 = handle
            .read(|c| Ok(c.query_row("SELECT COUNT(*) FROM component", [], |r| r.get(0))?))
            .unwrap();
        assert_eq!(count, 0);
        let fts: i64 = handle
            .read(|c| Ok(c.query_row("SELECT COUNT(*) FROM component_fts", [], |r| r.get(0))?))
            .unwrap();
        assert_eq!(fts, 0);
    }
}
