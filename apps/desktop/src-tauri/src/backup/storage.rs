//! Backup storage backend.
//!
//! `BackupStorage` is the trait `docs/15-backup-and-restore.md`
//! section 15.10 specifies as the production-migration boundary. The
//! v0 implementation, [`LocalDirectoryStorage`], writes blobs under
//! `~/.aseye-backup/blobs/`. A future `S3Storage` would implement the
//! same trait and the orchestrator would not change.

use std::fs;
use std::path::{Path, PathBuf};

use crate::fs::{safe_atomic_write_with_options, FsError};

/// Trait abstracting the put / get / list / delete surface a backup
/// backend exposes.
///
/// All methods are synchronous and blocking - the orchestrator runs
/// in a `spawn_blocking` task so the IPC reactor never stalls. Errors
/// funnel through [`StorageError`] which carries enough context for
/// the IPC layer to route to the right `BackupErrorKind`.
pub trait BackupStorage: Send + Sync {
    /// Write `bytes` to the slot identified by `relative_path`. The
    /// path is the storage-local key (for the local backend, a
    /// directory-relative path; for an S3 backend, the object key).
    /// Idempotent.
    fn put_blob(&self, relative_path: &str, bytes: &[u8]) -> Result<(), StorageError>;

    /// Read the bytes previously written to `relative_path`.
    fn get_blob(&self, relative_path: &str) -> Result<Vec<u8>, StorageError>;

    /// Best-effort delete; missing keys are not an error.
    fn delete_blob(&self, relative_path: &str) -> Result<(), StorageError>;

    /// Filesystem location backing this storage, surfaced in
    /// `backup_status.backupDir`. Returns the absolute path string.
    fn root_for_display(&self) -> String;
}

/// Errors raised by a `BackupStorage` implementation.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error(transparent)]
    Fs(#[from] FsError),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error("backup root cannot be resolved (no home directory available)")]
    NoHome,

    #[error("blob {path} not found in storage")]
    NotFound { path: String },
}

/// Filesystem-backed implementation. Writes go through
/// [`safe_atomic_write_with_options`] with `allow_outside_home: true`
/// so an exotic CI host whose `~/.aseye-backup` resolves outside
/// `dirs::home_dir()` still works for tests.
pub struct LocalDirectoryStorage {
    /// Absolute path of the backup root (e.g. `~/.aseye-backup`). The
    /// `blobs/` subdir is appended on every read/write.
    root: PathBuf,
}

impl LocalDirectoryStorage {
    /// Build a storage rooted at the user's home + `.aseye-backup`.
    /// Returns `NoHome` if `dirs::home_dir()` is unavailable.
    pub fn at_default_root() -> Result<Self, StorageError> {
        let home = dirs::home_dir().ok_or(StorageError::NoHome)?;
        Ok(Self::at(home.join(".aseye-backup")))
    }

    /// Build a storage rooted at `root`. The directory is created
    /// lazily on the first put.
    #[must_use]
    pub fn at(root: PathBuf) -> Self {
        Self { root }
    }

    fn blob_dir(&self) -> PathBuf {
        self.root.join("blobs")
    }

    fn full_path(&self, relative_path: &str) -> PathBuf {
        self.blob_dir().join(relative_path)
    }
}

impl BackupStorage for LocalDirectoryStorage {
    fn put_blob(&self, relative_path: &str, bytes: &[u8]) -> Result<(), StorageError> {
        let target = self.full_path(relative_path);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        // The atomic write helper already handles temp + fsync +
        // rename + parent fsync. We pass the storage root as the only
        // trusted root and `allow_outside_home: true` so the helper
        // does not enforce the home guard (the user might mount
        // `~/.aseye-backup` outside their home).
        let root = self.root.clone();
        if !root.exists() {
            fs::create_dir_all(&root)?;
        }
        let roots: [&Path; 1] = [&root];
        safe_atomic_write_with_options(
            &target, bytes, &roots, /* allow_outside_home: */ true,
        )?;
        Ok(())
    }

    fn get_blob(&self, relative_path: &str) -> Result<Vec<u8>, StorageError> {
        let target = self.full_path(relative_path);
        if !target.exists() {
            return Err(StorageError::NotFound {
                path: relative_path.to_owned(),
            });
        }
        let bytes = fs::read(&target)?;
        Ok(bytes)
    }

    fn delete_blob(&self, relative_path: &str) -> Result<(), StorageError> {
        let target = self.full_path(relative_path);
        if target.exists() {
            fs::remove_file(&target)?;
        }
        Ok(())
    }

    fn root_for_display(&self) -> String {
        self.root.to_string_lossy().into_owned()
    }
}

/// Build the storage-relative path for a blob hash. Uses the
/// `<first 2 hex>/<remaining hex>.bin` shape from spec 15.4 so the
/// directory does not balloon to N entries when a user has tens of
/// thousands of components. Returns the path as a string with `/`
/// separators (storage backends agree on POSIX-flavoured keys; the
/// local backend converts to platform-native separators internally).
#[must_use]
pub fn blob_path_for_hash(blob_hash: &str) -> String {
    if blob_hash.len() < 2 {
        // Defensive: an unexpectedly short hash still produces a
        // valid string rather than panicking.
        return format!("xx/{blob_hash}.bin");
    }
    format!("{}/{}.bin", &blob_hash[..2], &blob_hash[2..])
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn put_then_get_round_trip() {
        let dir = tempdir().expect("tempdir");
        let storage = LocalDirectoryStorage::at(dir.path().to_path_buf());
        let key = "ab/cdef0123.bin";
        storage.put_blob(key, b"hello backup").expect("put");
        let bytes = storage.get_blob(key).expect("get");
        assert_eq!(bytes, b"hello backup");
        // The on-disk shape matches the storage-relative key.
        let on_disk = dir.path().join("blobs").join("ab").join("cdef0123.bin");
        assert!(on_disk.exists());
    }

    #[test]
    fn put_overwrites_existing_blob() {
        let dir = tempdir().expect("tempdir");
        let storage = LocalDirectoryStorage::at(dir.path().to_path_buf());
        let key = "00/aaaa.bin";
        storage.put_blob(key, b"v1").unwrap();
        storage.put_blob(key, b"v2").unwrap();
        assert_eq!(storage.get_blob(key).unwrap(), b"v2");
    }

    #[test]
    fn get_missing_returns_not_found() {
        let dir = tempdir().expect("tempdir");
        let storage = LocalDirectoryStorage::at(dir.path().to_path_buf());
        let err = storage.get_blob("zz/nope.bin").expect_err("missing");
        assert!(matches!(err, StorageError::NotFound { .. }));
    }

    #[test]
    fn delete_is_best_effort() {
        let dir = tempdir().expect("tempdir");
        let storage = LocalDirectoryStorage::at(dir.path().to_path_buf());
        // Deleting a never-written blob is fine.
        storage.delete_blob("00/missing.bin").expect("missing ok");
        // Writing then deleting also fine.
        storage.put_blob("11/exists.bin", b"x").unwrap();
        storage.delete_blob("11/exists.bin").unwrap();
        assert!(matches!(
            storage.get_blob("11/exists.bin"),
            Err(StorageError::NotFound { .. })
        ));
    }

    #[test]
    fn root_for_display_is_stable() {
        let dir = tempdir().expect("tempdir");
        let storage = LocalDirectoryStorage::at(dir.path().to_path_buf());
        let root = storage.root_for_display();
        assert!(!root.is_empty());
        assert_eq!(root, dir.path().to_string_lossy());
    }

    #[test]
    fn blob_path_for_hash_uses_two_byte_prefix() {
        let path = blob_path_for_hash("abcdef0123456789");
        assert_eq!(path, "ab/cdef0123456789.bin");
    }

    #[test]
    fn blob_path_for_short_hash_does_not_panic() {
        let path = blob_path_for_hash("a");
        assert!(path.contains(".bin"));
    }
}
