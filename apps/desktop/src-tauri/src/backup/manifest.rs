//! `backup_manifest` `SQLite` reads and writes.
//!
//! The manifest is the bridge between an indexed component (identified
//! by its URI) and the encrypted blob written to the backup storage
//! backend. One row per component, atomic upsert keyed on
//! `component_id`. The orchestrator consults this table to decide
//! whether a re-encrypt is necessary - if `plaintext_hash` matches the
//! file's current SHA-256, we skip.
//!
//! Schema is defined in `docs/15-backup-and-restore.md` section 15.4
//! and applied by migration v6.

use rusqlite::{params, OptionalExtension};

use crate::index::{IndexError, IndexHandle};

/// One row in `backup_manifest`. Identifies the encrypted blob,
/// records both content hashes, and timestamps the encryption.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupManifestEntry {
    pub component_id: String,
    /// Storage-relative path. For the local-directory backend this is
    /// the `<2-hex>/<rest>.bin` shape (relative to the backup root).
    pub blob_path: String,
    /// SHA-256 of the source plaintext. Drives idempotency.
    pub plaintext_hash: String,
    /// SHA-256 of the encrypted blob bytes. Used by the integration
    /// test + a future `verify` command to detect on-disk corruption.
    pub blob_hash: String,
    pub plaintext_size: u64,
    pub blob_size: u64,
    /// Unix seconds when the encryption finished.
    pub encrypted_at: i64,
}

/// Errors raised by the manifest layer.
#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    #[error(transparent)]
    Index(#[from] IndexError),
}

/// Insert or update the manifest entry for `component_id`. Idempotent.
pub fn upsert_manifest_entry(
    handle: &IndexHandle,
    entry: &BackupManifestEntry,
) -> Result<(), ManifestError> {
    handle.write(|conn| {
        // SQLite stores u64 as INTEGER; cast to i64 because rusqlite
        // does not bind u64 directly. Sizes well below 2^63 in
        // practice (a 5 MiB cap on the parser side keeps us tiny).
        let plaintext_size = i64::try_from(entry.plaintext_size).unwrap_or(i64::MAX);
        let blob_size = i64::try_from(entry.blob_size).unwrap_or(i64::MAX);
        conn.execute(
            "INSERT INTO backup_manifest (
                 component_id, blob_path, plaintext_hash, blob_hash,
                 plaintext_size, blob_size, encrypted_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(component_id) DO UPDATE SET
                 blob_path      = excluded.blob_path,
                 plaintext_hash = excluded.plaintext_hash,
                 blob_hash      = excluded.blob_hash,
                 plaintext_size = excluded.plaintext_size,
                 blob_size      = excluded.blob_size,
                 encrypted_at   = excluded.encrypted_at",
            params![
                entry.component_id,
                entry.blob_path,
                entry.plaintext_hash,
                entry.blob_hash,
                plaintext_size,
                blob_size,
                entry.encrypted_at,
            ],
        )?;
        Ok(())
    })?;
    Ok(())
}

/// Read the manifest entry for `component_id`. Returns `Ok(None)` for
/// components that have never been backed up.
pub fn read_manifest_entry(
    handle: &IndexHandle,
    component_id: &str,
) -> Result<Option<BackupManifestEntry>, ManifestError> {
    let entry = handle.read(|conn| {
        conn.query_row(
            "SELECT component_id, blob_path, plaintext_hash, blob_hash,
                    plaintext_size, blob_size, encrypted_at
             FROM backup_manifest WHERE component_id = ?1",
            params![component_id],
            |row| {
                Ok(BackupManifestEntry {
                    component_id: row.get::<_, String>(0)?,
                    blob_path: row.get::<_, String>(1)?,
                    plaintext_hash: row.get::<_, String>(2)?,
                    blob_hash: row.get::<_, String>(3)?,
                    plaintext_size: u64::try_from(row.get::<_, i64>(4)?.max(0)).unwrap_or(0),
                    blob_size: u64::try_from(row.get::<_, i64>(5)?.max(0)).unwrap_or(0),
                    encrypted_at: row.get::<_, i64>(6)?,
                })
            },
        )
        .optional()
        .map_err(IndexError::from)
    })?;
    Ok(entry)
}

/// Delete the manifest entry for `component_id`. Returns the number
/// of rows removed (0 or 1). Useful when a component was dropped from
/// the index entirely.
pub fn delete_manifest_entry(
    handle: &IndexHandle,
    component_id: &str,
) -> Result<usize, ManifestError> {
    let count = handle.write(|conn| {
        let n = conn.execute(
            "DELETE FROM backup_manifest WHERE component_id = ?1",
            params![component_id],
        )?;
        Ok(n)
    })?;
    Ok(count)
}

/// Total manifest entry count - used by `backup_status`.
pub fn manifest_count(handle: &IndexHandle) -> Result<u32, ManifestError> {
    let count = handle.read(|conn| {
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM backup_manifest", [], |r| r.get(0))?;
        Ok(n)
    })?;
    Ok(u32::try_from(count.max(0)).unwrap_or(u32::MAX))
}

/// Iterate every manifest row in `encrypted_at ASC` order. The closure
/// receives one entry at a time so the orchestrator does not need to
/// hold the whole list in memory at once.
pub fn for_each_entry<F>(handle: &IndexHandle, mut f: F) -> Result<(), ManifestError>
where
    F: FnMut(BackupManifestEntry),
{
    handle.read(|conn| {
        let mut stmt = conn.prepare(
            "SELECT component_id, blob_path, plaintext_hash, blob_hash,
                    plaintext_size, blob_size, encrypted_at
             FROM backup_manifest
             ORDER BY encrypted_at ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(BackupManifestEntry {
                component_id: row.get::<_, String>(0)?,
                blob_path: row.get::<_, String>(1)?,
                plaintext_hash: row.get::<_, String>(2)?,
                blob_hash: row.get::<_, String>(3)?,
                plaintext_size: u64::try_from(row.get::<_, i64>(4)?.max(0)).unwrap_or(0),
                blob_size: u64::try_from(row.get::<_, i64>(5)?.max(0)).unwrap_or(0),
                encrypted_at: row.get::<_, i64>(6)?,
            })
        })?;
        for row in rows {
            f(row?);
        }
        Ok(())
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;

    fn entry(id: &str, hash: &str, t: i64) -> BackupManifestEntry {
        let suffix_start = hash.len().min(2);
        BackupManifestEntry {
            component_id: id.into(),
            blob_path: format!("{}/{}.bin", &hash[..suffix_start], &hash[suffix_start..]),
            plaintext_hash: hash.into(),
            blob_hash: format!("blob-{hash}"),
            plaintext_size: 100,
            blob_size: 212,
            encrypted_at: t,
        }
    }

    /// Seed a parent component row so the manifest's FK can resolve.
    /// We bypass the upsert path here - the manifest layer is what we
    /// are testing, and a synthetic insert keeps the test focused.
    fn seed_parent(handle: &IndexHandle, id: &str) {
        handle
            .write(|conn| {
                conn.execute(
                    "INSERT INTO component (
                         id, type, tool, scope, origin, name, path, format,
                         enabled, use_count, hash, updated_at
                     ) VALUES (?1, 'skill', 'claude-code', 'user', 'tool',
                              ?1, '/dev/null', 'markdown', 1, 0, '00', 0)",
                    params![id],
                )?;
                Ok(())
            })
            .expect("seed parent");
    }

    #[test]
    fn upsert_then_read_round_trip() {
        let handle = IndexHandle::open_in_memory().expect("open");
        let e = entry("aseye://x/user/skill/foo", "deadbeef", 1_700_000_000);
        seed_parent(&handle, &e.component_id);
        upsert_manifest_entry(&handle, &e).expect("upsert");
        let got = read_manifest_entry(&handle, &e.component_id)
            .expect("read")
            .expect("Some");
        assert_eq!(got, e);
    }

    #[test]
    fn upsert_is_idempotent_and_replaces_fields() {
        let handle = IndexHandle::open_in_memory().expect("open");
        let mut e = entry("aseye://x/y/z/foo", "first", 100);
        seed_parent(&handle, &e.component_id);
        upsert_manifest_entry(&handle, &e).expect("first");
        e.plaintext_hash = "second".into();
        e.encrypted_at = 200;
        upsert_manifest_entry(&handle, &e).expect("second");
        let got = read_manifest_entry(&handle, &e.component_id)
            .expect("read")
            .expect("Some");
        assert_eq!(got.plaintext_hash, "second");
        assert_eq!(got.encrypted_at, 200);

        let n = manifest_count(&handle).expect("count");
        assert_eq!(n, 1, "second upsert must update, not insert");
    }

    #[test]
    fn read_returns_none_for_missing() {
        let handle = IndexHandle::open_in_memory().expect("open");
        let got = read_manifest_entry(&handle, "aseye://nope").expect("read");
        assert!(got.is_none());
    }

    #[test]
    fn delete_removes_row() {
        let handle = IndexHandle::open_in_memory().expect("open");
        let e = entry("aseye://x/y/z/g", "abc", 1);
        seed_parent(&handle, &e.component_id);
        upsert_manifest_entry(&handle, &e).expect("upsert");
        let removed = delete_manifest_entry(&handle, &e.component_id).expect("delete");
        assert_eq!(removed, 1);
        assert!(read_manifest_entry(&handle, &e.component_id)
            .unwrap()
            .is_none());
    }

    #[test]
    fn for_each_iterates_in_encrypted_at_order() {
        let handle = IndexHandle::open_in_memory().expect("open");
        for id in ["a", "b", "c"] {
            seed_parent(&handle, id);
        }
        upsert_manifest_entry(&handle, &entry("a", "h1", 300)).unwrap();
        upsert_manifest_entry(&handle, &entry("b", "h2", 100)).unwrap();
        upsert_manifest_entry(&handle, &entry("c", "h3", 200)).unwrap();
        let mut order = Vec::new();
        for_each_entry(&handle, |e| order.push(e.component_id)).unwrap();
        assert_eq!(order, vec!["b", "c", "a"]);
    }

    /// FK cascade: deleting the parent component scrubs the manifest
    /// row automatically. This is the safety net that keeps the
    /// orchestrator's `restore_now` from chasing dangling
    /// `component_id`s - the FK ensures deletion stays consistent
    /// without cleanup code in the orchestrator.
    #[test]
    fn deleting_parent_cascades_to_manifest() {
        let handle = IndexHandle::open_in_memory().expect("open");
        let e = entry("aseye://x/y/z/foo", "h", 1);
        seed_parent(&handle, &e.component_id);
        upsert_manifest_entry(&handle, &e).expect("upsert");
        // Drop the component row.
        handle
            .write(|c| {
                c.execute(
                    "DELETE FROM component WHERE id = ?1",
                    params![e.component_id],
                )?;
                Ok(())
            })
            .unwrap();
        assert!(read_manifest_entry(&handle, &e.component_id)
            .unwrap()
            .is_none());
    }
}
