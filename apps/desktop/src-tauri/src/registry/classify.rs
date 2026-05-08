//! Path-to-component classification.
//!
//! Phase 1.6 lives at the seam between the file watcher (raw filesystem
//! paths) and the parser/index pipeline (typed components). Given a path
//! delivered by `notify`, [`classify_path`] decides:
//!
//! 1. Which registered tool the path belongs to (if any).
//! 2. Which `ComponentRoot` glob inside that tool the path matched.
//! 3. The component's identity name.
//!
//! Glob semantics follow `globset` (gitignore-style):
//! * `*` matches any single path segment.
//! * `**` matches zero or more path segments recursively.
//!
//! Folder-style components (`is_folder = true`, e.g. Claude skills whose
//! identity is the parent directory) yield the parent directory's name as
//! the component name; file-style components use the file stem.

use std::path::Path;

use globset::{Glob, GlobMatcher};

use super::detect::expand_home;
use super::types::{ComponentRoot, ToolDescriptor, ToolId};

/// Outcome of a successful classification.
///
/// We intentionally clone the matched `ComponentRoot` so downstream
/// callers (the upsert pipeline) don't need to keep the registry slice
/// alive across awaits. The clone is cheap - a handful of strings.
#[derive(Debug, Clone)]
pub struct Classification {
    pub tool: ToolId,
    pub component_root: ComponentRoot,
    pub component_name: String,
}

/// Classify `path` against every `ComponentRoot` in the registry and
/// return the first match.
///
/// Returns `None` when the path lies outside every tool's component
/// roots. The function is pure given a fixed `home`; production callers
/// pass `None` to use the system HOME.
#[must_use]
pub fn classify_path(
    path: &Path,
    registry: &[ToolDescriptor],
    home: Option<&Path>,
) -> Option<Classification> {
    for tool in registry {
        for root in &tool.component_roots {
            if let Some(name) = match_component_root(path, root, home) {
                return Some(Classification {
                    tool: tool.id,
                    component_root: root.clone(),
                    component_name: name,
                });
            }
        }
    }
    None
}

/// Try to match `path` against a single `ComponentRoot`. Returns the
/// extracted component name when the glob matches.
fn match_component_root(path: &Path, root: &ComponentRoot, home: Option<&Path>) -> Option<String> {
    let pattern = expand_home(&root.path_pattern, home);
    let matcher = build_matcher(&pattern)?;
    if !matcher.is_match(path) {
        return None;
    }

    Some(extract_name(path, root))
}

/// Build a globset matcher from a (HOME-expanded) absolute pattern.
///
/// Returns `None` for patterns the matcher rejects - we treat that as
/// "this descriptor never matches anything" rather than panicking, so a
/// broken pattern in one descriptor doesn't poison classification for
/// the others.
fn build_matcher(pattern: &Path) -> Option<GlobMatcher> {
    let pattern_str = pattern.to_str()?;
    Glob::new(pattern_str).ok().map(|g| g.compile_matcher())
}

/// Extract a component identity name from a matched path.
///
/// * Folder-style components (`is_folder = true`): the parent directory
///   name. Matches the convention "the folder that contains the
///   `SKILL.md` file IS the skill" used by Claude Code, Codex, and
///   Antigravity per `docs/03-component-model.md`.
/// * File-style components: the file stem (filename without final
///   extension). For `~/.claude/agents/foo.md` the name is `foo`.
fn extract_name(path: &Path, root: &ComponentRoot) -> String {
    if root.is_folder {
        // The matched path is the inner file (e.g. `.../foo/SKILL.md`);
        // the component name is the *parent directory* of that file.
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

#[cfg(test)]
mod tests {
    use super::super::tools;
    use super::*;

    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn classify_path_skill_folder() {
        let home = tempdir().expect("tempdir");
        let skill_dir = home.path().join(".claude").join("skills").join("foo");
        fs::create_dir_all(&skill_dir).expect("create skill dir");
        let skill_md = skill_dir.join("SKILL.md");
        fs::write(&skill_md, b"---\nname: foo\n---\nbody\n").expect("write skill");

        let registry = tools::all_descriptors();
        let result =
            classify_path(&skill_md, &registry, Some(home.path())).expect("skill must classify");

        assert_eq!(result.tool, ToolId::ClaudeCode);
        assert_eq!(result.component_name, "foo");
        assert!(result.component_root.is_folder);
    }

    #[test]
    fn classify_path_outside_returns_none() {
        let home = tempdir().expect("tempdir");
        // A path that has no relation to any registered tool.
        let stray = home.path().join("not-a-tool").join("random.txt");

        let registry = tools::all_descriptors();
        let result = classify_path(&stray, &registry, Some(home.path()));
        assert!(result.is_none());
    }

    #[test]
    fn classify_path_settings_file() {
        let home = tempdir().expect("tempdir");
        let claude_dir = home.path().join(".claude");
        fs::create_dir_all(&claude_dir).expect("mkdir");
        let settings = claude_dir.join("settings.json");
        fs::write(&settings, b"{}").expect("write");

        let registry = tools::all_descriptors();
        let result =
            classify_path(&settings, &registry, Some(home.path())).expect("settings must classify");
        assert_eq!(result.tool, ToolId::ClaudeCode);
        assert_eq!(result.component_name, "settings");
    }

    #[test]
    fn classify_path_codex_session_recursive() {
        let home = tempdir().expect("tempdir");
        let nested = home
            .path()
            .join(".codex")
            .join("sessions")
            .join("2026")
            .join("05");
        fs::create_dir_all(&nested).expect("mkdir");
        let session = nested.join("rollout-abc.jsonl");
        fs::write(&session, b"{}\n").expect("write");

        let registry = tools::all_descriptors();
        let result =
            classify_path(&session, &registry, Some(home.path())).expect("recursive ** must match");
        assert_eq!(result.tool, ToolId::Codex);
    }

    #[test]
    fn extract_name_uses_file_stem_for_file_style() {
        let home = tempdir().expect("tempdir");
        let agents_dir = home.path().join(".claude").join("agents");
        fs::create_dir_all(&agents_dir).expect("mkdir");
        let agent = agents_dir.join("aseye-rust-backend.md");
        fs::write(&agent, b"---\nname: x\n---\nbody\n").expect("write");

        let registry = tools::all_descriptors();
        let result =
            classify_path(&agent, &registry, Some(home.path())).expect("agent must classify");
        assert_eq!(result.component_name, "aseye-rust-backend");
    }
}
