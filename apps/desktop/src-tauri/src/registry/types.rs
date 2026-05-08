//! Domain types for the tool registry.
//!
//! These types describe how a host tool (Claude Code, Codex, ...) is laid out
//! on disk, where each component lives, and what we found when we probed for
//! it. Every type here crosses the IPC boundary, so each one derives `TS`
//! and is exported into `bindings/` for the React side to consume verbatim.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

// All TS bindings for this phase land in `bindings/registry/` next to
// the crate, configured per-type via `#[ts(export_to = ...)]`. The
// frontend imports them through the workspace package.

/// Identifier for a host tool we know about.
///
/// Serialised as kebab-case strings (`claude-code`, `codex`, `cursor`,
/// `antigravity`) so the wire format is stable across Rust and TS.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export, export_to = "../bindings/registry/ToolId.ts")]
#[ts(rename_all = "kebab-case")]
pub enum ToolId {
    ClaudeCode,
    Codex,
    Cursor,
    Antigravity,
}

/// The unified component taxonomy from `docs/03-component-model.md` (the
/// 16 first-class component types). Serialised as camelCase strings so the
/// wire format is the JS-native shape.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../bindings/registry/ComponentType.ts")]
#[ts(rename_all = "camelCase")]
pub enum ComponentType {
    Tool,
    Settings,
    Memory,
    Rule,
    Skill,
    Command,
    Agent,
    Mcp,
    Hook,
    Plugin,
    Marketplace,
    Session,
    Task,
    OutputStyle,
    Statusline,
    Permission,
}

/// Where a component lives in the layered scope hierarchy.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export, export_to = "../bindings/registry/Scope.ts")]
#[ts(rename_all = "lowercase")]
pub enum Scope {
    User,
    Project,
    Enterprise,
    Plugin,
}

/// On-disk format for a component or its container file.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export, export_to = "../bindings/registry/Format.ts")]
#[ts(rename_all = "lowercase")]
pub enum Format {
    Json,
    Toml,
    Yaml,
    Markdown,
    /// Markdown with YAML frontmatter (skills, agents, rules, ...).
    MarkdownFrontmatter,
    /// Cursor's `.mdc` rules - same shape as `MarkdownFrontmatter` with a
    /// distinct extension and conventional MIME.
    Mdc,
    Jsonl,
    Sqlite,
    Binary,
}

/// Description of where to look for a particular component type within a tool.
///
/// `path_pattern` may contain `~/` (HOME) and glob wildcards (`*`, `**`). When
/// `is_folder` is true, the pattern resolves to a directory (typically the
/// folder that contains a `SKILL.md`). When `key_path` is set, the component
/// is embedded inside the file at `path_pattern` under that JSON / TOML key
/// (e.g. Claude Code's `mcpServers` lives inside `~/.claude.json`).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../bindings/registry/ComponentRoot.ts")]
#[ts(rename_all = "camelCase")]
pub struct ComponentRoot {
    pub component_type: ComponentType,
    pub path_pattern: String,
    pub format: Format,
    /// Filename flavour the parser should preserve on save (`CLAUDE.md`,
    /// `AGENTS.md`, `GEMINI.md`, ...). Optional because most components are
    /// flavour-less.
    pub flavour: Option<String>,
    pub scope: Scope,
    /// True when the component identity is the folder, not a single file
    /// (e.g. skill folders).
    pub is_folder: bool,
    /// Dot-separated key path inside the container file when the component
    /// is embedded (e.g. `"mcpServers"` inside `~/.claude.json`).
    pub key_path: Option<String>,
}

/// Static description of a host tool we know how to detect and inspect.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../bindings/registry/ToolDescriptor.ts")]
#[ts(rename_all = "camelCase")]
pub struct ToolDescriptor {
    pub id: ToolId,
    pub display_name: String,
    /// Names to look for on the user's `PATH` when probing for a CLI binary.
    pub binary_names: Vec<String>,
    /// Filesystem paths whose existence implies the tool is installed
    /// (HOME-relative paths use a leading `~/`).
    pub detection_paths: Vec<String>,
    /// Command to run to capture the tool version. The first whitespace
    /// token is the executable; subsequent tokens are passed as arguments.
    pub version_command: Option<String>,
    /// Per-component on-disk locations for this tool.
    pub component_roots: Vec<ComponentRoot>,
    /// Paths the watcher should subscribe to for live updates (Phase 1.3).
    pub watch_paths: Vec<String>,
}

/// Result of probing the host system for a particular tool.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../bindings/registry/DetectedTool.ts")]
#[ts(rename_all = "camelCase")]
pub struct DetectedTool {
    pub id: ToolId,
    pub display_name: String,
    pub detected: bool,
    /// Absolute path of the matched binary on `PATH`, when one was found.
    pub binary: Option<String>,
    /// Trimmed stdout of the version command, when it ran successfully.
    pub version: Option<String>,
    /// Subset of `ToolDescriptor::detection_paths` whose targets exist
    /// on the local filesystem (with `~/` expanded).
    pub existing_root_paths: Vec<String>,
}
