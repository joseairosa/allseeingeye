//! Project discovery.
//!
//! A project IS the parent directory of any indexed memory file. We
//! query the component table for `type='memory' AND scope='project'`,
//! group by parent path, and surface one [`ProjectSummary`] per
//! distinct parent.
//!
//! No filesystem walking - the index already has every memory file
//! the user cares about, courtesy of Phase 14A. The only IO this
//! module does is a `path.exists()` check for `.git/` to populate
//! the `has_git` flag.

use std::path::{Path, PathBuf};

use crate::index::{IndexError, IndexHandle};
use crate::projects::types::{MemoryFileSummary, ProjectSummary, ProjectsError};

/// Threshold matching the inventory size chip / Health view bloat
/// pane. Spec docs/17 §17.2: `is_oversized = size > 8 KiB`.
const OVERSIZED_BYTES: u64 = 8 * 1024;

/// Token estimate divisor matching `lib/tokens.ts::estimateTokens`.
/// 4 chars per token is the documented heuristic; the UI shows the
/// caveat tooltip.
const CHARS_PER_TOKEN: u64 = 4;

/// Filename preference for the "primary" memory file when a project
/// has more than one. CLAUDE.md wins because it's the most common,
/// AGENTS.md second, GEMINI.md third.
fn preference(basename: &str) -> u8 {
    match basename {
        "CLAUDE.md" | "CLAUDE.local.md" => 0,
        "AGENTS.md" => 1,
        "GEMINI.md" => 2,
        _ => 255,
    }
}

/// List every project surfaced by the index, sorted by display name
/// ascending. Read-only; the only filesystem call is the `.git/` probe
/// per project.
pub fn list_projects(handle: &IndexHandle) -> Result<Vec<ProjectSummary>, ProjectsError> {
    let memory_rows = collect_memory_rows(handle)?;
    let mut by_project: std::collections::BTreeMap<PathBuf, Vec<MemoryFileSummary>> =
        std::collections::BTreeMap::new();

    for (path_str, size, mtime) in memory_rows {
        let path = PathBuf::from(&path_str);
        let parent = match path.parent() {
            Some(p) => p.to_path_buf(),
            None => continue, // path with no parent is malformed; skip
        };
        let basename = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_owned();
        if basename.is_empty() {
            continue;
        }
        by_project
            .entry(parent)
            .or_default()
            .push(MemoryFileSummary {
                basename,
                path: path_str,
                size,
                mtime,
            });
    }

    let mut out: Vec<ProjectSummary> = by_project
        .into_iter()
        .map(|(project_path, mut memory_files)| {
            memory_files.sort_by_key(|m| (preference(&m.basename), m.basename.clone()));
            let primary = memory_files
                .first()
                .expect("at least one entry by construction")
                .clone();
            let display_name = derive_display_name(&project_path);
            let has_git = project_path.join(".git").exists();
            let primary_memory_tokens_est = primary.size / CHARS_PER_TOKEN;
            let is_oversized = primary.size > OVERSIZED_BYTES;
            ProjectSummary {
                project_path: project_path.to_string_lossy().into_owned(),
                display_name,
                memory_files,
                primary_memory_path: primary.path,
                primary_memory_size: primary.size,
                primary_memory_tokens_est,
                is_oversized,
                has_git,
            }
        })
        .collect();

    out.sort_by(|a, b| a.display_name.cmp(&b.display_name));
    Ok(out)
}

/// Pull every project-scoped memory row from the index. Returns
/// `(path_string, size, mtime)` triples; the parent grouping happens
/// in `list_projects` so the SQL stays simple.
fn collect_memory_rows(handle: &IndexHandle) -> Result<Vec<(String, u64, i64)>, IndexError> {
    handle.read(|conn| {
        let mut stmt = conn.prepare(
            "SELECT path, COALESCE(size, 0), COALESCE(mtime, 0)
               FROM component
              WHERE type = 'memory' AND scope = 'project'",
        )?;
        let rows = stmt.query_map([], |row| {
            let path: String = row.get(0)?;
            let size: i64 = row.get(1)?;
            let mtime: i64 = row.get(2)?;
            Ok((path, u64::try_from(size.max(0)).unwrap_or(0), mtime))
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    })
}

/// Pretty display name: last 2 path segments joined with `/`, falling
/// back to the absolute path when the project lives at a top-level
/// directory.
fn derive_display_name(project_path: &Path) -> String {
    let segments: Vec<&str> = project_path
        .components()
        .filter_map(|c| match c {
            std::path::Component::Normal(n) => n.to_str(),
            _ => None,
        })
        .collect();
    if segments.len() >= 2 {
        let n = segments.len();
        format!("{}/{}", segments[n - 2], segments[n - 1])
    } else {
        project_path.to_string_lossy().into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;
    use tempfile::tempdir;

    fn seed_memory(handle: &IndexHandle, id: &str, path: &str, size: i64, mtime: i64) {
        handle
            .write(|conn| {
                conn.execute(
                    "INSERT INTO component (
                         id, type, tool, scope, origin, name, path, format,
                         size, mtime, enabled, use_count, hash, updated_at
                     ) VALUES (?1, 'memory', 'claude-code', 'project', 'tool',
                              ?2, ?3, 'markdown', ?4, ?5, 1, 0, '00', 0)",
                    params![id, id, path, size, mtime],
                )?;
                Ok(())
            })
            .expect("seed");
    }

    #[test]
    fn list_returns_one_summary_per_distinct_parent() {
        let handle = IndexHandle::open_in_memory().expect("open");
        seed_memory(
            &handle,
            "aseye://x/proj-a/memory/CLAUDE",
            "/Users/jose/Dev/proj-a/CLAUDE.md",
            1234,
            100,
        );
        seed_memory(
            &handle,
            "aseye://x/proj-b/memory/CLAUDE",
            "/Users/jose/Dev/proj-b/CLAUDE.md",
            5678,
            200,
        );

        let projects = list_projects(&handle).expect("list");
        assert_eq!(projects.len(), 2);
        assert_eq!(projects[0].display_name, "Dev/proj-a");
        assert_eq!(projects[1].display_name, "Dev/proj-b");
    }

    #[test]
    fn multiple_memory_files_in_one_project_collapse_to_one_summary() {
        let handle = IndexHandle::open_in_memory().expect("open");
        seed_memory(
            &handle,
            "aseye://x/proj/memory/CLAUDE",
            "/work/proj/CLAUDE.md",
            100,
            10,
        );
        seed_memory(
            &handle,
            "aseye://x/proj/memory/AGENTS",
            "/work/proj/AGENTS.md",
            200,
            20,
        );
        seed_memory(
            &handle,
            "aseye://x/proj/memory/GEMINI",
            "/work/proj/GEMINI.md",
            300,
            30,
        );

        let projects = list_projects(&handle).expect("list");
        assert_eq!(projects.len(), 1);
        let p = &projects[0];
        assert_eq!(p.memory_files.len(), 3);
        // Primary preference: CLAUDE.md first.
        assert!(p.primary_memory_path.ends_with("CLAUDE.md"));
        assert_eq!(p.primary_memory_size, 100);
    }

    #[test]
    fn oversized_flag_set_above_threshold() {
        let handle = IndexHandle::open_in_memory().expect("open");
        seed_memory(
            &handle,
            "aseye://x/big/memory/CLAUDE",
            "/work/big/CLAUDE.md",
            10_000,
            0,
        );
        seed_memory(
            &handle,
            "aseye://x/small/memory/CLAUDE",
            "/work/small/CLAUDE.md",
            500,
            0,
        );

        let projects = list_projects(&handle).expect("list");
        let big = projects
            .iter()
            .find(|p| p.display_name == "work/big")
            .unwrap();
        let small = projects
            .iter()
            .find(|p| p.display_name == "work/small")
            .unwrap();
        assert!(big.is_oversized);
        assert!(!small.is_oversized);
    }

    #[test]
    fn token_estimate_uses_4_chars_per_token() {
        let handle = IndexHandle::open_in_memory().expect("open");
        seed_memory(
            &handle,
            "aseye://x/proj/memory/CLAUDE",
            "/work/proj/CLAUDE.md",
            4000,
            0,
        );
        let projects = list_projects(&handle).expect("list");
        assert_eq!(projects[0].primary_memory_tokens_est, 1000);
    }

    #[test]
    fn has_git_reflects_dot_git_existence_on_disk() {
        let dir = tempdir().expect("tempdir");
        let project_path = dir.path().join("with-git");
        std::fs::create_dir_all(project_path.join(".git")).unwrap();
        std::fs::write(project_path.join("CLAUDE.md"), b"x").unwrap();

        let handle = IndexHandle::open_in_memory().expect("open");
        let path_str = project_path
            .join("CLAUDE.md")
            .to_string_lossy()
            .into_owned();
        seed_memory(&handle, "aseye://x/proj/memory/CLAUDE", &path_str, 1, 0);

        let projects = list_projects(&handle).expect("list");
        assert!(projects[0].has_git);

        // Project without .git/
        let other = dir.path().join("no-git");
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(other.join("CLAUDE.md"), b"x").unwrap();
        let other_path = other.join("CLAUDE.md").to_string_lossy().into_owned();
        seed_memory(&handle, "aseye://x/other/memory/CLAUDE", &other_path, 1, 0);

        let projects = list_projects(&handle).expect("list");
        let no_git = projects
            .iter()
            .find(|p| p.display_name.ends_with("no-git"))
            .unwrap();
        assert!(!no_git.has_git);
    }

    #[test]
    fn empty_index_returns_empty_list() {
        let handle = IndexHandle::open_in_memory().expect("open");
        let projects = list_projects(&handle).expect("list");
        assert!(projects.is_empty());
    }
}
