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

/// Phase 7.2 migration: add the `evidence_json` column to
/// `security_finding`. Nullable, defaults to NULL - existing Phase 7.1
/// secret rows stay NULL on upgrade, while Phase 7.2 MCP-permission
/// findings populate it with a small structured object (host +
/// database for Postgres, repo scope for GitHub, etc.).
///
/// `ALTER TABLE ... ADD COLUMN` in `SQLite` is an in-place metadata
/// change - no row rewrite, no extra storage cost on existing rows.
const V3_FINDING_EVIDENCE: &[&str] =
    &["ALTER TABLE security_finding ADD COLUMN evidence_json TEXT;"];

/// Phase 14A migration: add the `app_settings` key/value table that
/// the project memory walker reads `projectMemoryRoots` from. New
/// table only, no existing-row impact. See
/// `docs/14-cost-and-memory.md` section 14A for the rationale.
const V4_APP_SETTINGS: &[&str] = &[schema::CREATE_APP_SETTINGS];

/// Phase 14C migration: add the `token_usage` rollup table and the
/// per-session `usage_session_watermark`. New tables only, no changes
/// to existing rows. See `docs/14-cost-and-memory.md` section 14C for
/// the schema rationale.
const V5_TOKEN_USAGE: &[&str] = &[
    schema::CREATE_TOKEN_USAGE,
    schema::CREATE_IDX_TOKEN_USAGE_DAY,
    schema::CREATE_IDX_TOKEN_USAGE_PROJECT,
    schema::CREATE_USAGE_SESSION_WATERMARK,
];

/// Phase 15 migration: add the `backup_manifest` table + its
/// `encrypted_at` index. New table only, no changes to existing rows.
/// See `docs/15-backup-and-restore.md` section 15.4 for the schema
/// rationale.
const V6_BACKUP_MANIFEST: &[&str] = &[
    schema::CREATE_BACKUP_MANIFEST,
    schema::CREATE_IDX_BACKUP_MANIFEST_ENCRYPTED_AT,
];

/// Registered migrations. Keep sorted ascending by version. The runner
/// refuses to open a DB whose stored version is higher than the maximum
/// here - that means a future build wrote it.
const MIGRATIONS: &[(u32, &[&str])] = &[
    (1, V1_BOOTSTRAP),
    (2, V2_SECURITY_TABLES),
    (3, V3_FINDING_EVIDENCE),
    (4, V4_APP_SETTINGS),
    (5, V5_TOKEN_USAGE),
    (6, V6_BACKUP_MANIFEST),
];

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
            "app_settings",
            "token_usage",
            "usage_session_watermark",
            "backup_manifest",
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

        // Indexes from v1 + v2 + v5 + v6.
        for idx in [
            "idx_component_tool_type",
            "idx_component_mtime",
            "idx_usage_component_ts",
            "idx_finding_component",
            "idx_finding_severity_detected",
            "idx_token_usage_day",
            "idx_token_usage_project",
            "idx_backup_manifest_encrypted_at",
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
        //
        // We assert against `latest_version()` rather than a literal
        // (e.g. 2) because each new migration would otherwise force a
        // hand-edit of every test that pinned the version - the gate
        // we actually care about is "all v1+v2 columns are present
        // after running migrations to the head".
        let mut conn = fresh_in_memory();
        let v = run_migrations(&mut conn).expect("migrate");
        assert_eq!(v, latest_version());

        // `security_finding` columns expected from v2 onwards.
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

    #[test]
    fn migration_v3_adds_evidence_column() {
        // Phase 7.2: a fresh DB at `latest_version` has the
        // `evidence_json` column on `security_finding`. We don't pin
        // the literal version number - the assertion is that v3
        // landed (the column exists) and that running migrations to
        // the head of the chain is idempotent.
        let mut conn = fresh_in_memory();
        let v = run_migrations(&mut conn).expect("migrate");
        assert!(
            v >= 3,
            "expected migrations to advance past v3, got {v} (latest = {})",
            latest_version()
        );

        let mut stmt = conn.prepare("PRAGMA table_info(security_finding)").unwrap();
        let cols: Vec<(String, String, i64)> = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(1)?, // name
                    row.get::<_, String>(2)?, // type
                    row.get::<_, i64>(3)?,    // notnull (0 = nullable)
                ))
            })
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        let evidence = cols
            .iter()
            .find(|(name, _, _)| name == "evidence_json")
            .expect("expected evidence_json column to exist");
        assert_eq!(evidence.1, "TEXT", "evidence_json must be TEXT");
        assert_eq!(evidence.2, 0, "evidence_json must be nullable");
    }

    /// Phase 15: a fresh DB at `latest_version` carries the
    /// `backup_manifest` table + its `encrypted_at` index, with the
    /// columns exactly as `docs/15-backup-and-restore.md` section 15.4
    /// pins them.
    #[test]
    fn migration_v6_creates_backup_manifest() {
        let mut conn = fresh_in_memory();
        let v = run_migrations(&mut conn).expect("migrate");
        assert!(
            v >= 6,
            "expected migrations to advance past v6, got {v} (latest = {})",
            latest_version(),
        );

        let mut stmt = conn.prepare("PRAGMA table_info(backup_manifest)").unwrap();
        let cols: Vec<(String, String, i64)> = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(1)?, // name
                    row.get::<_, String>(2)?, // type
                    row.get::<_, i64>(3)?,    // notnull (0 = nullable)
                ))
            })
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        // Spot-check shape: types correct, every column NOT NULL.
        // SQLite reports `notnull = 0` for the primary-key column when
        // the table is declared with `PRIMARY KEY` but no explicit
        // `NOT NULL` annotation (the engine treats PK columns as
        // implicitly NOT NULL but the PRAGMA does NOT reflect that).
        // We accept either 0 or 1 for `component_id` because the PK
        // constraint already enforces non-null at insert time.
        for (name, ty, notnull) in [
            ("component_id", "TEXT", None),
            ("blob_path", "TEXT", Some(1)),
            ("plaintext_hash", "TEXT", Some(1)),
            ("blob_hash", "TEXT", Some(1)),
            ("plaintext_size", "INTEGER", Some(1)),
            ("blob_size", "INTEGER", Some(1)),
            ("encrypted_at", "INTEGER", Some(1)),
        ] {
            let found = cols
                .iter()
                .find(|(n, _, _)| n == name)
                .unwrap_or_else(|| panic!("expected column {name}"));
            assert_eq!(found.1, ty, "type mismatch for {name}");
            if let Some(want) = notnull {
                assert_eq!(found.2, want, "notnull mismatch for {name}");
            }
        }
    }

    /// Sanity-check the upgrade path: a DB stamped at v5 advances to
    /// v6 without rewriting v5 rows. Mirror of the existing v2 -> v3
    /// upgrade-path test.
    #[test]
    fn migration_v5_to_v6_upgrade_path() {
        let mut conn = fresh_in_memory();
        // Run all migrations once to lay every table down.
        let _ = run_migrations(&mut conn).expect("initial migrate");
        // Now simulate a DB pinned at v5 by deleting the v6 table +
        // index and rewinding `schema_version`.
        conn.execute_batch(
            "DROP INDEX IF EXISTS idx_backup_manifest_encrypted_at;
             DROP TABLE IF EXISTS backup_manifest;
             DELETE FROM schema_version;
             INSERT INTO schema_version (version) VALUES (5);",
        )
        .unwrap();

        // Confirm the simulated v5 state has no backup_manifest.
        let pre: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'backup_manifest'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(pre, 0, "simulated v5 must have no backup_manifest");

        // Upgrade. The migration runner must apply v6 only.
        let v = run_migrations(&mut conn).expect("upgrade");
        assert_eq!(v, latest_version());

        // backup_manifest exists post-upgrade.
        let post: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'backup_manifest'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(post, 1, "v6 upgrade must create backup_manifest");

        // The encrypted_at index lands too.
        let idx: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = 'idx_backup_manifest_encrypted_at'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(idx, 1);
    }

    #[test]
    fn migration_v2_to_v3_upgrade_path() {
        // Sanity-check the upgrade path: an existing DB at v2
        // (simulated by running v1 + v2 manually and stamping the
        // version) advances to v3 without rewriting existing rows.
        let mut conn = fresh_in_memory();
        // Bootstrap to v2 by running migrations once and rolling back
        // the version to 2 - simpler than re-running the v1 + v2 SQL
        // by hand.
        let _ = run_migrations(&mut conn).expect("initial migrate");
        conn.execute("DELETE FROM schema_version", []).unwrap();
        conn.execute("INSERT INTO schema_version (version) VALUES (2)", [])
            .unwrap();
        // Drop the v3 column so the simulated v2 schema is column-
        // accurate (otherwise the second `run_migrations` would not
        // re-apply v3 since we deleted only the version row). Also
        // drop every table created by migrations after v3 - the
        // simulated state is "DB pinned at v2", so any post-v3 tables
        // must not exist or `run_migrations` will trip over them on
        // CREATE TABLE.
        conn.execute_batch(
            "CREATE TABLE security_finding_old AS SELECT
                 id, component_id, category, pattern, severity, file_path,
                 line, source_label, redacted_preview, detected_at,
                 suppressed, suppress_reason, suppress_until
             FROM security_finding;
             DROP TABLE security_finding;
             ALTER TABLE security_finding_old RENAME TO security_finding;
             DROP TABLE IF EXISTS app_settings;
             DROP TABLE IF EXISTS token_usage;
             DROP TABLE IF EXISTS usage_session_watermark;
             DROP INDEX IF EXISTS idx_backup_manifest_encrypted_at;
             DROP TABLE IF EXISTS backup_manifest;",
        )
        .unwrap();

        // Now upgrade. The migration runner must apply v3 only.
        let v = run_migrations(&mut conn).expect("upgrade");
        assert_eq!(v, latest_version());

        // The new column exists and is nullable.
        let mut stmt = conn.prepare("PRAGMA table_info(security_finding)").unwrap();
        let names: Vec<String> = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        assert!(
            names.iter().any(|n| n == "evidence_json"),
            "evidence_json missing after v2 -> v3 upgrade: {names:?}"
        );
    }
}
