//! Per-tool JSON Schema validator.
//!
//! Phase 3.2 - bundled JSON Schemas for the (tool, `component_type`)
//! tuples we cover, with a lenient-by-default validation engine. Public
//! surface:
//!
//! * [`validate`] - core entry point. Pure function, no I/O. Takes a
//!   [`crate::parser::ParsedComponent`] and a `(ToolId, ComponentType)`
//!   tuple and returns a [`ValidationOutcome`].
//! * [`validate_by_id`] - SQLite-backed wrapper. Looks up a component
//!   by URI, deserialises its cached `parsed_json`, classifies the
//!   tool/type, and runs validation. Used by the IPC command.
//! * [`ValidationOutcome`] - flat result shape with errors and
//!   warnings. Crosses the IPC boundary; bindings are emitted to
//!   `bindings/validator/`.
//!
//! References:
//! * `docs/05-data-architecture.md` "Validator" section.
//! * `docs/04-data-sources.md` Sections 4.1 - 4.4 for per-tool shapes.
//! * `docs/03-component-model.md` for the unified taxonomy.

pub mod error;
pub mod schemas;
pub mod validate;

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub use error::{Result, ValidatorError};
pub use validate::validate;

use crate::index::IndexHandle;
use crate::parser::ParsedComponent;
use crate::registry::types::{ComponentType, Format, ToolId};

/// Outcome of validating a parsed component against its bundled schema.
///
/// `ok` is `true` when there are zero validation errors. Warnings do
/// not affect `ok` - the user can still save a component with unknown
/// fields, but the UI surfaces them so the user can decide whether to
/// remove them.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../bindings/validator/ValidationOutcome.ts")]
#[ts(rename_all = "camelCase")]
pub struct ValidationOutcome {
    /// `true` when `errors` is empty.
    pub ok: bool,
    /// Hard validation failures (missing required fields, wrong type,
    /// failed constraint). The save path uses these to gate "save
    /// anyway" prompts.
    pub errors: Vec<ValidationError>,
    /// Soft warnings (unknown fields the schema doesn't list under
    /// `properties`). Surfaced in the UI as informational badges; never
    /// block a save.
    pub warnings: Vec<ValidationWarning>,
}

/// A single validation error: where it occurred (JSON pointer), what
/// was wrong (human-readable message), and which schema keyword was
/// violated (`required`, `type`, `enum`, `pattern`, ...).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../bindings/validator/ValidationError.ts")]
#[ts(rename_all = "camelCase")]
pub struct ValidationError {
    /// JSON pointer (RFC 6901) to the offending value, e.g.
    /// `/frontmatter/name` or `/mcpServers/github/command`. Empty
    /// string for root-level violations (e.g. `required` errors that
    /// reference a missing field at the root).
    pub path: String,
    /// Human-readable description from the underlying validator.
    pub message: String,
    /// The JSON Schema keyword that triggered the failure. Stable
    /// camelCase strings: `required`, `type`, `enum`, `anyOf`,
    /// `oneOf`, `pattern`, `minLength`, `maxLength`, `format`, ...
    pub schema_keyword: String,
}

/// A non-blocking warning emitted alongside validation. Currently the
/// only kind is [`ValidationWarningKind::UnknownField`] but the type
/// is intentionally extensible so Phase 7.3 can add lint-level
/// warnings without a wire-format change.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../bindings/validator/ValidationWarning.ts")]
#[ts(rename_all = "camelCase")]
pub struct ValidationWarning {
    pub kind: ValidationWarningKind,
    /// JSON pointer to the field the warning references.
    pub path: String,
    pub message: String,
}

/// Discriminator for the kinds of warning the validator emits.
///
/// New variants are additive; existing ones are stable. The TS binding
/// renders each as a camelCase string for UI pattern-matching.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../bindings/validator/ValidationWarningKind.ts")]
#[ts(rename_all = "camelCase")]
pub enum ValidationWarningKind {
    /// A top-level field is present in the instance but not declared
    /// under the schema's `properties`. The schema permits it
    /// (`additionalProperties: true` by default), but the user may
    /// have made a typo.
    UnknownField,
}

/// Source of the `parse_errors` entry written by the upsert layer.
///
/// Phase 3.2 - the existing `parse_errors` column now carries either a
/// hard parse failure or a validation failure. The on-wire shape is a
/// JSON object with `kind` set to one of these strings so the UI can
/// render different badges. Existing entries (pre-3.2) are pure
/// `{"message": "..."}` objects without a `kind` field; the UI treats
/// those as `Parse` for back-compat.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../bindings/validator/ParseErrorKind.ts")]
#[ts(rename_all = "camelCase")]
pub enum ParseErrorKind {
    /// The parser rejected the file - JSON syntax error, malformed
    /// frontmatter, etc.
    Parse,
    /// The parser succeeded but the validator found one or more
    /// schema violations.
    Validation,
}

/// JSON tag used by the upsert layer when serialising a validation
/// failure into the `parse_errors` column. The pre-3.2 entry layout
/// (`{"message": "..."}`) maps to `ParseErrorKind::Parse` implicitly
/// when `kind` is missing.
///
/// Layout for `ParseErrorKind::Parse`:
/// ```json
/// { "kind": "parse", "message": "invalid JSON at line 1, column 5: ..." }
/// ```
///
/// Layout for `ParseErrorKind::Validation`:
/// ```json
/// {
///   "kind": "validation",
///   "errors": [
///     { "path": "/name", "message": "...", "schemaKeyword": "required" }
///   ],
///   "warnings": [
///     { "kind": "unknownField", "path": "/xyz", "message": "..." }
///   ]
/// }
/// ```
///
/// The IPC layer (`get_component`) returns the column verbatim - the
/// React side parses the JSON and switches on `kind`.
#[must_use]
pub fn render_validation_outcome_for_storage(outcome: &ValidationOutcome) -> serde_json::Value {
    serde_json::json!({
        "kind": "validation",
        "errors": outcome.errors,
        "warnings": outcome.warnings,
    })
}

/// IPC entry point for re-running validation by component id.
///
/// 1. Looks up the component row.
/// 2. Resolves the (tool, `component_type`) tuple.
/// 3. Reconstructs a minimal [`ParsedComponent`] from the cached
///    `parsed_json` column - the validator only reads `frontmatter`
///    and `structured`, so we don't need the file body.
/// 4. Calls [`validate`] and returns the outcome.
///
/// Errors:
/// * [`ValidatorError::NotFound`] - no row for the supplied id.
/// * [`ValidatorError::UnknownComponentClassification`] - row exists
///   but tool/type strings don't map to current enums.
/// * [`ValidatorError::Sqlite`] - read failed.
/// * [`ValidatorError::InvalidCachedJson`] - the cached `parsed_json`
///   column is corrupt; would only happen after a manual DB edit.
pub fn validate_by_id(handle: &IndexHandle, id: &str) -> Result<ValidationOutcome> {
    let row = handle.read(|conn| {
        let lookup: Option<(String, String, String, Option<String>)> = conn
            .query_row(
                "SELECT tool, type, format, parsed_json FROM component WHERE id = ?1",
                params![id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, Option<String>>(3)?,
                    ))
                },
            )
            .optional()?;
        Ok(lookup)
    })?;

    let Some((tool_str, type_str, format_str, parsed_json)) = row else {
        return Err(ValidatorError::NotFound(id.to_owned()));
    };

    let (tool, component_type) = classify(&tool_str, &type_str).ok_or_else(|| {
        ValidatorError::UnknownComponentClassification {
            id: id.to_owned(),
            tool: tool_str.clone(),
            component_type: type_str.clone(),
        }
    })?;

    let format = parse_format(&format_str).ok_or_else(|| {
        // Treat an unrecognised format as "no instance to validate" -
        // the engine will lenient-pass. This is dead code in practice
        // because the upsert layer only writes formats from the
        // `Format` enum, but we don't want to add a separate error
        // variant for an impossibility.
        ValidatorError::UnknownComponentClassification {
            id: id.to_owned(),
            tool: tool_str,
            component_type: format_str.clone(),
        }
    })?;

    // Reconstruct just enough of `ParsedComponent` for the validator.
    // `raw`, `hash`, `size`, `warnings`, and `body` are not consulted
    // by `validate`, so we feed dummy values rather than re-reading
    // the file off disk.
    let parsed = parsed_component_from_cache(format, parsed_json.as_deref())?;

    Ok(validate(&parsed, tool, component_type))
}

/// Reconstruct a `ParsedComponent` from the cached `parsed_json` value.
///
/// The cached column holds either the structured value (for
/// JSON/TOML/YAML) or the frontmatter (for Markdown/MDC). We dispatch
/// on `format` to slot the deserialised JSON into the right field.
fn parsed_component_from_cache(
    format: Format,
    parsed_json: Option<&str>,
) -> Result<ParsedComponent> {
    let value: Option<serde_json::Value> = match parsed_json {
        Some(s) if !s.is_empty() => Some(serde_json::from_str(s)?),
        _ => None,
    };

    let (frontmatter, structured) = match format {
        Format::Markdown | Format::MarkdownFrontmatter | Format::Mdc => (value, None),
        Format::Json | Format::Toml | Format::Yaml => (None, value),
        Format::Jsonl | Format::Sqlite | Format::Binary => (None, None),
    };

    Ok(ParsedComponent {
        frontmatter,
        body: None,
        structured,
        raw: Vec::new(),
        hash: String::new(),
        format,
        size: 0,
        warnings: Vec::new(),
    })
}

/// Return the bundled schema string for a `(tool, component_type)`
/// tuple, or `None` when no schema is bundled.
///
/// Phase 3.3 surfaces the raw JSON Schema text to the React form pane
/// so it can render type-appropriate inputs without round-tripping the
/// validator. The returned string is parsed once on the JS side and
/// cached - the React layer never deserialises it back into a Rust
/// type, so we keep the public surface as a `&'static str`.
#[must_use]
pub fn schema_for_tuple(tool: ToolId, component_type: ComponentType) -> Option<&'static str> {
    match (tool, component_type) {
        (ToolId::ClaudeCode, ComponentType::Skill) => Some(schemas::SCHEMA_CLAUDE_CODE_SKILL),
        (ToolId::ClaudeCode, ComponentType::Agent) => Some(schemas::SCHEMA_CLAUDE_CODE_AGENT),
        (ToolId::ClaudeCode, ComponentType::Command) => Some(schemas::SCHEMA_CLAUDE_CODE_COMMAND),
        (ToolId::ClaudeCode, ComponentType::Rule) => Some(schemas::SCHEMA_CLAUDE_CODE_RULE),
        (ToolId::ClaudeCode, ComponentType::Mcp) => Some(schemas::SCHEMA_CLAUDE_CODE_MCP),
        (ToolId::ClaudeCode, ComponentType::Hook) => Some(schemas::SCHEMA_CLAUDE_CODE_HOOK),
        (ToolId::ClaudeCode, ComponentType::Settings) => Some(schemas::SCHEMA_CLAUDE_CODE_SETTINGS),
        (ToolId::Codex, ComponentType::Skill) => Some(schemas::SCHEMA_CODEX_SKILL),
        (ToolId::Codex, ComponentType::Mcp) => Some(schemas::SCHEMA_CODEX_MCP),
        (ToolId::Cursor, ComponentType::Rule) => Some(schemas::SCHEMA_CURSOR_RULE),
        (ToolId::Cursor, ComponentType::Mcp) => Some(schemas::SCHEMA_CURSOR_MCP),
        (ToolId::Antigravity, ComponentType::Skill) => Some(schemas::SCHEMA_ANTIGRAVITY_SKILL),
        (ToolId::Antigravity, ComponentType::Rule) => Some(schemas::SCHEMA_ANTIGRAVITY_RULE),
        (ToolId::Antigravity, ComponentType::Command) => Some(schemas::SCHEMA_ANTIGRAVITY_WORKFLOW),
        _ => None,
    }
}

/// Map the on-wire `(tool, type)` strings stored in `component.tool`
/// and `component.type` back to their enums. The mapping mirrors the
/// `serde(rename_all = ...)` attributes on the enums.
fn classify(tool: &str, component_type: &str) -> Option<(ToolId, ComponentType)> {
    let tool = match tool {
        "claude-code" => ToolId::ClaudeCode,
        "codex" => ToolId::Codex,
        "cursor" => ToolId::Cursor,
        "antigravity" => ToolId::Antigravity,
        _ => return None,
    };
    let component_type = match component_type {
        "tool" => ComponentType::Tool,
        "settings" => ComponentType::Settings,
        "memory" => ComponentType::Memory,
        "rule" => ComponentType::Rule,
        "skill" => ComponentType::Skill,
        "command" => ComponentType::Command,
        "agent" => ComponentType::Agent,
        "mcp" => ComponentType::Mcp,
        "hook" => ComponentType::Hook,
        "plugin" => ComponentType::Plugin,
        "marketplace" => ComponentType::Marketplace,
        "session" => ComponentType::Session,
        "task" => ComponentType::Task,
        "outputStyle" => ComponentType::OutputStyle,
        "statusline" => ComponentType::Statusline,
        "permission" => ComponentType::Permission,
        _ => return None,
    };
    Some((tool, component_type))
}

/// Map the on-wire `format` string stored in `component.format` back to
/// the [`Format`] enum. Mirrors `format_to_str` in `index::upsert`.
fn parse_format(s: &str) -> Option<Format> {
    Some(match s {
        "json" => Format::Json,
        "toml" => Format::Toml,
        "yaml" => Format::Yaml,
        "markdown" => Format::Markdown,
        "markdownFrontmatter" => Format::MarkdownFrontmatter,
        "mdc" => Format::Mdc,
        "jsonl" => Format::Jsonl,
        "sqlite" => Format::Sqlite,
        "binary" => Format::Binary,
        _ => return None,
    })
}
