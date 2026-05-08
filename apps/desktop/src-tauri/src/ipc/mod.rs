//! Tauri IPC commands and events.
//!
//! Phase 1.6 wires the read-only command surface plus the event bridge:
//!
//! Commands (read-only):
//! * `list_tools` - registry detection (Phase 1.1).
//! * `list_components` - filtered component summaries.
//! * `get_component` - full row including parsed JSON + parse errors.
//! * `search` - FTS5 query with snippets.
//! * `start_full_scan` - synchronous full scan, returns a `ScanReport`.
//! * `get_health_summary` - per-tool / per-kind component counts.
//!
//! Events (server -> client) bridged from `Pipeline::subscribe_events`:
//! * `pipeline-event` carrying a `PipelineEvent` payload.
//!
//! Mutating commands (saveComponent, toggle, tag, pin, exportBundle, ...)
//! are deliberately out of scope for this phase; they land in Phase 1.7.

pub mod commands;
pub mod types;

use tauri::{AppHandle, Emitter, Runtime};
use tokio::sync::broadcast;

use crate::pipeline::PipelineEvent;

/// Spawn a Tokio task that bridges the pipeline's broadcast channel to
/// Tauri events. Every `PipelineEvent` is emitted via
/// `app.emit("pipeline-event", payload)`.
///
/// Returns immediately; the bridge runs until the broadcast sender is
/// dropped (i.e. the pipeline tears down).
pub fn spawn_event_bridge<R: Runtime>(
    app: AppHandle<R>,
    mut rx: broadcast::Receiver<PipelineEvent>,
) {
    tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(event) => {
                    if let Err(err) = app.emit("pipeline-event", &event) {
                        tracing::warn!(?err, "failed to emit pipeline-event");
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!(skipped = n, "ipc event bridge lagged");
                }
                Err(broadcast::error::RecvError::Closed) => {
                    tracing::debug!("ipc event bridge exiting; pipeline closed");
                    break;
                }
            }
        }
    });
}
