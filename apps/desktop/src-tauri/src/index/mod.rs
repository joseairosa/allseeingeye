//! `SQLite` + FTS5 index.
//!
//! Phase 1.2 lands the schema, forward-only migrations, and the
//! `IndexHandle` that owns the read pool + single write connection.
//! Public surface area is intentionally tight - everything consumers
//! need is re-exported here.
//!
//! Layers:
//!   - `schema`  - one `pub const` per CREATE statement, mirrored from
//!     `docs/05-data-architecture.md`.
//!   - `migrate` - forward-only migration runner.
//!   - `conn`    - `IndexHandle` (read pool + write conn + PRAGMAs).
//!   - `error`   - `IndexError` / `Result` shared by the module.

pub mod conn;
pub mod error;
pub mod migrate;
pub mod schema;

use std::path::PathBuf;

pub use conn::{IndexHandle, ReadConnection};
pub use error::{IndexError, Result};

/// Per-platform default path for the index database.
///
/// - macOS:   `~/Library/Application Support/AllSeeingEye/index.sqlite`
/// - Linux:   `${XDG_DATA_HOME:-~/.local/share}/AllSeeingEye/index.sqlite`
/// - Windows: `%APPDATA%/AllSeeingEye/index.sqlite`
///
/// Falls back to `./AllSeeingEye/index.sqlite` if `dirs` cannot resolve
/// a platform data dir (extremely rare; mostly happens in stripped-down
/// container environments). The parent directory is NOT created here -
/// `IndexHandle::open` creates it on first launch.
#[must_use]
pub fn default_db_path() -> PathBuf {
    let base = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join("AllSeeingEye").join("index.sqlite")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_db_path_under_data_dir() {
        let p = default_db_path();
        // Path always ends in our app dir + filename, regardless of host.
        let last_two: Vec<&std::ffi::OsStr> = p
            .iter()
            .rev()
            .take(2)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        assert_eq!(last_two[0], std::ffi::OsStr::new("AllSeeingEye"));
        assert_eq!(last_two[1], std::ffi::OsStr::new("index.sqlite"));
    }

    #[test]
    fn open_at_default_path_shape_succeeds() {
        // We don't open the real default path during tests (would
        // pollute the user's data dir). Instead we synthesise the same
        // shape under a tempdir and verify open() creates the parent.
        let dir = tempfile::tempdir().expect("tempdir");
        let synthetic = dir.path().join("AllSeeingEye").join("index.sqlite");
        assert!(!synthetic.parent().unwrap().exists());

        let handle = IndexHandle::open(&synthetic).expect("open synthetic default");
        assert!(synthetic.parent().unwrap().exists());
        assert!(synthetic.exists());
        assert!(handle.integrity_check().unwrap());
    }
}
