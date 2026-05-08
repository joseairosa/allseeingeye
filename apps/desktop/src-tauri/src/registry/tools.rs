//! Static tool descriptors.
//!
//! Each builder function returns the descriptor for one host tool. The data
//! mirrors `docs/04-data-sources.md` sections 4.1 - 4.4. Adding a new tool is
//! a code change, not a runtime config (per `docs/05-data-architecture.md`
//! "Tool registry").

use super::types::{
    ComponentRoot, ComponentType, Format, Scope, ToolDescriptor, ToolId,
};

/// Build the descriptor for Claude Code (docs/04 Section 4.1).
#[must_use]
pub fn claude_code() -> ToolDescriptor {
    ToolDescriptor {
        id: ToolId::ClaudeCode,
        display_name: "Claude Code".to_owned(),
        binary_names: vec!["claude".to_owned(), "claude-code".to_owned()],
        detection_paths: vec![
            "~/.claude".to_owned(),
            "~/.claude.json".to_owned(),
        ],
        version_command: Some("claude --version".to_owned()),
        component_roots: vec![
            // 2 - Settings (user level + local override).
            ComponentRoot {
                component_type: ComponentType::Settings,
                path_pattern: "~/.claude/settings.json".to_owned(),
                format: Format::Json,
                flavour: None,
                scope: Scope::User,
                is_folder: false,
                key_path: None,
            },
            ComponentRoot {
                component_type: ComponentType::Settings,
                path_pattern: "~/.claude/settings.local.json".to_owned(),
                format: Format::Json,
                flavour: Some("settings.local.json".to_owned()),
                scope: Scope::User,
                is_folder: false,
                key_path: None,
            },
            // 3 - Memory.
            ComponentRoot {
                component_type: ComponentType::Memory,
                path_pattern: "~/.claude/CLAUDE.md".to_owned(),
                format: Format::Markdown,
                flavour: Some("CLAUDE.md".to_owned()),
                scope: Scope::User,
                is_folder: false,
                key_path: None,
            },
            // 4 - Rules.
            ComponentRoot {
                component_type: ComponentType::Rule,
                path_pattern: "~/.claude/rules/*.md".to_owned(),
                format: Format::MarkdownFrontmatter,
                flavour: None,
                scope: Scope::User,
                is_folder: false,
                key_path: None,
            },
            // 5 - Skills (folder identity).
            ComponentRoot {
                component_type: ComponentType::Skill,
                path_pattern: "~/.claude/skills/*/SKILL.md".to_owned(),
                format: Format::MarkdownFrontmatter,
                flavour: None,
                scope: Scope::User,
                is_folder: true,
                key_path: None,
            },
            // 6 - Commands.
            ComponentRoot {
                component_type: ComponentType::Command,
                path_pattern: "~/.claude/commands/*.md".to_owned(),
                format: Format::Markdown,
                flavour: None,
                scope: Scope::User,
                is_folder: false,
                key_path: None,
            },
            // 7 - Agents.
            ComponentRoot {
                component_type: ComponentType::Agent,
                path_pattern: "~/.claude/agents/*.md".to_owned(),
                format: Format::MarkdownFrontmatter,
                flavour: None,
                scope: Scope::User,
                is_folder: false,
                key_path: None,
            },
            // 8 - MCP servers, embedded inside `~/.claude.json`.
            ComponentRoot {
                component_type: ComponentType::Mcp,
                path_pattern: "~/.claude.json".to_owned(),
                format: Format::Json,
                flavour: None,
                scope: Scope::User,
                is_folder: false,
                key_path: Some("mcpServers".to_owned()),
            },
            // 9 - Hooks, embedded inside `settings.json`.
            ComponentRoot {
                component_type: ComponentType::Hook,
                path_pattern: "~/.claude/settings.json".to_owned(),
                format: Format::Json,
                flavour: None,
                scope: Scope::User,
                is_folder: false,
                key_path: Some("hooks".to_owned()),
            },
            // 10 - Plugins.
            ComponentRoot {
                component_type: ComponentType::Plugin,
                path_pattern:
                    "~/.claude/plugins/cache/*/*/*/.claude-plugin/plugin.json"
                        .to_owned(),
                format: Format::Json,
                flavour: None,
                scope: Scope::Plugin,
                is_folder: false,
                key_path: None,
            },
        ],
        watch_paths: vec![
            "~/.claude".to_owned(),
            "~/.claude.json".to_owned(),
        ],
    }
}

/// Build the descriptor for Codex (docs/04 Section 4.2).
#[must_use]
pub fn codex() -> ToolDescriptor {
    ToolDescriptor {
        id: ToolId::Codex,
        display_name: "Codex".to_owned(),
        binary_names: vec!["codex".to_owned()],
        detection_paths: vec!["~/.codex".to_owned()],
        version_command: Some("codex --version".to_owned()),
        component_roots: vec![
            // 2 - Config.
            ComponentRoot {
                component_type: ComponentType::Settings,
                path_pattern: "~/.codex/config.toml".to_owned(),
                format: Format::Toml,
                flavour: None,
                scope: Scope::User,
                is_folder: false,
                key_path: None,
            },
            // 3 - Memory (project-level only on disk).
            ComponentRoot {
                component_type: ComponentType::Memory,
                path_pattern: "AGENTS.md".to_owned(),
                format: Format::Markdown,
                flavour: Some("AGENTS.md".to_owned()),
                scope: Scope::Project,
                is_folder: false,
                key_path: None,
            },
            // 5 - Skills (folder identity).
            ComponentRoot {
                component_type: ComponentType::Skill,
                path_pattern: "~/.codex/skills/*/SKILL.md".to_owned(),
                format: Format::MarkdownFrontmatter,
                flavour: None,
                scope: Scope::User,
                is_folder: true,
                key_path: None,
            },
            // 8 - MCP servers, embedded inside `config.toml` `mcp_servers`
            // table.
            ComponentRoot {
                component_type: ComponentType::Mcp,
                path_pattern: "~/.codex/config.toml".to_owned(),
                format: Format::Toml,
                flavour: None,
                scope: Scope::User,
                is_folder: false,
                key_path: Some("mcp_servers".to_owned()),
            },
            // 12 - Sessions (date-partitioned JSONL).
            ComponentRoot {
                component_type: ComponentType::Session,
                path_pattern: "~/.codex/sessions/**/*.jsonl".to_owned(),
                format: Format::Jsonl,
                flavour: None,
                scope: Scope::User,
                is_folder: false,
                key_path: None,
            },
            // 12 - History (chat log, append-only).
            ComponentRoot {
                component_type: ComponentType::Session,
                path_pattern: "~/.codex/history.jsonl".to_owned(),
                format: Format::Jsonl,
                flavour: Some("history.jsonl".to_owned()),
                scope: Scope::User,
                is_folder: false,
                key_path: None,
            },
        ],
        watch_paths: vec!["~/.codex".to_owned()],
    }
}

/// Build the descriptor for Cursor (docs/04 Section 4.3).
#[must_use]
pub fn cursor() -> ToolDescriptor {
    ToolDescriptor {
        id: ToolId::Cursor,
        display_name: "Cursor".to_owned(),
        binary_names: vec!["cursor".to_owned()],
        detection_paths: vec!["~/.cursor".to_owned()],
        version_command: Some("cursor --version".to_owned()),
        component_roots: vec![
            // 4 - Rules (user level, .mdc).
            ComponentRoot {
                component_type: ComponentType::Rule,
                path_pattern: "~/.cursor/rules/*.mdc".to_owned(),
                format: Format::Mdc,
                flavour: None,
                scope: Scope::User,
                is_folder: false,
                key_path: None,
            },
            // 8 - MCP (sibling JSON file, not embedded).
            ComponentRoot {
                component_type: ComponentType::Mcp,
                path_pattern: "~/.cursor/mcp.json".to_owned(),
                format: Format::Json,
                flavour: None,
                scope: Scope::User,
                is_folder: false,
                key_path: None,
            },
            // 9 - Hooks.
            ComponentRoot {
                component_type: ComponentType::Hook,
                path_pattern: "~/.cursor/hooks.json".to_owned(),
                format: Format::Json,
                flavour: None,
                scope: Scope::User,
                is_folder: false,
                key_path: None,
            },
        ],
        watch_paths: vec!["~/.cursor".to_owned()],
    }
}

/// Build the descriptor for Antigravity (docs/04 Section 4.4).
#[must_use]
pub fn antigravity() -> ToolDescriptor {
    ToolDescriptor {
        id: ToolId::Antigravity,
        display_name: "Antigravity".to_owned(),
        // No widely-shipped CLI yet; detection is path-based.
        binary_names: vec![],
        detection_paths: vec![
            "~/.gemini".to_owned(),
            "~/.gemini/antigravity".to_owned(),
        ],
        version_command: None,
        component_roots: vec![
            // 3 - Memory (shared with Gemini CLI; see gotcha in docs/04).
            ComponentRoot {
                component_type: ComponentType::Memory,
                path_pattern: "~/.gemini/GEMINI.md".to_owned(),
                format: Format::Markdown,
                flavour: Some("GEMINI.md".to_owned()),
                scope: Scope::User,
                is_folder: false,
                key_path: None,
            },
            // 5 - Skills (folder identity, global Antigravity location).
            ComponentRoot {
                component_type: ComponentType::Skill,
                path_pattern:
                    "~/.gemini/antigravity/skills/*/SKILL.md".to_owned(),
                format: Format::MarkdownFrontmatter,
                flavour: None,
                scope: Scope::User,
                is_folder: true,
                key_path: None,
            },
            // 8 - MCP (sibling JSON file).
            ComponentRoot {
                component_type: ComponentType::Mcp,
                path_pattern:
                    "~/.gemini/antigravity/mcp_config.json".to_owned(),
                format: Format::Json,
                flavour: None,
                scope: Scope::User,
                is_folder: false,
                key_path: None,
            },
        ],
        watch_paths: vec!["~/.gemini".to_owned()],
    }
}

/// All descriptors known at build time, in canonical order.
#[must_use]
pub fn all_descriptors() -> Vec<ToolDescriptor> {
    vec![claude_code(), codex(), cursor(), antigravity()]
}
