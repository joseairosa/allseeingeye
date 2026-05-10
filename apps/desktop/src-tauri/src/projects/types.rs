//! Shared report + payload types for the projects IPC surface.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// One project surfaced in the Projects view. A "project" is the
/// parent directory of an indexed memory component (CLAUDE.md /
/// AGENTS.md / GEMINI.md).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../bindings/projects/ProjectSummary.ts")]
#[ts(rename_all = "camelCase")]
pub struct ProjectSummary {
    /// Absolute path to the project root.
    pub project_path: String,
    /// Human-readable label - last 1-2 path segments, e.g.
    /// `"Development/projectfinish"`.
    pub display_name: String,
    /// Every indexed memory file at the project root. There is
    /// usually one (CLAUDE.md) but a project can have multiple
    /// (CLAUDE.md + AGENTS.md if it's used by both Claude and Codex).
    pub memory_files: Vec<MemoryFileSummary>,
    /// Size in bytes of the primary memory file. Preference order
    /// CLAUDE.md > AGENTS.md > GEMINI.md.
    pub primary_memory_path: String,
    pub primary_memory_size: u64,
    /// Token estimate for the primary memory file (size / 4 heuristic,
    /// matching the inventory size chip).
    pub primary_memory_tokens_est: u64,
    /// True when the primary memory file is over the spec's 8 KiB
    /// "oversized" threshold. Drives the warning glyph in the UI.
    pub is_oversized: bool,
    /// True iff the project root contains a `.git/` directory. Drives
    /// whether the worktree audit action is enabled.
    pub has_git: bool,
}

/// One memory file inside a project. Multiple are possible when a
/// project is used by more than one tool.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../bindings/projects/MemoryFileSummary.ts")]
#[ts(rename_all = "camelCase")]
pub struct MemoryFileSummary {
    /// Filename only (e.g. `"CLAUDE.md"`).
    pub basename: String,
    /// Absolute path to the file.
    pub path: String,
    pub size: u64,
    /// Last-modified time as unix seconds.
    pub mtime: i64,
}

/// Errors that prevent the orchestrator from even starting a project
/// action (vs. per-row errors that collect inside a report).
#[derive(Debug, thiserror::Error)]
pub enum ProjectsError {
    #[error(transparent)]
    Index(#[from] crate::index::IndexError),
}
