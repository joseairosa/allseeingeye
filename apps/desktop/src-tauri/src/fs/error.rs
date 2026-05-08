//! Error types for the `fs` module.
//!
//! `FsError` is the unified error type returned by every public function in
//! this module. Variants are deliberately granular so callers (and tests)
//! can distinguish *which* phase of an atomic write or safety check failed
//! without parsing nested `io::Error` messages.

use std::io;
use std::path::PathBuf;

use thiserror::Error;

/// Errors emitted by the `fs::atomic` and `fs::safety` modules.
#[derive(Error, Debug)]
pub enum FsError {
    /// Failed to create the parent directory tree before writing.
    #[error("failed to create parent directory `{path}`: {source}")]
    ParentMkdir {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    /// `O_CREAT|O_EXCL` failed when opening the temp file. Either a temp-name
    /// collision (extremely unlikely with v4 UUIDs) or a deeper FS problem.
    #[error("failed to create temp file `{path}`: {source}")]
    TempCreate {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    /// Writing bytes to the temp file failed (disk full, EIO, etc.).
    #[error("failed to write to temp file `{path}`: {source}")]
    Write {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    /// `fsync(2)` failed on the temp file or parent directory.
    #[error("fsync failed on `{path}`: {source}")]
    Fsync {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    /// `rename(2)` of temp -> final path failed. On POSIX this is the only
    /// step that is *not* automatically rolled back; if it fails the original
    /// file (if any) is still intact and the temp is cleaned up.
    #[error("rename `{from}` -> `{to}` failed: {source}")]
    Rename {
        from: PathBuf,
        to: PathBuf,
        #[source]
        source: io::Error,
    },

    /// Could not fsync the parent directory after rename. POSIX only.
    #[error("parent directory fsync failed on `{path}`: {source}")]
    ParentFsync {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    /// Target already exists in a state that prevents safe write (rare —
    /// reserved for future locking semantics, currently unused but kept so
    /// callers can match exhaustively without a churn later).
    #[error("target `{path}` is locked or already exists")]
    AlreadyExists { path: PathBuf },

    /// Path canonicalisation failed (does not exist, permission denied, etc.).
    #[error("failed to canonicalise `{path}`: {source}")]
    Canonicalize {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    /// A path resolved outside its declared root after symlink expansion.
    #[error("path `{path}` escapes root `{root}`")]
    EscapeDetected { path: PathBuf, root: PathBuf },

    /// Path contains a forbidden segment (`.git`, `node_modules`, etc.).
    #[error("path `{path}` contains forbidden segment `{segment}`")]
    ForbiddenSegment { path: PathBuf, segment: String },

    /// Path resolved outside the user's home directory and the caller did
    /// not opt in via `allow_outside_home`.
    #[error("path `{path}` is outside the user home directory")]
    OutsideHome { path: PathBuf },

    /// Path did not match any of the registered roots passed to
    /// `safe_atomic_write`.
    #[error("path `{path}` is not within any registered root")]
    NotInAnyRoot { path: PathBuf },

    /// `dirs::home_dir()` returned `None`. Surfaced as a typed error so tests
    /// and callers can react sensibly on exotic platforms.
    #[error("could not determine user home directory")]
    HomeUnavailable,
}
