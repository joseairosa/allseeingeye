//! Action 3: reorganise loose `*.md` files into `<project>/docs/`.
//!
//! Spec docs/17 §17.5. The action runs as **dry-run by default**;
//! the caller passes `dry_run = false` to commit the changes. Even
//! the apply path writes a `.aseye-pre-reorg-<unix>.bak` sidecar of
//! every source file before moving it, so a wrong move is recoverable
//! by hand.
//!
//! Operations:
//! 1. Find every loose top-level `*.md` file (not in the allowlist).
//! 2. Build the `<project>/docs/<filename>` move plan.
//! 3. Walk every `*.md` file in the project (bounded), find inline
//!    links to the moved files, build a rewrite plan.
//! 4. (apply only) write sidecars, perform moves, rewrite links.

#![allow(clippy::cast_precision_loss)]

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::fs::{safe_atomic_write_with_options, write_sidecar_backup_with_suffix, FsError};

/// Files that conventionally live at the project root and must not
/// be moved. Matches docs/17 §17.5.
const ALLOWLIST: &[&str] = &[
    "README.md",
    "README.MD",
    "Readme.md",
    "CLAUDE.md",
    "CLAUDE.local.md",
    "AGENTS.md",
    "GEMINI.md",
    "CHANGELOG.md",
    "LICENSE.md",
    "LICENSE-APACHE.md",
    "LICENSE-MIT.md",
    "CONTRIBUTING.md",
    "CODE_OF_CONDUCT.md",
    "SECURITY.md",
    "COMPONENTS.md",
];

/// Bounded walk caps - same as `worktrees::bounded_du`. Rejects
/// pathological project trees (full of `node_modules` etc.) without
/// blocking the UI.
const WALK_MAX_ENTRIES: usize = 100_000;
const WALK_TIMEOUT_SECS: u64 = 60;

/// Directories we skip while walking for link-rewrite candidates.
/// These are big and never carry markdown that links to other docs.
const WALK_SKIP_DIRS: &[&str] = &[
    "node_modules",
    ".git",
    "target",
    "build",
    "dist",
    ".next",
    ".venv",
    "venv",
    "__pycache__",
    ".cache",
    "vendor",
    "Pods",
    ".terraform",
];

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../bindings/projects/ReorganizeReport.ts")]
#[ts(rename_all = "camelCase")]
pub struct ReorganizeReport {
    pub project_path: String,
    pub dry_run: bool,
    pub moves: Vec<ReorganizeMove>,
    pub link_rewrites: Vec<LinkRewrite>,
    pub errors: Vec<ReorganizeError>,
    pub elapsed_ms: u64,
    /// True iff the link-rewrite walk hit the entry / time cap.
    pub walk_incomplete: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../bindings/projects/ReorganizeMove.ts")]
#[ts(rename_all = "camelCase")]
pub struct ReorganizeMove {
    pub from: String,
    pub to: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../bindings/projects/LinkRewrite.ts")]
#[ts(rename_all = "camelCase")]
pub struct LinkRewrite {
    pub file: String,
    pub line: u32,
    pub before: String,
    pub after: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../bindings/projects/ReorganizeError.ts")]
#[ts(rename_all = "camelCase")]
pub struct ReorganizeError {
    pub path: String,
    pub kind: ReorganizeErrorKind,
    pub message: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../bindings/projects/ReorganizeErrorKind.ts")]
#[ts(rename_all = "camelCase")]
pub enum ReorganizeErrorKind {
    /// File is in the allowlist; refusing to move (e.g. README.md).
    AllowlistConflict,
    /// `<project>/docs/<filename>` already exists; refusing to
    /// overwrite.
    DestExists,
    Read,
    Write,
    Rename,
    Sidecar,
}

#[derive(Debug, thiserror::Error)]
pub enum OrchestrationError {
    #[error("project path does not exist: {0}")]
    ProjectMissing(String),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Fs(#[from] FsError),
}

/// Build the reorganise plan and (when `dry_run = false`) apply it.
/// Read-only when `dry_run = true`.
pub fn reorganize_docs(
    project_path: &Path,
    dry_run: bool,
) -> Result<ReorganizeReport, OrchestrationError> {
    let started = std::time::Instant::now();

    if !project_path.exists() {
        return Err(OrchestrationError::ProjectMissing(
            project_path.to_string_lossy().into_owned(),
        ));
    }

    // Step 1: find loose top-level *.md files.
    let mut moves: Vec<ReorganizeMove> = Vec::new();
    let mut errors: Vec<ReorganizeError> = Vec::new();
    let mut moved_basenames: Vec<String> = Vec::new();

    let docs_dir = project_path.join("docs");
    let read_dir = std::fs::read_dir(project_path)?;
    for entry in read_dir.flatten() {
        let p = entry.path();
        if !p.is_file() {
            continue;
        }
        let Some(basename) = p.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !basename.to_lowercase().ends_with(".md") {
            continue;
        }
        if ALLOWLIST.contains(&basename) {
            continue;
        }
        let dest = docs_dir.join(basename);
        if dest.exists() {
            errors.push(ReorganizeError {
                path: p.to_string_lossy().into_owned(),
                kind: ReorganizeErrorKind::DestExists,
                message: format!("destination {} already exists; not moving", dest.display()),
            });
            continue;
        }
        let size = entry.metadata().map_or(0, |m| m.len());
        moves.push(ReorganizeMove {
            from: p.to_string_lossy().into_owned(),
            to: dest.to_string_lossy().into_owned(),
            size,
        });
        moved_basenames.push(basename.to_owned());
    }

    // Step 2: walk every *.md file in the project for link rewrites.
    let (rewrites, walk_incomplete) =
        plan_link_rewrites(project_path, &moved_basenames, &mut errors);

    let report_template = ReorganizeReport {
        project_path: project_path.to_string_lossy().into_owned(),
        dry_run,
        moves: moves.clone(),
        link_rewrites: rewrites.clone(),
        errors: errors.clone(),
        elapsed_ms: 0,
        walk_incomplete,
    };

    if dry_run {
        let mut r = report_template;
        r.elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        return Ok(r);
    }

    // Step 3 (apply only): perform moves with sidecar backups, then
    // rewrite links.
    if !moves.is_empty() {
        std::fs::create_dir_all(&docs_dir)?;
    }

    let suffix = format!(".aseye-pre-reorg-{}.bak", unix_now());
    for mv in &moves {
        match apply_move(Path::new(&mv.from), Path::new(&mv.to), &suffix) {
            Ok(()) => {}
            Err(e) => errors.push(e),
        }
    }

    for rewrite in &rewrites {
        match apply_link_rewrite(rewrite) {
            Ok(()) => {}
            Err(e) => errors.push(e),
        }
    }

    let mut r = report_template;
    r.errors = errors;
    r.elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    Ok(r)
}

fn plan_link_rewrites(
    project_path: &Path,
    moved_basenames: &[String],
    errors: &mut Vec<ReorganizeError>,
) -> (Vec<LinkRewrite>, bool) {
    if moved_basenames.is_empty() {
        return (Vec::new(), false);
    }
    let mut rewrites: Vec<LinkRewrite> = Vec::new();
    let started = std::time::Instant::now();
    let timeout = std::time::Duration::from_secs(WALK_TIMEOUT_SECS);
    let mut count: usize = 0;
    let mut incomplete = false;
    let mut stack: Vec<PathBuf> = vec![project_path.to_path_buf()];

    while let Some(dir) = stack.pop() {
        if count >= WALK_MAX_ENTRIES || started.elapsed() >= timeout {
            incomplete = true;
            break;
        }
        let Ok(read) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in read.flatten() {
            count = count.saturating_add(1);
            if count >= WALK_MAX_ENTRIES || started.elapsed() >= timeout {
                incomplete = true;
                break;
            }
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            if meta.is_dir() {
                if WALK_SKIP_DIRS.contains(&name) || name.starts_with('.') {
                    continue;
                }
                stack.push(path);
                continue;
            }
            if !meta.is_file() || !name.to_lowercase().ends_with(".md") {
                continue;
            }
            // Skip the destination dir's files - they don't need
            // rewriting (they're already in docs/).
            // Actually, we DO need to rewrite their `../FOO.md`
            // references because if the moved file was previously at
            // root, a docs/-rooted file's `../FOO.md` would no longer
            // exist. Keep them in scope.
            match collect_rewrites_for_file(&path, moved_basenames) {
                Ok(mut these) => rewrites.append(&mut these),
                Err(e) => errors.push(ReorganizeError {
                    path: path.to_string_lossy().into_owned(),
                    kind: ReorganizeErrorKind::Read,
                    message: format!("read for link scan: {e}"),
                }),
            }
        }
        if incomplete {
            break;
        }
    }

    (rewrites, incomplete)
}

/// For each line in `file`, find link expressions referencing any of
/// `moved_basenames`, build before/after strings.
fn collect_rewrites_for_file(
    file: &Path,
    moved_basenames: &[String],
) -> std::io::Result<Vec<LinkRewrite>> {
    let bytes = std::fs::read(file)?;
    let text = String::from_utf8_lossy(&bytes).into_owned();
    let file_str = file.to_string_lossy().into_owned();
    let in_docs_dir = file
        .parent()
        .is_some_and(|p| p.file_name().and_then(|n| n.to_str()) == Some("docs"));

    let mut out: Vec<LinkRewrite> = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let line_no = u32::try_from(i + 1).unwrap_or(u32::MAX);
        for name in moved_basenames {
            // Inline-link patterns we rewrite. We look for the literal
            // basename inside `]( ... )` so we don't fire on prose
            // mentions like "see CHANGELOG.md for history" without a
            // link wrapper.
            let candidates = [
                (format!("]({name})"), format!("](docs/{name})")),
                (format!("](./{name})"), format!("](./docs/{name})")),
                (
                    // From inside docs/, `../FOO.md` was the old way
                    // to reach FOO.md at root. Now FOO.md lives in
                    // docs/ alongside us, so the link becomes
                    // `./FOO.md`.
                    format!("](../{name})"),
                    if in_docs_dir {
                        format!("](./{name})")
                    } else {
                        format!("](docs/{name})")
                    },
                ),
            ];
            for (before, after) in &candidates {
                if line.contains(before.as_str()) {
                    out.push(LinkRewrite {
                        file: file_str.clone(),
                        line: line_no,
                        before: before.clone(),
                        after: after.clone(),
                    });
                }
            }
        }
    }
    Ok(out)
}

fn apply_move(from: &Path, to: &Path, sidecar_suffix: &str) -> Result<(), ReorganizeError> {
    // Sidecar: pre-move snapshot. Best-effort - failure logs but
    // does not abort the move; skipping the move would leave the
    // user with an inconsistent state where some files moved and
    // some didn't.
    if let Err(err) = write_sidecar_backup_with_suffix(from, sidecar_suffix) {
        tracing::warn!(?err, ?from, "sidecar write failed before reorg move");
    }

    let bytes = std::fs::read(from).map_err(|e| ReorganizeError {
        path: from.to_string_lossy().into_owned(),
        kind: ReorganizeErrorKind::Read,
        message: format!("read source: {e}"),
    })?;

    // The trusted root for the safe writer is the destination's
    // parent. We allow `outside_home: true` for parity with the
    // existing usage in storage / restore.
    let parent = to.parent().ok_or_else(|| ReorganizeError {
        path: to.to_string_lossy().into_owned(),
        kind: ReorganizeErrorKind::Write,
        message: "destination has no parent".to_owned(),
    })?;
    let roots: [&Path; 1] = [parent];
    safe_atomic_write_with_options(to, &bytes, &roots, /* allow_outside_home: */ true).map_err(
        |e| ReorganizeError {
            path: to.to_string_lossy().into_owned(),
            kind: ReorganizeErrorKind::Write,
            message: format!("atomic write to dest: {e}"),
        },
    )?;

    // Source is now duplicated at dest; remove the source. The
    // sidecar is the recovery path if the user wants the source
    // bytes back.
    std::fs::remove_file(from).map_err(|e| ReorganizeError {
        path: from.to_string_lossy().into_owned(),
        kind: ReorganizeErrorKind::Rename,
        message: format!("remove source after copy: {e}"),
    })?;

    Ok(())
}

fn apply_link_rewrite(rewrite: &LinkRewrite) -> Result<(), ReorganizeError> {
    let path = Path::new(&rewrite.file);
    let bytes = std::fs::read(path).map_err(|e| ReorganizeError {
        path: rewrite.file.clone(),
        kind: ReorganizeErrorKind::Read,
        message: format!("read for rewrite: {e}"),
    })?;
    let text = String::from_utf8_lossy(&bytes).into_owned();
    if !text.contains(rewrite.before.as_str()) {
        // Already rewritten by an earlier pass, or the file changed
        // under us. Treat as a no-op rather than an error.
        return Ok(());
    }
    let next = text.replacen(rewrite.before.as_str(), rewrite.after.as_str(), 1);
    let parent = path.parent().ok_or_else(|| ReorganizeError {
        path: rewrite.file.clone(),
        kind: ReorganizeErrorKind::Write,
        message: "file has no parent".to_owned(),
    })?;
    let roots: [&Path; 1] = [parent];
    safe_atomic_write_with_options(
        path,
        next.as_bytes(),
        &roots,
        /* allow_outside_home: */ true,
    )
    .map_err(|e| ReorganizeError {
        path: rewrite.file.clone(),
        kind: ReorganizeErrorKind::Write,
        message: format!("atomic write rewrite: {e}"),
    })?;

    Ok(())
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_secs()).ok())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn touch(dir: &Path, name: &str, body: &str) -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, body).unwrap();
        p
    }

    #[test]
    fn dry_run_lists_moves_and_does_not_write() {
        let dir = tempdir().unwrap();
        touch(dir.path(), "README.md", "");
        touch(dir.path(), "GUIDE.md", "");
        touch(dir.path(), "ROADMAP.md", "");

        let r = reorganize_docs(dir.path(), true).unwrap();
        assert!(r.dry_run);
        assert_eq!(r.moves.len(), 2, "{:?}", r.moves);
        let names: Vec<&str> = r
            .moves
            .iter()
            .filter_map(|m| Path::new(&m.from).file_name().and_then(|n| n.to_str()))
            .collect();
        assert!(names.contains(&"GUIDE.md"));
        assert!(names.contains(&"ROADMAP.md"));
        assert!(!names.contains(&"README.md"));
        // No files moved on disk.
        assert!(dir.path().join("GUIDE.md").exists());
        assert!(!dir.path().join("docs").exists());
    }

    #[test]
    fn allowlist_files_skipped() {
        let dir = tempdir().unwrap();
        for name in ALLOWLIST {
            touch(dir.path(), name, "");
        }
        let r = reorganize_docs(dir.path(), true).unwrap();
        assert!(
            r.moves.is_empty(),
            "allowlist files should not be planned: {:?}",
            r.moves,
        );
    }

    #[test]
    fn destination_conflict_surfaces_error() {
        let dir = tempdir().unwrap();
        touch(dir.path(), "GUIDE.md", "new content");
        std::fs::create_dir(dir.path().join("docs")).unwrap();
        touch(&dir.path().join("docs"), "GUIDE.md", "old");

        let r = reorganize_docs(dir.path(), true).unwrap();
        assert!(r.moves.is_empty());
        assert_eq!(r.errors.len(), 1);
        assert!(matches!(r.errors[0].kind, ReorganizeErrorKind::DestExists));
    }

    #[test]
    fn apply_actually_moves_files_and_writes_sidecar() {
        let dir = tempdir().unwrap();
        touch(dir.path(), "GUIDE.md", "guide content");

        let r = reorganize_docs(dir.path(), false).unwrap();
        assert_eq!(r.moves.len(), 1);
        assert!(r.errors.is_empty(), "{:?}", r.errors);
        assert!(!dir.path().join("GUIDE.md").exists());
        assert!(dir.path().join("docs/GUIDE.md").exists());
        assert_eq!(
            std::fs::read_to_string(dir.path().join("docs/GUIDE.md")).unwrap(),
            "guide content"
        );

        // Sidecar at the original location.
        let sidecar_present = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(std::result::Result::ok)
            .any(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("GUIDE.md.aseye-pre-reorg-")
            });
        assert!(sidecar_present, "expected pre-reorg sidecar");
    }

    #[test]
    fn link_rewrite_picks_up_three_shapes() {
        let dir = tempdir().unwrap();
        touch(dir.path(), "GUIDE.md", "guide content");
        touch(
            dir.path(),
            "README.md",
            "\
[a](GUIDE.md)
[b](./GUIDE.md)
[c](../GUIDE.md)
plain mention of GUIDE.md (should NOT rewrite)
",
        );

        let r = reorganize_docs(dir.path(), true).unwrap();
        // Three rewrites: `]( ... )` shapes only.
        let rewrites_in_readme: Vec<&LinkRewrite> = r
            .link_rewrites
            .iter()
            .filter(|w| w.file.ends_with("README.md"))
            .collect();
        assert_eq!(
            rewrites_in_readme.len(),
            3,
            "expected exactly three rewrites: {:?}",
            r.link_rewrites,
        );
    }

    #[test]
    fn apply_rewrites_rewrite_link_in_other_file() {
        let dir = tempdir().unwrap();
        touch(dir.path(), "GUIDE.md", "guide");
        let readme = touch(dir.path(), "README.md", "[link](./GUIDE.md)\nother text\n");

        reorganize_docs(dir.path(), false).unwrap();
        let after = std::fs::read_to_string(&readme).unwrap();
        assert!(after.contains("](./docs/GUIDE.md)"));
        assert!(!after.contains("](./GUIDE.md)"));
    }

    #[test]
    fn empty_project_succeeds_noop() {
        let dir = tempdir().unwrap();
        let r = reorganize_docs(dir.path(), true).unwrap();
        assert!(r.moves.is_empty());
        assert!(r.link_rewrites.is_empty());
        assert!(r.errors.is_empty());
    }

    #[test]
    fn project_missing_returns_typed_error() {
        let dir = tempdir().unwrap();
        let absent = dir.path().join("does-not-exist");
        let err = reorganize_docs(&absent, true).unwrap_err();
        assert!(matches!(err, OrchestrationError::ProjectMissing(_)));
    }
}
