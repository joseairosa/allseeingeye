//! Validation engine.
//!
//! Phase 3.2 - public [`validate`] entry point + private helpers that:
//!
//! 1. Resolve the (tool, `component_type`) tuple to a bundled schema
//!    string.
//! 2. Compile the schema once via `OnceLock` (per tuple) so the upsert
//!    hot path only pays the validation cost.
//! 3. Pick the right instance to validate from a `ParsedComponent`:
//!    * `frontmatter` for Markdown / MDC formats (skills, agents,
//!      rules, ...).
//!    * `structured` for JSON / TOML / YAML.
//! 4. Map every `jsonschema::ValidationError` into our flat
//!    [`ValidationError`] (JSON pointer + message + schema keyword).
//! 5. Emit warnings for unknown top-level fields (those NOT in the
//!    schema's `properties`) so unknown fields never block a save.
//!
//! Lenient by default - tuples with no bundled schema return an
//! `ok: true` outcome with no findings.

use std::collections::BTreeSet;
use std::sync::OnceLock;

use jsonschema::{Draft, JSONSchema};
use serde_json::Value;

use super::{
    schemas, ValidationError, ValidationOutcome, ValidationWarning, ValidationWarningKind,
};
use crate::parser::ParsedComponent;
use crate::registry::types::{ComponentType, Format, ToolId};

/// Validate a parsed component against the bundled schema for its
/// (tool, `component_type`) tuple.
///
/// Lenient by default: when no bundled schema covers the tuple the
/// outcome is a clean pass with no errors and no warnings. The same
/// applies when the parsed component carries no instance to validate
/// (e.g. a Markdown rule file with no frontmatter).
#[must_use]
pub fn validate(
    parsed: &ParsedComponent,
    tool: ToolId,
    component_type: ComponentType,
) -> ValidationOutcome {
    let Some(compiled) = compiled_schema_for(tool, component_type) else {
        // No schema bundled - lenient pass. The frontend treats `ok=true`
        // with empty errors as "validated" so the user sees the same
        // green badge whether we have a schema or not.
        return ValidationOutcome {
            ok: true,
            errors: Vec::new(),
            warnings: Vec::new(),
        };
    };

    let Some(instance) = pick_instance(parsed) else {
        // The file is one we know how to validate (the tuple maps to a
        // schema) but the parser produced nothing to validate against.
        // For Markdown without frontmatter this is a clean pass; the
        // schema's `required` constraints would otherwise spuriously
        // fail every CLAUDE.md-style memory.
        return ValidationOutcome {
            ok: true,
            errors: Vec::new(),
            warnings: Vec::new(),
        };
    };

    let mut errors = Vec::new();
    if let Err(iter) = compiled.validate(instance) {
        for err in iter {
            errors.push(map_validation_error(&err));
        }
    }

    let warnings = collect_unknown_field_warnings(compiled.schema_root_value(), instance);

    ValidationOutcome {
        ok: errors.is_empty(),
        errors,
        warnings,
    }
}

// ---------------------------------------------------------------------------
// Schema compilation cache
// ---------------------------------------------------------------------------

/// Wrapper holding the compiled schema and the original JSON value so
/// we can introspect `properties` for the unknown-field warning pass.
pub(crate) struct CompiledBundle {
    schema_value: Value,
    compiled: JSONSchema,
}

impl CompiledBundle {
    /// Validate `instance` and return the iterator over errors.
    fn validate<'a>(&'a self, instance: &'a Value) -> Result<(), jsonschema::ErrorIterator<'a>> {
        self.compiled.validate(instance)
    }

    /// Borrow the parsed schema JSON.
    fn schema_root_value(&self) -> &Value {
        &self.schema_value
    }
}

/// Compile a schema string against Draft 2020-12.
///
/// Panics only if the bundled schema is malformed - this is a
/// programmer error caught at startup by the
/// `compile_each_bundled_schema` test, never user input.
fn compile(schema_text: &str) -> CompiledBundle {
    let schema_value: Value = serde_json::from_str(schema_text)
        .expect("bundled schema must be valid JSON; this is a compile-time invariant");
    let compiled = JSONSchema::options()
        .with_draft(Draft::Draft202012)
        .compile(&schema_value)
        .expect("bundled schema must compile; this is a compile-time invariant");
    CompiledBundle {
        schema_value,
        compiled,
    }
}

// One `OnceLock` per (tool, `component_type`) tuple. Adding a new schema
// is a one-line addition here plus a `match` arm in
// [`compiled_schema_for`]; we deliberately avoid a `HashMap` indexed by
// the runtime tuple because the static lookup encodes "every tuple is
// known at compile time" into the type system.

macro_rules! schema_lock {
    ($name:ident, $text:expr) => {
        fn $name() -> &'static CompiledBundle {
            static LOCK: OnceLock<CompiledBundle> = OnceLock::new();
            LOCK.get_or_init(|| compile($text))
        }
    };
}

schema_lock!(claude_code_skill, schemas::SCHEMA_CLAUDE_CODE_SKILL);
schema_lock!(claude_code_agent, schemas::SCHEMA_CLAUDE_CODE_AGENT);
schema_lock!(claude_code_command, schemas::SCHEMA_CLAUDE_CODE_COMMAND);
schema_lock!(claude_code_rule, schemas::SCHEMA_CLAUDE_CODE_RULE);
schema_lock!(claude_code_mcp, schemas::SCHEMA_CLAUDE_CODE_MCP);
schema_lock!(claude_code_hook, schemas::SCHEMA_CLAUDE_CODE_HOOK);
schema_lock!(claude_code_settings, schemas::SCHEMA_CLAUDE_CODE_SETTINGS);
schema_lock!(codex_skill, schemas::SCHEMA_CODEX_SKILL);
schema_lock!(codex_mcp, schemas::SCHEMA_CODEX_MCP);
schema_lock!(cursor_rule, schemas::SCHEMA_CURSOR_RULE);
schema_lock!(cursor_mcp, schemas::SCHEMA_CURSOR_MCP);
schema_lock!(antigravity_skill, schemas::SCHEMA_ANTIGRAVITY_SKILL);
schema_lock!(antigravity_rule, schemas::SCHEMA_ANTIGRAVITY_RULE);
schema_lock!(antigravity_workflow, schemas::SCHEMA_ANTIGRAVITY_WORKFLOW);

/// Map a (tool, `component_type`) tuple to its bundled compiled schema.
///
/// Returns `None` for tuples with no bundled schema - the caller treats
/// that as a lenient pass.
fn compiled_schema_for(
    tool: ToolId,
    component_type: ComponentType,
) -> Option<&'static CompiledBundle> {
    match (tool, component_type) {
        (ToolId::ClaudeCode, ComponentType::Skill) => Some(claude_code_skill()),
        (ToolId::ClaudeCode, ComponentType::Agent) => Some(claude_code_agent()),
        (ToolId::ClaudeCode, ComponentType::Command) => Some(claude_code_command()),
        (ToolId::ClaudeCode, ComponentType::Rule) => Some(claude_code_rule()),
        (ToolId::ClaudeCode, ComponentType::Mcp) => Some(claude_code_mcp()),
        (ToolId::ClaudeCode, ComponentType::Hook) => Some(claude_code_hook()),
        (ToolId::ClaudeCode, ComponentType::Settings) => Some(claude_code_settings()),
        (ToolId::Codex, ComponentType::Skill) => Some(codex_skill()),
        (ToolId::Codex, ComponentType::Mcp) => Some(codex_mcp()),
        (ToolId::Cursor, ComponentType::Rule) => Some(cursor_rule()),
        (ToolId::Cursor, ComponentType::Mcp) => Some(cursor_mcp()),
        (ToolId::Antigravity, ComponentType::Skill) => Some(antigravity_skill()),
        (ToolId::Antigravity, ComponentType::Rule) => Some(antigravity_rule()),
        (ToolId::Antigravity, ComponentType::Command) => Some(antigravity_workflow()),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Instance picking
// ---------------------------------------------------------------------------

/// Decide which slice of the parsed component to validate.
///
/// Markdown-flavoured formats validate the YAML frontmatter (the
/// structured part). Pure-data formats validate the parsed value.
/// Returns `None` when the relevant slice is missing - we treat that
/// as "nothing to validate" rather than a hard failure (a memory
/// file with no frontmatter is valid, not "missing required fields").
fn pick_instance(parsed: &ParsedComponent) -> Option<&Value> {
    match parsed.format {
        Format::Markdown | Format::MarkdownFrontmatter | Format::Mdc => parsed.frontmatter.as_ref(),
        Format::Json | Format::Toml | Format::Yaml => parsed.structured.as_ref(),
        // Streaming / binary formats are out of scope for the parser
        // dispatch and therefore for the validator. The upsert layer
        // gates on `is_data_format`, so this arm is unreachable in
        // practice; we return `None` rather than panic so a future
        // wiring change degrades gracefully.
        Format::Jsonl | Format::Sqlite | Format::Binary => None,
    }
}

// ---------------------------------------------------------------------------
// Error mapping
// ---------------------------------------------------------------------------

/// Translate a `jsonschema::ValidationError` into our flat
/// [`ValidationError`] shape: JSON pointer (`/foo/0/bar`), human
/// message, and the violated schema keyword (`required`, `type`,
/// `enum`, ...).
fn map_validation_error(err: &jsonschema::ValidationError<'_>) -> ValidationError {
    let path = err.instance_path.to_string();
    // Pointers are emitted with no leading `/` when empty, but with a
    // leading `/` for any non-empty path. Normalise the empty case to
    // `""` (root) explicitly so the UI doesn't show `null`.
    let normalised_path = if path.is_empty() { String::new() } else { path };
    ValidationError {
        path: normalised_path,
        message: err.to_string(),
        schema_keyword: keyword_for(err).to_owned(),
    }
}

/// Map a `ValidationErrorKind` to a stable keyword string. We pull from
/// a manual table rather than relying on `Display` so a future variant
/// reorder in `jsonschema` doesn't silently break our wire format.
///
/// Several `ValidationErrorKind` variants surface the same JSON Schema
/// keyword (e.g. `BacktrackLimitExceeded` and `Pattern` both fire on a
/// `pattern` constraint, and `InvalidReference` / `UnknownReferenceScheme`
/// / `Resolver` all describe a `$ref` failure). We collapse them into
/// a single match arm so the wire format reflects the **schema
/// keyword**, not the upstream variant.
fn keyword_for(err: &jsonschema::ValidationError<'_>) -> &'static str {
    use jsonschema::error::ValidationErrorKind as K;
    match &err.kind {
        K::AdditionalItems { .. } => "additionalItems",
        K::AdditionalProperties { .. } => "additionalProperties",
        K::AnyOf => "anyOf",
        K::BacktrackLimitExceeded { .. } | K::Pattern { .. } => "pattern",
        K::Constant { .. } => "const",
        K::Contains => "contains",
        K::ContentEncoding { .. } => "contentEncoding",
        K::ContentMediaType { .. } => "contentMediaType",
        K::Custom { .. } => "custom",
        K::Enum { .. } => "enum",
        K::ExclusiveMaximum { .. } => "exclusiveMaximum",
        K::ExclusiveMinimum { .. } => "exclusiveMinimum",
        K::FalseSchema => "false",
        K::FileNotFound { .. } => "fileNotFound",
        K::Format { .. } | K::InvalidURL { .. } => "format",
        K::FromUtf8 { .. } => "fromUtf8",
        K::Utf8 { .. } => "utf8",
        K::JSONParse { .. } => "jsonParse",
        K::InvalidReference { .. } | K::UnknownReferenceScheme { .. } | K::Resolver { .. } => "ref",
        K::MaxItems { .. } => "maxItems",
        K::Maximum { .. } => "maximum",
        K::MaxLength { .. } => "maxLength",
        K::MaxProperties { .. } => "maxProperties",
        K::MinItems { .. } => "minItems",
        K::Minimum { .. } => "minimum",
        K::MinLength { .. } => "minLength",
        K::MinProperties { .. } => "minProperties",
        K::MultipleOf { .. } => "multipleOf",
        K::Not { .. } => "not",
        K::OneOfMultipleValid | K::OneOfNotValid => "oneOf",
        K::PropertyNames { .. } => "propertyNames",
        K::Required { .. } => "required",
        K::Schema => "schema",
        K::Type { .. } => "type",
        K::UnevaluatedProperties { .. } => "unevaluatedProperties",
        K::UniqueItems => "uniqueItems",
    }
}

// ---------------------------------------------------------------------------
// Unknown-field warnings
// ---------------------------------------------------------------------------

/// Collect unknown top-level fields - those present in the instance
/// object but absent from the schema's `properties` keyword.
///
/// The schema is leniently `additionalProperties: true` (default) so
/// these never produce a hard error. We surface them as warnings so
/// the UI can prompt the user (Phase 7.3 / future) without blocking
/// the save path.
///
/// Non-object instances (arrays, scalars) produce no warnings.
fn collect_unknown_field_warnings(schema_root: &Value, instance: &Value) -> Vec<ValidationWarning> {
    let Some(instance_obj) = instance.as_object() else {
        return Vec::new();
    };
    let known: BTreeSet<&str> = schema_root
        .get("properties")
        .and_then(Value::as_object)
        .map(|m| m.keys().map(String::as_str).collect())
        .unwrap_or_default();
    if known.is_empty() {
        // Schema declares no properties - we can't tell "known" from
        // "unknown", so we don't emit any warnings rather than warning
        // on every field.
        return Vec::new();
    }

    let mut out = Vec::new();
    for key in instance_obj.keys() {
        if !known.contains(key.as_str()) {
            out.push(ValidationWarning {
                kind: ValidationWarningKind::UnknownField,
                path: format!("/{}", json_pointer_escape(key)),
                message: format!("unknown field `{key}` (not in schema)"),
            });
        }
    }
    out
}

/// Escape a single segment for inclusion in a JSON Pointer per RFC
/// 6901: `~` -> `~0`, `/` -> `~1`. Order matters - escape `~` first
/// so we don't double-escape the result.
fn json_pointer_escape(segment: &str) -> String {
    segment.replace('~', "~0").replace('/', "~1")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_bytes;
    use crate::registry::types::Format;
    use serde_json::json;

    /// Compile-time invariant: every bundled schema parses as JSON and
    /// compiles as a Draft 2020-12 schema. A failure here means a
    /// schema constant in `schemas.rs` is malformed - the panic message
    /// names the schema so the fix is one constant away.
    #[test]
    fn compile_each_bundled_schema() {
        for (name, text) in schemas::all_schemas() {
            let value: Value = serde_json::from_str(text)
                .unwrap_or_else(|err| panic!("schema {name} is not valid JSON: {err}"));
            JSONSchema::options()
                .with_draft(Draft::Draft202012)
                .compile(&value)
                .unwrap_or_else(|err| panic!("schema {name} failed to compile: {err}"));
        }
    }

    /// Build a `ParsedComponent` for a Markdown skill frontmatter. The
    /// helper composes the file shape inline so each test is
    /// self-contained.
    fn parsed_skill(frontmatter_yaml: &str) -> ParsedComponent {
        let file = format!("---\n{frontmatter_yaml}\n---\nbody\n");
        parse_bytes(file.as_bytes(), Format::MarkdownFrontmatter).expect("md+fm parses")
    }

    #[test]
    fn validate_claude_code_skill_minimal() {
        // Description-only skill is the minimum the spec accepts.
        let parsed = parsed_skill("description: a simple skill");
        let outcome = validate(&parsed, ToolId::ClaudeCode, ComponentType::Skill);
        assert!(outcome.ok, "outcome: {outcome:?}");
        assert!(outcome.errors.is_empty());
        assert!(outcome.warnings.is_empty());
    }

    #[test]
    fn validate_claude_code_skill_missing_description_errors() {
        // Required field omitted; expect one validation error keyed on
        // `required` with the JSON pointer at the root (the missing
        // field has no path of its own in JSON Schema's pointer model).
        let parsed = parsed_skill("name: only-name");
        let outcome = validate(&parsed, ToolId::ClaudeCode, ComponentType::Skill);
        assert!(!outcome.ok, "outcome should be invalid: {outcome:?}");
        assert!(
            outcome
                .errors
                .iter()
                .any(|e| e.schema_keyword == "required"),
            "expected `required` error, got {:?}",
            outcome.errors
        );
    }

    #[test]
    fn validate_claude_code_skill_unknown_field_warns_not_errors() {
        // `disable-model-invocation` is in the schema; `xyz` is not.
        // Schema is `additionalProperties: true` by default so the
        // unknown field must surface as a warning, not an error.
        let parsed = parsed_skill("description: ok\nxyz: 123");
        let outcome = validate(&parsed, ToolId::ClaudeCode, ComponentType::Skill);
        assert!(outcome.ok);
        assert!(outcome.errors.is_empty());
        assert_eq!(outcome.warnings.len(), 1);
        let warn = &outcome.warnings[0];
        assert_eq!(warn.kind, ValidationWarningKind::UnknownField);
        assert_eq!(warn.path, "/xyz");
    }

    #[test]
    fn validate_claude_code_agent_requires_name_and_description() {
        // Agent with no name and no description must produce TWO
        // `required` errors (one per missing field).
        let parsed = parsed_skill("model: opus");
        let outcome = validate(&parsed, ToolId::ClaudeCode, ComponentType::Agent);
        assert!(!outcome.ok);
        let required_count = outcome
            .errors
            .iter()
            .filter(|e| e.schema_keyword == "required")
            .count();
        assert_eq!(
            required_count, 2,
            "expected 2 required errors, got {:?}",
            outcome.errors
        );
    }

    #[test]
    fn validate_claude_code_mcp_stdio_requires_command() {
        // MCP without command and without url violates the `anyOf`
        // contract. The keyword on the wire is `anyOf` (the parent
        // schema constraint), even though conceptually "missing
        // command".
        let value = json!({ "args": ["-y", "@scope/server"] });
        let parsed = ParsedComponent {
            frontmatter: None,
            body: None,
            structured: Some(value),
            raw: b"{}".to_vec(),
            hash: "h".to_owned(),
            format: Format::Json,
            size: 2,
            warnings: Vec::new(),
        };
        let outcome = validate(&parsed, ToolId::ClaudeCode, ComponentType::Mcp);
        assert!(!outcome.ok);
        assert!(
            outcome.errors.iter().any(|e| e.schema_keyword == "anyOf"),
            "expected anyOf failure, got {:?}",
            outcome.errors
        );
    }

    #[test]
    fn validate_claude_code_mcp_http_requires_url() {
        // HTTP-shaped entry without url should fail. With `url` set it
        // must pass.
        let with_url = json!({ "transport": "http", "url": "https://example.com/mcp" });
        let parsed = ParsedComponent {
            frontmatter: None,
            body: None,
            structured: Some(with_url),
            raw: b"{}".to_vec(),
            hash: "h".to_owned(),
            format: Format::Json,
            size: 2,
            warnings: Vec::new(),
        };
        let outcome = validate(&parsed, ToolId::ClaudeCode, ComponentType::Mcp);
        assert!(outcome.ok, "expected valid http MCP, got {outcome:?}");

        let without_url = json!({ "transport": "http" });
        let parsed = ParsedComponent {
            frontmatter: None,
            body: None,
            structured: Some(without_url),
            raw: b"{}".to_vec(),
            hash: "h".to_owned(),
            format: Format::Json,
            size: 2,
            warnings: Vec::new(),
        };
        let outcome = validate(&parsed, ToolId::ClaudeCode, ComponentType::Mcp);
        assert!(!outcome.ok, "transport=http without url should fail");
    }

    #[test]
    fn validate_cursor_rule_globs_must_be_array() {
        // `globs: "*.md"` (string) violates the schema; `globs: ["*.md"]`
        // (array) passes. We assert both directions to pin the
        // type-checking behaviour. The string form is quoted because
        // bare `*` is YAML alias syntax and would fail at parse time
        // before the schema even sees the value.
        let bad = parsed_skill("description: r\nglobs: \"*.md\"");
        let outcome = validate(&bad, ToolId::Cursor, ComponentType::Rule);
        assert!(!outcome.ok);
        assert!(
            outcome.errors.iter().any(|e| e.schema_keyword == "type"),
            "expected `type` error for non-array globs, got {:?}",
            outcome.errors
        );

        let good = parsed_skill("description: r\nglobs:\n  - \"*.md\"");
        let outcome = validate(&good, ToolId::Cursor, ComponentType::Rule);
        assert!(outcome.ok, "{outcome:?}");
    }

    #[test]
    fn validate_unknown_tool_returns_lenient_ok() {
        // A tuple we have no bundled schema for must return a clean
        // pass. We pick (Cursor, Skill) because `docs/04 §4.3` doesn't
        // declare a Cursor skill shape.
        let parsed = parsed_skill("name: x");
        let outcome = validate(&parsed, ToolId::Cursor, ComponentType::Skill);
        assert!(outcome.ok);
        assert!(outcome.errors.is_empty());
        assert!(outcome.warnings.is_empty());
    }

    #[test]
    fn validation_error_path_is_json_pointer() {
        // The MCP schema declares `command` as `minLength: 1`. An
        // empty-string command must surface as `path = "/command"` -
        // RFC 6901 JSON pointer with a leading slash.
        let value = json!({ "command": "" });
        let parsed = ParsedComponent {
            frontmatter: None,
            body: None,
            structured: Some(value),
            raw: b"{}".to_vec(),
            hash: "h".to_owned(),
            format: Format::Json,
            size: 2,
            warnings: Vec::new(),
        };
        let outcome = validate(&parsed, ToolId::ClaudeCode, ComponentType::Mcp);
        assert!(!outcome.ok);
        let by_path: Vec<&str> = outcome.errors.iter().map(|e| e.path.as_str()).collect();
        assert!(
            by_path.contains(&"/command"),
            "expected `/command` pointer, got {by_path:?}"
        );
    }

    #[test]
    fn validate_idempotent() {
        // Same input twice must produce the same outcome. The
        // `OnceLock` schema cache means the second call exercises the
        // hot path, not the compile path.
        let parsed = parsed_skill("description: same\nxyz: extra");
        let first = validate(&parsed, ToolId::ClaudeCode, ComponentType::Skill);
        let second = validate(&parsed, ToolId::ClaudeCode, ComponentType::Skill);
        assert_eq!(first.ok, second.ok);
        assert_eq!(first.errors.len(), second.errors.len());
        assert_eq!(first.warnings.len(), second.warnings.len());
        for (a, b) in first.warnings.iter().zip(second.warnings.iter()) {
            assert_eq!(a.path, b.path);
            assert_eq!(a.kind, b.kind);
        }
    }

    #[test]
    fn validate_missing_frontmatter_is_lenient() {
        // Markdown with no frontmatter and a tuple that does have a
        // bundled schema: outcome must be a clean pass (we have
        // nothing to validate against the schema's `required`
        // constraints).
        let parsed = parse_bytes(b"# Title\n\nbody\n", Format::Markdown).expect("md");
        let outcome = validate(&parsed, ToolId::ClaudeCode, ComponentType::Rule);
        assert!(outcome.ok);
        assert!(outcome.errors.is_empty());
    }

    #[test]
    fn unknown_field_warning_path_escapes_json_pointer_specials() {
        // RFC 6901 escaping: `~` -> `~0`, `/` -> `~1`. A frontmatter
        // key containing both characters must round-trip through the
        // warning path correctly. We don't test the JSON Schema
        // validation result (the field is unknown to the schema and
        // doesn't violate any constraint); we only test the pointer.
        let parsed = parsed_skill("description: ok\n\"a/b~c\": 1");
        let outcome = validate(&parsed, ToolId::ClaudeCode, ComponentType::Skill);
        assert!(outcome.ok);
        let paths: Vec<&str> = outcome.warnings.iter().map(|w| w.path.as_str()).collect();
        assert!(
            paths.contains(&"/a~1b~0c"),
            "expected escaped pointer, got {paths:?}"
        );
    }
}
