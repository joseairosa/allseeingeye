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
    write_sidecar_backup_with_suffix(path, ".aseye-backup")
}

/// Like [`write_sidecar_backup`] but with a caller-supplied suffix.
///
/// The restore flow uses this with `".aseye-pre-restore-<unix>.bak"`
/// so multiple restores produce distinct sidecars rather than
/// clobbering each other. Editor save keeps using the no-arg form
/// because it overwrites in place and only the latest pre-save copy
/// is interesting for one-shot recovery.
///
/// Returns `Ok(Some(sidecar_path))` on success, `Ok(None)` if the
/// source path does not exist (nothing to back up), or the matching
/// `FsError` variant if reading the source or writing the sidecar
/// fails.
pub fn write_sidecar_backup_with_suffix(
    path: &Path,
    suffix: &str,
) -> Result<Option<PathBuf>, FsError> {
    if !path.exists() {
        return Ok(None);
    }
    let bytes = fs::read(path).map_err(|source| FsError::Write {
        path: path.to_path_buf(),
        source,
    })?;
    let backup = sidecar_path_for(path, suffix);
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

/// Compute a sidecar path next to `path` with the given suffix
/// appended to the filename: `<filename><suffix>` inside the parent
/// directory.
fn sidecar_path_for(path: &Path, suffix: &str) -> PathBuf {
    let parent = parent_dir(path).unwrap_or_else(|| PathBuf::from("."));
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    parent.join(format!("{file_name}{suffix}"))
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

    /// Concurrent soak: 4 OS threads each write 1 000 distinct paths
    /// in the same tempdir. After every thread finishes:
    ///
    /// * each path's final content matches the last bytes that thread
    ///   wrote to it (no inter-thread clobbering on the rename step), and
    /// * no `.aseye-tmp-*` debris remains in the dir (the temp file
    ///   lifecycle stays clean across threads).
    ///
    /// Phase 5.1 - exercises the rename + parent-fsync path under
    /// concurrency. Distinct paths per thread guarantees no logical
    /// race on the *target* file; the threads do contend on the
    /// shared parent directory, which is exactly the surface we
    /// want to stress.
    #[test]
    #[ignore = "long-running soak test; run with --ignored"]
    fn soak_atomic_writes_concurrent() {
        use std::sync::Arc;
        use std::thread;

        let dir = tempdir().expect("tempdir");
        let dir_path = Arc::new(dir.path().to_path_buf());
        let writes_per_thread: u32 = 1000;
        let thread_count: u32 = 4;

        // Per-thread "expected final content" map, returned on join
        // so the assertion can run in the parent thread (rather than
        // panicking inside a worker).
        let mut handles = Vec::new();
        for tid in 0..thread_count {
            let dir_path = Arc::clone(&dir_path);
            handles.push(thread::spawn(
                move || -> Vec<(std::path::PathBuf, Vec<u8>)> {
                    let mut state: u64 = 0x00C0_FFEE_0000_0000_u64.wrapping_add(u64::from(tid));
                    let mut last: Vec<(std::path::PathBuf, Vec<u8>)> = Vec::new();
                    for i in 0..writes_per_thread {
                        state ^= state << 13;
                        state ^= state >> 7;
                        state ^= state << 17;
                        // One distinct path per (thread, iteration).
                        let target = dir_path.join(format!("t{tid}-i{i}.bin"));
                        let len = usize::try_from(state & 0x1FF).expect("9-bit mask") + 1;
                        let mut buf = Vec::with_capacity(len);
                        for j in 0_u32..u32::try_from(len).expect("len fits u32") {
                            let mixed = i.wrapping_add(j) ^ tid.wrapping_mul(7);
                            buf.push(u8::try_from(mixed & 0xFF).expect("byte mask"));
                        }
                        atomic_write(&target, &buf).expect("concurrent atomic_write");
                        last.push((target, buf));
                    }
                    last
                },
            ));
        }

        let mut all_expected: Vec<(std::path::PathBuf, Vec<u8>)> = Vec::new();
        for h in handles {
            all_expected.extend(h.join().expect("thread joined cleanly"));
        }

        // Every final file must equal the bytes that thread wrote to
        // it on its last iteration. Distinct paths means no
        // last-writer-wins ambiguity; the assertion is exact.
        for (path, expected) in &all_expected {
            let got = stdfs::read(path).expect("read final");
            assert_eq!(&got, expected, "content mismatch for {path:?}");
        }

        // No `.aseye-tmp-*` debris in the parent dir: the cleanup
        // path inside `atomic_write` removed every intermediate temp
        // file, even under concurrency.
        let entries: Vec<String> = stdfs::read_dir(dir.path())
            .expect("read_dir")
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert!(
            !entries.iter().any(|name| name.contains(".aseye-tmp-")),
            "temp file debris under concurrency: {entries:?}"
        );
    }

    /// Soak: rotate the trusted-root list while writes are in flight.
    ///
    /// Two trusted roots, a writer thread, and a rotator thread.
    /// The writer keeps issuing `safe_atomic_write_with_options`
    /// against a path under root A. The rotator alternately presents
    /// `[root_a]`, `[root_a, root_b]`, and `[root_b, root_a]` as the
    /// allowed-roots slice. Goal: regardless of which slice is in
    /// effect, the writer either (a) succeeds because A is in the
    /// slice, or (b) fails cleanly with `EscapeDetected` /
    /// `NotInAnyRoot`. No partial state, no debris, no panics.
    ///
    /// Phase 5.1 - the pattern is meant to mirror what happens when
    /// the user rescans tools or a tool descriptor reloads under the
    /// watcher's nose: the trusted-root set changes; in-flight
    /// writers must remain safe.
    #[test]
    #[ignore = "long-running soak test; run with --ignored"]
    fn soak_safe_atomic_write_under_changing_roots() {
        use crate::fs::safety::safe_atomic_write_with_options;
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;
        use std::thread;
        use std::time::Duration;

        let root_a = tempdir().expect("root_a");
        let root_b = tempdir().expect("root_b");
        // Pre-create the parent dir for the writer's target file so
        // canonicalisation works on every iteration. The writer
        // writes a brand-new file each iteration but always under
        // the same parent.
        let target_parent = root_a.path().join("aseye");
        stdfs::create_dir_all(&target_parent).expect("mkdir target parent");

        let stop = Arc::new(AtomicBool::new(false));

        // Rotator: cycle the allowed-roots slice every ~2 ms. No
        // synchronisation with the writer beyond the AtomicBool.
        // The rotator is purely a stress generator; the writer
        // chooses its own slice from a snapshot at call time.
        //
        // We model "rotation" as a phase counter the writer reads
        // each iteration. This avoids a shared mutable Vec across
        // threads (which would need a Mutex and obscure the real
        // contention shape we want to stress).
        let phase = Arc::new(parking_lot::Mutex::new(0_u32));
        let rotator = {
            let stop = Arc::clone(&stop);
            let phase = Arc::clone(&phase);
            thread::spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    {
                        let mut p = phase.lock();
                        *p = p.wrapping_add(1);
                    }
                    thread::sleep(Duration::from_millis(2));
                }
            })
        };

        // Writer: 4 000 iterations, one new file per iteration.
        let writer_count: u32 = 4000;
        let mut succeeded: u32 = 0;
        let mut failed_clean: u32 = 0;
        for i in 0..writer_count {
            let p = *phase.lock();
            let roots: Vec<&Path> = match p % 3 {
                0 => vec![root_a.path()],
                1 => vec![root_a.path(), root_b.path()],
                _ => vec![root_b.path(), root_a.path()],
            };
            let target = target_parent.join(format!("rot-{i}.bin"));
            let payload = format!("rot {i}").into_bytes();
            match safe_atomic_write_with_options(
                &target, &payload, &roots, /* allow_outside_home: */ true,
            ) {
                Ok(()) => succeeded += 1,
                Err(
                    crate::fs::error::FsError::EscapeDetected { .. }
                    | crate::fs::error::FsError::NotInAnyRoot { .. },
                ) => {
                    failed_clean += 1;
                }
                Err(other) => panic!("unexpected error during root rotation: {other:?}"),
            }
        }

        stop.store(true, Ordering::Relaxed);
        rotator.join().expect("rotator joined");

        // Every iteration must terminate as either a clean success
        // or a clean root-rejection - never a panic, never a partial
        // file. The split is incidental; what matters is the total.
        assert_eq!(
            succeeded + failed_clean,
            writer_count,
            "every iteration must complete cleanly: \
             succeeded={succeeded}, failed_clean={failed_clean}",
        );
        // root_a is in every slice configuration this rotator
        // produces, so every write should have succeeded. Asserting
        // the success count proves the rotation didn't drop the
        // root-A inclusion, which would cause spurious failures.
        assert_eq!(
            succeeded, writer_count,
            "writes against root_a must always succeed: \
             succeeded={succeeded}, failed_clean={failed_clean}",
        );

        // No temp-file debris in the writer's directory.
        let entries: Vec<String> = stdfs::read_dir(&target_parent)
            .expect("read_dir")
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert!(
            !entries.iter().any(|name| name.contains(".aseye-tmp-")),
            "temp file debris under root rotation: {entries:?}"
        );
    }
}
