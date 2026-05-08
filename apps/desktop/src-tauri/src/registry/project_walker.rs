//! Project-tree memory walker.
//!
//! Phase 14A - finds every project-level memory file
//! (`CLAUDE.md` / `CLAUDE.local.md` / `AGENTS.md` / `GEMINI.md`) under
//! a configured set of project search roots and routes each hit to the
//! right tool. The user-level memory roots (e.g. `~/.claude/CLAUDE.md`)
//! are still handled by the existing glob walker; this module covers
//! the long-tail of project-tree memories that the user has dotted
//! around `~/Development` (and elsewhere).
//!
//! Walker contract (mirrored from `docs/14-cost-and-memory.md` § 14A):
//! * Max depth: 4 levels from each root. Deeper directories are
//!   ignored.
//! * Symlinks: followed once; cycles are detected via a canonical-path
//!   visited set.
//! * Denylist: `node_modules`, `.git`, `.next`, `dist`, `build`,
//!   `target`, `.venv`, `venv`, `__pycache__`, `.cache`, `.Trash`,
//!   `Library`, `vendor`, `Pods`, `.terraform`, `out`.
//! * Project marker (any of): `.git/`, `package.json`, `Cargo.toml`,
//!   `pyproject.toml`, `Gemfile`, `go.mod`, `pubspec.yaml`,
//!   `composer.json`, `mix.exs`, `Project.toml`.
//! * Hidden directories are skipped except for `.claude/` and
//!   `.cursor/`, which carry tool config we still want to peek into.
//! * Each root traversal has a hard wall-clock budget (`WALK_BUDGET`)
//!   so a pathological tree never wedges the scan thread.
//!
//! The walker is intentionally synchronous and stack-bounded - it uses
//! an iterative DFS rather than recursion so we never blow the stack
//! on deeply nested trees, and it never spawns threads.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::types::ToolId;

/// Hard wall-clock budget per root traversal. Once we exceed this we
/// stop walking that root and return whatever we have. 30s is generous
/// for any realistic `~/Development` tree (the developer's tree with
/// 100+ project dirs walks in well under a second on SSD); the budget
/// only ever fires on a pathological symlink farm or a network mount
/// gone wrong.
const WALK_BUDGET: Duration = Duration::from_secs(30);

/// Max depth from each root (root itself is depth 0, its immediate
/// children are depth 1, ...). Set to 4 so a typical layout like
/// `~/Development/<workspace>/<repo>/<package>/CLAUDE.md` is reachable
/// while still bounding the search.
const MAX_DEPTH: usize = 4;

/// Directory basenames we always skip.
const DENYLIST: &[&str] = &[
    "node_modules",
    ".git",
    ".next",
    "dist",
    "build",
    "target",
    ".venv",
    "venv",
    "__pycache__",
    ".cache",
    ".Trash",
    "Library",
    "vendor",
    "Pods",
    ".terraform",
    "out",
];

/// Hidden directories we are willing to descend into despite the
/// "skip dotfiles" default. These carry tool config that the registry
/// otherwise wires up via user-scope globs.
const HIDDEN_ALLOWLIST: &[&str] = &[".claude", ".cursor"];

/// Files whose presence in a directory marks it as a "project" for the
/// purposes of the walker. A directory must contain at least one of
/// these (or be a `.git/` directory; we check `.git` specially since
/// it is itself denylisted and would never appear as a regular entry).
const PROJECT_MARKERS: &[&str] = &[
    "package.json",
    "Cargo.toml",
    "pyproject.toml",
    "Gemfile",
    "go.mod",
    "pubspec.yaml",
    "composer.json",
    "mix.exs",
    "Project.toml",
];

/// One match emitted by the walker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectMemoryHit {
    /// Tool the matched filename routes to per `MEMORY_FILE_ROUTING`.
    pub tool: ToolId,
    /// Absolute path of the matched file on disk.
    pub path: PathBuf,
    /// Original basename (`CLAUDE.md`, `CLAUDE.local.md`, `AGENTS.md`,
    /// `GEMINI.md`). Preserved verbatim so the upsert layer can stamp
    /// it as the parser flavour without losing the `.local` distinction.
    pub basename: String,
}

/// Per-filename routing table. Adding a new memory filename is a code
/// change here, mirroring how `registry/tools.rs` is the source of
/// truth for the rest of the descriptor surface.
const MEMORY_FILE_ROUTING: &[(&str, ToolId)] = &[
    ("CLAUDE.md", ToolId::ClaudeCode),
    ("CLAUDE.local.md", ToolId::ClaudeCode),
    ("AGENTS.md", ToolId::Codex),
    ("GEMINI.md", ToolId::Antigravity),
];

/// Walk every root and return every project-memory file we find.
///
/// `roots` is the configured `projectMemoryRoots` list. `home` is the
/// optional HOME override used to expand `~/`-prefixed entries; tests
/// pass a tempdir, production passes the resolved system HOME.
///
/// The function never panics. I/O errors on individual entries are
/// swallowed (logged via `tracing` at debug level) so a single
/// permission-denied directory does not abort a scan.
#[must_use]
pub fn walk_project_memory(roots: &[PathBuf], home: Option<&Path>) -> Vec<ProjectMemoryHit> {
    let mut out = Vec::new();
    let mut seen_canon: HashSet<PathBuf> = HashSet::new();

    for raw in roots {
        let resolved = expand_root(raw, home);
        if !resolved.exists() {
            continue;
        }
        // Canonicalise the root once so symlink-loop detection has a
        // stable key for "we have already walked this real directory".
        let Ok(canon_root) = std::fs::canonicalize(&resolved) else {
            continue;
        };
        if !seen_canon.insert(canon_root.clone()) {
            // Another root already covered this real directory.
            continue;
        }
        walk_one_root(&canon_root, &mut seen_canon, &mut out);
    }

    out
}

/// Expand a configured root entry to an absolute path.
///
/// Accepts:
/// * `~/foo` and `~` (HOME expansion via `home` override or
///   `dirs::home_dir`).
/// * Absolute paths verbatim.
/// * Relative paths verbatim (resolved against `cwd` by the OS at I/O
///   time; production callers always configure absolute or `~/`
///   entries so this is a defensive fallback).
fn expand_root(raw: &Path, home: Option<&Path>) -> PathBuf {
    let Some(s) = raw.to_str() else {
        return raw.to_path_buf();
    };
    if let Some(rest) = s.strip_prefix("~/") {
        if let Some(h) = home {
            return h.join(rest);
        }
        if let Some(h) = dirs::home_dir() {
            return h.join(rest);
        }
    }
    if s == "~" {
        if let Some(h) = home {
            return h.to_path_buf();
        }
        if let Some(h) = dirs::home_dir() {
            return h;
        }
    }
    raw.to_path_buf()
}

/// Iterative DFS walk of a single (already canonicalised) root.
///
/// `visited` is shared across roots so a symlink that escapes one root
/// into another already-walked root never produces duplicate hits.
fn walk_one_root(root: &Path, visited: &mut HashSet<PathBuf>, out: &mut Vec<ProjectMemoryHit>) {
    let started = Instant::now();
    // Stack of (path, depth). Depth 0 is the root itself.
    let mut stack: Vec<(PathBuf, usize)> = vec![(root.to_path_buf(), 0)];

    while let Some((dir, depth)) = stack.pop() {
        if started.elapsed() >= WALK_BUDGET {
            // `as_millis()` is `u128`; clamp to `u64` for the log
            // field (a 30s budget never overflows but clippy still
            // wants us to be explicit about the cast).
            let budget_ms = u64::try_from(WALK_BUDGET.as_millis()).unwrap_or(u64::MAX);
            tracing::warn!(
                root = ?root,
                budget_ms,
                "project memory walker hit per-root budget; aborting traversal",
            );
            return;
        }

        // Honour the project-marker requirement at every depth from 1
        // onwards (the root itself is allowed to not be a project; we
        // are looking for projects underneath it). We still SCAN every
        // directory on the way down because intermediate dirs (like
        // `~/Development/`) are not projects but contain projects.
        if depth >= 1 && is_project_dir(&dir) {
            // Emit memory files that live at the project root itself.
            collect_memory_in_dir(&dir, out);
        }

        if depth >= MAX_DEPTH {
            continue;
        }

        let Ok(entries) = std::fs::read_dir(&dir) else {
            tracing::debug!(?dir, "read_dir failed; skipping subtree");
            continue;
        };

        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };

            // Resolve symlinks once. Cycles are caught via the visited
            // set keyed on the canonical target.
            let target = if file_type.is_symlink() {
                match std::fs::canonicalize(&path) {
                    Ok(t) => t,
                    Err(err) => {
                        tracing::debug!(?path, ?err, "symlink canonicalise failed; skipping");
                        continue;
                    }
                }
            } else {
                path.clone()
            };

            // We only descend into directories. Memory files at the
            // project root were emitted above by `collect_memory_in_dir`.
            // Files anywhere except the project root are ignored on the
            // way down; the next `is_project_dir` pass covers them.
            let target_is_dir = if file_type.is_symlink() {
                std::fs::metadata(&target).is_ok_and(|m| m.is_dir())
            } else {
                file_type.is_dir()
            };
            if !target_is_dir {
                continue;
            }

            let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
                continue;
            };

            if DENYLIST.contains(&name) {
                continue;
            }

            // Hidden-dir handling: skip dotfiles unless they are on
            // the explicit allowlist. We also always allow `.git/` to
            // be detected as a project marker (handled in
            // `is_project_dir`); we do not descend into it here.
            if name.starts_with('.') && !HIDDEN_ALLOWLIST.contains(&name) {
                continue;
            }

            // Cycle detection: canonicalise the target and bail if
            // already visited. We canonicalise non-symlinks too so two
            // distinct paths to the same real directory (e.g. via a
            // bind mount) get deduped.
            let canon = if file_type.is_symlink() {
                target.clone()
            } else {
                std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone())
            };
            if !visited.insert(canon.clone()) {
                continue;
            }

            stack.push((path, depth.saturating_add(1)));
        }
    }
}

/// Return true when `dir` looks like a software project root.
///
/// Either contains a `.git/` directory (the most common case) or any
/// of the language-specific manifest files. Files-only check; we don't
/// recurse - the caller has already descended into `dir`.
fn is_project_dir(dir: &Path) -> bool {
    if dir.join(".git").is_dir() {
        return true;
    }
    PROJECT_MARKERS.iter().any(|m| dir.join(m).is_file())
}

/// Look at every entry in `dir` and emit a `ProjectMemoryHit` for each
/// filename that matches the routing table. Symlinked memory files
/// follow once; the canonical target is what we record.
fn collect_memory_in_dir(dir: &Path, out: &mut Vec<ProjectMemoryHit>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        // Only consider regular files (or symlinks pointing at one).
        let is_file = if file_type.is_symlink() {
            std::fs::metadata(&path).is_ok_and(|m| m.is_file())
        } else {
            file_type.is_file()
        };
        if !is_file {
            continue;
        }
        let Some(basename) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        let Some((_, tool)) = MEMORY_FILE_ROUTING
            .iter()
            .find(|(name, _)| *name == basename)
        else {
            continue;
        };
        // Use the canonical path so two routes to the same file are
        // deduped on the upsert side via path-stable URI hashing. We
        // fall back to the literal path on canonicalise failure rather
        // than dropping the hit.
        let resolved = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
        out.push(ProjectMemoryHit {
            tool: *tool,
            path: resolved,
            basename: basename.to_owned(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs as unix_fs;
    use tempfile::tempdir;

    /// Lay out a fake project with a `.git/` marker at `dir` plus the
    /// supplied memory files.
    fn make_project(dir: &Path, files: &[&str]) {
        fs::create_dir_all(dir.join(".git")).expect("mkdir .git");
        for f in files {
            fs::write(dir.join(f), b"# memory\n").expect("write memory");
        }
    }

    #[test]
    fn finds_claude_md_under_project() {
        let tmp = tempdir().unwrap();
        let proj = tmp.path().join("workspace").join("alpha");
        make_project(&proj, &["CLAUDE.md"]);

        let hits = walk_project_memory(&[tmp.path().to_path_buf()], None);
        assert_eq!(hits.len(), 1, "got {hits:?}");
        assert_eq!(hits[0].tool, ToolId::ClaudeCode);
        assert_eq!(hits[0].basename, "CLAUDE.md");
        assert!(hits[0].path.ends_with("alpha/CLAUDE.md"));
    }

    #[test]
    fn routes_each_filename_to_the_right_tool() {
        let tmp = tempdir().unwrap();
        let proj = tmp.path().join("multi");
        make_project(
            &proj,
            &["CLAUDE.md", "CLAUDE.local.md", "AGENTS.md", "GEMINI.md"],
        );

        let mut hits = walk_project_memory(&[tmp.path().to_path_buf()], None);
        hits.sort_by(|a, b| a.basename.cmp(&b.basename));
        assert_eq!(hits.len(), 4);
        assert_eq!(hits[0].basename, "AGENTS.md");
        assert_eq!(hits[0].tool, ToolId::Codex);
        assert_eq!(hits[1].basename, "CLAUDE.local.md");
        assert_eq!(hits[1].tool, ToolId::ClaudeCode);
        assert_eq!(hits[2].basename, "CLAUDE.md");
        assert_eq!(hits[2].tool, ToolId::ClaudeCode);
        assert_eq!(hits[3].basename, "GEMINI.md");
        assert_eq!(hits[3].tool, ToolId::Antigravity);
    }

    #[test]
    fn skips_directory_without_project_marker() {
        let tmp = tempdir().unwrap();
        // A CLAUDE.md sitting in a plain directory (no `.git/`, no
        // language manifest) must NOT be picked up.
        let plain = tmp.path().join("notes");
        fs::create_dir_all(&plain).unwrap();
        fs::write(plain.join("CLAUDE.md"), b"loose memory\n").unwrap();

        let hits = walk_project_memory(&[tmp.path().to_path_buf()], None);
        assert!(
            hits.is_empty(),
            "expected zero hits for a non-project dir, got {hits:?}"
        );
    }

    #[test]
    fn denylist_skips_node_modules_and_friends() {
        let tmp = tempdir().unwrap();
        let proj = tmp.path().join("alpha");
        make_project(&proj, &["CLAUDE.md"]);
        // A nested project inside node_modules MUST be ignored.
        let nm_proj = proj.join("node_modules").join("foo");
        make_project(&nm_proj, &["CLAUDE.md"]);
        // Same for target/, .next/, etc.
        for hidden in ["target", ".next", "dist", "build", "vendor"] {
            let p = proj.join(hidden).join("inner");
            make_project(&p, &["CLAUDE.md"]);
        }

        let hits = walk_project_memory(&[tmp.path().to_path_buf()], None);
        assert_eq!(
            hits.len(),
            1,
            "denylisted dirs must not yield hits; got {hits:?}"
        );
        assert!(hits[0].path.ends_with("alpha/CLAUDE.md"));
    }

    #[test]
    fn depth_limit_excludes_deeper_projects() {
        let tmp = tempdir().unwrap();
        // Root is depth 0; depth 1, 2, 3, 4 are walkable; depth 5 is
        // beyond the limit. We build a depth-5 project to confirm it
        // is not reached.
        let mut path = tmp.path().to_path_buf();
        for level in 1..=5 {
            path = path.join(format!("level{level}"));
        }
        make_project(&path, &["CLAUDE.md"]);

        let hits = walk_project_memory(&[tmp.path().to_path_buf()], None);
        assert!(
            hits.is_empty(),
            "depth-5 project must not be reached, got {hits:?}"
        );

        // Sanity: a depth-4 project IS reached.
        let path4 = tmp.path().join("a").join("b").join("c").join("d");
        make_project(&path4, &["AGENTS.md"]);
        let hits = walk_project_memory(&[tmp.path().to_path_buf()], None);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].basename, "AGENTS.md");
    }

    #[test]
    fn hidden_dirs_skipped_except_allowlist() {
        let tmp = tempdir().unwrap();
        // .secret/ is hidden and not on the allowlist.
        let hidden = tmp.path().join(".secret").join("proj");
        make_project(&hidden, &["CLAUDE.md"]);
        // .claude/ is on the allowlist - a project under it is
        // reachable.
        let allowed = tmp.path().join(".claude").join("proj");
        make_project(&allowed, &["AGENTS.md"]);

        let hits = walk_project_memory(&[tmp.path().to_path_buf()], None);
        // Only the .claude/proj hit shows up.
        assert_eq!(hits.len(), 1, "got {hits:?}");
        assert_eq!(hits[0].basename, "AGENTS.md");
    }

    #[test]
    fn project_marker_via_language_manifest() {
        let tmp = tempdir().unwrap();
        // No `.git/`, but a `Cargo.toml` makes this a project.
        let proj = tmp.path().join("rusty");
        fs::create_dir_all(&proj).unwrap();
        fs::write(proj.join("Cargo.toml"), b"[package]\nname=\"x\"\n").unwrap();
        fs::write(proj.join("CLAUDE.md"), b"# m\n").unwrap();

        let hits = walk_project_memory(&[tmp.path().to_path_buf()], None);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].basename, "CLAUDE.md");
    }

    #[cfg(unix)]
    #[test]
    fn symlink_cycle_does_not_hang() {
        let tmp = tempdir().unwrap();
        let a = tmp.path().join("a");
        let b = tmp.path().join("b");
        fs::create_dir_all(&a).unwrap();
        fs::create_dir_all(&b).unwrap();
        // Create a project at `a` so the walker has a reason to visit
        // it.
        make_project(&a, &["CLAUDE.md"]);
        // a/loop -> b, b/loop -> a. Following the loop without cycle
        // detection would walk forever.
        unix_fs::symlink(&b, a.join("loop")).unwrap();
        unix_fs::symlink(&a, b.join("loop")).unwrap();

        let hits = walk_project_memory(&[tmp.path().to_path_buf()], None);
        assert_eq!(
            hits.len(),
            1,
            "cycle-protected walker must terminate with the single real hit, got {hits:?}"
        );
    }

    #[test]
    fn nonexistent_root_is_silently_skipped() {
        let tmp = tempdir().unwrap();
        let phantom = tmp.path().join("does-not-exist");
        let hits = walk_project_memory(&[phantom], None);
        assert!(hits.is_empty());
    }

    #[test]
    fn duplicate_roots_do_not_double_count() {
        let tmp = tempdir().unwrap();
        let proj = tmp.path().join("alpha");
        make_project(&proj, &["CLAUDE.md"]);

        let root = tmp.path().to_path_buf();
        let hits = walk_project_memory(&[root.clone(), root], None);
        assert_eq!(hits.len(), 1, "got {hits:?}");
    }

    #[test]
    fn home_expansion_works() {
        let tmp = tempdir().unwrap();
        let proj = tmp.path().join("alpha");
        make_project(&proj, &["CLAUDE.md"]);

        let hits = walk_project_memory(&[PathBuf::from("~")], Some(tmp.path()));
        assert_eq!(hits.len(), 1, "got {hits:?}");
    }
}
