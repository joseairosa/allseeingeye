//! Forward-only schema migrations.
//!
//! Migrations are stored as a static `&[(version, &[&str])]`. Each
//! migration's SQL is a slice of statements that are applied in order
//! inside one transaction. `run_migrations` reads the current
//! `schema_version`, applies every pending migration, then writes the
//! new version back. If anything in a migration fails, the transaction
//! rolls back and the database is unchanged.
//!
//! Adding a migration:
//!   1. Append a new `(N, &[..sql])` tuple to `MIGRATIONS`.
//!   2. Never edit an existing tuple - users will already have run it.
//!   3. Each statement may itself be multi-statement; we drive them with
//!      `execute_batch`, so semicolon-separated statements work.

use rusqlite::{params, Connection, OptionalExtension};

use super::error::{IndexError, Result};
use super::schema;

/// Bootstrap schema. Each entry is one logical CREATE statement,
/// applied in order inside a single transaction by the migration
/// runner. Order matters only for the `component_file -> component`
/// foreign key (FK is allowed to forward-reference under deferred FK
/// checks, but readable is readable).
const V1_BOOTSTRAP: &[&str] = &[
    schema::CREATE_COMPONENT,
    schema::CREATE_IDX_COMPONENT_TOOL_TYPE,
    schema::CREATE_IDX_COMPONENT_MTIME,
    schema::CREATE_COMPONENT_FILE,
    schema::CREATE_RELATION,
    schema::CREATE_TAG,
    schema::CREATE_PIN,
    schema::CREATE_NOTE,
    schema::CREATE_COMPONENT_FTS,
    schema::CREATE_HEALTH_PROBE,
    schema::CREATE_USAGE_EVENT,
    schema::CREATE_IDX_USAGE_COMPONENT_TS,
];

/// Phase 7.1 migration: add the `security_finding` and
/// `security_finding_suppression` tables. Mirrored from
/// `docs/12-security.md` ("Privacy model and finding data"). Order
/// matters: `security_finding`'s FK to `component(id)` is created
/// before the indexes that reference its columns.
const V2_SECURITY_TABLES: &[&str] = &[
    schema::CREATE_SECURITY_FINDING,
    schema::CREATE_IDX_FINDING_COMPONENT,
    schema::CREATE_IDX_FINDING_SEVERITY_DETECTED,
    schema::CREATE_SECURITY_FINDING_SUPPRESSION,
];

/// Registered migrations. Keep sorted ascending by version. The runner
/// refuses to open a DB whose stored version is higher than the maximum
/// here - that means a future build wrote it.
const MIGRATIONS: &[(u32, &[&str])] = &[(1, V1_BOOTSTRAP), (2, V2_SECURITY_TABLES)];

/// Highest migration version known to this build.
#[must_use]
pub fn latest_version() -> u32 {
    MIGRATIONS.last().map_or(0, |(v, _)| *v)
}

/// Read the current schema version, creating the `schema_version`
/// table if needed. Returns 0 for a freshly-created database.
fn current_version(conn: &Connection) -> Result<u32> {
    // Always-on: the version table is created with IF NOT EXISTS so this
    // reader is safe to call at any time, including before any migration
    // has run. We reuse `schema::CREATE_SCHEMA_VERSION` as the source of
    // truth and patch in the IF NOT EXISTS guard so the bootstrap and
    // the resume path stay literally identical.
    conn.execute_batch(&schema::CREATE_SCHEMA_VERSION.replacen(
        "CREATE TABLE",
        "CREATE TABLE IF NOT EXISTS",
        1,
    ))?;

    let v: Option<u32> = conn
        .query_row("SELECT version FROM schema_version LIMIT 1", [], |row| {
            row.get(0)
        })
        .optional()?;
    Ok(v.unwrap_or(0))
}

/// Apply all pending migrations. Returns the resulting schema version.
///
/// - Fresh DB (no rows in `schema_version`) -> applies every migration.
/// - DB at v=N -> applies only migrations with version > N.
/// - DB at v > latest -> returns `SchemaVersionMismatch` (we refuse to
///   open futures we cannot model).
/// - On SQL failure inside any migration -> transaction rolls back, DB
///   stays at the previous good version, error bubbles up.
pub fn run_migrations(conn: &mut Connection) -> Result<u32> {
    let mut current = current_version(conn)?;
    let target = latest_version();

    if current > target {
        return Err(IndexError::SchemaVersionMismatch {
            found: current,
            known: target,
        });
    }

    for (version, statements) in MIGRATIONS {
        if *version <= current {
            continue;
        }

        let tx = conn.transaction()?;
        for stmt in *statements {
            tx.execute_batch(stmt)?;
        }

        // Single-row contract: replace whatever's in schema_version with
        // the version we just landed at.
        tx.execute("DELETE FROM schema_version", [])?;
        tx.execute(
            "INSERT INTO schema_version (version) VALUES (?1)",
            params![*version],
        )?;

        tx.commit()?;
        current = *version;
    }

    Ok(current)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh_in_memory() -> Connection {
        Connection::open_in_memory().expect("open in-memory sqlite")
    }

    #[test]
    fn migrate_clean_database() {
        let mut conn = fresh_in_memory();
        let v = run_migrations(&mut conn).expect("run migrations");
        assert_eq!(v, latest_version());

        // schema_version row must exist with the latest version.
        let stored: u32 = conn
            .query_row("SELECT version FROM schema_version", [], |row| row.get(0))
            .expect("read schema_version");
        assert_eq!(stored, latest_version());

        // All v1 tables (docs/05) + v2 security tables + schema_version.
        // FTS5 virtual tables register as `type='table'` in
        // sqlite_master, so a name-equality check is enough.
        for table in [
            "component",
            "component_file",
            "relation",
            "tag",
            "pin",
            "note",
            "component_fts",
            "health_probe",
            "usage_event",
            "schema_version",
            "security_finding",
            "security_finding_suppression",
        ] {
            let n: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type IN ('table','view') AND name = ?1",
                    params![table],
                    |row| row.get(0),
                )
                .unwrap_or(0);
            assert!(n >= 1, "expected table {table} to exist");
        }

        // Indexes from v1 + v2.
        for idx in [
            "idx_component_tool_type",
            "idx_component_mtime",
            "idx_usage_component_ts",
            "idx_finding_component",
            "idx_finding_severity_detected",
        ] {
            let n: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = ?1",
                    params![idx],
                    |row| row.get(0),
                )
                .unwrap_or(0);
            assert_eq!(n, 1, "expected index {idx} to exist");
        }
    }

    #[test]
    fn migrate_idempotent() {
        let mut conn = fresh_in_memory();
        let first = run_migrations(&mut conn).expect("first run");
        let second = run_migrations(&mut conn).expect("second run");
        assert_eq!(first, latest_version());
        assert_eq!(second, latest_version());

        // No duplicate `component` table got created on second run.
        let comp_tables: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='component'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(comp_tables, 1);

        // schema_version still has exactly one row.
        let rows: i64 = conn
            .query_row("SELECT COUNT(*) FROM schema_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rows, 1);
    }

    #[test]
    fn refuses_future_schema() {
        let mut conn = fresh_in_memory();
        // Simulate a DB created by a future build at v999.
        conn.execute_batch(
            "CREATE TABLE schema_version (version INTEGER NOT NULL); INSERT INTO schema_version VALUES (999);",
        )
        .unwrap();
        let err = run_migrations(&mut conn).expect_err("must reject future version");
        match err {
            IndexError::SchemaVersionMismatch { found, known } => {
                assert_eq!(found, 999);
                assert_eq!(known, latest_version());
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn migration_v2_creates_security_tables() {
        // Open a clean in-memory DB, run migrations, and assert the
        // security tables exist with the expected columns. The
        // doctest for `migrate_clean_database` covers presence; this
        // test pins the column shape so future migrations don't drift
        // it without a notice.
        let mut conn = fresh_in_memory();
        let v = run_migrations(&mut conn).expect("migrate");
        assert_eq!(v, 2);

        // `security_finding` columns.
        let mut stmt = conn.prepare("PRAGMA table_info(security_finding)").unwrap();
        let cols: Vec<String> = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        for col in [
            "id",
            "component_id",
            "category",
            "pattern",
            "severity",
            "file_path",
            "line",
            "source_label",
            "redacted_preview",
            "detected_at",
            "suppressed",
            "suppress_reason",
            "suppress_until",
        ] {
            assert!(
                cols.iter().any(|c| c == col),
                "expected column {col} on security_finding, got {cols:?}"
            );
        }

        // `security_finding_suppression` columns.
        let mut stmt = conn
            .prepare("PRAGMA table_info(security_finding_suppression)")
            .unwrap();
        let cols: Vec<String> = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        for col in ["component_id", "pattern", "suppressed_at", "reason"] {
            assert!(
                cols.iter().any(|c| c == col),
                "expected column {col} on security_finding_suppression, got {cols:?}"
            );
        }
    }
}
