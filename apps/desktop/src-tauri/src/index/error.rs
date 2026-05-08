//! Errors raised by the index layer.
//!
//! `IndexError` is the single error type returned from every public function
//! in this module. We wrap the upstream errors from `rusqlite` and `r2d2`
//! and add our own variants for migration mismatches and path-shape
//! problems detected on `open`. Conversions are derived through
//! `thiserror::Error` so callers get clean `?` propagation.

use std::path::PathBuf;

/// Errors returned by the `SQLite` index module.
///
/// Variants are kept narrow and named after the failure they describe so
/// callers (Tauri commands, IPC layer) can map them to user-facing
/// messages without inspecting nested causes.
#[derive(Debug, thiserror::Error)]
pub enum IndexError {
    /// Underlying rusqlite error - covers IO, busy, constraint, etc.
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    /// r2d2 connection-pool error (e.g. timed out acquiring a read conn).
    #[error("connection pool error: {0}")]
    Pool(#[from] r2d2::Error),

    /// The on-disk schema version is newer than the embedded migrations
    /// know how to run. We refuse to open such a database to avoid
    /// silently corrupting data written by a future build.
    #[error("schema version mismatch: db is at v{found}, embedded migrations only know up to v{known}")]
    SchemaVersionMismatch {
        /// Version stored in the on-disk `schema_version` table.
        found: u32,
        /// Highest version the running binary's embedded migrations know.
        known: u32,
    },

    /// Caller passed a path that exists but is not a regular file. We
    /// surface this explicitly because `rusqlite` would otherwise fall
    /// through to a low-level "unable to open" error.
    #[error("path is not a regular file: {0}")]
    PathNotFile(PathBuf),

    /// Standard `io::Error` - mostly raised when creating the parent
    /// directory of the database file fails.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// Convenience alias for module-internal `Result`s.
pub type Result<T> = std::result::Result<T, IndexError>;
