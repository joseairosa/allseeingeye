//! TOML parser.
//!
//! Parses TOML and projects the result onto `serde_json::Value` so the
//! downstream component shape is uniform regardless of source format.
//! Per `docs/04-data-sources.md` the only TOML we currently consume is
//! Codex's `~/.codex/config.toml`.
//!
//! Conversion rules (TOML -> JSON Value):
//! * tables    -> objects (`Map<String, Value>`)
//! * arrays    -> arrays
//! * strings   -> strings
//! * integers  -> numbers (i64)
//! * floats    -> numbers (f64); NaN / +-Inf -> null because JSON has no
//!   representation for them and silently dropping the value is safer
//!   than emitting invalid JSON
//! * booleans  -> booleans
//! * datetimes -> ISO 8601 strings (the canonical wire form), preserving
//!   the original textual representation via `Display`

use crate::parser::error::{ParseError, Result};

/// Parse `content` as TOML and convert the result to a `serde_json::Value`.
///
/// `serde_json::Value` is the uniform downstream type so JSON / TOML / YAML
/// can be handled interchangeably by the rest of the parser.
pub fn parse(content: &[u8]) -> Result<serde_json::Value> {
    // TOML is text, but the parser dispatch hands us bytes. Decode
    // first so an `InvalidUtf8` shows up as a typed variant instead
    // of a confusing TOML lexer error.
    let text = std::str::from_utf8(content).map_err(|source| ParseError::InvalidUtf8 { source })?;

    let value: toml::Value = toml::from_str(text).map_err(|err| ParseError::from_toml(&err))?;
    Ok(toml_to_json(value))
}

/// Recursive conversion from `toml::Value` to `serde_json::Value`.
///
/// Kept as a free function so the unit tests can hit edge cases (NaN,
/// nested arrays of tables) without spinning up a parser.
fn toml_to_json(value: toml::Value) -> serde_json::Value {
    match value {
        toml::Value::String(s) => serde_json::Value::String(s),
        toml::Value::Integer(n) => serde_json::Value::Number(n.into()),
        toml::Value::Float(f) => {
            // `serde_json::Number::from_f64` rejects NaN / +-Inf
            // because RFC 8259 forbids them. Map to JSON `null`
            // rather than panicking - matches what most JS-side
            // serialisers would do.
            serde_json::Number::from_f64(f)
                .map_or(serde_json::Value::Null, serde_json::Value::Number)
        }
        toml::Value::Boolean(b) => serde_json::Value::Bool(b),
        toml::Value::Datetime(dt) => {
            // `Display` on `toml::value::Datetime` emits the ISO 8601
            // representation we want (e.g. `2024-04-15T10:00:00Z`).
            serde_json::Value::String(dt.to_string())
        }
        toml::Value::Array(items) => {
            serde_json::Value::Array(items.into_iter().map(toml_to_json).collect())
        }
        toml::Value::Table(map) => {
            // `toml::map::Map` iterates entries in insertion order;
            // `serde_json::Map` preserves order when the
            // `preserve_order` feature is on. Without that feature
            // we still get deterministic output because BTreeMap
            // (the default backing store) is sorted by key, which
            // is fine for our purposes.
            let mut out = serde_json::Map::with_capacity(map.len());
            for (k, v) in map {
                out.insert(k, toml_to_json(v));
            }
            serde_json::Value::Object(out)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::parse;
    use crate::parser::error::ParseError;

    #[test]
    fn toml_parses_valid_with_tables_and_arrays() {
        // Mirrors the Codex `config.toml` shape we will encounter
        // in the wild: top-level keys plus nested `[mcp_servers.*]`
        // tables and array-of-strings values.
        let src = br#"
approval_policy = "on-request"
sandbox_mode = "workspace-write"

[mcp_servers.fs]
command = "uv"
args = ["run", "mcp-server-fs"]

[mcp_servers.fs.env]
ROOT = "/tmp"

[[profiles]]
name = "default"
model = "gpt-5"
"#;
        let v = parse(src).expect("must parse");
        assert_eq!(v["approval_policy"], "on-request");
        assert_eq!(v["sandbox_mode"], "workspace-write");
        assert_eq!(v["mcp_servers"]["fs"]["command"], "uv");
        assert_eq!(v["mcp_servers"]["fs"]["args"][0], "run");
        assert_eq!(v["mcp_servers"]["fs"]["args"][1], "mcp-server-fs");
        assert_eq!(v["mcp_servers"]["fs"]["env"]["ROOT"], "/tmp");
        assert_eq!(v["profiles"][0]["name"], "default");
    }

    #[test]
    fn toml_rejects_invalid() {
        // Invalid: unclosed table header.
        let err = parse(b"[broken\nfoo = 1").expect_err("must error");
        match err {
            ParseError::Toml { message, .. } => assert!(!message.is_empty()),
            other => panic!("expected Toml error, got {other:?}"),
        }
    }

    #[test]
    fn toml_datetime_serialises_as_string() {
        // The TOML datetime representation is preserved verbatim
        // (matches `toml::value::Datetime::Display`).
        let v = parse(b"created = 2026-04-15T10:00:00Z").expect("must parse");
        assert_eq!(v["created"], "2026-04-15T10:00:00Z");
    }

    #[test]
    fn toml_invalid_utf8_typed_error() {
        // `\xFF` is not valid UTF-8; we surface it as `InvalidUtf8`
        // instead of leaking through as a TOML lexer error.
        let err = parse(&[0xFF, 0xFE, b'a']).expect_err("must error");
        assert!(matches!(err, ParseError::InvalidUtf8 { .. }));
    }

    #[test]
    fn toml_float_nan_becomes_null() {
        // RFC 8259 forbids NaN / Inf; we map to JSON `null` instead
        // of panicking. (TOML *does* allow `nan` / `inf`.)
        let v = parse(b"x = nan").expect("must parse");
        assert!(v["x"].is_null());
    }
}
