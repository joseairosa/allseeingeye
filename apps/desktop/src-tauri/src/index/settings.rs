//! Key/value `app_settings` accessor.
//!
//! Phase 14A - the project-memory walker needs a configurable list of
//! search roots, and we want adding more settings later (Health view
//! thresholds, Cost view date windows, ...) to be cheap. The
//! `app_settings` table is a flat `(key TEXT PRIMARY KEY, value TEXT)`
//! map - values are JSON strings so the schema stays type-uniform.
//!
//! Public surface is intentionally narrow:
//! * `read_setting_raw` / `write_setting_raw` - generic JSON-string IO.
//! * `read_project_memory_roots` - the typed accessor the walker calls.
//!
//! Settings reads fall back to documented defaults when the row is
//! absent so callers never have to handle "missing key" specially.

use rusqlite::{params, OptionalExtension};
use serde_json::Value as JsonValue;

use super::error::Result;
use super::IndexHandle;

/// Setting key for the project-memory walker's search roots. Stored as
/// a JSON array of strings (`["~/Development", "~"]`). Documented in
/// `docs/14-cost-and-memory.md` section 14A.
pub const KEY_PROJECT_MEMORY_ROOTS: &str = "projectMemoryRoots";

/// Default value for `KEY_PROJECT_MEMORY_ROOTS`. Two entries balance
/// "find the obvious projects in `~/Development`" with "still surface
/// loose `CLAUDE.md` files dotted around HOME". The walker's
/// project-marker requirement keeps the second entry from being
/// expensive (it only descends into directories that look like
/// projects).
pub const DEFAULT_PROJECT_MEMORY_ROOTS: &[&str] = &["~/Development", "~"];

/// Setting key for the per-tool "skip this tool" flag set. Stored as a
/// JSON array of `ToolId` strings (kebab-case to match
/// `serde(rename_all = "kebab-case")` on `ToolId`). Tools listed here
/// are skipped by the pipeline scan and the live watcher dispatch.
/// Backs the Settings -> Tools index toggle (audit issue #2).
pub const KEY_EXCLUDED_TOOL_IDS: &str = "excludedToolIds";

// Phase 15 - end-to-end encrypted local backup. Three keys cover the
// state the backup module needs to surface from app_settings (the
// keychain owns the private side):
//
// * `backupPublicKey` - hex-encoded 32-byte X25519 public key. Cached
//   here so encryption never has to touch the keychain (only restore
//   does). See `docs/15-backup-and-restore.md` section 15.2.
// * `backupAutoEnabled` - boolean. When true, the auto-backup
//   debouncer runs on `componentUpserted` events. Default true.
// * `backupLastRun` - i64 unix seconds, or null. Last time
//   `backup_now` finished a sweep (success OR failure with at least
//   one encrypted blob). Drives the "last backup: 5 minutes ago" line
//   in Settings.

/// Setting key for the cached device backup public key (hex-encoded).
pub const KEY_BACKUP_PUBLIC_KEY: &str = "backupPublicKey";

/// Setting key for the auto-backup-on-edit toggle. Default `true`.
pub const KEY_BACKUP_AUTO_ENABLED: &str = "backupAutoEnabled";

/// Setting key for the last-backup timestamp (unix seconds).
pub const KEY_BACKUP_LAST_RUN: &str = "backupLastRun";

/// Read a setting as a JSON value. Returns `Ok(None)` when the key is
/// absent. Returns `Ok(Some(Null))` only if the row was explicitly
/// stored as the JSON string `"null"`.
pub fn read_setting_raw(handle: &IndexHandle, key: &str) -> Result<Option<JsonValue>> {
    handle.read(|conn| {
        let raw: Option<String> = conn
            .query_row(
                "SELECT value FROM app_settings WHERE key = ?1",
                params![key],
                |row| row.get(0),
            )
            .optional()?;
        match raw {
            None => Ok(None),
            Some(s) => match serde_json::from_str(&s) {
                Ok(v) => Ok(Some(v)),
                Err(err) => {
                    // A corrupt row is the user's fault (manual DB
                    // edit gone wrong). Treat as "key absent" so the
                    // app keeps working with defaults rather than
                    // failing every scan.
                    tracing::warn!(
                        key,
                        ?err,
                        "app_settings row contained invalid JSON; falling back to default",
                    );
                    Ok(None)
                }
            },
        }
    })
}

/// Write a setting. Replaces any existing row.
pub fn write_setting_raw(handle: &IndexHandle, key: &str, value: &JsonValue) -> Result<()> {
    let serialised = value.to_string();
    handle.write(|conn| {
        conn.execute(
            "INSERT INTO app_settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, serialised],
        )?;
        Ok(())
    })
}

/// Read the persisted set of `excludedToolIds` (audit issue #2).
///
/// Returns an empty `Vec` when the row is absent, malformed, or the
/// stored value is not an array of strings. The scan / watcher
/// dispatch consult this list to skip tools the user has marked as
/// "skipped" in Settings.
#[must_use]
pub fn read_excluded_tool_ids(handle: &IndexHandle) -> Vec<String> {
    let raw = read_setting_raw(handle, KEY_EXCLUDED_TOOL_IDS)
        .ok()
        .flatten();
    let parsed: Option<Vec<String>> = raw.and_then(|v| match v {
        JsonValue::Array(items) => Some(
            items
                .into_iter()
                .filter_map(|x| match x {
                    JsonValue::String(s) => Some(s),
                    _ => None,
                })
                .collect(),
        ),
        _ => None,
    });
    parsed.unwrap_or_default()
}

/// Persist the set of `excludedToolIds`. Duplicates are removed and
/// entries are stored in sorted order so a re-write of an unchanged
/// set produces a byte-identical row.
pub fn write_excluded_tool_ids(handle: &IndexHandle, ids: &[String]) -> Result<()> {
    let mut deduped: Vec<String> = ids.iter().map(|s| s.trim().to_owned()).collect();
    deduped.retain(|s| !s.is_empty());
    deduped.sort();
    deduped.dedup();
    let value = JsonValue::Array(deduped.into_iter().map(JsonValue::String).collect());
    write_setting_raw(handle, KEY_EXCLUDED_TOOL_IDS, &value)
}

/// Add `tool_id` to the excluded set; idempotent. Returns the resulting
/// list so callers can reflect it back into a query cache without a
/// round-trip read.
pub fn add_excluded_tool_id(handle: &IndexHandle, tool_id: &str) -> Result<Vec<String>> {
    let mut current = read_excluded_tool_ids(handle);
    let trimmed = tool_id.trim();
    if !current.iter().any(|s| s == trimmed) {
        current.push(trimmed.to_owned());
    }
    write_excluded_tool_ids(handle, &current)?;
    Ok(read_excluded_tool_ids(handle))
}

/// Remove `tool_id` from the excluded set; idempotent.
pub fn remove_excluded_tool_id(handle: &IndexHandle, tool_id: &str) -> Result<Vec<String>> {
    let mut current = read_excluded_tool_ids(handle);
    let trimmed = tool_id.trim();
    current.retain(|s| s != trimmed);
    write_excluded_tool_ids(handle, &current)?;
    Ok(read_excluded_tool_ids(handle))
}

/// Read the configured project-memory walker roots.
///
/// Returns the stored value when it parses as an array of strings, or
/// the documented default (`["~/Development", "~"]`) otherwise. We
/// never propagate a parse error here - a corrupt row falls back to
/// defaults so the scan still runs.
#[must_use]
pub fn read_project_memory_roots(handle: &IndexHandle) -> Vec<String> {
    let raw = read_setting_raw(handle, KEY_PROJECT_MEMORY_ROOTS)
        .ok()
        .flatten();
    let parsed: Option<Vec<String>> = raw.and_then(|v| match v {
        JsonValue::Array(items) => Some(
            items
                .into_iter()
                .filter_map(|x| match x {
                    JsonValue::String(s) => Some(s),
                    _ => None,
                })
                .collect(),
        ),
        _ => None,
    });
    parsed.filter(|v| !v.is_empty()).unwrap_or_else(|| {
        DEFAULT_PROJECT_MEMORY_ROOTS
            .iter()
            .map(|s| (*s).to_owned())
            .collect()
    })
}

// ─── Phase 15 - typed accessors for backup-related settings ────────

/// Read `backupAutoEnabled`. Defaults to `true` per spec 15.5
/// ("Backup automatically after edits") - the user opts out, not in.
#[must_use]
pub fn read_backup_auto_enabled(handle: &IndexHandle) -> bool {
    let raw = read_setting_raw(handle, KEY_BACKUP_AUTO_ENABLED)
        .ok()
        .flatten();
    match raw {
        Some(JsonValue::Bool(v)) => v,
        _ => true,
    }
}

/// Persist `backupAutoEnabled`. Idempotent. Used by the
/// `backup_set_auto` IPC command and the auto-debouncer's tests.
#[allow(dead_code)] // wired in via IPC commands committed in step 4.
pub fn write_backup_auto_enabled(handle: &IndexHandle, enabled: bool) -> Result<()> {
    write_setting_raw(handle, KEY_BACKUP_AUTO_ENABLED, &JsonValue::Bool(enabled))
}

/// Read `backupLastRun` as unix seconds. `None` when unset (no
/// backup has ever completed). Used by the `backup_status` IPC
/// command.
#[allow(dead_code)] // wired in via IPC commands committed in step 4.
#[must_use]
pub fn read_backup_last_run(handle: &IndexHandle) -> Option<i64> {
    let raw = read_setting_raw(handle, KEY_BACKUP_LAST_RUN).ok().flatten();
    match raw {
        Some(JsonValue::Number(n)) => n.as_i64(),
        _ => None,
    }
}

/// Persist `backupLastRun` as unix seconds.
pub fn write_backup_last_run(handle: &IndexHandle, unix_seconds: i64) -> Result<()> {
    write_setting_raw(
        handle,
        KEY_BACKUP_LAST_RUN,
        &JsonValue::Number(unix_seconds.into()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_returned_when_key_missing() {
        let handle = IndexHandle::open_in_memory().expect("open");
        let roots = read_project_memory_roots(&handle);
        assert_eq!(
            roots,
            vec!["~/Development".to_owned(), "~".to_owned()],
            "missing key must yield documented defaults",
        );
    }

    #[test]
    fn write_then_read_round_trip() {
        let handle = IndexHandle::open_in_memory().expect("open");
        let value = serde_json::json!(["~/Code", "~/work"]);
        write_setting_raw(&handle, KEY_PROJECT_MEMORY_ROOTS, &value).expect("write");
        let roots = read_project_memory_roots(&handle);
        assert_eq!(roots, vec!["~/Code".to_owned(), "~/work".to_owned()]);
    }

    #[test]
    fn corrupt_value_falls_back_to_defaults() {
        let handle = IndexHandle::open_in_memory().expect("open");
        // Write garbage to the row directly.
        handle
            .write(|conn| {
                conn.execute(
                    "INSERT INTO app_settings (key, value) VALUES (?1, ?2)",
                    params![KEY_PROJECT_MEMORY_ROOTS, "not-json"],
                )?;
                Ok(())
            })
            .unwrap();
        let roots = read_project_memory_roots(&handle);
        // Falls back rather than panicking.
        assert_eq!(roots, vec!["~/Development".to_owned(), "~".to_owned()]);
    }

    #[test]
    fn empty_array_falls_back_to_defaults() {
        let handle = IndexHandle::open_in_memory().expect("open");
        let value = serde_json::json!([]);
        write_setting_raw(&handle, KEY_PROJECT_MEMORY_ROOTS, &value).expect("write");
        let roots = read_project_memory_roots(&handle);
        // An explicit empty list is treated as "use the defaults"
        // rather than "do not walk anything" - the latter would be
        // surprising and never the intent.
        assert_eq!(roots, vec!["~/Development".to_owned(), "~".to_owned()]);
    }

    #[test]
    fn non_string_entries_are_filtered() {
        let handle = IndexHandle::open_in_memory().expect("open");
        let value = serde_json::json!(["~/Code", 42, true, "~/Other"]);
        write_setting_raw(&handle, KEY_PROJECT_MEMORY_ROOTS, &value).expect("write");
        let roots = read_project_memory_roots(&handle);
        assert_eq!(roots, vec!["~/Code".to_owned(), "~/Other".to_owned()]);
    }

    // ─── Audit follow-up - excludedToolIds (issue #2) ─────────────────

    /// A missing row reads as an empty exclusion set so the scan runs
    /// against every detected tool by default - matches the docs/03
    /// promise that "out of the box, every detected tool is indexed".
    #[test]
    fn excluded_tool_ids_default_to_empty() {
        let handle = IndexHandle::open_in_memory().expect("open");
        assert!(read_excluded_tool_ids(&handle).is_empty());
    }

    /// Round-trip: write a set, read it back. The reader returns the
    /// sorted, deduplicated list the writer persisted.
    #[test]
    fn excluded_tool_ids_round_trip() {
        let handle = IndexHandle::open_in_memory().expect("open");
        write_excluded_tool_ids(
            &handle,
            &[
                "codex".to_owned(),
                "claude-code".to_owned(),
                "codex".to_owned(),
            ],
        )
        .expect("write");
        assert_eq!(
            read_excluded_tool_ids(&handle),
            vec!["claude-code".to_owned(), "codex".to_owned()],
        );
    }

    /// `add_excluded_tool_id` is idempotent - inserting the same id
    /// twice produces a one-entry list, not a duplicate.
    #[test]
    fn add_excluded_tool_id_is_idempotent() {
        let handle = IndexHandle::open_in_memory().expect("open");
        let after_first = add_excluded_tool_id(&handle, "antigravity").expect("add 1");
        let after_second = add_excluded_tool_id(&handle, "antigravity").expect("add 2");
        assert_eq!(after_first, vec!["antigravity".to_owned()]);
        assert_eq!(after_second, vec!["antigravity".to_owned()]);
    }

    /// `remove_excluded_tool_id` is idempotent and survives a missing
    /// id without erroring - the IPC contract is "ensure this is not
    /// present" rather than "delete or fail".
    #[test]
    fn remove_excluded_tool_id_handles_missing() {
        let handle = IndexHandle::open_in_memory().expect("open");
        let after = remove_excluded_tool_id(&handle, "claude-code").expect("remove");
        assert!(after.is_empty());
        add_excluded_tool_id(&handle, "claude-code").expect("add");
        let after_real = remove_excluded_tool_id(&handle, "claude-code").expect("remove real");
        assert!(after_real.is_empty());
    }

    /// A corrupted row parses back as empty rather than panicking, so
    /// a manual edit-gone-wrong does not lock the user out of running
    /// scans.
    #[test]
    fn excluded_tool_ids_corrupt_row_falls_back_to_empty() {
        let handle = IndexHandle::open_in_memory().expect("open");
        handle
            .write(|conn| {
                conn.execute(
                    "INSERT INTO app_settings (key, value) VALUES (?1, ?2)",
                    params![KEY_EXCLUDED_TOOL_IDS, "garbage"],
                )?;
                Ok(())
            })
            .expect("insert");
        assert!(read_excluded_tool_ids(&handle).is_empty());
    }
}
