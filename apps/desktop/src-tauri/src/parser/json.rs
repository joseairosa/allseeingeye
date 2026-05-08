//! JSON parser.
//!
//! Thin wrapper over `serde_json::from_slice` that converts the upstream
//! error into our typed `ParseError` and rejects empty input outright
//! (an empty document is not valid JSON and we don't want to silently
//! coerce it to `null`).

use crate::parser::error::{ParseError, Result};

/// Parse `content` as a single JSON value.
///
/// Returns `ParseError::EmptyDocument` if `content` has zero non-whitespace
/// bytes (`serde_json` would otherwise reject this with a generic
/// "EOF while parsing a value" error which is less informative).
pub fn parse(content: &[u8]) -> Result<serde_json::Value> {
    if content.iter().all(u8::is_ascii_whitespace) {
        return Err(ParseError::EmptyDocument);
    }

    serde_json::from_slice(content).map_err(|err| ParseError::from_json(&err))
}

#[cfg(test)]
mod tests {
    use super::parse;
    use crate::parser::error::ParseError;

    #[test]
    fn json_parses_valid() {
        let v = parse(br#"{"a": 1, "b": [true, null, "x"]}"#).expect("must parse");
        assert_eq!(v["a"], 1);
        assert_eq!(v["b"][0], true);
        assert!(v["b"][1].is_null());
        assert_eq!(v["b"][2], "x");
    }

    #[test]
    fn json_rejects_invalid() {
        let err = parse(br"{not: valid}").expect_err("must error");
        match err {
            ParseError::Json {
                line,
                column,
                message,
            } => {
                // serde_json reports 1-based line/column. We don't
                // pin the exact numbers (they can shift with crate
                // versions) but we want them to be non-zero so the
                // UI can highlight the offending location.
                assert!(line >= 1, "line should be reported, got {line}");
                assert!(column >= 1, "column should be reported, got {column}");
                assert!(!message.is_empty());
            }
            other => panic!("expected Json error, got {other:?}"),
        }
    }

    #[test]
    fn json_rejects_empty() {
        let err = parse(b"").expect_err("empty must error");
        assert!(matches!(err, ParseError::EmptyDocument));

        let err_ws = parse(b"   \n\t  ").expect_err("all-whitespace must error");
        assert!(matches!(err_ws, ParseError::EmptyDocument));
    }
}
