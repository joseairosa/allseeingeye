//! Shared report + error types for the backup IPC surface.
//!
//! Mirrors the contract in `docs/15-backup-and-restore.md` section
//! 15.7 ("IPC surface"). All types derive `TS` so the React side
//! consumes generated bindings under `bindings/backup/` rather than
//! hand-written shapes.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Outcome of a `backup_now` sweep.
///
/// Per-component failures collect in `errors` but do NOT abort the
/// pass - the sweep continues so one bad row does not lose another
/// component's chance at a fresh blob.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../bindings/backup/BackupReport.ts")]
#[ts(rename_all = "camelCase")]
pub struct BackupReport {
    /// Total number of indexed components considered.
    pub total: u32,
    /// Components encrypted + written this pass (insert or refresh).
    pub encrypted: u32,
    /// Components whose plaintext hash matched the manifest - no work
    /// performed.
    pub skipped_unchanged: u32,
    /// Per-component failures. Each entry carries enough context for
    /// the UI to surface a row-level error toast without a follow-up
    /// IPC call.
    pub errors: Vec<BackupErrorEntry>,
    /// Wall-clock duration of the pass in milliseconds.
    pub elapsed_ms: u64,
}

/// One row in `BackupReport.errors`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../bindings/backup/BackupErrorEntry.ts")]
#[ts(rename_all = "camelCase")]
pub struct BackupErrorEntry {
    pub component_id: String,
    pub kind: BackupErrorKind,
    pub message: String,
}

/// Coarse category for a backup-pass per-component failure.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../bindings/backup/BackupErrorKind.ts")]
#[ts(rename_all = "camelCase")]
pub enum BackupErrorKind {
    /// Reading the source file failed (permissions, EIO, missing).
    Read,
    /// Hashing or encrypting the bytes failed.
    Encrypt,
    /// Writing the encrypted blob to storage failed.
    Write,
    /// Could not access the keychain (e.g. Linux without libsecret).
    KeychainUnavailable,
    /// Manifest insert/update against `SQLite` failed.
    Manifest,
}

/// Outcome of a `restore_now` sweep.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../bindings/backup/RestoreReport.ts")]
#[ts(rename_all = "camelCase")]
pub struct RestoreReport {
    /// Total entries in the manifest considered.
    pub total: u32,
    /// Files actually restored to disk.
    pub restored: u32,
    /// Files skipped because the local copy was newer than the backup
    /// (`encrypted_at`).
    pub skipped_local_newer: u32,
    /// Per-entry failures.
    pub errors: Vec<RestoreErrorEntry>,
    /// Wall-clock duration of the pass in milliseconds.
    pub elapsed_ms: u64,
    /// Whether this was a dry-run (no writes performed).
    pub dry_run: bool,
}

/// One row in `RestoreReport.errors`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../bindings/backup/RestoreErrorEntry.ts")]
#[ts(rename_all = "camelCase")]
pub struct RestoreErrorEntry {
    pub component_id: String,
    pub kind: RestoreErrorKind,
    pub message: String,
}

/// Coarse category for a restore-pass per-entry failure.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../bindings/backup/RestoreErrorKind.ts")]
#[ts(rename_all = "camelCase")]
pub enum RestoreErrorKind {
    /// Reading the encrypted blob from storage failed.
    Read,
    /// Decryption (auth-tag mismatch, malformed header, version
    /// rejection) failed.
    Decrypt,
    /// Atomic write to the target component path failed.
    Write,
    /// Could not access the keychain.
    KeychainUnavailable,
    /// The component row referenced by the manifest no longer exists
    /// (e.g. user removed the file before running restore).
    ComponentMissing,
    /// The target path's parent directory cannot be reached (e.g. a
    /// project root that no longer exists).
    PathUnreachable,
}
