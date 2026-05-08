//! Connection management for the index.
//!
//! `IndexHandle` owns:
//!   - the single write `Connection` (behind `parking_lot::Mutex`), and
//!   - an `r2d2::Pool<SqliteConnectionManager>` for read-only queries.
//!
//! This split mirrors docs/05 "Concurrency model": a single index-writer
//! task serialises mutations while frontend reads draw from a pool. We
//! never hand out a `&Connection` directly - callers go through `read`
//! / `write` closures so we keep the lock acquisition centralised.
//!
//! On `open` we run, in order:
//!   1. `journal_mode = WAL` so concurrent reads + a single writer
//!      coexist without locking each other out.
//!   2. `synchronous = NORMAL` - pairs with WAL for sane durability/perf.
//!   3. `foreign_keys = ON` - opt in per-connection (`SQLite` default off).
//!   4. `run_migrations` to land the latest schema.

use std::path::Path;
use std::time::Duration;

use parking_lot::Mutex;
use r2d2::PooledConnection;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{Connection, OpenFlags};

use super::error::{IndexError, Result};
use super::migrate;

/// Default size of the read pool. Four matches the four MCP probe tasks
/// in docs/08's threading model and is plenty for a typical desktop
/// workload (frontend + watcher + relation recomputer).
const READ_POOL_SIZE: u32 = 4;

/// Type alias for a pooled read connection. Exposed in signatures so
/// callers don't need to import `r2d2::PooledConnection<...>` directly.
pub type ReadConnection = PooledConnection<SqliteConnectionManager>;

/// Owns the `SQLite` read pool and the single write connection.
///
/// Construct via `open` (file-backed) or `open_in_memory` (tests). All
/// I/O happens through `read` / `write`; the inner connections never
/// leave this struct.
///
/// The manual `Debug` impl avoids dragging the underlying `Connection`
/// and `Pool` into the formatter (neither implements `Debug`); we
/// render a fixed type-name marker instead, which is enough for
/// `expect_err` in tests and for `tracing` spans.
pub struct IndexHandle {
    /// Write connection - serialised through a `parking_lot::Mutex`.
    write: Mutex<Connection>,
    /// Read pool - r2d2 manages the connection lifecycle.
    read: r2d2::Pool<SqliteConnectionManager>,
}

impl std::fmt::Debug for IndexHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Skip the inner write/read fields - neither rusqlite::Connection
        // nor r2d2::Pool implements Debug, and the type alone is enough
        // for the tests and tracing diagnostics that need it.
        f.debug_struct("IndexHandle").finish_non_exhaustive()
    }
}

/// PRAGMA setup that every fresh connection needs. Centralised here so
/// the read-pool init hook and the write-connection bootstrap stay in
/// sync.
fn apply_pragmas(conn: &Connection) -> Result<()> {
    // WAL mode is per-database (persisted in the file header) but we
    // set it on every connection so an in-memory DB picks it up too.
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    Ok(())
}

impl IndexHandle {
    /// Open (or create) the index at `path` and run migrations.
    ///
    /// Errors:
    /// - `IndexError::PathNotFile` if `path` exists and is a directory.
    /// - `IndexError::Io` if creating the parent directory fails.
    /// - `IndexError::Sqlite` for any rusqlite-side problem.
    /// - `IndexError::SchemaVersionMismatch` if the DB was written by a
    ///   newer build.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();

        // Reject early if the caller pointed us at a directory. SQLite
        // would happily try and emit a confusing error; we want a clear
        // one.
        if path.exists() && !path.is_file() {
            return Err(IndexError::PathNotFile(path.to_path_buf()));
        }

        // Ensure the parent dir exists so first-run on a clean machine
        // doesn't panic on "no such file or directory".
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                std::fs::create_dir_all(parent)?;
            }
        }

        // Build the read pool first. `with_init` runs the PRAGMAs every
        // time r2d2 hands out a fresh connection.
        let manager = SqliteConnectionManager::file(path)
            .with_flags(OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX | OpenFlags::SQLITE_OPEN_URI)
            .with_init(|c| {
                // Read-only conns can't change journal_mode; just set the
                // flags that affect reader behaviour.
                c.pragma_update(None, "foreign_keys", "ON")?;
                Ok(())
            });

        // We open the writer FIRST so it gets a chance to create the
        // file before any reader pool conn races to open it read-only.
        let mut writer = Connection::open(path)?;
        // `busy_timeout` lets the writer back off briefly when a reader
        // pool conn is mid-snapshot. 1s matches docs/05's exponential
        // backoff cap.
        writer.busy_timeout(Duration::from_secs(1))?;
        apply_pragmas(&writer)?;
        migrate::run_migrations(&mut writer)?;

        let pool = r2d2::Pool::builder()
            .max_size(READ_POOL_SIZE)
            // Don't hang the UI thread forever waiting for a slot.
            .connection_timeout(Duration::from_secs(5))
            .build(manager)?;

        Ok(Self {
            write: Mutex::new(writer),
            read: pool,
        })
    }

    /// Open a fresh in-memory database. Tests only - the pool's reader
    /// connections see a separate database from the writer because
    /// `SQLite` in-memory DBs are per-connection. We work around that
    /// by using a shared-cache URI so reads and writes converge.
    ///
    /// Each call gets a process-unique in-memory name so concurrent
    /// tests don't collide on a single shared backing store. Without
    /// the unique tag, `cargo test` would run two `open_in_memory`
    /// invocations against the same `file::memory:?cache=shared` URI,
    /// and the second migration run would trip over the FTS5 virtual
    /// table created by the first ("database schema is locked").
    pub fn open_in_memory() -> Result<Self> {
        // Process-unique counter to disambiguate concurrent test cases.
        // `AtomicU64` is cheap, monotonic, and avoids dragging in extra
        // deps (uuid lives in the `fs` module, not here).
        use std::sync::atomic::{AtomicU64, Ordering};
        static IN_MEMORY_COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = IN_MEMORY_COUNTER.fetch_add(1, Ordering::Relaxed);
        let uri = format!("file:aseye_mem_{n}?mode=memory&cache=shared");

        let manager = SqliteConnectionManager::file(&uri)
            .with_flags(
                OpenFlags::SQLITE_OPEN_READ_WRITE
                    | OpenFlags::SQLITE_OPEN_URI
                    | OpenFlags::SQLITE_OPEN_CREATE
                    | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            )
            .with_init(|c| {
                c.pragma_update(None, "foreign_keys", "ON")?;
                Ok(())
            });

        let mut writer = Connection::open_with_flags(
            &uri,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_URI
                | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        writer.busy_timeout(Duration::from_secs(1))?;
        // In-memory DBs reject `journal_mode=WAL` (no file to back the
        // log against) - apply only the safe subset.
        writer.pragma_update(None, "synchronous", "NORMAL")?;
        writer.pragma_update(None, "foreign_keys", "ON")?;
        migrate::run_migrations(&mut writer)?;

        let pool = r2d2::Pool::builder()
            .max_size(READ_POOL_SIZE)
            .connection_timeout(Duration::from_secs(5))
            .build(manager)?;

        Ok(Self {
            write: Mutex::new(writer),
            read: pool,
        })
    }

    /// Run a closure with the write connection held.
    ///
    /// The lock is held only for the duration of `f` - keep it short.
    /// The caller decides whether to wrap multi-statement work in a
    /// transaction by calling `conn.transaction()` themselves.
    pub fn write<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&Connection) -> Result<T>,
    {
        let guard = self.write.lock();
        f(&guard)
    }

    /// Run a closure with a read-only pooled connection.
    ///
    /// The pool will block up to 5s if all conns are in use. Errors
    /// from acquisition map to `IndexError::Pool`.
    pub fn read<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&ReadConnection) -> Result<T>,
    {
        let conn = self.read.get()?;
        f(&conn)
    }

    /// Run `PRAGMA integrity_check` against the writer. Returns true
    /// iff every check returned the literal string `"ok"`. Useful at
    /// app launch to detect corruption (per docs/05 "Failure modes").
    pub fn integrity_check(&self) -> Result<bool> {
        self.write(|conn| {
            // PRAGMA integrity_check returns one row per problem, or
            // a single "ok" row when the DB is healthy.
            let mut stmt = conn.prepare("PRAGMA integrity_check;")?;
            let rows: Vec<String> = stmt
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows.len() == 1 && rows[0] == "ok")
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn fts_index_works() {
        let handle = IndexHandle::open_in_memory().expect("open in-memory");
        // Insert directly into the FTS5 virtual table to exercise it
        // end-to-end. Real upserts go through component_fts via the
        // index writer, but the FTS engine is what we want to verify.
        handle
            .write(|c| {
                c.execute(
                    "INSERT INTO component_fts (id, name, description, body) VALUES ('aseye://x/y/z/foo', 'foo skill', 'does foo things', 'this body mentions foo and bar')",
                    [],
                )?;
                Ok(())
            })
            .unwrap();

        let hits: Vec<String> = handle
            .read(|c| {
                let mut stmt = c.prepare(
                    "SELECT id FROM component_fts WHERE component_fts MATCH ?1",
                )?;
                let rows = stmt
                    .query_map(["foo"], |row| row.get::<_, String>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .unwrap();

        assert_eq!(hits, vec!["aseye://x/y/z/foo".to_string()]);
    }

    #[test]
    fn integrity_check_passes() {
        let handle = IndexHandle::open_in_memory().expect("open in-memory");
        assert!(handle.integrity_check().unwrap());
    }

    #[test]
    fn read_pool_concurrent() {
        let handle = Arc::new(IndexHandle::open_in_memory().expect("open in-memory"));
        let mut threads = Vec::new();
        for _ in 0..4 {
            let h = Arc::clone(&handle);
            threads.push(thread::spawn(move || {
                h.read(|c| {
                    let n: i64 = c
                        .query_row("SELECT COUNT(*) FROM component", [], |row| row.get(0))?;
                    Ok(n)
                })
                .unwrap()
            }));
        }
        for t in threads {
            assert_eq!(t.join().unwrap(), 0);
        }
    }

    #[test]
    fn write_lock_serialises() {
        let handle = Arc::new(IndexHandle::open_in_memory().expect("open in-memory"));
        // Insert a parent component first because tag has no FK but we
        // want the data realistic. (Tag table itself has no FK.)
        let h1 = Arc::clone(&handle);
        let t1 = thread::spawn(move || {
            h1.write(|c| {
                c.execute(
                    "INSERT INTO tag (component_id, tag) VALUES ('aseye://t1', 'thread1')",
                    [],
                )?;
                Ok(())
            })
            .unwrap();
        });
        let h2 = Arc::clone(&handle);
        let t2 = thread::spawn(move || {
            h2.write(|c| {
                c.execute(
                    "INSERT INTO tag (component_id, tag) VALUES ('aseye://t2', 'thread2')",
                    [],
                )?;
                Ok(())
            })
            .unwrap();
        });
        t1.join().unwrap();
        t2.join().unwrap();

        let total: i64 = handle
            .read(|c| {
                Ok(c.query_row("SELECT COUNT(*) FROM tag", [], |row| row.get(0))?)
            })
            .unwrap();
        assert_eq!(total, 2);
    }

    #[test]
    fn wal_mode_set_on_disk() {
        // WAL mode only applies to file-backed databases; an in-memory
        // DB stays on the default "memory" journal. Use a tempdir.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("aseye-wal-test.sqlite");
        let handle = IndexHandle::open(&path).expect("open file-backed");
        let mode: String = handle
            .write(|c| {
                Ok(c.query_row("PRAGMA journal_mode", [], |row| row.get::<_, String>(0))?)
            })
            .unwrap();
        assert_eq!(mode.to_lowercase(), "wal");
    }

    #[test]
    fn rejects_directory_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        let err = IndexHandle::open(dir.path()).expect_err("must reject dir");
        match err {
            IndexError::PathNotFile(p) => assert_eq!(p, dir.path()),
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn open_creates_parent_dir() {
        let dir = tempfile::tempdir().expect("tempdir");
        // Two levels of nesting that don't exist yet.
        let path = dir.path().join("nested/dir/index.sqlite");
        assert!(!path.parent().unwrap().exists());
        let _handle = IndexHandle::open(&path).expect("open should create parents");
        assert!(path.parent().unwrap().exists());
        assert!(path.exists());
    }
}
