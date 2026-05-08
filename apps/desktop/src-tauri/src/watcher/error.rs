//! Error type for the `watcher` module.
//!
//! `WatcherError` is the unified error returned by every public function in
//! this module. Variants are deliberately granular so callers (and the IPC
//! layer in Phase 1.6) can map specific failures to user-facing messages
//! without parsing nested strings.

use std::io;
use std::path::PathBuf;

use thiserror::Error;

use crate::fs::FsError;

/// Errors emitted by the file watcher.
#[derive(Error, Debug)]
pub enum WatcherError {
    /// `notify::Watcher::new` or platform watcher init failed.
    #[error("failed to initialise file watcher: {source}")]
    Init {
        #[source]
        source: notify::Error,
    },

    /// `notify::Watcher::watch` failed for a reason other than the OS watch
    /// limit. The original `notify::Error` is preserved for diagnostics.
    #[error("failed to watch `{path}`: {source}")]
    Watch {
        path: PathBuf,
        #[source]
        source: notify::Error,
    },

    /// `notify::Watcher::unwatch` failed.
    #[error("failed to unwatch `{path}`: {source}")]
    Unwatch {
        path: PathBuf,
        #[source]
        source: notify::Error,
    },

    /// `MaxFilesWatch` (Linux inotify saturation) or its platform equivalent.
    /// Surfaces TR-3 from `docs/11-risks.md`. The IPC layer turns this into a
    /// UI-visible warning suggesting the recommended sysctl value.
    ///
    /// Recommended remediation:
    /// `sudo sysctl -w fs.inotify.max_user_watches=524288`
    #[error(
        "OS watch limit exceeded; consider raising `fs.inotify.max_user_watches` to {recommended_value}"
    )]
    WatchLimitExceeded { recommended_value: u32 },

    /// The requested path canonicalised to a location outside the trusted
    /// roots passed to `Watcher::start`. Mirrors SR-3 from `docs/11-risks.md`
    /// ("path traversal via tool config") and the same containment rule used
    /// by `fs::safety::assert_within_root`.
    #[error("path `{path}` escapes all trusted roots")]
    PathEscape { path: PathBuf },

    /// Path canonicalisation failed (does not exist, permission denied, ...).
    #[error("failed to canonicalise `{path}`: {source}")]
    Canonicalize {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    /// Containment check delegated to `fs::safety` failed for a non-escape
    /// reason (e.g. `Canonicalize` on a missing root). Wraps the underlying
    /// `FsError` rather than re-encoding it.
    #[error("file-system safety check failed: {source}")]
    Fs {
        #[source]
        source: FsError,
    },
}

impl WatcherError {
    /// Map a raw `notify::Error` returned by `Watcher::watch` into our typed
    /// variant. Splits the `MaxFilesWatch` case out so the IPC layer can react
    /// to it without string-matching.
    pub(crate) fn from_watch_error(path: PathBuf, source: notify::Error) -> Self {
        if matches!(source.kind, notify::ErrorKind::MaxFilesWatch) {
            // 524288 is the value recommended by the kernel docs and matches
            // what we surface in `docs/11-risks.md` TR-3.
            return Self::WatchLimitExceeded {
                recommended_value: 524_288,
            };
        }
        Self::Watch { path, source }
    }
}
