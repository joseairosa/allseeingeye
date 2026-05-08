//! Atomic write primitive.
//!
//! Implements the temp + fsync + rename + parent-fsync sequence described in
//! `docs/05-data-architecture.md` ("Atomic writes"). The crate-wide rule is
//! that *every* mutation of a tool config flows through `atomic_write`.
//!
//! Failure modes (from `docs/11-risks.md` TR-2 and the failure-modes table
//! in docs/05) are mapped onto distinct `FsError` variants so callers can
//! react to "rename failed" vs "fsync failed" vs "parent dir not creatable"
//! with full fidelity. Per docs/05 ("Failure modes and recovery"), if disk
//! is full mid-save the atomic write fails *before* rename and the original
//! file is untouched.
//!
//! Cross-platform notes:
//! * On POSIX (`#[cfg(unix)]`) we open with `O_CREAT|O_EXCL|O_WRONLY` and
//!   `fsync(2)` the parent directory after rename so the rename is durable
//!   across power loss.
//! * On Windows we use `std::fs::rename`, which is *not* fully atomic for
//!   pre-existing targets but is the cross-platform-stable primitive
//!   shipping with std. POSIX rename is fully atomic; Windows rename is
//!   best-effort. Acceptable trade-off for the MVP; revisit if data
//!   integrity issues surface (TR-2 mitigation row 1).

use std::fs;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

use uuid::Uuid;

use super::error::FsError;

/// Build the `OpenOptions` for the temp file.
///
/// On all supported platforms `create_new(true)` translates to
/// `O_CREAT|O_EXCL` on Unix and `CREATE_NEW` on Windows, which matches the
/// "fail if it already exists" semantics required by the atomic-write
/// contract (a v4 UUID temp name should never collide, but if it ever
/// does we want a hard error rather than a silent overwrite).
fn temp_open_options() -> OpenOptions {
    let mut o = OpenOptions::new();
    o.write(true).create_new(true);
    o
}

/// Atomically write `content` to `path`.
///
/// Sequence (matches `docs/05-data-architecture.md`):
/// 1. Ensure the parent directory exists (`mkdir -p`).
/// 2. Open `<path>.aseye-tmp-<uuid>` with `O_CREAT|O_EXCL|O_WRONLY`.
/// 3. Write the full content (loops until done — handles short writes).
/// 4. `fsync` the temp file's fd.
/// 5. `rename(temp, path)` — atomic on POSIX, best-effort on Windows.
/// 6. `fsync` the parent directory fd (POSIX only; no-op on Windows).
///
/// On any error before the rename, the temp file is removed best-effort so
/// the parent directory does not accumulate `.aseye-tmp-*` debris.
///
/// # Errors
/// Returns the matching `FsError` variant for the failed step. The original
/// file (if any) is untouched until the rename succeeds.
pub fn atomic_write(path: &Path, content: &[u8]) -> Result<(), FsError> {
    // Step 1: ensure parent exists.
    let parent = parent_dir(path);
    if let Some(parent) = parent.as_deref() {
        fs::create_dir_all(parent).map_err(|source| FsError::ParentMkdir {
            path: parent.to_path_buf(),
            source,
        })?;
    }

    // Step 2: open temp file with O_EXCL (create_new on std).
    let temp_path = temp_path_for(path);
    let mut temp_file =
        temp_open_options()
            .open(&temp_path)
            .map_err(|source| FsError::TempCreate {
                path: temp_path.clone(),
                source,
            })?;

    // Helper closure to drive the rest of the writer; on any failure we
    // remove the temp file (best-effort) before bubbling up.
    let result = (|| -> Result<(), FsError> {
        // Step 3: write content fully (loop handles short writes).
        write_all_loop(&mut temp_file, content, &temp_path)?;

        // Step 4: fsync the temp file.
        temp_file.sync_all().map_err(|source| FsError::Fsync {
            path: temp_path.clone(),
            source,
        })?;
        // Drop the file handle before rename for Windows compat.
        drop(temp_file);

        // Step 5: rename temp -> final path. Atomic on POSIX, best-effort on
        // Windows (see module-level note).
        fs::rename(&temp_path, path).map_err(|source| FsError::Rename {
            from: temp_path.clone(),
            to: path.to_path_buf(),
            source,
        })?;

        // Step 6: parent directory fsync (POSIX only).
        #[cfg(unix)]
        if let Some(parent) = parent.as_deref() {
            fsync_dir(parent)?;
        }
        Ok(())
    })();

    // On error before rename: clean up the temp file if it still exists.
    if result.is_err() && temp_path.exists() {
        // Best-effort remove; we don't surface a secondary error.
        let _ = fs::remove_file(&temp_path);
    }
    result
}

/// Write a sidecar backup of `path` to `<path>.aseye-backup`, copying the
/// current bytes verbatim. Used as advisory restore material per
/// `docs/11-risks.md` TR-2 mitigation row 3.
///
/// Returns `Ok(Some(backup_path))` if a backup was created, or `Ok(None)`
/// if `path` does not exist (no source bytes to back up — not an error).
///
/// # Errors
/// Returns the underlying I/O error wrapped in `FsError::Write` if reading
/// the source or writing the backup fails. Backup itself is written via
/// `atomic_write` so a partially written backup file is never observed.
pub fn write_sidecar_backup(path: &Path) -> Result<Option<PathBuf>, FsError> {
    if !path.exists() {
        return Ok(None);
    }
    let bytes = fs::read(path).map_err(|source| FsError::Write {
        path: path.to_path_buf(),
        source,
    })?;
    let backup = backup_path_for(path);
    atomic_write(&backup, &bytes)?;
    Ok(Some(backup))
}

// --- internals -------------------------------------------------------------

/// Compute the temp file path for an atomic write target. Uses a v4 UUID
/// so two concurrent writers to the same target cannot collide.
fn temp_path_for(path: &Path) -> PathBuf {
    let parent = parent_dir(path).unwrap_or_else(|| PathBuf::from("."));
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    parent.join(format!("{file_name}.aseye-tmp-{}", Uuid::new_v4()))
}

/// Compute the sidecar backup path: `<path>.aseye-backup`.
fn backup_path_for(path: &Path) -> PathBuf {
    let parent = parent_dir(path).unwrap_or_else(|| PathBuf::from("."));
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    parent.join(format!("{file_name}.aseye-backup"))
}

/// Lift `Path::parent` into a `PathBuf` for ergonomic use across the call
/// graph. Returns `None` for paths with no parent (e.g. bare filenames).
fn parent_dir(path: &Path) -> Option<PathBuf> {
    path.parent().map(Path::to_path_buf)
}

/// Loop on `write` until all bytes are consumed. `write_all` already does
/// this internally on std but we wrap it to attribute any failure to the
/// `Write` variant with the full path for diagnostics.
fn write_all_loop(file: &mut fs::File, content: &[u8], temp_path: &Path) -> Result<(), FsError> {
    file.write_all(content).map_err(|source| FsError::Write {
        path: temp_path.to_path_buf(),
        source,
    })
}

/// fsync a directory file descriptor. POSIX-only — Windows does not expose
/// directory-fd fsync, so this is gated and a no-op on non-unix.
#[cfg(unix)]
fn fsync_dir(dir: &Path) -> Result<(), FsError> {
    let dir_file = fs::File::open(dir).map_err(|source| FsError::ParentFsync {
        path: dir.to_path_buf(),
        source,
    })?;
    dir_file.sync_all().map_err(|source| FsError::ParentFsync {
        path: dir.to_path_buf(),
        source,
    })
}

// --- tests -----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs as stdfs;
    use tempfile::tempdir;

    #[test]
    fn atomic_write_creates_new_file() {
        let dir = tempdir().expect("tempdir");
        let target = dir.path().join("hello.txt");
        atomic_write(&target, b"hello world").expect("atomic_write");
        let read = stdfs::read(&target).expect("read");
        assert_eq!(read, b"hello world");
    }

    #[test]
    fn atomic_write_overwrites_existing() {
        let dir = tempdir().expect("tempdir");
        let target = dir.path().join("a.txt");
        stdfs::write(&target, b"OLD").expect("seed");
        atomic_write(&target, b"NEW").expect("atomic_write");
        let read = stdfs::read(&target).expect("read");
        assert_eq!(read, b"NEW");
        assert_ne!(read, b"OLD");
    }

    /// Simulates a failure mid-write by writing into a *read-only* parent
    /// directory. The read-only perms cause `O_CREAT|O_EXCL` (the temp open)
    /// to fail; an existing file in the same dir remains intact because we
    /// never reach the rename step. POSIX-only because Windows perms model
    /// is different and the failure mode is not directly equivalent.
    #[cfg(unix)]
    #[test]
    fn atomic_write_no_corruption_on_failure() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempdir().expect("tempdir");
        let target = dir.path().join("orig.txt");
        stdfs::write(&target, b"ORIGINAL").expect("seed");

        // Make the parent dir read-only (no write/exec for owner).
        let perms_before = stdfs::metadata(dir.path()).unwrap().permissions();
        stdfs::set_permissions(dir.path(), stdfs::Permissions::from_mode(0o500))
            .expect("chmod 0500");

        let result = atomic_write(&target, b"REPLACED");
        assert!(result.is_err(), "atomic write must fail on read-only dir");

        // Restore perms before reading back / cleanup.
        stdfs::set_permissions(dir.path(), perms_before).expect("chmod restore");

        let read = stdfs::read(&target).expect("read");
        assert_eq!(read, b"ORIGINAL", "original file must be intact");
    }

    /// Same scenario as above; assert no `.aseye-tmp-*` debris remains.
    #[cfg(unix)]
    #[test]
    fn atomic_write_temp_cleaned_on_error() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempdir().expect("tempdir");
        let target = dir.path().join("orig.txt");
        stdfs::write(&target, b"x").expect("seed");

        let perms_before = stdfs::metadata(dir.path()).unwrap().permissions();
        stdfs::set_permissions(dir.path(), stdfs::Permissions::from_mode(0o500))
            .expect("chmod 0500");

        let _ = atomic_write(&target, b"y");

        stdfs::set_permissions(dir.path(), perms_before).expect("chmod restore");

        // Iterate the dir; assert no `.aseye-tmp-*` files leaked.
        let entries: Vec<_> = stdfs::read_dir(dir.path())
            .expect("read_dir")
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert!(
            !entries.iter().any(|name| name.contains(".aseye-tmp-")),
            "temp files leaked: {entries:?}"
        );
    }

    #[test]
    fn atomic_write_handles_short_writes() {
        // 5 MB matches the parser cap from docs/05; this exercises the
        // write_all loop without flirting with the cap.
        let dir = tempdir().expect("tempdir");
        let target = dir.path().join("big.bin");
        // Cast via masking to keep clippy::cast_possible_truncation /
        // cast_sign_loss happy; the masked value fits in u8 by construction.
        let payload: Vec<u8> = (0_u32..5 * 1024 * 1024)
            .map(|i| u8::try_from(i & 0xFF).expect("masked to byte"))
            .collect();
        atomic_write(&target, &payload).expect("atomic_write big");
        let read = stdfs::read(&target).expect("read");
        assert_eq!(read.len(), payload.len());
        assert_eq!(read, payload);
    }

    #[test]
    fn sidecar_backup_copies_existing() {
        let dir = tempdir().expect("tempdir");
        let target = dir.path().join("with-backup.txt");
        stdfs::write(&target, b"keepme").expect("seed");
        let backup = write_sidecar_backup(&target)
            .expect("backup")
            .expect("Some(path)");
        let read = stdfs::read(&backup).expect("read backup");
        assert_eq!(read, b"keepme");
        assert!(backup
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.ends_with(".aseye-backup")));
    }

    #[test]
    fn sidecar_backup_returns_none_for_missing() {
        let dir = tempdir().expect("tempdir");
        let target = dir.path().join("does-not-exist.txt");
        let result = write_sidecar_backup(&target).expect("ok");
        assert!(result.is_none());
    }

    /// Soak test: 1000 random-content writes to a tmpdir, then assert the
    /// final file matches the last write content. Gated with `#[ignore]`
    /// per the task brief; run via `cargo test -- --ignored soak_atomic_writes`.
    #[test]
    #[ignore = "long-running soak test; run with --ignored"]
    fn soak_atomic_writes() {
        let dir = tempdir().expect("tempdir");
        let target = dir.path().join("soak.bin");

        // Tiny xorshift-style PRNG to avoid pulling in `rand` for one test.
        let mut state: u64 = 0xDEAD_BEEF_CAFE_F00D;
        let mut last: Vec<u8> = Vec::new();
        for i in 0..1000_u32 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            // Vary size 1..=4096 bytes; vary content per iteration. We mask
            // first then `try_from` to keep clippy's cast_possible_truncation
            // happy; the masked value is guaranteed to fit in usize.
            let len = usize::try_from(state & 0xFFF).expect("12-bit mask fits usize") + 1;
            let mut buf = Vec::with_capacity(len);
            for j in 0_u32..u32::try_from(len).expect("len is at most 4096") {
                let mixed = i.wrapping_add(j) ^ u32::try_from(state & 0xFFFF_FFFF).expect("masked");
                buf.push(u8::try_from(mixed & 0xFF).expect("byte mask"));
            }
            atomic_write(&target, &buf).expect("soak atomic_write");
            last = buf;
        }

        let read = stdfs::read(&target).expect("read final");
        assert_eq!(read, last, "final file must match last write");
    }
}
