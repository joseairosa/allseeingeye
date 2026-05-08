//! Security audit framework.
//!
//! Phase 7.1 lands the secret-detection engine: a curated regex set
//! mirroring `docs/12-security.md` Section A. Secret exposure, run as
//! a synchronous pass over each parsed component during upsert.
//!
//! Scope of this phase:
//! * `Category::Secret` only - other categories (MCP permission, hook,
//!   plugin, path traversal, sensitive directory, license) land in
//!   later phases.
//! * Findings are produced by the scanner and persisted by the upsert
//!   layer through [`persist_findings`]. The scanner itself is pure
//!   (no DB access), so it stays unit-testable without the index.
//! * Suppressions are honoured during persistence: a row in
//!   `security_finding_suppression` (`component_id`, `pattern`) prevents
//!   re-insertion of an existing matching finding on subsequent
//!   upserts.
//!
//! Layers:
//! * `error`    - [`SecurityError`] / [`Result`].
//! * `finding`  - [`Finding`] / [`Severity`] / [`Category`] + redaction.
//! * `patterns` - the curated [`patterns::PATTERNS`] table + compiled
//!   regex caches.
//! * `scanner`  - [`scanner::scan_parsed`] / [`scanner::scan_text`].

pub mod error;
pub mod finding;
pub mod mcp_audit;
pub mod patterns;
pub mod scanner;

use rusqlite::{params, Connection};

pub use error::{Result, SecurityError};
pub use finding::{redact, Category, Finding, Severity};
pub use scanner::{scan_parsed, scan_text};

/// Persist a vector of findings under a known component id.
///
/// Behaviour:
/// * Findings whose `(component_id, pattern)` pair has a row in the
///   `security_finding_suppression` table are skipped (the user
///   suppressed them previously and we honour that across upserts).
/// * Existing rows for the same finding `id` are kept; on conflict we
///   `ON CONFLICT DO NOTHING` so a re-scan that produces the same
///   stable id is a no-op rather than churning timestamps.
/// * Rows for the same component that no longer match the current
///   findings set stay put. The Phase 7.3 UI is responsible for
///   showing "stale" findings; this module never silently deletes.
///
/// `file_path` is the absolute path of the source file the findings
/// came from - persisted into the `security_finding.file_path` column
/// so the UI can hop the user to the file without joining back to
/// `component`.
pub fn persist_findings(
    conn: &Connection,
    component_id: &str,
    file_path: &str,
    findings: &[Finding],
) -> Result<()> {
    if findings.is_empty() {
        return Ok(());
    }

    // Pre-fetch the suppression set for this component once. Per-row
    // queries would scale poorly if a component accumulates dozens of
    // patterns over time.
    let suppressed: Vec<String> = {
        let mut stmt = conn
            .prepare("SELECT pattern FROM security_finding_suppression WHERE component_id = ?1")?;
        let rows = stmt.query_map(params![component_id], |row| row.get::<_, String>(0))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };

    for finding in findings {
        if suppressed.iter().any(|p| p == &finding.pattern) {
            continue;
        }
        // Phase 7.2: serialise the optional evidence object into the
        // `evidence_json` column. Phase 7.1 secret findings carry
        // `evidence = None` and write SQL NULL, which round-trips back
        // through the (currently unused) read path the same way.
        let evidence_json: Option<String> = match finding.evidence.as_ref() {
            Some(value) => Some(serde_json::to_string(value).map_err(|err| {
                SecurityError::Internal(format!("serialise evidence_json: {err}"))
            })?),
            None => None,
        };
        conn.execute(
            "INSERT INTO security_finding (
                id, component_id, category, pattern, severity, file_path,
                line, source_label, redacted_preview, detected_at,
                suppressed, suppress_reason, suppress_until, evidence_json
             ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 0, NULL, NULL, ?11
             )
             ON CONFLICT(id) DO NOTHING",
            params![
                finding.id,
                component_id,
                finding.category.as_str(),
                finding.pattern,
                finding.severity.as_str(),
                file_path,
                finding.line,
                finding.source_label,
                finding.redacted_preview,
                finding.detected_at,
                evidence_json,
            ],
        )?;
    }
    Ok(())
}

/// Read back a single finding's evidence blob. Used by Phase 7.2 tests
/// to assert evidence round-trips; no IPC consumer surfaces this in
/// Phase 7.2 (UI surface lands in 7.3).
///
/// Returns `Ok(None)` when the row exists but `evidence_json IS NULL`,
/// `Ok(Some(value))` when it parses, or `SecurityError::Internal` when
/// the column holds malformed JSON. Returns `Ok(None)` and emits a
/// debug log when the row id doesn't exist.
///
/// Marked `allow(dead_code)` because the only consumers right now are
/// the tests in this module and `mcp_audit`. The Phase 7.3 Security
/// IPC commands will join over the column directly rather than
/// calling this helper, so it stays test-only - but kept in the
/// public API surface so future audit categories can reuse it without
/// re-deriving the read path.
#[allow(dead_code)]
pub fn load_finding_evidence(
    conn: &Connection,
    finding_id: &str,
) -> Result<Option<serde_json::Value>> {
    let row: Option<Option<String>> = conn
        .query_row(
            "SELECT evidence_json FROM security_finding WHERE id = ?1",
            params![finding_id],
            |row| row.get::<_, Option<String>>(0),
        )
        .ok();
    let Some(payload) = row else {
        return Ok(None);
    };
    let Some(json) = payload else {
        return Ok(None);
    };
    let value = serde_json::from_str(&json).map_err(|err| {
        SecurityError::Internal(format!(
            "evidence_json parse failed for {finding_id}: {err}"
        ))
    })?;
    Ok(Some(value))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::IndexHandle;

    fn fresh() -> IndexHandle {
        IndexHandle::open_in_memory().expect("open in-memory")
    }

    fn seed_component(handle: &IndexHandle, id: &str) {
        handle
            .write(|c| {
                c.execute(
                    "INSERT INTO component (
                        id, type, tool, scope, origin, name, path, format, hash, updated_at
                     ) VALUES (?1, 'settings', 'claude-code', 'user', 'userCreated',
                              'settings', '/tmp/x.json', 'json', 'h', 0)",
                    params![id],
                )?;
                Ok(())
            })
            .unwrap();
    }

    fn make_finding(pattern: &str, id: &str) -> Finding {
        Finding {
            id: id.to_owned(),
            component_id: None,
            category: Category::Secret,
            pattern: pattern.to_owned(),
            severity: Severity::Critical,
            source_label: "body".to_owned(),
            line: Some(1),
            redacted_preview: "abcd\u{2026}wxyz".to_owned(),
            detected_at: 0,
            evidence: None,
        }
    }

    #[test]
    fn persist_inserts_rows() {
        let handle = fresh();
        seed_component(&handle, "aseye://test/c1");
        let findings = vec![make_finding("anthropic-key", "f-1")];
        handle
            .write(|c| {
                persist_findings(c, "aseye://test/c1", "/tmp/x.json", &findings).expect("persist");
                Ok(())
            })
            .unwrap();
        let n: i64 = handle
            .read(|c| {
                Ok(c.query_row(
                    "SELECT COUNT(*) FROM security_finding WHERE component_id = ?1",
                    params!["aseye://test/c1"],
                    |r| r.get(0),
                )?)
            })
            .unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn persist_idempotent_on_id_conflict() {
        let handle = fresh();
        seed_component(&handle, "aseye://test/c1");
        let findings = vec![make_finding("anthropic-key", "f-1")];
        for _ in 0..3 {
            handle
                .write(|c| {
                    persist_findings(c, "aseye://test/c1", "/tmp/x.json", &findings)
                        .expect("persist");
                    Ok(())
                })
                .unwrap();
        }
        let n: i64 = handle
            .read(|c| {
                Ok(c.query_row(
                    "SELECT COUNT(*) FROM security_finding WHERE component_id = ?1",
                    params!["aseye://test/c1"],
                    |r| r.get(0),
                )?)
            })
            .unwrap();
        assert_eq!(n, 1, "duplicate persist should not multiply rows");
    }

    #[test]
    fn persist_honours_suppression() {
        let handle = fresh();
        seed_component(&handle, "aseye://test/c1");
        // Seed suppression for `anthropic-key` BEFORE persisting.
        handle
            .write(|c| {
                c.execute(
                    "INSERT INTO security_finding_suppression
                       (component_id, pattern, suppressed_at, reason)
                     VALUES ('aseye://test/c1', 'anthropic-key', 0, 'ack')",
                    [],
                )?;
                Ok(())
            })
            .unwrap();
        let findings = vec![
            make_finding("anthropic-key", "f-1"),
            make_finding("openai-key", "f-2"),
        ];
        handle
            .write(|c| {
                persist_findings(c, "aseye://test/c1", "/tmp/x.json", &findings).expect("persist");
                Ok(())
            })
            .unwrap();
        let patterns: Vec<String> = handle
            .read(|c| {
                let mut stmt = c.prepare(
                    "SELECT pattern FROM security_finding WHERE component_id = ?1 ORDER BY pattern",
                )?;
                let rows = stmt.query_map(params!["aseye://test/c1"], |r| r.get::<_, String>(0))?;
                Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
            })
            .unwrap();
        assert_eq!(patterns, vec!["openai-key"]);
    }
}
