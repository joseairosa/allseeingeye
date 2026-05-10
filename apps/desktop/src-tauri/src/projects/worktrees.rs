//! Action 2: git worktree audit (read-only).
//!
//! Runs `git -C <project> worktree list --porcelain` and parses the
//! output into structured rows. Adds bounded `du` per worktree so
//! the UI can show disk usage. Never invokes `git worktree remove` -
//! v1 ships read-only; the user runs the removal themselves.
//!
//! `git worktree list --porcelain` output shape (one entry per
//! worktree, blank-line separated):
//!
//! ```text
//! worktree /Users/jose/Dev/proj
//! HEAD abc123...
//! branch refs/heads/main
//!
//! worktree /Users/jose/Dev/proj/.worktrees/feature
//! HEAD def456...
//! branch refs/heads/feature
//! locked reason
//! ```

#![allow(clippy::cast_precision_loss)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Hard cap on disk-usage walks per worktree. Stops at 100k entries
/// or 60s wall-clock so a runaway `node_modules` does not freeze
/// the report. Surfaced in the entry's `incomplete` flag.
const DU_MAX_ENTRIES: usize = 100_000;
const DU_TIMEOUT_SECS: u64 = 60;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../bindings/projects/WorktreeReport.ts")]
#[ts(rename_all = "camelCase")]
pub struct WorktreeReport {
    pub project_path: String,
    pub worktrees: Vec<WorktreeEntry>,
    pub total_disk_usage_bytes: u64,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../bindings/projects/WorktreeEntry.ts")]
#[ts(rename_all = "camelCase")]
pub struct WorktreeEntry {
    /// Absolute path to the worktree.
    pub path: String,
    /// Branch checked out, when the worktree is on a branch (vs.
    /// detached HEAD).
    pub branch: Option<String>,
    /// Commit SHA the worktree's HEAD points at.
    pub head: String,
    /// True iff `git worktree list --porcelain` reported `locked`.
    pub locked: bool,
    /// Lock reason string when `locked = true`.
    pub lock_reason: Option<String>,
    /// Mtime of the worktree directory (the most recent file
    /// modification we could observe). Drives the "age" hint in the
    /// UI. Unix seconds; 0 when unavailable.
    pub mtime_unix: i64,
    /// Recursive disk usage in bytes. May be 0 when the walk failed
    /// outright (which the UI surfaces as `(unknown size)`).
    pub disk_usage_bytes: u64,
    /// True iff the disk-usage walk hit the entry / time cap before
    /// finishing. The UI shows a "incomplete" hint so the user knows
    /// the number is a lower bound.
    pub incomplete: bool,
    /// True iff this is the main worktree (the project root itself).
    /// `git worktree list` always emits the main first.
    pub is_main: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum WorktreeError {
    #[error("not a git repository: {0}")]
    NotAGitRepo(String),

    #[error("git command failed (exit {code}): {stderr}")]
    GitFailed { code: i32, stderr: String },

    #[error("could not invoke git: {0}")]
    InvokeFailed(std::io::Error),

    #[error("could not parse `git worktree list --porcelain` output")]
    ParseFailed,
}

/// Run the worktree audit for `project_path`. Returns
/// [`WorktreeError::NotAGitRepo`] when the project root has no
/// `.git/` (callers should check `ProjectSummary.has_git` first; the
/// IPC double-checks defensively).
pub fn audit_worktrees(project_path: &Path) -> Result<WorktreeReport, WorktreeError> {
    let started = std::time::Instant::now();

    if !project_path.join(".git").exists() {
        return Err(WorktreeError::NotAGitRepo(
            project_path.to_string_lossy().into_owned(),
        ));
    }

    let output = Command::new("git")
        .arg("-C")
        .arg(project_path)
        .arg("worktree")
        .arg("list")
        .arg("--porcelain")
        .output()
        .map_err(WorktreeError::InvokeFailed)?;

    if !output.status.success() {
        return Err(WorktreeError::GitFailed {
            code: output.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut entries = parse_porcelain(&stdout).ok_or(WorktreeError::ParseFailed)?;

    // Decorate each entry with mtime + disk usage. Both can fail
    // independently; failures default to 0 with `incomplete: true`.
    for (idx, entry) in entries.iter_mut().enumerate() {
        entry.is_main = idx == 0;
        entry.mtime_unix = read_mtime_unix(Path::new(&entry.path));
        let (size, incomplete) = bounded_du(Path::new(&entry.path));
        entry.disk_usage_bytes = size;
        entry.incomplete = incomplete;
    }

    let total_disk_usage_bytes: u64 = entries.iter().map(|e| e.disk_usage_bytes).sum();

    let elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    Ok(WorktreeReport {
        project_path: project_path.to_string_lossy().into_owned(),
        worktrees: entries,
        total_disk_usage_bytes,
        elapsed_ms,
    })
}

/// Parse the porcelain output. Returns `None` when an entry is
/// missing the required `worktree` / `HEAD` lines so the IPC layer
/// can surface a typed `ParseFailed` rather than guessing.
fn parse_porcelain(stdout: &str) -> Option<Vec<WorktreeEntry>> {
    let mut entries: Vec<WorktreeEntry> = Vec::new();
    let mut current: Option<PartialEntry> = None;

    for line in stdout.lines() {
        if line.is_empty() {
            if let Some(p) = current.take() {
                entries.push(p.finalise()?);
            }
            continue;
        }
        let entry = current.get_or_insert_with(PartialEntry::default);
        if let Some(rest) = line.strip_prefix("worktree ") {
            entry.path = Some(rest.to_owned());
        } else if let Some(rest) = line.strip_prefix("HEAD ") {
            entry.head = Some(rest.to_owned());
        } else if let Some(rest) = line.strip_prefix("branch ") {
            // `branch refs/heads/main` -> `main`
            let trimmed = rest.strip_prefix("refs/heads/").unwrap_or(rest);
            entry.branch = Some(trimmed.to_owned());
        } else if line == "detached" {
            entry.detached = true;
        } else if let Some(rest) = line.strip_prefix("locked") {
            entry.locked = true;
            // Lock reason follows after a single space when present.
            let reason = rest.trim();
            if !reason.is_empty() {
                entry.lock_reason = Some(reason.to_owned());
            }
        }
    }
    if let Some(p) = current.take() {
        entries.push(p.finalise()?);
    }

    Some(entries)
}

#[derive(Default)]
struct PartialEntry {
    path: Option<String>,
    head: Option<String>,
    branch: Option<String>,
    detached: bool,
    locked: bool,
    lock_reason: Option<String>,
}

impl PartialEntry {
    fn finalise(self) -> Option<WorktreeEntry> {
        Some(WorktreeEntry {
            path: self.path?,
            head: self.head?,
            branch: if self.detached { None } else { self.branch },
            locked: self.locked,
            lock_reason: self.lock_reason,
            mtime_unix: 0,
            disk_usage_bytes: 0,
            incomplete: false,
            is_main: false,
        })
    }
}

/// Recursive `du` with a hard entry cap and a wall-clock cap.
/// Returns `(bytes, incomplete)`. Symlinks are NOT followed - we
/// only count the link's metadata size, not the target.
fn bounded_du(root: &Path) -> (u64, bool) {
    let started = std::time::Instant::now();
    let timeout = std::time::Duration::from_secs(DU_TIMEOUT_SECS);
    let mut total: u64 = 0;
    let mut count: usize = 0;
    let mut incomplete = false;
    let mut stack: Vec<PathBuf> = vec![root.to_path_buf()];

    while let Some(dir) = stack.pop() {
        if count >= DU_MAX_ENTRIES || started.elapsed() >= timeout {
            incomplete = true;
            break;
        }
        let Ok(read) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in read.flatten() {
            count = count.saturating_add(1);
            if count >= DU_MAX_ENTRIES || started.elapsed() >= timeout {
                incomplete = true;
                break;
            }
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            // Symlinks: count the link itself, don't follow.
            if meta.is_symlink() {
                total = total.saturating_add(meta.len());
                continue;
            }
            if meta.is_file() {
                total = total.saturating_add(meta.len());
            } else if meta.is_dir() {
                stack.push(entry.path());
            }
        }
        if incomplete {
            break;
        }
    }

    (total, incomplete)
}

fn read_mtime_unix(path: &Path) -> i64 {
    let Ok(meta) = std::fs::metadata(path) else {
        return 0;
    };
    let Ok(mtime) = meta.modified() else {
        return 0;
    };
    let Ok(dur) = mtime.duration_since(UNIX_EPOCH) else {
        return 0;
    };
    i64::try_from(dur.as_secs()).unwrap_or(0)
}

#[allow(dead_code)]
fn now_unix() -> i64 {
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

    #[test]
    fn parses_single_worktree_porcelain() {
        let stdout = "\
worktree /Users/jose/Dev/proj
HEAD abcdef0123456789
branch refs/heads/main

";
        let entries = parse_porcelain(stdout).expect("parse");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].path, "/Users/jose/Dev/proj");
        assert_eq!(entries[0].head, "abcdef0123456789");
        assert_eq!(entries[0].branch.as_deref(), Some("main"));
        assert!(!entries[0].locked);
    }

    #[test]
    fn parses_multiple_worktrees_with_locked_and_detached() {
        let stdout = "\
worktree /Users/jose/Dev/proj
HEAD abcdef
branch refs/heads/main

worktree /Users/jose/Dev/proj/.worktrees/feature
HEAD 123456
detached
locked WIP rebase

";
        let entries = parse_porcelain(stdout).expect("parse");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[1].branch, None);
        assert!(entries[1].locked);
        assert_eq!(entries[1].lock_reason.as_deref(), Some("WIP rebase"));
    }

    #[test]
    fn parses_locked_without_reason() {
        let stdout = "\
worktree /Users/jose/Dev/proj/.worktrees/x
HEAD aaa
branch refs/heads/x
locked

";
        let entries = parse_porcelain(stdout).expect("parse");
        assert!(entries[0].locked);
        assert_eq!(entries[0].lock_reason, None);
    }

    #[test]
    fn missing_required_fields_returns_none() {
        // Missing `HEAD` line -> finalise() returns None -> parse_porcelain
        // returns None overall.
        let stdout = "worktree /broken\nbranch refs/heads/main\n\n";
        assert!(parse_porcelain(stdout).is_none());
    }

    #[test]
    fn audit_returns_not_a_git_repo_when_dot_git_absent() {
        let dir = tempdir().unwrap();
        // tempdir has no .git/ directory.
        let err = audit_worktrees(dir.path()).unwrap_err();
        assert!(matches!(err, WorktreeError::NotAGitRepo(_)));
    }

    #[test]
    fn audit_against_a_real_git_repo_succeeds() {
        // Smoke test: this crate's own repo is a git repo, so we can
        // run the audit against `..` which contains the .git/ dir.
        // Skip if `git` isn't on PATH (rare on dev machines but
        // possible in stripped CI).
        if which::which("git").is_err() {
            return;
        }
        let here = std::env::current_dir().unwrap();
        // Walk up to find a parent with .git/.
        let mut probe = here.clone();
        let mut found: Option<PathBuf> = None;
        for _ in 0..6 {
            if probe.join(".git").exists() {
                found = Some(probe.clone());
                break;
            }
            if !probe.pop() {
                break;
            }
        }
        let Some(repo_root) = found else {
            return; // Test running outside a repo - skip silently.
        };
        let report = audit_worktrees(&repo_root).expect("audit succeeds");
        assert!(!report.worktrees.is_empty());
        assert!(report.worktrees[0].is_main);
        // The main worktree's path should match repo_root.
        assert_eq!(
            std::fs::canonicalize(&report.worktrees[0].path).unwrap_or_default(),
            std::fs::canonicalize(&repo_root).unwrap_or_default(),
        );
    }
}
