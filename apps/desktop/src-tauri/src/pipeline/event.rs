//! Pipeline event types - the post-classification stream.
//!
//! `PipelineEvent` is what every downstream consumer of the live-index
//! pipeline subscribes to (the IPC bridge that emits Tauri events, the
//! debug view, future telemetry counters). It is intentionally narrower
//! than `WatchEvent` because by the time an event leaves the pipeline:
//!
//! * The path has been classified into a `ToolId` + `ComponentType`.
//! * The index row has already been written.
//! * The component identity is the `aseye://` URI, not a filesystem
//!   path.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::index::UpsertKind;

/// Event emitted by the pipeline after a successful classify + index
/// step.
///
/// Serialised as a tagged union with `event` as the discriminator so it
/// doesn't collide with the `kind: UpsertKind` field on
/// `ComponentUpserted`. The TS bindings (and the React side via
/// `pipeline-event` Tauri events) use the same shape.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", tag = "event")]
#[ts(export, export_to = "../bindings/pipeline/PipelineEvent.ts")]
#[ts(rename_all = "camelCase")]
pub enum PipelineEvent {
    /// A component was inserted, updated, or confirmed unchanged. The
    /// `kind` discriminates between the three so the UI can skip
    /// invalidation work for `Unchanged` events.
    ComponentUpserted { id: String, kind: UpsertKind },
    /// A component's source file was removed and the row scrubbed.
    ComponentDeleted { id: String },
    /// Parse failed; the row is still present with `parse_errors`
    /// populated. The IPC layer surfaces this as a UI badge per
    /// `docs/05-data-architecture.md` "Failure modes".
    ParseError { id: String, path: String },
    /// A full scan completed.
    ScanCompleted { report: ScanReport },
}

/// Summary of a `Pipeline::full_scan` run. Returned synchronously from
/// the `start_full_scan` command and also emitted as a
/// `PipelineEvent::ScanCompleted` so subscribers can refresh totals.
#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../bindings/pipeline/ScanReport.ts")]
#[ts(rename_all = "camelCase")]
pub struct ScanReport {
    /// Number of tools the scan walked (i.e. detected tools).
    pub tools_scanned: u32,
    /// Number of components seen during the walk (one per file match).
    pub components_seen: u32,
    /// Number of components inserted for the first time during this
    /// scan.
    pub components_inserted: u32,
    /// Number of components whose row was updated.
    pub components_updated: u32,
    /// Number of components whose hash matched the existing row.
    pub components_unchanged: u32,
    /// Number of files that failed to parse but still recorded a row.
    pub parse_errors: u32,
}
