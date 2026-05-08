//! Path safety guards.
//!
//! Two complementary checks, both required before any write:
//!
//! 1. **Containment** (`assert_within_root`): the canonical (symlink-resolved)
//!    target path must lie under a registered root. Implements the SR-3
//!    mitigation from `docs/11-risks.md` ("All path resolution goes through
//!    a canonicaliser that asserts the result lies within an expected root.
//!    Symlinks pointing outside roots are not followed.").
//!
//! 2. **Forbidden segments** (`assert_safe_target`): rejects writes inside
//!    `.git`, `node_modules`, `target`, `dist`, `.venv`, `__pycache__`,
//!    `.next`, `build`. The first six are the docs-canonical list
//!    (docs/05 + docs/08 "File-system safety"); `.next` and `build` are
//!    added per the Phase 1.5 task brief as a tighter MVP set.
//!
//! Both checks are pure path computations — no writes, no side effects.

use std::path::{Component, Path, PathBuf};

use super::atomic::atomic_write;
use super::error::FsError;

/// Path components we refuse to write inside, anywhere in the resolved path.
/// Tighter than the docs/05 list by adding `.next` and `build` so we don't
/// scribble into framework build outputs that round-trip-regenerate from
/// source.
const FORBIDDEN_SEGMENTS: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    "dist",
    ".venv",
    "__pycache__",
    ".next",
    "build",
];

/// Assert that `path`, after symlink resolution, lies under `root`. Returns
/// the canonicalised path on success.
///
/// Special case: when `path` does not yet exist (e.g. writing a brand-new
/// component file), we canonicalise the *parent* and append the file name.
/// This avoids `ENOENT` on the target while still resolving any symlinks
/// in the prefix.
///
/// # Errors
/// * `FsError::Canonicalize` — `canonicalize` itself fails (deeper FS error).
/// * `FsError::EscapeDetected` — canonical(path) does not start with
///   canonical(root).
pub fn assert_within_root(path: &Path, root: &Path) -> Result<PathBuf, FsError> {
    let canonical_root = canonicalize(root)?;
    let canonical_path = canonicalize_existing_or_parent(path)?;

    if canonical_path.starts_with(&canonical_root) {
        Ok(canonical_path)
    } else {
        Err(FsError::EscapeDetected {
            path: canonical_path,
            root: canonical_root,
        })
    }
}

/// Assert that the path does not contain a forbidden segment and resolves
/// inside the user's home directory.
///
/// # Errors
/// * `FsError::ForbiddenSegment` — any component matches the deny list.
/// * `FsError::OutsideHome` — canonicalised path is outside `dirs::home_dir()`.
/// * `FsError::HomeUnavailable` — `dirs::home_dir()` returned `None`.
/// * `FsError::Canonicalize` — see `assert_within_root`.
pub fn assert_safe_target(path: &Path) -> Result<(), FsError> {
    assert_safe_target_inner(path, /* allow_outside_home: */ false)
}

/// Same as `assert_safe_target` but with an explicit override for tests
/// that need to write into a tmpdir (which is typically outside `$HOME` on
/// macOS / Linux CI).
///
/// # Errors
/// See `assert_safe_target`. `OutsideHome` is suppressed when
/// `allow_outside_home` is `true`.
pub fn assert_safe_target_with_override(
    path: &Path,
    allow_outside_home: bool,
) -> Result<(), FsError> {
    assert_safe_target_inner(path, allow_outside_home)
}

fn assert_safe_target_inner(path: &Path, allow_outside_home: bool) -> Result<(), FsError> {
    // Forbidden-segment check operates on the *raw* path so callers cannot
    // bypass it by symlinking a node_modules dir under a benign-looking
    // root. We also re-check after canonicalisation for symlink dodge.
    if let Some(seg) = first_forbidden_component(path) {
        return Err(FsError::ForbiddenSegment {
            path: path.to_path_buf(),
            segment: seg,
        });
    }

    let canonical = canonicalize_existing_or_parent(path)?;
    if let Some(seg) = first_forbidden_component(&canonical) {
        return Err(FsError::ForbiddenSegment {
            path: canonical,
            segment: seg,
        });
    }

    if !allow_outside_home {
        let home = dirs::home_dir().ok_or(FsError::HomeUnavailable)?;
        let canonical_home = canonicalize(&home)?;
        if !canonical.starts_with(&canonical_home) {
            return Err(FsError::OutsideHome { path: canonical });
        }
    }
    Ok(())
}

/// Combine the safety checks and the atomic writer.
///
/// Order:
/// 1. `assert_safe_target` (forbidden segments + home).
/// 2. For each root in `roots`, try `assert_within_root`. First match wins.
/// 3. If no root matches, return `NotInAnyRoot`.
/// 4. Call `atomic_write`.
///
/// # Errors
/// Any error from the underlying checks or `atomic_write`.
pub fn safe_atomic_write(
    path: &Path,
    content: &[u8],
    roots: &[&Path],
) -> Result<(), FsError> {
    safe_atomic_write_with_options(path, content, roots, /* allow_outside_home: */ false)
}

/// Test/internal hook: same as `safe_atomic_write` but lets callers opt out
/// of the home-dir guard. Production callers always pass `false`.
///
/// # Errors
/// As `safe_atomic_write`.
pub fn safe_atomic_write_with_options(
    path: &Path,
    content: &[u8],
    roots: &[&Path],
    allow_outside_home: bool,
) -> Result<(), FsError> {
    assert_safe_target_with_override(path, allow_outside_home)?;

    let mut last_err: Option<FsError> = None;
    let mut matched = false;
    for root in roots {
        match assert_within_root(path, root) {
            Ok(_) => {
                matched = true;
                break;
            }
            Err(e @ FsError::EscapeDetected { .. }) => {
                last_err = Some(e);
            }
            Err(other) => return Err(other),
        }
    }
    if !matched {
        return Err(last_err.unwrap_or(FsError::NotInAnyRoot {
            path: path.to_path_buf(),
        }));
    }

    atomic_write(path, content)
}

// --- internals -------------------------------------------------------------

/// Walk `path`'s components and return the first one that matches the
/// forbidden-segment list, if any. Case-sensitive — matches the docs which
/// list lowercase-only segments and reflects the on-disk reality of the
/// directories we want to avoid.
fn first_forbidden_component(path: &Path) -> Option<String> {
    for component in path.components() {
        if let Component::Normal(os) = component {
            let s = os.to_string_lossy();
            if FORBIDDEN_SEGMENTS.iter().any(|forbidden| *forbidden == s) {
                return Some(s.into_owned());
            }
        }
    }
    None
}

/// Canonicalise an existing path. Wraps the std error in our typed variant.
fn canonicalize(path: &Path) -> Result<PathBuf, FsError> {
    std::fs::canonicalize(path).map_err(|source| FsError::Canonicalize {
        path: path.to_path_buf(),
        source,
    })
}

/// If `path` exists, canonicalise it. Otherwise canonicalise the parent and
/// re-append the file name. This handles the "writing a brand-new file
/// inside an existing root" case without bouncing on `ENOENT`.
fn canonicalize_existing_or_parent(path: &Path) -> Result<PathBuf, FsError> {
    if path.exists() {
        return canonicalize(path);
    }
    let parent = path.parent().ok_or_else(|| FsError::Canonicalize {
        path: path.to_path_buf(),
        source: std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "path has no parent and does not exist",
        ),
    })?;
    let canonical_parent = canonicalize(parent)?;
    let file_name = path.file_name().ok_or_else(|| FsError::Canonicalize {
        path: path.to_path_buf(),
        source: std::io::Error::new(std::io::ErrorKind::InvalidInput, "path has no file name"),
    })?;
    Ok(canonical_parent.join(file_name))
}

// --- tests -----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs as stdfs;
    use tempfile::tempdir;

    #[test]
    fn assert_within_root_rejects_escape() {
        // Use two tmp dirs so we have stable canonicalised roots regardless
        // of the host OS (avoids /etc weirdness on CI).
        let dir_a = tempdir().expect("dir_a");
        let dir_b = tempdir().expect("dir_b");
        let outside = dir_b.path().join("outside.txt");
        stdfs::write(&outside, b"x").expect("seed outside");

        let err =
            assert_within_root(&outside, dir_a.path()).expect_err("escape must be rejected");
        match err {
            FsError::EscapeDetected { .. } => {}
            other => panic!("expected EscapeDetected, got {other:?}"),
        }
    }

    #[test]
    fn assert_within_root_accepts_inside() {
        let dir = tempdir().expect("dir");
        let inside = dir.path().join("good.txt");
        stdfs::write(&inside, b"x").expect("seed");
        let canonical = assert_within_root(&inside, dir.path()).expect("accept");
        assert!(canonical.starts_with(stdfs::canonicalize(dir.path()).unwrap()));
    }

    /// Symlink dodge: create a symlink inside `root_a` that points into
    /// `root_b`. When canonicalised against `root_a`, the symlink resolves
    /// into `root_b` and must be rejected. POSIX-only: Windows symlink
    /// creation requires elevated privileges or developer mode.
    #[cfg(unix)]
    #[test]
    fn assert_within_root_handles_symlink_escape() {
        use std::os::unix::fs::symlink;
        let root_a = tempdir().expect("root_a");
        let root_b = tempdir().expect("root_b");

        // Create a real target file inside root_b.
        let real_target = root_b.path().join("secrets.txt");
        stdfs::write(&real_target, b"secret").expect("seed");

        // Create a symlink at root_a/escape -> root_b/secrets.txt.
        let link = root_a.path().join("escape");
        symlink(&real_target, &link).expect("symlink");

        let err = assert_within_root(&link, root_a.path())
            .expect_err("symlink escape must be rejected");
        match err {
            FsError::EscapeDetected { .. } => {}
            other => panic!("expected EscapeDetected, got {other:?}"),
        }
    }

    #[test]
    fn assert_safe_target_rejects_node_modules() {
        // Use a path inside HOME so the OutsideHome check doesn't fire
        // first. We also use allow_outside_home=true via the override
        // helper to keep the test robust on CI hosts where HOME is exotic.
        let dir = tempdir().expect("dir");
        let bad = dir.path().join("project").join("node_modules").join("foo.json");
        // Create the parent so canonicalisation succeeds.
        stdfs::create_dir_all(bad.parent().unwrap()).expect("mkdirs");
        let err = assert_safe_target_with_override(&bad, true)
            .expect_err("node_modules must be rejected");
        match err {
            FsError::ForbiddenSegment { segment, .. } => assert_eq!(segment, "node_modules"),
            other => panic!("expected ForbiddenSegment, got {other:?}"),
        }
    }

    #[test]
    fn assert_safe_target_rejects_git() {
        let dir = tempdir().expect("dir");
        let bad = dir.path().join(".git").join("config");
        stdfs::create_dir_all(bad.parent().unwrap()).expect("mkdirs");
        let err = assert_safe_target_with_override(&bad, true)
            .expect_err(".git must be rejected");
        match err {
            FsError::ForbiddenSegment { segment, .. } => assert_eq!(segment, ".git"),
            other => panic!("expected ForbiddenSegment, got {other:?}"),
        }
    }

    #[test]
    fn assert_safe_target_accepts_normal() {
        // Normal-looking path inside a tmpdir; allow_outside_home=true so
        // CI tmp paths don't trigger the home guard.
        let dir = tempdir().expect("dir");
        let good = dir.path().join(".claude").join("settings.json");
        stdfs::create_dir_all(good.parent().unwrap()).expect("mkdirs");
        // Pre-create the parent so canonicalisation works for a
        // non-existing target file.
        assert_safe_target_with_override(&good, true).expect("normal path accepted");
    }

    #[test]
    fn safe_atomic_write_full_path() {
        let root = tempdir().expect("root");
        let target = root.path().join(".claude").join("settings.json");
        stdfs::create_dir_all(target.parent().unwrap()).expect("mkdirs");
        safe_atomic_write_with_options(
            &target,
            b"{\"ok\":true}",
            &[root.path()],
            /* allow_outside_home: */ true,
        )
        .expect("safe write");
        let read = stdfs::read(&target).expect("read");
        assert_eq!(read, b"{\"ok\":true}");
    }

    #[test]
    fn safe_atomic_write_blocks_outside_root() {
        let root = tempdir().expect("root");
        let other = tempdir().expect("other");
        let target = other.path().join("evil.json");
        // Pre-create parent so canonicalisation succeeds.
        let err = safe_atomic_write_with_options(
            &target,
            b"x",
            &[root.path()],
            /* allow_outside_home: */ true,
        )
        .expect_err("write outside root must fail");
        match err {
            FsError::EscapeDetected { .. } | FsError::NotInAnyRoot { .. } => {}
            other => panic!("expected escape/not-in-root, got {other:?}"),
        }
        assert!(!target.exists(), "target must not have been created");
    }
}
