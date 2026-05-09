//! Full-scan walker.
//!
//! Walks every detected tool's `component_roots` patterns and upserts
//! everything found. The walker is intentionally simple: it reads each
//! pattern segment-by-segment, handling `*` and `**` glob wildcards,
//! and stat-checks each candidate. We could pull in `walkdir` and
//! `globset` to compose this, but the patterns we ship are short
//! (typically ≤ 6 segments) and the data we walk is tiny (skill +
//! agent + rule files), so the bespoke walker is faster than a generic
//! recursive walker filtered by glob.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use globset::Glob;
use tokio::sync::broadcast;

use super::error::PipelineError;
use super::event::{PipelineEvent, ScanReport};
use crate::index::settings::read_excluded_tool_ids;
use crate::index::{read_project_memory_roots, upsert_component, IndexHandle, UpsertKind};
use crate::registry::project_walker::{walk_project_memory, ProjectMemoryHit};
use crate::registry::types::{ComponentRoot, ComponentType, Format, Scope, ToolId};
use crate::registry::{detect, registry as registry_slice};

/// Scan implementation shared by `Pipeline::full_scan` and tests that
/// drive a scan without standing up a watcher.
///
/// The function currently never returns `Err` - `SQLite` write failures
/// are logged and swallowed so a single broken descriptor doesn't
/// abort the whole scan. We keep the `Result` wrapper because the
/// IPC contract surfaces this signature and we'll need it once the
/// pipeline learns to propagate watcher-init failures (Phase 1.7).
#[allow(clippy::unnecessary_wraps)]
pub fn full_scan_inner(
    index: &Arc<IndexHandle>,
    detected_tool_ids: &[ToolId],
    home: Option<&Path>,
    events_tx: &broadcast::Sender<PipelineEvent>,
) -> Result<ScanReport, PipelineError> {
    let descriptors = registry_slice();

    // Audit issue #2 - the user can mark a detected tool as "skipped"
    // from Settings -> Tools. Filter the iteration set before any
    // walking happens so excluded tools never produce upserts. Stored
    // ids are kebab-case (`ToolId` serde rename); we drop unknown
    // strings silently because a user editing the row by hand is
    // expected to be exploring, not breaking the scan.
    let excluded_strings = read_excluded_tool_ids(index);
    let excluded_ids: Vec<ToolId> = excluded_strings
        .iter()
        .filter_map(|s| crate::index::upsert::parse_tool_id(s))
        .collect();
    let active_tool_ids: Vec<ToolId> = detected_tool_ids
        .iter()
        .copied()
        .filter(|id| !excluded_ids.contains(id))
        .collect();

    let mut report = ScanReport {
        tools_scanned: u32::try_from(active_tool_ids.len()).unwrap_or(u32::MAX),
        ..ScanReport::default()
    };

    for tool_id in &active_tool_ids {
        let Some(descriptor) = descriptors.iter().find(|d| d.id == *tool_id) else {
            continue;
        };

        for root in &descriptor.component_roots {
            let pattern = detect::expand_home(&root.path_pattern, home);
            for matched in expand_glob(&pattern) {
                report.components_seen = report.components_seen.saturating_add(1);

                let name = component_name_for(&matched, root);
                match upsert_component(index, descriptor, root, &matched, &name) {
                    Ok(outcome) => {
                        match outcome.kind {
                            UpsertKind::Inserted => {
                                report.components_inserted =
                                    report.components_inserted.saturating_add(1);
                            }
                            UpsertKind::Updated => {
                                report.components_updated =
                                    report.components_updated.saturating_add(1);
                            }
                            UpsertKind::Unchanged => {
                                report.components_unchanged =
                                    report.components_unchanged.saturating_add(1);
                            }
                        }
                        if outcome.had_parse_error {
                            report.parse_errors = report.parse_errors.saturating_add(1);
                            let _ = events_tx.send(PipelineEvent::ParseError {
                                id: outcome.id,
                                path: matched.to_string_lossy().into_owned(),
                            });
                        } else {
                            let _ = events_tx.send(PipelineEvent::ComponentUpserted {
                                id: outcome.id,
                                kind: outcome.kind,
                            });
                        }
                    }
                    Err(err) => {
                        tracing::warn!(path = ?matched, ?err, "scan upsert failed");
                    }
                }
            }
        }
    }

    // Phase 14A: project-tree memory walker. Runs after the per-tool
    // glob walkers so user-level memory rows are written first; a
    // duplicate path on disk (e.g. a symlink from `~/Development/foo`
    // to `~/.claude`) would be a no-op upsert on the second pass via
    // the hash-equal short-circuit. We always call the walker - even
    // when no project-memory tools are detected - because Codex
    // detection is HOME-dir based and a project-only Codex install
    // (no `~/.codex/`) is a real configuration the user can have.
    //
    // Audit issue #2 - the walker also honours the excludedToolIds
    // setting: a hit attributed to a tool in the exclusion list is
    // dropped before upsert.
    walk_and_upsert_project_memory(index, home, events_tx, &mut report, &excluded_ids);

    let _ = events_tx.send(PipelineEvent::ScanCompleted {
        report: report.clone(),
    });
    Ok(report)
}

/// Run the project-memory walker and upsert each hit.
///
/// Each hit is recorded as a `Memory` component with `scope = Project`
/// and a derived `name` of `<project-dir>/<basename>` (e.g.
/// `projectfinish/CLAUDE.md`). The name is what differentiates rows in
/// the URI - two `CLAUDE.md` files in different projects produce
/// distinct `aseye://claude-code/project/memory/<name>` IDs.
fn walk_and_upsert_project_memory(
    index: &Arc<IndexHandle>,
    home: Option<&Path>,
    events_tx: &broadcast::Sender<PipelineEvent>,
    report: &mut ScanReport,
    excluded_ids: &[ToolId],
) {
    let descriptors = registry_slice();
    let raw_roots = read_project_memory_roots(index);
    let roots: Vec<PathBuf> = raw_roots.iter().map(PathBuf::from).collect();
    let hits = walk_project_memory(&roots, home);

    for hit in hits {
        // Skip hits attributed to a tool the user has excluded so a
        // skipped tool doesn't show up via the project-tree walker.
        if excluded_ids.contains(&hit.tool) {
            continue;
        }

        report.components_seen = report.components_seen.saturating_add(1);

        let Some(descriptor) = descriptors.iter().find(|d| d.id == hit.tool) else {
            continue;
        };
        let name = project_memory_name(&hit);
        let synthetic_root = synthetic_memory_root(&hit);

        match upsert_component(index, descriptor, &synthetic_root, &hit.path, &name) {
            Ok(outcome) => {
                match outcome.kind {
                    UpsertKind::Inserted => {
                        report.components_inserted = report.components_inserted.saturating_add(1);
                    }
                    UpsertKind::Updated => {
                        report.components_updated = report.components_updated.saturating_add(1);
                    }
                    UpsertKind::Unchanged => {
                        report.components_unchanged = report.components_unchanged.saturating_add(1);
                    }
                }
                if outcome.had_parse_error {
                    report.parse_errors = report.parse_errors.saturating_add(1);
                    let _ = events_tx.send(PipelineEvent::ParseError {
                        id: outcome.id,
                        path: hit.path.to_string_lossy().into_owned(),
                    });
                } else {
                    let _ = events_tx.send(PipelineEvent::ComponentUpserted {
                        id: outcome.id,
                        kind: outcome.kind,
                    });
                }
            }
            Err(err) => {
                tracing::warn!(path = ?hit.path, ?err, "project memory upsert failed");
            }
        }
    }
}

/// Derive `<project-dir>/<basename>` from a walker hit. The project
/// directory is the immediate parent of the matched file; we drop any
/// further path context so rows stay short and grep-friendly. Two
/// projects with the same final dir name (e.g. `~/work/foo` and
/// `~/personal/foo`) still produce distinct URIs because the path
/// hash on the index side disambiguates them.
fn project_memory_name(hit: &ProjectMemoryHit) -> String {
    let parent = hit
        .path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|s| s.to_str())
        .unwrap_or("project");
    format!("{parent}/{}", hit.basename)
}

/// Build a synthetic `ComponentRoot` for a project-memory hit. The
/// registry's existing `Memory` roots are user-scope (Claude Code,
/// Antigravity) or only declare a relative `AGENTS.md` glob (Codex);
/// neither matches a project-tree file. We synthesize one here so the
/// upsert path can stamp the right scope, format, and flavour without
/// lying about which static descriptor the row came from.
fn synthetic_memory_root(hit: &ProjectMemoryHit) -> ComponentRoot {
    ComponentRoot {
        component_type: ComponentType::Memory,
        path_pattern: hit.path.to_string_lossy().into_owned(),
        format: Format::Markdown,
        flavour: Some(hit.basename.clone()),
        scope: Scope::Project,
        is_folder: false,
        key_path: None,
    }
}

/// Compute a component identity name for a path matched during scan.
///
/// Mirrors the rule used by [`crate::registry::classify_path`]:
/// folder-style components use the parent dir name; file-style use the
/// file stem.
fn component_name_for(path: &Path, root: &crate::registry::types::ComponentRoot) -> String {
    if root.is_folder {
        if let Some(parent) = path.parent() {
            if let Some(name) = parent.file_name() {
                return name.to_string_lossy().into_owned();
            }
        }
    }
    path.file_stem().map_or_else(
        || path.to_string_lossy().into_owned(),
        |s| s.to_string_lossy().into_owned(),
    )
}

/// Expand an absolute glob pattern into the set of matching files.
///
/// Supports:
/// * Literal segments (`config.toml`).
/// * `*` - any single segment (no `/`).
/// * `**` - zero or more segments, recursively.
///
/// Returns paths in the order they're discovered. Non-existent prefixes
/// produce an empty result rather than an error - the registry's
/// patterns intentionally describe optional locations.
fn expand_glob(pattern: &Path) -> Vec<PathBuf> {
    let mut segments: Vec<&std::ffi::OsStr> = pattern.iter().collect();
    if segments.is_empty() {
        return Vec::new();
    }

    // Pull off the absolute root (`/` on unix, drive letter on windows).
    let mut bases: Vec<PathBuf> = vec![PathBuf::from(segments.remove(0))];

    let mut out = Vec::new();
    walk(&mut bases, &segments, 0, &mut out);
    out
}

/// Recursive worker for `expand_glob`.
///
/// `depth` is the current segment index; `segments` is the *remaining*
/// glob (the absolute root has been stripped). `bases` is the set of
/// directories we're currently exploring.
fn walk(
    bases: &mut Vec<PathBuf>,
    segments: &[&std::ffi::OsStr],
    depth: usize,
    out: &mut Vec<PathBuf>,
) {
    if depth >= segments.len() {
        // No more segments to consume - the bases are the matches.
        for base in bases {
            out.push(base.clone());
        }
        return;
    }

    let seg = segments[depth].to_string_lossy();
    let mut next: Vec<PathBuf> = Vec::new();

    for base in bases.iter() {
        match seg.as_ref() {
            "**" => {
                // `**` matches zero segments (continue with this base)
                // OR any non-zero number of segments (recurse into all
                // descendants and re-evaluate).
                next.push(base.clone());
                for descendant in descendants_of(base) {
                    next.push(descendant);
                }
            }
            literal if !literal.contains('*') && !literal.contains('?') => {
                let candidate = base.join(literal);
                if candidate.exists() {
                    next.push(candidate);
                }
            }
            pattern => {
                // Mixed segment like `*.md`, `SKILL.*`, or `foo*bar`.
                // Compile a glob matcher restricted to the basename and
                // walk the current `base` directory once.
                let matcher = Glob::new(pattern).map(|g| g.compile_matcher()).ok();
                if let (Some(matcher), Ok(entries)) = (matcher, fs::read_dir(base)) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if let Some(name) = path.file_name() {
                            if matcher.is_match(Path::new(name)) {
                                next.push(path);
                            }
                        }
                    }
                }
            }
        }
    }

    walk(&mut next, segments, depth + 1, out);
}

/// Enumerate every descendant of `base` (depth-first). Used by the
/// `**` handler. Symlinks are not followed - matching the scope
/// containment philosophy.
fn descendants_of(base: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack: Vec<PathBuf> = vec![base.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            // Skip symlinks. `fs::symlink_metadata` is the right
            // syscall for "what is this entry without following".
            if let Ok(meta) = fs::symlink_metadata(&path) {
                if meta.file_type().is_symlink() {
                    continue;
                }
                if meta.is_dir() {
                    stack.push(path.clone());
                }
            }
            out.push(path);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn expand_glob_matches_single_star() {
        let dir = tempdir().expect("tempdir");
        let nested = dir.path().join("foo");
        fs::create_dir_all(&nested).unwrap();
        fs::write(nested.join("a.md"), "x").unwrap();
        fs::write(nested.join("b.md"), "x").unwrap();

        let pattern = dir.path().join("foo").join("*.md");
        let mut found = expand_glob(&pattern);
        found.sort();
        assert_eq!(found.len(), 2);
        assert!(found[0].ends_with("a.md") || found[0].ends_with("b.md"));
    }

    #[test]
    fn expand_glob_matches_double_star() {
        let dir = tempdir().expect("tempdir");
        let deep = dir.path().join("a").join("b").join("c");
        fs::create_dir_all(&deep).unwrap();
        fs::write(deep.join("x.jsonl"), "{}\n").unwrap();
        fs::write(dir.path().join("a").join("y.jsonl"), "{}\n").unwrap();

        let pattern = dir.path().join("**").join("*.jsonl");
        let mut found = expand_glob(&pattern);
        found.sort();
        assert_eq!(found.len(), 2);
    }

    #[test]
    fn expand_glob_returns_empty_for_missing_prefix() {
        let dir = tempdir().expect("tempdir");
        let pattern = dir.path().join("never").join("*.md");
        let found = expand_glob(&pattern);
        assert!(found.is_empty());
    }
}
