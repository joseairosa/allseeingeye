//! Phase 14A end-to-end proof: a real-home full scan must surface
//! at least 5 project-level memory rows under `~/Development`.
//!
//! Like `real_home_scan_proof.rs`, this test is gated on the host
//! actually having a `~/Development` tree. CI hosts and contributors
//! who don't keep their projects there see a clear skip rather than a
//! failure.

use std::path::Path;
use std::sync::Arc;

use aseye_desktop_lib::{IndexHandle, Pipeline};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn full_scan_finds_real_project_memory_files() {
    let Some(home) = dirs::home_dir() else {
        eprintln!("skip: no HOME on this host");
        return;
    };

    let dev = home.join("Development");
    if !dev.exists() {
        eprintln!(
            "skip: {} does not exist on this host (CI / no project tree)",
            dev.display()
        );
        return;
    }

    // Count CLAUDE.md / AGENTS.md / GEMINI.md files under
    // `~/Development` so the assertion floor is grounded in disk
    // reality. The `count_memory_files_in_projects` helper applies
    // the same denylist as the walker, so the count is comparable.
    let on_disk = count_memory_files_in_projects(&dev);
    eprintln!(
        "project memory files on disk under {}: {on_disk}",
        dev.display()
    );
    if on_disk < 5 {
        eprintln!(
            "skip: fewer than 5 project memory files on disk (found {on_disk}); \
             nothing to assert against",
        );
        return;
    }

    // Open an in-memory index and run the production scan path with
    // `home: None` (production call shape).
    let index = Arc::new(IndexHandle::open_in_memory().expect("open in-memory db"));
    let pipeline = Pipeline::start_with_home(Arc::clone(&index), None).expect("start pipeline");

    let report = pipeline.full_scan().expect("scan should succeed");
    eprintln!(
        "real-home project-memory scan: components_seen={} inserted={} updated={} unchanged={}",
        report.components_seen,
        report.components_inserted,
        report.components_updated,
        report.components_unchanged,
    );

    // Spot-check the index contents: at least 5 project memory rows
    // got written. We query `WHERE type='memory' AND scope='project'`
    // to scope down to exactly the rows the walker produced (the
    // user-level memory rows are scope='user').
    let count: i64 = index
        .read(|c| {
            c.query_row(
                "SELECT COUNT(*) FROM component WHERE type='memory' AND scope='project'",
                [],
                |row| row.get(0),
            )
            .map_err(Into::into)
        })
        .expect("count rows");

    eprintln!("indexed memory rows scope=project: {count}");
    assert!(
        count >= 5,
        "expected >= 5 project memory rows, got {count} (disk had {on_disk})"
    );
}

/// Count `CLAUDE.md` / `CLAUDE.local.md` / `AGENTS.md` / `GEMINI.md`
/// files under any "project" directory inside `dev`. A project is a
/// directory that contains a `.git/` or one of the common
/// language-specific manifests. Mirrors the walker's contract closely
/// enough for the test floor; we deliberately keep the check shallow
/// (max-depth 4 from `dev`) to avoid scanning the whole filesystem.
fn count_memory_files_in_projects(dev: &Path) -> u32 {
    const MEMORY_FILES: &[&str] = &["CLAUDE.md", "CLAUDE.local.md", "AGENTS.md", "GEMINI.md"];
    const MARKERS: &[&str] = &[
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
    const HIDDEN_ALLOWLIST: &[&str] = &[".claude", ".cursor"];
    const MAX_DEPTH: usize = 4;

    fn is_project(dir: &Path, markers: &[&str]) -> bool {
        if dir.join(".git").is_dir() {
            return true;
        }
        markers.iter().any(|m| dir.join(m).is_file())
    }

    let mut found: u32 = 0;
    let mut stack: Vec<(std::path::PathBuf, usize)> = vec![(dev.to_path_buf(), 0)];
    while let Some((dir, depth)) = stack.pop() {
        if depth >= 1 && is_project(&dir, MARKERS) {
            if let Ok(entries) = std::fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    let p = entry.path();
                    let Some(name) = p.file_name().and_then(|s| s.to_str()) else {
                        continue;
                    };
                    if MEMORY_FILES.contains(&name) && p.is_file() {
                        found = found.saturating_add(1);
                    }
                }
            }
        }
        if depth >= MAX_DEPTH {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let p = entry.path();
            let Ok(ft) = entry.file_type() else { continue };
            if !ft.is_dir() && !ft.is_symlink() {
                continue;
            }
            let Some(name) = p.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            if DENYLIST.contains(&name) {
                continue;
            }
            if name.starts_with('.') && !HIDDEN_ALLOWLIST.contains(&name) {
                continue;
            }
            // Treat symlinks as dirs only when the target is a dir.
            let descend = if ft.is_symlink() {
                std::fs::metadata(&p).is_ok_and(|m| m.is_dir())
            } else {
                true
            };
            if descend {
                stack.push((p, depth.saturating_add(1)));
            }
        }
    }
    found
}
