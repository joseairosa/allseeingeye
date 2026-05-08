//! Bundled JSON Schemas.
//!
//! Phase 3.2 - one schema per (tool, `component_type`) tuple where the
//! tool defines a meaningful structure for that type. Each schema is
//! embedded as a `&'static str` so the binary is self-contained -
//! shipping reference schemas as runtime files would force a
//! resource-loader and break the "single executable" invariant of the
//! Tauri build.
//!
//! Schemas use Draft 2020-12 (the most recent stable draft) and are
//! deliberately **lenient by default**:
//!
//! * `additionalProperties` is left at its default (`true`) for object
//!   schemas. Tool versions ship and remove fields between releases;
//!   a strict schema would block working configurations after a tool
//!   update. Unknown fields are surfaced separately as
//!   [`super::ValidationWarning`]s so the UI can flag them without
//!   gating saves.
//! * Required fields are limited to the **structural** ones (a skill
//!   without `description` is unparseable, but a Claude Code skill
//!   without `tools` is fine).
//! * Optional fields enumerate the values we know about; we use
//!   `enum` only where the host tool itself enforces a closed set
//!   (transport, isolation).
//!
//! References:
//! * `docs/04-data-sources.md` Section 4.1 (Claude Code), 4.2 (Codex),
//!   4.3 (Cursor), 4.4 (Antigravity).
//! * `docs/03-component-model.md` Sections 3.4 - 3.9 for the per-type
//!   shapes.
//! * `https://json.schemastore.org/claude-code-settings.json` for the
//!   settings shape; we mirror the required keys, leaving the rest as
//!   permissive `additionalProperties`.

// Structure is `pub const SCHEMA_<TOOL>_<TYPE>: &str = r#"{...}"#;` -
// uppercased tool, uppercased type, snake_case-with-underscores as in
// docs/04. The compiler asserts each is parseable JSON via the
// `compile_each_bundled_schema` test in `validate.rs`.

// ---------------------------------------------------------------------------
// Claude Code
// ---------------------------------------------------------------------------

/// Claude Code skill (Markdown + YAML frontmatter).
///
/// `description` is the only required field per `docs/04 §4.1` (skills
/// declared without one are unselectable by the model invocation logic).
/// Optional fields mirror the documented frontmatter shape.
pub const SCHEMA_CLAUDE_CODE_SKILL: &str = r#"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "Claude Code Skill",
  "type": "object",
  "required": ["description"],
  "properties": {
    "name": { "type": "string", "minLength": 1 },
    "description": { "type": "string", "minLength": 1 },
    "disable-model-invocation": { "type": "boolean" },
    "tools": {
      "type": "array",
      "items": { "type": "string" }
    },
    "model": { "type": "string" }
  }
}"#;

/// Claude Code agent (Markdown + YAML frontmatter).
///
/// Both `name` and `description` are required - the agent dispatcher
/// keys on `name`, and the description is what the model uses to
/// decide whether to delegate.
pub const SCHEMA_CLAUDE_CODE_AGENT: &str = r#"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "Claude Code Agent",
  "type": "object",
  "required": ["name", "description"],
  "properties": {
    "name": { "type": "string", "minLength": 1 },
    "description": { "type": "string", "minLength": 1 },
    "tools": {
      "type": "array",
      "items": { "type": "string" }
    },
    "model": { "type": "string" },
    "isolation": { "type": "string", "enum": ["worktree"] },
    "hooks": { "type": "array" }
  }
}"#;

/// Claude Code command (slash-invoked saved prompt).
///
/// `description` is optional - some commands ship with the description
/// embedded in the Markdown body rather than the frontmatter.
pub const SCHEMA_CLAUDE_CODE_COMMAND: &str = r#"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "Claude Code Command",
  "type": "object",
  "properties": {
    "description": { "type": "string" },
    "args": {
      "type": "array",
      "items": { "type": "object" }
    }
  }
}"#;

/// Claude Code rule (`.claude/rules/*.md` frontmatter).
///
/// All fields are optional - a rule with no frontmatter is a valid
/// always-on rule. `paths` (Claude's path-specific globs) and
/// `alwaysApply` (Cursor cross-pollination via `AGENTS.md`) are both
/// allowed shapes.
pub const SCHEMA_CLAUDE_CODE_RULE: &str = r#"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "Claude Code Rule",
  "type": "object",
  "properties": {
    "description": { "type": "string" },
    "paths": {
      "type": "array",
      "items": { "type": "string" }
    },
    "alwaysApply": { "type": "boolean" }
  }
}"#;

/// Claude Code MCP server entry (per-server object inside `mcpServers`).
///
/// Stdio servers carry `command`/`args`/`env`. HTTP/SSE servers carry
/// `url`/`headers`. We require **at least one** of `command` or `url`
/// via `anyOf` rather than splitting into two schemas - the host tool
/// keys on transport but accepts either shape under the same
/// `mcpServers` map.
pub const SCHEMA_CLAUDE_CODE_MCP: &str = r#"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "Claude Code MCP Server",
  "type": "object",
  "anyOf": [
    { "required": ["command"] },
    { "required": ["url"] }
  ],
  "properties": {
    "command": { "type": "string", "minLength": 1 },
    "args": {
      "type": "array",
      "items": { "type": "string" }
    },
    "env": {
      "type": "object",
      "additionalProperties": { "type": "string" }
    },
    "transport": { "type": "string", "enum": ["stdio", "sse", "http"] },
    "url": { "type": "string", "format": "uri" },
    "headers": {
      "type": "object",
      "additionalProperties": { "type": "string" }
    }
  }
}"#;

/// Claude Code hook entry (one element of `settings.hooks`).
///
/// Hook events come from the docs/04 §4.1 Hooks row. The handler is
/// one of five shapes (oneOf); the discriminator is `type`. We do
/// not require `event` to match a closed set - new events arrive every
/// few releases.
pub const SCHEMA_CLAUDE_CODE_HOOK: &str = r#"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "Claude Code Hook",
  "type": "object",
  "required": ["event", "handler"],
  "properties": {
    "event": { "type": "string", "minLength": 1 },
    "matcher": { "type": "string" },
    "handler": {
      "type": "object",
      "required": ["type"],
      "properties": {
        "type": { "type": "string", "enum": ["command", "prompt", "agent", "http", "mcp_tool"] }
      }
    },
    "async": { "type": "boolean" }
  }
}"#;

/// Claude Code top-level settings (`~/.claude/settings.json`).
///
/// Mirrors the keys we know about from the schemastore reference plus
/// docs/03 §3.16. All fields are optional - a settings file may
/// declare permissions only, hooks only, etc.
pub const SCHEMA_CLAUDE_CODE_SETTINGS: &str = r#"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "Claude Code Settings",
  "type": "object",
  "properties": {
    "permissions": { "type": "object" },
    "hooks": { "type": "object" },
    "mcpServers": { "type": "object" },
    "statusline": { "type": "object" },
    "outputStyle": { "type": "string" },
    "enabledPlugins": { "type": "object" },
    "extraKnownMarketplaces": { "type": "array" },
    "env": {
      "type": "object",
      "additionalProperties": { "type": "string" }
    }
  }
}"#;

// ---------------------------------------------------------------------------
// Codex
// ---------------------------------------------------------------------------

/// Codex skill (`~/.codex/skills/<name>/SKILL.md`).
///
/// Codex skills mirror Claude Code's frontmatter shape per `docs/04
/// §4.2`. Same required fields, same lenient handling for optional
/// fields.
pub const SCHEMA_CODEX_SKILL: &str = r#"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "Codex Skill",
  "type": "object",
  "required": ["description"],
  "properties": {
    "name": { "type": "string", "minLength": 1 },
    "description": { "type": "string", "minLength": 1 },
    "disable-model-invocation": { "type": "boolean" },
    "tools": {
      "type": "array",
      "items": { "type": "string" }
    },
    "model": { "type": "string" }
  }
}"#;

/// Codex MCP entry (one entry from `[mcp_servers.X]` in
/// `~/.codex/config.toml`).
///
/// Codex stores MCP entries as TOML tables; the parser projects them
/// into the same JSON shape the registry uses for Claude Code. The
/// schema mirrors `docs/04 §4.2` (`command`, `args`, `env`).
/// `transport` is left optional because Codex defaults to stdio when
/// omitted.
pub const SCHEMA_CODEX_MCP: &str = r#"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "Codex MCP Server",
  "type": "object",
  "required": ["command"],
  "properties": {
    "command": { "type": "string", "minLength": 1 },
    "args": {
      "type": "array",
      "items": { "type": "string" }
    },
    "env": {
      "type": "object",
      "additionalProperties": { "type": "string" }
    },
    "transport": { "type": "string", "enum": ["stdio", "sse", "http"] }
  }
}"#;

// ---------------------------------------------------------------------------
// Cursor
// ---------------------------------------------------------------------------

/// Cursor rule (`.cursor/rules/*.mdc` frontmatter).
///
/// Cursor's frontmatter shape per `docs/04 §4.3`: `description`,
/// `alwaysApply`, `globs`. All optional; an empty frontmatter is a
/// valid manual-trigger rule.
pub const SCHEMA_CURSOR_RULE: &str = r#"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "Cursor Rule",
  "type": "object",
  "properties": {
    "description": { "type": "string" },
    "alwaysApply": { "type": "boolean" },
    "globs": {
      "type": "array",
      "items": { "type": "string" }
    }
  }
}"#;

/// Cursor MCP server (`~/.cursor/mcp.json` or project equivalent).
///
/// Same shape as Claude Code's MCP entry - the protocol is the
/// driver, not the host tool. We duplicate the schema rather than
/// alias it so future Cursor-specific divergence doesn't force a
/// breaking change to the Claude Code copy.
pub const SCHEMA_CURSOR_MCP: &str = r#"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "Cursor MCP Server",
  "type": "object",
  "anyOf": [
    { "required": ["command"] },
    { "required": ["url"] }
  ],
  "properties": {
    "command": { "type": "string", "minLength": 1 },
    "args": {
      "type": "array",
      "items": { "type": "string" }
    },
    "env": {
      "type": "object",
      "additionalProperties": { "type": "string" }
    },
    "transport": { "type": "string", "enum": ["stdio", "sse", "http"] },
    "url": { "type": "string", "format": "uri" },
    "headers": {
      "type": "object",
      "additionalProperties": { "type": "string" }
    }
  }
}"#;

// ---------------------------------------------------------------------------
// Antigravity
// ---------------------------------------------------------------------------

/// Antigravity skill (`<repo>/.agent/skills/<name>/SKILL.md` or the
/// global counterpart).
///
/// Same Markdown + YAML frontmatter shape as Claude Code skills per
/// `docs/04 §4.4`. Distinct schema constant so future
/// Antigravity-specific divergence is a one-file change.
pub const SCHEMA_ANTIGRAVITY_SKILL: &str = r#"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "Antigravity Skill",
  "type": "object",
  "required": ["description"],
  "properties": {
    "name": { "type": "string", "minLength": 1 },
    "description": { "type": "string", "minLength": 1 },
    "tools": {
      "type": "array",
      "items": { "type": "string" }
    },
    "model": { "type": "string" }
  }
}"#;

/// Antigravity rule (`<repo>/.agents/rules/*.md`).
///
/// Antigravity rules carry a free-form Markdown body without a fixed
/// frontmatter contract. We accept any object shape - the schema's
/// only purpose is to reject non-object frontmatter (lists, scalars,
/// nulls) which would indicate a malformed file.
pub const SCHEMA_ANTIGRAVITY_RULE: &str = r#"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "Antigravity Rule",
  "type": "object",
  "properties": {
    "description": { "type": "string" },
    "globs": {
      "type": "array",
      "items": { "type": "string" }
    }
  }
}"#;

/// Antigravity workflow (`global_workflows/*.md` or the workspace
/// equivalent). Slash-invoked saved prompt; only `description` is
/// meaningful in frontmatter, the body is the prompt.
pub const SCHEMA_ANTIGRAVITY_WORKFLOW: &str = r#"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "Antigravity Workflow",
  "type": "object",
  "properties": {
    "description": { "type": "string" }
  }
}"#;

/// Iterator over every bundled schema for compile-time / startup
/// audit purposes. Each tuple is `(name, schema_text)` - the name is
/// the Rust constant name so a compile failure in the test harness
/// prints which schema is bad without a stack trace lookup.
///
/// Marked `#[allow(dead_code)]` because the only consumer right now is
/// the in-crate test harness (`compile_each_bundled_schema`). The
/// helper stays public so future audit code (e.g. an "export
/// schemas" command for tooling) can re-use it without re-deriving
/// the registration table.
#[allow(dead_code)]
#[must_use]
pub fn all_schemas() -> Vec<(&'static str, &'static str)> {
    vec![
        ("SCHEMA_CLAUDE_CODE_SKILL", SCHEMA_CLAUDE_CODE_SKILL),
        ("SCHEMA_CLAUDE_CODE_AGENT", SCHEMA_CLAUDE_CODE_AGENT),
        ("SCHEMA_CLAUDE_CODE_COMMAND", SCHEMA_CLAUDE_CODE_COMMAND),
        ("SCHEMA_CLAUDE_CODE_RULE", SCHEMA_CLAUDE_CODE_RULE),
        ("SCHEMA_CLAUDE_CODE_MCP", SCHEMA_CLAUDE_CODE_MCP),
        ("SCHEMA_CLAUDE_CODE_HOOK", SCHEMA_CLAUDE_CODE_HOOK),
        ("SCHEMA_CLAUDE_CODE_SETTINGS", SCHEMA_CLAUDE_CODE_SETTINGS),
        ("SCHEMA_CODEX_SKILL", SCHEMA_CODEX_SKILL),
        ("SCHEMA_CODEX_MCP", SCHEMA_CODEX_MCP),
        ("SCHEMA_CURSOR_RULE", SCHEMA_CURSOR_RULE),
        ("SCHEMA_CURSOR_MCP", SCHEMA_CURSOR_MCP),
        ("SCHEMA_ANTIGRAVITY_SKILL", SCHEMA_ANTIGRAVITY_SKILL),
        ("SCHEMA_ANTIGRAVITY_RULE", SCHEMA_ANTIGRAVITY_RULE),
        ("SCHEMA_ANTIGRAVITY_WORKFLOW", SCHEMA_ANTIGRAVITY_WORKFLOW),
    ]
}
