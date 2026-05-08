//! Public watcher event type.
//!
//! `WatchEvent` is the post-coalescer event the rest of the crate consumes
//! (parser dispatch, index writer, IPC layer). It is intentionally narrower
//! than `notify::Event`: by the time an event leaves the coalescer it has
//! already been classified, deduplicated within the 200 ms window, and -
//! where applicable - paired (rename from + rename to).
//!
//! `PathBuf` is preserved at the wire layer here. The IPC boundary in Phase
//! 1.6 is responsible for converting paths to UTF-8 strings before sending to
//! the React side, alongside the same conversion every other Rust-typed path
//! goes through.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// One coalesced file-system event.
///
/// Variants:
/// * `Created`  - a path that did not exist now does.
/// * `Modified` - the contents of an existing path changed (covers both raw
///   modify events and "atomic save" delete+create bursts that collapse to
///   a single `Modified`).
/// * `Deleted`  - a path that existed no longer does.
/// * `Renamed`  - a single `from -> to` move where both endpoints arrived
///   within the coalescing window. If only one half arrives, the coalescer
///   downgrades to `Deleted` (from-only) or `Created` (to-only).
#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase", tag = "kind")]
#[ts(export, export_to = "../bindings/watcher/WatchEvent.ts")]
#[ts(rename_all = "camelCase")]
pub enum WatchEvent {
    Created { path: PathBuf },
    Modified { path: PathBuf },
    Deleted { path: PathBuf },
    Renamed { from: PathBuf, to: PathBuf },
}

impl WatchEvent {
    /// The "primary" path for this event. For `Renamed`, this is the
    /// destination - the path now on disk. Used by indexers that key on
    /// "where is the file now".
    #[must_use]
    pub fn primary_path(&self) -> &PathBuf {
        match self {
            Self::Created { path } | Self::Modified { path } | Self::Deleted { path } => path,
            Self::Renamed { to, .. } => to,
        }
    }
}
