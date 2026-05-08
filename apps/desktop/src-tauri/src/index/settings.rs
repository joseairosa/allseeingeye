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
#[allow(dead_code)]
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
}
