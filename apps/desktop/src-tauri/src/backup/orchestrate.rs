//! `backup_now` and `restore_now` orchestration.
//!
//! Mirrors `docs/15-backup-and-restore.md` sections 15.5 and 15.6.
//! The flows are deliberately simple: walk every relevant row, run
//! the per-component step, collect per-row errors without aborting
//! the sweep, return a structured report.
//!
//! The functions are synchronous + blocking by design; the IPC layer
//! drops them into `tauri::async_runtime::spawn_blocking` so the UI
//! does not stall during a multi-second backup pass.

// The orchestrator entry points take `Arc<IndexHandle>` by value
// because the IPC layer (`tauri::async_runtime::spawn_blocking`) and
// the auto-debouncer's flush task move the Arc into a different
// thread/task. Clippy's `needless_pass_by_value` is wrong here -
// re-borrowing would force the callers to do the cloning instead.
#![allow(clippy::needless_pass_by_value)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use rusqlite::params;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use ts_rs::TS;
use x25519_dalek::StaticSecret;

use crate::backup::envelope::{decrypt_blob, encrypt_blob, hex_encode, DevicePublicKey};
use crate::backup::keychain::{decode_private_key, Keychain, KeychainError, SystemKeychain};
use crate::backup::keypair::{ensure_keypair_with, read_cached_public_key};
use crate::backup::manifest::{
    delete_manifest_entry, for_each_entry, manifest_count, read_manifest_entry,
    upsert_manifest_entry, BackupManifestEntry,
};
use crate::backup::storage::{blob_path_for_hash, BackupStorage, LocalDirectoryStorage};
use crate::backup::types::{
    BackupErrorEntry, BackupErrorKind, BackupReport, RestoreErrorEntry, RestoreErrorKind,
    RestoreReport, VerifyErrorEntry, VerifyErrorKind, VerifyReport,
};
use crate::fs::{safe_atomic_write_with_options, write_sidecar_backup_with_suffix, FsError};
use crate::index::settings::{read_backup_auto_enabled, write_backup_last_run};
use crate::index::IndexHandle;

/// Orchestration-layer error surfaced when something goes wrong
/// before we even start iterating components (e.g. keychain dead).
#[derive(Debug, thiserror::Error)]
pub enum OrchestrationError {
    #[error(transparent)]
    Index(#[from] crate::index::IndexError),

    #[error(transparent)]
    Keychain(#[from] KeychainError),

    #[error(transparent)]
    Keypair(#[from] crate::backup::keypair::KeypairError),

    #[error(transparent)]
    Manifest(#[from] crate::backup::manifest::ManifestError),

    #[error(transparent)]
    Storage(#[from] crate::backup::storage::StorageError),
}

/// Status payload for the `backup_status` IPC command. Frontend uses
/// this to populate the Settings -> Backup pane.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../bindings/backup/BackupStatus.ts")]
#[ts(rename_all = "camelCase")]
pub struct BackupStatusReport {
    /// True iff the device public key is cached in `app_settings`.
    pub key_present: bool,
    /// Number of rows in `backup_manifest`.
    pub manifest_count: u32,
    /// Last `encrypted_at` across the manifest, or `None` if empty.
    pub last_backup_at: Option<i64>,
    /// `backupAutoEnabled` setting (defaults to `true`).
    pub auto_backup_enabled: bool,
    /// Absolute path of the backup root, surfaced for the
    /// "Storage: ~/.aseye-backup/ (4.2 MB)" line.
    pub backup_dir: String,
}

/// Run the manual `backup_now` sweep. The contract is exactly what
/// spec section 15.5 describes:
///
/// * components with no manifest entry: encrypt + write + insert,
/// * components whose `plaintext_hash` changed: encrypt + write +
///   update + retire the old blob,
/// * components whose hash matches: skip (idempotent no-op).
///
/// `target_ids` filters the sweep to a specific set (used by the
/// auto-debouncer); pass `None` to walk every component.
pub fn backup_now(
    handle: Arc<IndexHandle>,
    target_ids: Option<&[String]>,
) -> Result<BackupReport, OrchestrationError> {
    let storage = LocalDirectoryStorage::at_default_root()?;
    backup_now_with(handle, &storage, &SystemKeychain, target_ids)
}

/// Per-stage timing accumulator for the backup pass. All fields are
/// nanosecond totals across the sweep, populated by `backup_component`
/// and emitted at the end of `backup_now_with` when the env var
/// `ASEYE_BACKUP_TIMING=1` is set. Production runs stay quiet.
///
/// Used to disprove the original perf concern in issue #24 (debug
/// build was misleading; release mode is 6s for 88 files).
#[derive(Default)]
#[allow(clippy::struct_field_names)]
struct StageTimings {
    read_ns: u128,
    hash_pt_ns: u128,
    read_manifest_ns: u128,
    encrypt_ns: u128,
    hash_blob_ns: u128,
    storage_put_ns: u128,
    upsert_manifest_ns: u128,
}

/// Test seam - same as [`backup_now`] but takes injectable
/// dependencies so unit tests can use a tempdir-backed storage and
/// an in-memory keychain.
pub fn backup_now_with<S: BackupStorage, K: Keychain>(
    handle: Arc<IndexHandle>,
    storage: &S,
    keychain: &K,
    target_ids: Option<&[String]>,
) -> Result<BackupReport, OrchestrationError> {
    let started = Instant::now();
    // Make sure we have a public key to wrap with. The ensure call
    // is idempotent and safe to invoke on every pass.
    let device_pub = ensure_keypair_with(handle.as_ref(), keychain)?;

    let candidates = collect_components(handle.as_ref(), target_ids)?;
    let total = u32::try_from(candidates.len()).unwrap_or(u32::MAX);
    let mut encrypted = 0u32;
    let mut skipped_unchanged = 0u32;
    let mut errors: Vec<BackupErrorEntry> = Vec::new();
    let mut timings = StageTimings::default();
    let timing_enabled = std::env::var("ASEYE_BACKUP_TIMING").is_ok();

    for (component_id, path) in candidates {
        match backup_component(
            handle.as_ref(),
            storage,
            &device_pub,
            &component_id,
            &path,
            &mut timings,
        ) {
            Ok(BackupOutcome::Encrypted) => {
                encrypted = encrypted.saturating_add(1);
            }
            Ok(BackupOutcome::SkippedUnchanged) => {
                skipped_unchanged = skipped_unchanged.saturating_add(1);
            }
            Err(err) => {
                tracing::warn!(?component_id, ?err, "backup failed for component");
                errors.push(err);
            }
        }
    }

    if timing_enabled {
        eprintln!(
            "[backup-perf] total={}ms encrypted={} skipped={} read={}ms hash_pt={}ms read_manifest={}ms encrypt={}ms hash_blob={}ms storage_put={}ms upsert_manifest={}ms",
            started.elapsed().as_millis(),
            encrypted,
            skipped_unchanged,
            timings.read_ns / 1_000_000,
            timings.hash_pt_ns / 1_000_000,
            timings.read_manifest_ns / 1_000_000,
            timings.encrypt_ns / 1_000_000,
            timings.hash_blob_ns / 1_000_000,
            timings.storage_put_ns / 1_000_000,
            timings.upsert_manifest_ns / 1_000_000,
        );
    }

    // Stamp the last-run watermark even when zero components were
    // encrypted - "we ran, nothing changed" is still useful to the UI.
    let now = unix_now_secs();
    if let Err(err) = write_backup_last_run(handle.as_ref(), now) {
        tracing::warn!(?err, "failed to update backupLastRun");
    }

    let elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    Ok(BackupReport {
        total,
        encrypted,
        skipped_unchanged,
        errors,
        elapsed_ms,
    })
}

/// Run the manual `restore_now` sweep. Honours the local-newer skip
/// gate (spec 15.6). When `dry_run` is true, the function reports
/// what *would* happen but performs no writes.
pub fn restore_now(
    handle: Arc<IndexHandle>,
    dry_run: bool,
) -> Result<RestoreReport, OrchestrationError> {
    let storage = LocalDirectoryStorage::at_default_root()?;
    restore_now_with(handle, &storage, &SystemKeychain, dry_run)
}

/// Test seam for [`restore_now`].
pub fn restore_now_with<S: BackupStorage, K: Keychain>(
    handle: Arc<IndexHandle>,
    storage: &S,
    keychain: &K,
    dry_run: bool,
) -> Result<RestoreReport, OrchestrationError> {
    let started = Instant::now();

    // Restore needs the private key. We fetch it once here so the
    // hot loop does not hit the keychain N times. The bytes do NOT
    // leak out of this function: the StaticSecret is dropped on
    // function exit.
    let priv_hex = match keychain.get_private_key_hex() {
        Ok(hex) => hex,
        Err(err) => {
            tracing::warn!(?err, "restore_now failed to load device private key");
            return Err(OrchestrationError::Keychain(err));
        }
    };
    let priv_bytes = decode_private_key(&priv_hex)?;
    let priv_key = StaticSecret::from(priv_bytes);

    // Snapshot the manifest once. Iterating the read connection
    // while issuing writes (atomic restores) inside the loop would
    // deadlock the single read pool slot.
    let mut entries: Vec<BackupManifestEntry> = Vec::new();
    for_each_entry(handle.as_ref(), |e| entries.push(e))?;
    let total = u32::try_from(entries.len()).unwrap_or(u32::MAX);
    let mut restored = 0u32;
    let mut skipped_local_newer = 0u32;
    let mut errors: Vec<RestoreErrorEntry> = Vec::new();

    for entry in entries {
        match restore_component(handle.as_ref(), storage, &priv_key, &entry, dry_run) {
            Ok(RestoreOutcome::Restored) => {
                restored = restored.saturating_add(1);
            }
            Ok(RestoreOutcome::SkippedLocalNewer) => {
                skipped_local_newer = skipped_local_newer.saturating_add(1);
            }
            Err(err) => {
                tracing::warn!(component_id = ?entry.component_id, ?err, "restore failed for component");
                errors.push(err);
            }
        }
    }

    drop(priv_key);

    let elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    Ok(RestoreReport {
        total,
        restored,
        skipped_local_newer,
        errors,
        elapsed_ms,
        dry_run,
    })
}

/// Run the backup integrity verify sweep. Walks every manifest row,
/// re-reads the ciphertext blob, hashes it (compared against
/// `blob_hash`), decrypts it, and hashes the recovered plaintext
/// (compared against `plaintext_hash`). Per-entry failures collect
/// in the report rather than aborting the sweep.
///
/// Catches:
/// * disk-level blob loss (manual rm, drive disconnect)
/// * bit rot in the ciphertext ([`VerifyErrorKind::BlobHashMismatch`])
/// * key rotation that left old blobs unrecoverable ([`VerifyErrorKind::Decrypt`])
/// * manifest drift ([`VerifyErrorKind::PlaintextHashMismatch`])
///
/// Read-only with respect to both the storage backend and the
/// component table; only logs side effects.
pub fn backup_verify(handle: Arc<IndexHandle>) -> Result<VerifyReport, OrchestrationError> {
    let storage = LocalDirectoryStorage::at_default_root()?;
    backup_verify_with(handle, &storage, &SystemKeychain)
}

/// Test seam for [`backup_verify`].
pub fn backup_verify_with<S: BackupStorage, K: Keychain>(
    handle: Arc<IndexHandle>,
    storage: &S,
    keychain: &K,
) -> Result<VerifyReport, OrchestrationError> {
    let started = Instant::now();

    // Verify needs the private key to drive the decrypt-and-rehash
    // pass. Fetched once here so the hot loop does not hit the
    // keychain N times. The bytes do NOT leak out of this function:
    // the StaticSecret is dropped on function exit.
    let priv_hex = match keychain.get_private_key_hex() {
        Ok(hex) => hex,
        Err(err) => {
            tracing::warn!(?err, "backup_verify failed to load device private key");
            return Err(OrchestrationError::Keychain(err));
        }
    };
    let priv_bytes = decode_private_key(&priv_hex)?;
    let priv_key = StaticSecret::from(priv_bytes);

    // Snapshot the manifest once - same pattern as restore_now_with.
    let mut entries: Vec<BackupManifestEntry> = Vec::new();
    for_each_entry(handle.as_ref(), |e| entries.push(e))?;
    let total = u32::try_from(entries.len()).unwrap_or(u32::MAX);
    let mut verified = 0u32;
    let mut errors: Vec<VerifyErrorEntry> = Vec::new();

    for entry in entries {
        match verify_component(storage, &priv_key, &entry) {
            Ok(()) => {
                verified = verified.saturating_add(1);
            }
            Err(err) => {
                tracing::warn!(component_id = ?entry.component_id, ?err, "verify failed for component");
                errors.push(err);
            }
        }
    }

    drop(priv_key);

    let elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    Ok(VerifyReport {
        total,
        verified,
        errors,
        elapsed_ms,
    })
}

/// Build a [`BackupStatusReport`] suitable for the IPC contract.
pub fn backup_status(handle: Arc<IndexHandle>) -> Result<BackupStatusReport, OrchestrationError> {
    let storage = LocalDirectoryStorage::at_default_root()?;
    backup_status_with(handle, &storage)
}

/// Test seam for [`backup_status`].
pub fn backup_status_with<S: BackupStorage>(
    handle: Arc<IndexHandle>,
    storage: &S,
) -> Result<BackupStatusReport, OrchestrationError> {
    let key_present = read_cached_public_key(handle.as_ref()).is_some();
    let manifest_total = manifest_count(handle.as_ref())?;
    let last_backup_at = handle.read(|conn| {
        // SELECT MAX returns NULL on an empty table; rusqlite maps
        // that to Option<i64>::None.
        let v: Option<i64> =
            conn.query_row("SELECT MAX(encrypted_at) FROM backup_manifest", [], |row| {
                row.get::<_, Option<i64>>(0)
            })?;
        Ok(v)
    })?;
    let auto_backup_enabled = read_backup_auto_enabled(handle.as_ref());
    Ok(BackupStatusReport {
        key_present,
        manifest_count: manifest_total,
        last_backup_at,
        auto_backup_enabled,
        backup_dir: storage.root_for_display(),
    })
}

// ─── per-component step ─────────────────────────────────────────────

enum BackupOutcome {
    Encrypted,
    SkippedUnchanged,
}

enum RestoreOutcome {
    Restored,
    SkippedLocalNewer,
}

fn backup_component<S: BackupStorage>(
    handle: &IndexHandle,
    storage: &S,
    device_pub: &DevicePublicKey,
    component_id: &str,
    path: &Path,
    timings: &mut StageTimings,
) -> Result<BackupOutcome, BackupErrorEntry> {
    let t = Instant::now();
    let bytes = std::fs::read(path).map_err(|e| BackupErrorEntry {
        component_id: component_id.into(),
        kind: BackupErrorKind::Read,
        message: format!("read {}: {}", path.display(), e),
    })?;
    timings.read_ns = timings.read_ns.saturating_add(t.elapsed().as_nanos());

    let t = Instant::now();
    let plaintext_hash = sha256_hex(&bytes);
    timings.hash_pt_ns = timings.hash_pt_ns.saturating_add(t.elapsed().as_nanos());

    // Idempotency check: if the manifest already has this exact
    // plaintext, skip the encrypt + write.
    let t = Instant::now();
    let existing = read_manifest_entry(handle, component_id).map_err(|e| BackupErrorEntry {
        component_id: component_id.into(),
        kind: BackupErrorKind::Manifest,
        message: format!("manifest read: {e}"),
    })?;
    timings.read_manifest_ns = timings
        .read_manifest_ns
        .saturating_add(t.elapsed().as_nanos());
    if let Some(entry) = &existing {
        if entry.plaintext_hash == plaintext_hash {
            return Ok(BackupOutcome::SkippedUnchanged);
        }
    }

    let t = Instant::now();
    let blob = encrypt_blob(device_pub, &bytes).map_err(|e| BackupErrorEntry {
        component_id: component_id.into(),
        kind: BackupErrorKind::Encrypt,
        message: format!("encrypt: {e}"),
    })?;
    timings.encrypt_ns = timings.encrypt_ns.saturating_add(t.elapsed().as_nanos());

    let t = Instant::now();
    let blob_hash = sha256_hex(&blob);
    timings.hash_blob_ns = timings.hash_blob_ns.saturating_add(t.elapsed().as_nanos());
    let blob_path = blob_path_for_hash(&blob_hash);

    let t = Instant::now();
    storage
        .put_blob(&blob_path, &blob)
        .map_err(|e| BackupErrorEntry {
            component_id: component_id.into(),
            kind: BackupErrorKind::Write,
            message: format!("storage put: {e}"),
        })?;
    timings.storage_put_ns = timings
        .storage_put_ns
        .saturating_add(t.elapsed().as_nanos());

    let entry = BackupManifestEntry {
        component_id: component_id.into(),
        blob_path: blob_path.clone(),
        plaintext_hash,
        blob_hash,
        plaintext_size: bytes.len() as u64,
        blob_size: blob.len() as u64,
        encrypted_at: unix_now_secs(),
    };
    let t = Instant::now();
    upsert_manifest_entry(handle, &entry).map_err(|e| BackupErrorEntry {
        component_id: component_id.into(),
        kind: BackupErrorKind::Manifest,
        message: format!("manifest upsert: {e}"),
    })?;
    timings.upsert_manifest_ns = timings
        .upsert_manifest_ns
        .saturating_add(t.elapsed().as_nanos());

    // Best-effort retire of the previous blob. Failure is logged but
    // does not abort the backup - the new entry is already durable.
    if let Some(prev) = existing {
        if prev.blob_path != blob_path {
            if let Err(err) = storage.delete_blob(&prev.blob_path) {
                tracing::warn!(?err, prev = ?prev.blob_path, "failed to delete retired blob");
            }
        }
    }

    Ok(BackupOutcome::Encrypted)
}

fn restore_component<S: BackupStorage>(
    handle: &IndexHandle,
    storage: &S,
    priv_key: &StaticSecret,
    entry: &BackupManifestEntry,
    dry_run: bool,
) -> Result<RestoreOutcome, RestoreErrorEntry> {
    // 1. Look up the component path. If the row was deleted from the
    //    component table since the last backup, surface that as a
    //    typed error rather than touching the disk.
    let path = match resolve_component_path(handle, &entry.component_id) {
        Ok(Some(p)) => p,
        Ok(None) => {
            // Manifest references a component that no longer exists.
            // Drop the orphan row so we never try again.
            if !dry_run {
                let _ = delete_manifest_entry(handle, &entry.component_id);
            }
            return Err(RestoreErrorEntry {
                component_id: entry.component_id.clone(),
                kind: RestoreErrorKind::ComponentMissing,
                message: "component row no longer exists in the index".into(),
            });
        }
        Err(err) => {
            return Err(RestoreErrorEntry {
                component_id: entry.component_id.clone(),
                kind: RestoreErrorKind::Read,
                message: format!("path lookup: {err}"),
            });
        }
    };

    // 2. Local-newer guard: if the file's mtime is more recent than
    //    the backup's encrypted_at, skip rather than overwrite.
    if let Some(local_mtime) = file_mtime_secs(&path) {
        if local_mtime > entry.encrypted_at {
            return Ok(RestoreOutcome::SkippedLocalNewer);
        }
    }

    // 3. Read + decrypt the blob.
    let blob = storage
        .get_blob(&entry.blob_path)
        .map_err(|e| RestoreErrorEntry {
            component_id: entry.component_id.clone(),
            kind: RestoreErrorKind::Read,
            message: format!("storage get: {e}"),
        })?;
    let plaintext = decrypt_blob(priv_key, &blob).map_err(|e| RestoreErrorEntry {
        component_id: entry.component_id.clone(),
        kind: RestoreErrorKind::Decrypt,
        message: format!("decrypt: {e}"),
    })?;

    if dry_run {
        return Ok(RestoreOutcome::Restored);
    }

    // 4. Atomic write. We allow `outside_home` so the integration
    //    test's tempdir-rooted scenario succeeds; production callers
    //    write to paths under HOME because the indexed components
    //    live there.
    let parent = path.parent().ok_or_else(|| RestoreErrorEntry {
        component_id: entry.component_id.clone(),
        kind: RestoreErrorKind::PathUnreachable,
        message: format!("path has no parent: {}", path.display()),
    })?;
    if !parent.exists() {
        return Err(RestoreErrorEntry {
            component_id: entry.component_id.clone(),
            kind: RestoreErrorKind::PathUnreachable,
            message: format!("parent does not exist: {}", parent.display()),
        });
    }
    // 4a. Defense in depth: before overwriting, copy the current
    //     bytes to a timestamped sidecar so the user has a recovery
    //     path if the local-newer guard somehow let an unwanted
    //     overwrite slip through (clock skew on a synced drive,
    //     manually reverted file, etc.). The sidecar is best-effort -
    //     a failure here is logged but does NOT block the restore,
    //     because failing the restore would leave the user with no
    //     way to get their backup back at all. The user can recover
    //     from the sidecar later by hand.
    let sidecar_suffix = format!(".aseye-pre-restore-{}.bak", unix_now_secs());
    match write_sidecar_backup_with_suffix(&path, &sidecar_suffix) {
        Ok(Some(sidecar)) => {
            tracing::debug!(
                ?sidecar,
                component_id = %entry.component_id,
                "restore wrote pre-overwrite sidecar"
            );
        }
        Ok(None) => {
            // Source path didn't exist - no current bytes to back up.
            // The atomic write below will create it fresh.
        }
        Err(err) => {
            tracing::warn!(
                ?err,
                component_id = %entry.component_id,
                "restore could not write sidecar; proceeding with overwrite"
            );
        }
    }

    // 4b. Atomic write. We allow `outside_home` so the integration
    //     test's tempdir-rooted scenario succeeds; production callers
    //     write to paths under HOME because the indexed components
    //     live there. The trusted root is the parent dir of the
    //     target so the safe writer permits the write while still
    //     rejecting .git / target / node_modules inside that root.
    let roots: [&Path; 1] = [parent];
    safe_atomic_write_with_options(
        &path, &plaintext, &roots, /* allow_outside_home: */ true,
    )
    .map_err(|e: FsError| RestoreErrorEntry {
        component_id: entry.component_id.clone(),
        kind: RestoreErrorKind::Write,
        message: format!("atomic write: {e}"),
    })?;

    Ok(RestoreOutcome::Restored)
}

/// Per-component step for `backup_verify`. Pure read path against
/// storage + the held private key. No side effects beyond logging.
fn verify_component<S: BackupStorage>(
    storage: &S,
    priv_key: &StaticSecret,
    entry: &BackupManifestEntry,
) -> Result<(), VerifyErrorEntry> {
    // 1. Read the blob bytes back from storage.
    let blob = storage
        .get_blob(&entry.blob_path)
        .map_err(|e| VerifyErrorEntry {
            component_id: entry.component_id.clone(),
            kind: VerifyErrorKind::Read,
            message: format!("storage get: {e}"),
        })?;

    // 2. Hash the ciphertext and compare against the manifest. This
    //    short-circuits before we run the AES-GCM decrypt; bit rot
    //    that flipped a single byte will be caught here cheaply.
    let blob_hash = sha256_hex(&blob);
    if blob_hash != entry.blob_hash {
        return Err(VerifyErrorEntry {
            component_id: entry.component_id.clone(),
            kind: VerifyErrorKind::BlobHashMismatch,
            message: format!("expected blob_hash {} got {}", entry.blob_hash, blob_hash),
        });
    }

    // 3. Decrypt. AES-GCM's auth tag means an invalid ciphertext or a
    //    wrong key surfaces as a Decrypt error; the BlobHashMismatch
    //    above already handles the corruption case in a more useful
    //    way (the user knows which way the inconsistency points).
    let plaintext = decrypt_blob(priv_key, &blob).map_err(|e| VerifyErrorEntry {
        component_id: entry.component_id.clone(),
        kind: VerifyErrorKind::Decrypt,
        message: format!("decrypt: {e}"),
    })?;

    // 4. Hash the recovered plaintext and compare against the
    //    manifest. Should never trigger unless the manifest row was
    //    hand-edited - the AES-GCM tag already protects content
    //    integrity end-to-end. Defensive check kept for completeness.
    let plaintext_hash = sha256_hex(&plaintext);
    if plaintext_hash != entry.plaintext_hash {
        return Err(VerifyErrorEntry {
            component_id: entry.component_id.clone(),
            kind: VerifyErrorKind::PlaintextHashMismatch,
            message: format!(
                "expected plaintext_hash {} got {}",
                entry.plaintext_hash, plaintext_hash
            ),
        });
    }

    Ok(())
}

// ─── helpers ────────────────────────────────────────────────────────

fn collect_components(
    handle: &IndexHandle,
    target_ids: Option<&[String]>,
) -> Result<Vec<(String, PathBuf)>, OrchestrationError> {
    let rows = handle.read(|conn| {
        if let Some(ids) = target_ids {
            // Build a parametrised IN clause. With small id lists
            // (debouncer typically 1-10 entries) the per-pass cost
            // is negligible vs. the encrypt + write that follows.
            if ids.is_empty() {
                return Ok(Vec::new());
            }
            let placeholders = (1..=ids.len())
                .map(|i| format!("?{i}"))
                .collect::<Vec<_>>()
                .join(",");
            let sql = format!(
                "SELECT id, path FROM component WHERE id IN ({placeholders}) ORDER BY id ASC"
            );
            let mut stmt = conn.prepare(&sql)?;
            let params: Vec<&dyn rusqlite::ToSql> =
                ids.iter().map(|s| s as &dyn rusqlite::ToSql).collect();
            let mapped = stmt.query_map(rusqlite::params_from_iter(params.iter()), |row| {
                let id: String = row.get(0)?;
                let path: String = row.get(1)?;
                Ok((id, PathBuf::from(path)))
            })?;
            let mut out = Vec::with_capacity(ids.len());
            for r in mapped {
                out.push(r?);
            }
            Ok(out)
        } else {
            let mut stmt = conn.prepare("SELECT id, path FROM component ORDER BY id ASC")?;
            let mapped = stmt.query_map([], |row| {
                let id: String = row.get(0)?;
                let path: String = row.get(1)?;
                Ok((id, PathBuf::from(path)))
            })?;
            let mut out = Vec::new();
            for r in mapped {
                out.push(r?);
            }
            Ok(out)
        }
    })?;
    Ok(rows)
}

fn resolve_component_path(
    handle: &IndexHandle,
    component_id: &str,
) -> Result<Option<PathBuf>, crate::index::IndexError> {
    handle.read(|conn| {
        let path: Option<String> = conn
            .query_row(
                "SELECT path FROM component WHERE id = ?1",
                params![component_id],
                |row| row.get(0),
            )
            .or_else(|err| match err {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                other => Err(other),
            })?;
        Ok(path.map(PathBuf::from))
    })
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex_encode(&hasher.finalize())
}

fn unix_now_secs() -> i64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => i64::try_from(d.as_secs()).unwrap_or(i64::MAX),
        Err(_) => 0,
    }
}

fn file_mtime_secs(path: &Path) -> Option<i64> {
    let meta = std::fs::metadata(path).ok()?;
    let mtime = meta.modified().ok()?;
    let dur = mtime.duration_since(UNIX_EPOCH).ok()?;
    i64::try_from(dur.as_secs()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backup::keychain::test_support::InMemoryKeychain;
    use crate::backup::storage::LocalDirectoryStorage;
    use rusqlite::params;
    use std::fs;
    use std::sync::Arc;
    use tempfile::tempdir;

    /// Insert a synthetic component row so the orchestrator has
    /// something to walk. We bypass `upsert_component` to keep the
    /// test laser-focused on the backup behaviour.
    fn seed_component(handle: &IndexHandle, id: &str, path: &Path) {
        handle
            .write(|conn| {
                conn.execute(
                    "INSERT INTO component (
                         id, type, tool, scope, origin, name, path, format,
                         enabled, use_count, hash, updated_at
                     ) VALUES (?1, 'skill', 'claude-code', 'user', 'tool',
                              ?2, ?3, 'markdown', 1, 0, '00', 0)",
                    params![id, id, path.to_string_lossy()],
                )?;
                Ok(())
            })
            .expect("seed");
    }

    #[test]
    fn first_backup_encrypts_idempotent_second() {
        let dir = tempdir().expect("tempdir");
        let storage = LocalDirectoryStorage::at(dir.path().to_path_buf());
        let kc = InMemoryKeychain::new();
        let handle = Arc::new(IndexHandle::open_in_memory().expect("open"));

        // Two synthetic components.
        let f1 = dir.path().join("a.md");
        fs::write(&f1, b"alpha").unwrap();
        seed_component(&handle, "aseye://x/y/z/a", &f1);
        let f2 = dir.path().join("b.md");
        fs::write(&f2, b"bravo").unwrap();
        seed_component(&handle, "aseye://x/y/z/b", &f2);

        let r1 = backup_now_with(Arc::clone(&handle), &storage, &kc, None).expect("backup 1");
        assert_eq!(r1.total, 2);
        assert_eq!(r1.encrypted, 2);
        assert_eq!(r1.skipped_unchanged, 0);
        assert!(r1.errors.is_empty());

        let r2 = backup_now_with(Arc::clone(&handle), &storage, &kc, None).expect("backup 2");
        assert_eq!(r2.total, 2);
        assert_eq!(r2.encrypted, 0);
        assert_eq!(r2.skipped_unchanged, 2);
        assert!(r2.errors.is_empty());
    }

    #[test]
    fn modified_file_is_re_encrypted() {
        let dir = tempdir().expect("tempdir");
        let storage = LocalDirectoryStorage::at(dir.path().to_path_buf());
        let kc = InMemoryKeychain::new();
        let handle = Arc::new(IndexHandle::open_in_memory().expect("open"));

        let f = dir.path().join("a.md");
        fs::write(&f, b"v1").unwrap();
        seed_component(&handle, "aseye://x/y/z/a", &f);
        backup_now_with(Arc::clone(&handle), &storage, &kc, None).expect("first");

        // Mutate the file so the SHA-256 changes.
        fs::write(&f, b"v2").unwrap();
        let r = backup_now_with(Arc::clone(&handle), &storage, &kc, None).expect("second");
        assert_eq!(r.encrypted, 1);
        assert_eq!(r.skipped_unchanged, 0);
    }

    #[test]
    fn target_ids_filters_to_subset() {
        let dir = tempdir().expect("tempdir");
        let storage = LocalDirectoryStorage::at(dir.path().to_path_buf());
        let kc = InMemoryKeychain::new();
        let handle = Arc::new(IndexHandle::open_in_memory().expect("open"));

        let f1 = dir.path().join("a.md");
        fs::write(&f1, b"a").unwrap();
        seed_component(&handle, "aseye://x/y/z/a", &f1);
        let f2 = dir.path().join("b.md");
        fs::write(&f2, b"b").unwrap();
        seed_component(&handle, "aseye://x/y/z/b", &f2);

        let only = vec!["aseye://x/y/z/a".to_string()];
        let r = backup_now_with(Arc::clone(&handle), &storage, &kc, Some(&only)).expect("filtered");
        assert_eq!(r.total, 1);
        assert_eq!(r.encrypted, 1);
    }

    #[test]
    fn restore_round_trips_files() {
        let dir = tempdir().expect("tempdir");
        let storage = LocalDirectoryStorage::at(dir.path().to_path_buf());
        let kc = InMemoryKeychain::new();
        let handle = Arc::new(IndexHandle::open_in_memory().expect("open"));

        let f = dir.path().join("a.md");
        fs::write(&f, b"original content").unwrap();
        seed_component(&handle, "aseye://x/y/z/a", &f);
        backup_now_with(Arc::clone(&handle), &storage, &kc, None).expect("backup");

        // Wipe the local file - simulate accidental rm.
        fs::remove_file(&f).unwrap();

        let r =
            restore_now_with(Arc::clone(&handle), &storage, &kc, /* dry */ false).expect("restore");
        assert_eq!(r.total, 1);
        assert_eq!(r.restored, 1);
        assert!(r.errors.is_empty());
        assert_eq!(fs::read(&f).unwrap(), b"original content");
    }

    #[test]
    fn restore_skips_local_newer() {
        let dir = tempdir().expect("tempdir");
        let storage = LocalDirectoryStorage::at(dir.path().to_path_buf());
        let kc = InMemoryKeychain::new();
        let handle = Arc::new(IndexHandle::open_in_memory().expect("open"));

        let f = dir.path().join("a.md");
        fs::write(&f, b"v1").unwrap();
        seed_component(&handle, "aseye://x/y/z/a", &f);

        // Set the manifest's encrypted_at to "an hour ago" by hand so
        // we have headroom to make the file newer.
        backup_now_with(Arc::clone(&handle), &storage, &kc, None).expect("backup");
        let one_hour_ago = unix_now_secs() - 3600;
        handle
            .write(|c| {
                c.execute(
                    "UPDATE backup_manifest SET encrypted_at = ?1",
                    params![one_hour_ago],
                )?;
                Ok(())
            })
            .unwrap();

        // Touch the file with newer content so its mtime jumps.
        fs::write(&f, b"v2 - I edited locally").unwrap();

        let r = restore_now_with(Arc::clone(&handle), &storage, &kc, false).expect("restore");
        assert_eq!(r.skipped_local_newer, 1);
        assert_eq!(r.restored, 0);
        assert_eq!(fs::read(&f).unwrap(), b"v2 - I edited locally");
    }

    #[test]
    fn dry_run_does_not_overwrite() {
        let dir = tempdir().expect("tempdir");
        let storage = LocalDirectoryStorage::at(dir.path().to_path_buf());
        let kc = InMemoryKeychain::new();
        let handle = Arc::new(IndexHandle::open_in_memory().expect("open"));

        let f = dir.path().join("a.md");
        fs::write(&f, b"original").unwrap();
        seed_component(&handle, "aseye://x/y/z/a", &f);
        backup_now_with(Arc::clone(&handle), &storage, &kc, None).expect("backup");

        // Wipe the file so a real restore *would* write.
        fs::remove_file(&f).unwrap();

        let r = restore_now_with(Arc::clone(&handle), &storage, &kc, /* dry */ true).expect("dry");
        assert!(r.dry_run);
        assert_eq!(r.restored, 1);
        // Dry-run reports what *would* happen but writes nothing.
        assert!(!f.exists(), "dry-run must not have written the file");
    }

    /// Defense in depth: before overwriting on restore, the current
    /// bytes land in a timestamped sidecar so the user can recover if
    /// the restore was wrong. We back up `v1`, mutate the file to
    /// `v2-locally-edited`, force `encrypted_at` forward so the
    /// local-newer guard does not skip, then restore. The sidecar
    /// must contain the pre-restore `v2` bytes.
    #[test]
    fn restore_writes_pre_overwrite_sidecar() {
        let dir = tempdir().expect("tempdir");
        let storage = LocalDirectoryStorage::at(dir.path().to_path_buf());
        let kc = InMemoryKeychain::new();
        let handle = Arc::new(IndexHandle::open_in_memory().expect("open"));

        let f = dir.path().join("original.md");
        std::fs::write(&f, b"v1").unwrap();
        seed_component(&handle, "aseye://x/y/z/orig", &f);
        backup_now_with(Arc::clone(&handle), &storage, &kc, None).expect("backup");

        // Locally edit; force encrypted_at into the future so the
        // local-newer guard does NOT skip.
        std::fs::write(&f, b"v2-locally-edited").unwrap();
        let one_hour_hence = unix_now_secs() + 3600;
        handle
            .write(|c| {
                c.execute(
                    "UPDATE backup_manifest SET encrypted_at = ?1",
                    params![one_hour_hence],
                )?;
                Ok(())
            })
            .unwrap();

        let r = restore_now_with(Arc::clone(&handle), &storage, &kc, false).expect("restore");
        assert_eq!(r.restored, 1, "expected the restore to overwrite");
        assert!(r.errors.is_empty(), "no errors expected: {:?}", r.errors);

        // The file now holds the backup bytes (v1).
        assert_eq!(std::fs::read(&f).unwrap(), b"v1");

        // A sidecar with the pre-restore bytes (v2) lives next to it.
        let entries: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(std::result::Result::ok)
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("original.md.aseye-pre-restore-")
            })
            .collect();
        assert_eq!(entries.len(), 1, "exactly one sidecar should exist");
        let sidecar = &entries[0];
        assert!(sidecar.file_name().to_string_lossy().ends_with(".bak"));
        assert_eq!(
            std::fs::read(sidecar.path()).unwrap(),
            b"v2-locally-edited",
            "sidecar must contain the pre-restore bytes",
        );
    }

    /// When the source file is absent (e.g. the user manually deleted
    /// it before restoring), there is nothing to sidecar. The restore
    /// proceeds and creates the file fresh; no `.aseye-pre-restore-*`
    /// sidecar is left behind.
    #[test]
    fn restore_does_not_create_sidecar_when_source_missing() {
        let dir = tempdir().expect("tempdir");
        let storage = LocalDirectoryStorage::at(dir.path().to_path_buf());
        let kc = InMemoryKeychain::new();
        let handle = Arc::new(IndexHandle::open_in_memory().expect("open"));

        let f = dir.path().join("ghost.md");
        std::fs::write(&f, b"original").unwrap();
        seed_component(&handle, "aseye://x/y/z/ghost", &f);
        backup_now_with(Arc::clone(&handle), &storage, &kc, None).expect("backup");

        // User wipes the file before restoring.
        std::fs::remove_file(&f).unwrap();

        let r = restore_now_with(Arc::clone(&handle), &storage, &kc, false).expect("restore");
        assert_eq!(r.restored, 1);
        assert_eq!(std::fs::read(&f).unwrap(), b"original");

        // No sidecar should exist since there were no pre-restore
        // bytes to back up.
        let sidecar_count = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(std::result::Result::ok)
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .contains(".aseye-pre-restore-")
            })
            .count();
        assert_eq!(sidecar_count, 0);
    }

    /// Backup integrity verify: clean state, every entry should
    /// pass both the ciphertext-hash and plaintext-hash checks.
    #[test]
    fn verify_clean_backup_reports_all_verified() {
        let dir = tempdir().expect("tempdir");
        let storage = LocalDirectoryStorage::at(dir.path().to_path_buf());
        let kc = InMemoryKeychain::new();
        let handle = Arc::new(IndexHandle::open_in_memory().expect("open"));

        for (id, body) in [
            ("aseye://x/y/z/a", "alpha bytes"),
            ("aseye://x/y/z/b", "bravo bytes"),
            ("aseye://x/y/z/c", "charlie bytes"),
        ] {
            let f = dir.path().join(format!("{}.md", id.replace('/', "_")));
            std::fs::write(&f, body).unwrap();
            seed_component(&handle, id, &f);
        }
        backup_now_with(Arc::clone(&handle), &storage, &kc, None).expect("backup");

        let r = backup_verify_with(Arc::clone(&handle), &storage, &kc).expect("verify");
        assert_eq!(r.total, 3);
        assert_eq!(r.verified, 3);
        assert!(r.errors.is_empty(), "no errors expected: {:?}", r.errors);
    }

    /// Tamper detection: flip one byte in a stored blob, verify
    /// must surface a `BlobHashMismatch` (the cheap pre-decrypt check
    /// catches it).
    #[test]
    fn verify_detects_tampered_blob() {
        let dir = tempdir().expect("tempdir");
        let storage = LocalDirectoryStorage::at(dir.path().to_path_buf());
        let kc = InMemoryKeychain::new();
        let handle = Arc::new(IndexHandle::open_in_memory().expect("open"));

        let f = dir.path().join("tamper.md");
        std::fs::write(&f, b"untouched").unwrap();
        seed_component(&handle, "aseye://x/y/z/tamper", &f);
        backup_now_with(Arc::clone(&handle), &storage, &kc, None).expect("backup");

        // Find the blob file under blobs/<2-hex>/<rest>.bin and
        // flip one byte in the ciphertext region (well past the
        // 8-byte magic + version header).
        let blob_dir = dir.path().join("blobs");
        let bp = std::fs::read_dir(&blob_dir)
            .unwrap()
            .filter_map(std::result::Result::ok)
            .map(|e| e.path())
            .find_map(|two_hex| {
                std::fs::read_dir(&two_hex)
                    .ok()?
                    .filter_map(std::result::Result::ok)
                    .map(|e| e.path())
                    .next()
            })
            .expect("blob exists on disk");
        let mut bytes = std::fs::read(&bp).unwrap();
        // Flip a byte deep into the ciphertext payload (offset 120
        // is past header + wrapped DEK + tag + nonce; well into the
        // AES-GCM ciphertext for any plaintext at least one byte).
        let target = 120.min(bytes.len() - 1);
        bytes[target] ^= 0xff;
        std::fs::write(&bp, &bytes).unwrap();

        let r = backup_verify_with(Arc::clone(&handle), &storage, &kc).expect("verify");
        assert_eq!(r.total, 1);
        assert_eq!(r.verified, 0);
        assert_eq!(r.errors.len(), 1);
        assert!(matches!(
            r.errors[0].kind,
            VerifyErrorKind::BlobHashMismatch
        ));
    }

    /// Missing blob: backup writes a blob, then we manually wipe it
    /// from storage. Verify surfaces a Read error - the manifest
    /// points at something the backend can no longer produce.
    #[test]
    fn verify_detects_missing_blob() {
        let dir = tempdir().expect("tempdir");
        let storage = LocalDirectoryStorage::at(dir.path().to_path_buf());
        let kc = InMemoryKeychain::new();
        let handle = Arc::new(IndexHandle::open_in_memory().expect("open"));

        let f = dir.path().join("vanish.md");
        std::fs::write(&f, b"goodbye").unwrap();
        seed_component(&handle, "aseye://x/y/z/vanish", &f);
        backup_now_with(Arc::clone(&handle), &storage, &kc, None).expect("backup");

        // Wipe the blob directory; the manifest still points at the
        // (now missing) blob.
        let blob_dir = dir.path().join("blobs");
        std::fs::remove_dir_all(&blob_dir).unwrap();

        let r = backup_verify_with(Arc::clone(&handle), &storage, &kc).expect("verify");
        assert_eq!(r.total, 1);
        assert_eq!(r.verified, 0);
        assert_eq!(r.errors.len(), 1);
        assert!(matches!(r.errors[0].kind, VerifyErrorKind::Read));
    }

    /// Empty manifest: verify reports total=0 with no work done.
    #[test]
    fn verify_empty_manifest_is_a_noop() {
        let dir = tempdir().expect("tempdir");
        let storage = LocalDirectoryStorage::at(dir.path().to_path_buf());
        let kc = InMemoryKeychain::new();
        let handle = Arc::new(IndexHandle::open_in_memory().expect("open"));

        // Generate a keypair so verify can fetch the private key.
        // Without this the in-memory keychain has nothing to return.
        let _public = ensure_keypair_with(handle.as_ref(), &kc).expect("keypair");

        let r = backup_verify_with(Arc::clone(&handle), &storage, &kc).expect("verify");
        assert_eq!(r.total, 0);
        assert_eq!(r.verified, 0);
        assert!(r.errors.is_empty());
    }

    /// FK cascade gives us defense-in-depth: dropping a component
    /// row scrubs the manifest entry automatically, so a real-world
    /// "user deleted the file" flow leaves nothing for the
    /// orchestrator to chase. We exercise the orphan-handler path
    /// explicitly by disabling foreign keys for one transaction so
    /// the orphan appears - that is the only way a manifest row can
    /// reference a missing component on a real DB.
    #[test]
    fn orphan_manifest_surfaces_typed_error() {
        let dir = tempdir().expect("tempdir");
        let storage = LocalDirectoryStorage::at(dir.path().to_path_buf());
        let kc = InMemoryKeychain::new();
        let handle = Arc::new(IndexHandle::open_in_memory().expect("open"));

        let f = dir.path().join("a.md");
        fs::write(&f, b"x").unwrap();
        seed_component(&handle, "aseye://x/y/z/a", &f);
        backup_now_with(Arc::clone(&handle), &storage, &kc, None).expect("backup");

        // Disable FK enforcement for this connection-scope only, drop
        // the component row, then re-enable. The manifest entry is
        // now an orphan referencing a non-existent component_id.
        handle
            .write(|c| {
                c.execute_batch(
                    "PRAGMA foreign_keys = OFF;
                     DELETE FROM component_fts WHERE id = 'aseye://x/y/z/a';
                     DELETE FROM component WHERE id = 'aseye://x/y/z/a';
                     PRAGMA foreign_keys = ON;",
                )?;
                Ok(())
            })
            .unwrap();
        // Manifest still has the orphan row; restore must surface
        // the ComponentMissing kind and clean it up.
        let r = restore_now_with(Arc::clone(&handle), &storage, &kc, false).expect("restore");
        assert_eq!(r.errors.len(), 1);
        assert!(matches!(
            r.errors[0].kind,
            RestoreErrorKind::ComponentMissing
        ));
        // Orphan manifest row was scrubbed by the orphan handler.
        let n = manifest_count(&handle).unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn backup_status_reports_basics() {
        let dir = tempdir().expect("tempdir");
        let storage = LocalDirectoryStorage::at(dir.path().to_path_buf());
        let kc = InMemoryKeychain::new();
        let handle = Arc::new(IndexHandle::open_in_memory().expect("open"));

        // Pre-key state: status reports key_present = false.
        let pre = backup_status_with(Arc::clone(&handle), &storage).expect("status pre");
        assert!(!pre.key_present);
        assert_eq!(pre.manifest_count, 0);
        assert!(pre.last_backup_at.is_none());
        assert!(pre.auto_backup_enabled, "default must be true");

        // Seed + back up one row.
        let f = dir.path().join("a.md");
        fs::write(&f, b"x").unwrap();
        seed_component(&handle, "aseye://x/y/z/a", &f);
        backup_now_with(Arc::clone(&handle), &storage, &kc, None).expect("backup");

        let post = backup_status_with(Arc::clone(&handle), &storage).expect("status post");
        assert!(post.key_present);
        assert_eq!(post.manifest_count, 1);
        assert!(post.last_backup_at.is_some());
        assert!(post.backup_dir.contains(dir.path().to_str().unwrap()));
    }
}
