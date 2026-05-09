// Phase 15: backup module surface. The IPC commands at the end of
// the phase consume every public symbol below; until that wiring
// lands a few items would otherwise read as "never constructed" /
// "never used" to the dead-code lint. We blanket-allow at the module
// level rather than peppering every public item with `#[allow]`,
// because the alternative produces noisy diffs on every IPC addition.
#![allow(dead_code)]

//! Phase 15 - end-to-end encrypted local backup.
//!
//! See `docs/15-backup-and-restore.md` for the full design. The module
//! splits the work into five layers so the crypto stays testable
//! independently of the storage / IPC plumbing:
//!
//! * [`envelope`] - pure crypto. AES-256-GCM data encryption + X25519
//!   ECDH + HKDF wrapping. No file `IO`, no `SQLite`, no keychain. Easiest
//!   to test in isolation.
//! * [`keychain`] - OS keychain integration via the `keyring` crate.
//!   Loads + stores the 32-byte X25519 device private key.
//! * [`keypair`] - `ensure_keypair` orchestrator. On first call
//!   generates a fresh keypair, persists private to keychain + public
//!   to `app_settings.backupPublicKey`. Idempotent across runs.
//! * [`storage`] - the swappable `BackupStorage` trait + the local
//!   filesystem implementation under `~/.aseye-backup/`.
//! * [`manifest`] - `backup_manifest` `SQLite` reads + writes.
//! * [`orchestrate`] - `backup_now` / `restore_now` flows.
//! * [`auto`] - debounced auto-backup listener.
//!
//! Errors live alongside the layer they originate from but funnel up
//! into [`BackupErrorEntry`] / [`RestoreErrorEntry`] for the IPC
//! surface.

pub mod auto;
pub mod envelope;
pub mod keychain;
pub mod keypair;
pub mod manifest;
pub mod orchestrate;
pub mod storage;
pub mod types;

// Re-exports the IPC layer + integration tests consume. We intentionally
// allow `unused_imports` here at the module-attribute level because
// some of these paths are only reached from the integration test
// crate under `tests/`, which compiles independently of the `lib`
// dead-code analysis.
#[allow(unused_imports)]
pub use envelope::{
    decrypt_blob, encrypt_blob, DevicePublicKey, EnvelopeError, BLOB_HEADER_LEN, BLOB_VERSION,
    MAGIC,
};
#[allow(unused_imports)]
pub use keychain::{KeychainError, BACKUP_KEYCHAIN_ACCOUNT, BACKUP_KEYCHAIN_SERVICE};
#[allow(unused_imports)]
pub use keypair::{ensure_keypair, KeypairError};
#[allow(unused_imports)]
pub use manifest::{
    delete_manifest_entry, read_manifest_entry, upsert_manifest_entry, BackupManifestEntry,
    ManifestError,
};
#[allow(unused_imports)]
pub use orchestrate::{
    backup_now, backup_status, restore_now, BackupStatusReport, OrchestrationError,
};
#[allow(unused_imports)]
pub use storage::{BackupStorage, LocalDirectoryStorage, StorageError};
#[allow(unused_imports)]
pub use types::{
    BackupErrorEntry, BackupErrorKind, BackupReport, RestoreErrorEntry, RestoreErrorKind,
    RestoreReport,
};
