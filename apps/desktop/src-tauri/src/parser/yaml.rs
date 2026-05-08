//! YAML parser.
//!
//! Multi-document YAML is supported only in the sense that we deserialise
//! the *first* document and emit a `MultipleYamlDocuments` warning when
//! more than one was present. None of the configs in `docs/04-data-sources.md`
//! use multi-doc YAML; surfacing it as a warning rather than an error is
//! the user-friendly choice.

use serde::Deserialize as _;
use serde_yaml::Value as YValue;

use crate::parser::error::{ParseError, Result};
use crate::parser::warning::{ParseWarning, ParseWarningKind};

/// Outcome of parsing a YAML document.
///
/// We return the parsed value plus any warnings the caller should
/// surface. Errors are returned via `Err` and never hide content.
#[derive(Debug)]
pub struct YamlOutcome {
    pub value: serde_json::Value,
    pub warnings: Vec<ParseWarning>,
}

/// Parse `content` as YAML, returning the first document plus any
/// non-fatal warnings.
///
/// The result is normalised to `serde_json::Value` so callers can
/// treat YAML / JSON / TOML uniformly downstream.
pub fn parse(content: &[u8]) -> Result<YamlOutcome> {
    parse_with_origin(content, "yaml")
}

/// Internal helper exposed to the markdown frontmatter parser so that
/// frontmatter errors can be tagged with a different `message_prefix`
/// without copy-pasting the whole function body.
pub(crate) fn parse_with_origin(content: &[u8], origin: &str) -> Result<YamlOutcome> {
    // YAML is text; reject non-UTF-8 up front so the underlying
    // parser doesn't surface a misleading "found character ..."
    // error halfway through.
    let text = std::str::from_utf8(content).map_err(|source| ParseError::InvalidUtf8 { source })?;

    // `Deserializer::from_str` yields one item per `---`-separated
    // document. We drain it once to count, then take the first.
    // This is O(input) but our inputs are bounded by the 5 MB cap.
    let mut docs: Vec<YValue> = Vec::with_capacity(1);
    for de in serde_yaml::Deserializer::from_str(text) {
        let doc = YValue::deserialize(de).map_err(|err| {
            // Tag the message so callers can tell whether the
            // failure was in a frontmatter block vs a top-level
            // YAML file. The location info is preserved either way.
            let mut e = ParseError::from_yaml(&err);
            if let ParseError::Yaml { message, .. } = &mut e {
                *message = format!("{origin}: {message}");
            }
            e
        })?;
        docs.push(doc);
    }

    let mut warnings = Vec::new();
    if docs.len() > 1 {
        warnings.push(ParseWarning {
            kind: ParseWarningKind::MultipleYamlDocuments,
            message: format!(
                "input contained {} YAML documents; only the first was parsed",
                docs.len()
            ),
            line: None,
        });
    }

    let first = docs.into_iter().next().ok_or(ParseError::EmptyDocument)?;

    let json = yaml_to_json(first);
    Ok(YamlOutcome {
        value: json,
        warnings,
    })
}

/// Convert a `serde_yaml::Value` to a `serde_json::Value`.
///
/// Edge cases:
/// * YAML mappings whose keys are not strings (numbers, sequences) are
///   stringified via `Display`. Pure-data formats we consume rarely use
///   non-string keys; surfacing them as their textual form keeps the
///   downstream JSON shape uniform.
/// * YAML tagged values (`!!str foo`) are unwrapped to their inner value
///   - we don't currently care about the tag.
/// * NaN / +-Inf floats become `null`, matching the TOML parser.
fn yaml_to_json(value: YValue) -> serde_json::Value {
    match value {
        YValue::Null => serde_json::Value::Null,
        YValue::Bool(b) => serde_json::Value::Bool(b),
        YValue::Number(n) => {
            // serde_yaml::Number doesn't expose a single "as_f64"
            // for everyone; check int paths first to preserve
            // precision for u64 / i64.
            if let Some(i) = n.as_i64() {
                serde_json::Value::Number(i.into())
            } else if let Some(u) = n.as_u64() {
                serde_json::Value::Number(u.into())
            } else if let Some(f) = n.as_f64() {
                serde_json::Number::from_f64(f)
                    .map_or(serde_json::Value::Null, serde_json::Value::Number)
            } else {
                serde_json::Value::Null
            }
        }
        YValue::String(s) => serde_json::Value::String(s),
        YValue::Sequence(items) => {
            serde_json::Value::Array(items.into_iter().map(yaml_to_json).collect())
        }
        YValue::Mapping(map) => {
            let mut out = serde_json::Map::with_capacity(map.len());
            for (k, v) in map {
                let key = match k {
                    YValue::String(s) => s,
                    other => yaml_value_to_string(&other),
                };
                out.insert(key, yaml_to_json(v));
            }
            serde_json::Value::Object(out)
        }
        YValue::Tagged(tagged) => yaml_to_json(tagged.value),
    }
}

/// Stringify a `YValue` for use as a JSON object key.
///
/// We keep this routine simple and stable: scalars get their
/// natural textual form, anything richer gets a debug-style fallback.
fn yaml_value_to_string(value: &YValue) -> String {
    match value {
        YValue::Null => "null".to_owned(),
        YValue::Bool(b) => b.to_string(),
        YValue::Number(n) => n.to_string(),
        YValue::String(s) => s.clone(),
        // Sequences / mappings as keys are exotic; we emit a
        // YAML-ish debug representation rather than dropping data.
        other => format!("{other:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::parse;
    use crate::parser::error::ParseError;
    use crate::parser::warning::ParseWarningKind;

    #[test]
    fn yaml_parses_valid() {
        let src = br"
name: spec
description: TDD spec runner
tools:
  - codegraph
  - probe
nested:
  count: 3
  enabled: true
";
        let outcome = parse(src).expect("must parse");
        assert!(outcome.warnings.is_empty());
        assert_eq!(outcome.value["name"], "spec");
        assert_eq!(outcome.value["tools"][0], "codegraph");
        assert_eq!(outcome.value["nested"]["count"], 3);
        assert_eq!(outcome.value["nested"]["enabled"], true);
    }

    #[test]
    fn yaml_warns_multidoc() {
        // `---` on its own line introduces a new document. We take
        // the first and warn about the rest.
        let src = b"name: a\n---\nname: b\n";
        let outcome = parse(src).expect("must parse");
        assert_eq!(outcome.value["name"], "a");
        assert_eq!(outcome.warnings.len(), 1);
        assert!(matches!(
            outcome.warnings[0].kind,
            ParseWarningKind::MultipleYamlDocuments
        ));
    }

    #[test]
    fn yaml_rejects_invalid() {
        // Unclosed flow mapping.
        let err = parse(b"{ a: 1, b: 2").expect_err("must error");
        match err {
            ParseError::Yaml { message, .. } => assert!(!message.is_empty()),
            other => panic!("expected Yaml error, got {other:?}"),
        }
    }

    #[test]
    fn yaml_invalid_utf8_typed_error() {
        let err = parse(&[0xFF, 0xFE, b'a']).expect_err("must error");
        assert!(matches!(err, ParseError::InvalidUtf8 { .. }));
    }
}
