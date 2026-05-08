//! Parser dispatch.
//!
//! Phase 1.4 - the public entry point that turns raw bytes off disk into
//! a typed `ParsedComponent` ready for indexing. Per
//! `docs/05-data-architecture.md` ("Parser dispatch") and
//! `docs/04-data-sources.md` ("Parsing strategy"), each on-disk format
//! routes to a format-specific parser:
//!
//! | Format                | Parser           |
//! |-----------------------|------------------|
//! | JSON                  | `parser::json`   |
//! | TOML                  | `parser::toml`   |
//! | YAML                  | `parser::yaml`   |
//! | Markdown / MD frontm. | `parser::markdown` |
//! | MDC                   | same as MD frontm.|
//!
//! `JSONL` / `SQLite` / `Binary` are deliberately *not* handled here -
//! they require streaming or library-specific access patterns that don't
//! fit the "load 5 MB and parse" model. Phase 1.6 layers those in.

pub mod error;
pub mod hash;
pub mod json;
pub mod markdown;
pub mod toml;
pub mod warning;
pub mod yaml;

use std::path::Path;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::registry::types::Format;

pub use error::{ParseError, Result};
pub use warning::{ParseWarning, ParseWarningKind};

/// Maximum input size accepted by the parser layer.
///
/// 5 MB matches `docs/05-data-architecture.md` ("Parser dispatch" step 1).
/// Files above the cap are rejected with `ParseError::SizeExceeded`; the
/// IPC layer surfaces this as a parse warning to the UI rather than
/// indexing partial content.
pub const MAX_PARSE_SIZE: u64 = 5 * 1024 * 1024;

/// Output of parsing a single component file.
///
/// Field semantics depend on the source format:
/// * `frontmatter` - YAML frontmatter for Markdown / MDC; `None` for
///   pure-data formats (JSON / TOML / YAML).
/// * `body` - Markdown / doc body text; `None` for pure-data formats.
/// * `structured` - the full parsed value for JSON / TOML / YAML;
///   `None` for Markdown without frontmatter.
/// * `raw` - verbatim bytes off disk, kept for round-trip serialisation.
/// * `hash` - SHA-256 hex of `raw`.
/// * `format` - the source format, copied through from the caller.
/// * `size` - `raw.len()` as `u64` for ergonomic comparison with
///   `std::fs::Metadata::len`.
/// * `warnings` - non-fatal warnings (`UnclosedFrontmatter`,
///   `MultipleYamlDocuments`, ...).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../bindings/parser/ParsedComponent.ts")]
#[ts(rename_all = "camelCase")]
pub struct ParsedComponent {
    pub frontmatter: Option<serde_json::Value>,
    pub body: Option<String>,
    pub structured: Option<serde_json::Value>,
    pub raw: Vec<u8>,
    pub hash: String,
    pub format: Format,
    pub size: u64,
    pub warnings: Vec<ParseWarning>,
}

/// Read `path` and parse it according to `format`.
///
/// Errors:
/// * `ParseError::Io` - failed to read the file off disk.
/// * `ParseError::SizeExceeded` - file is larger than `MAX_PARSE_SIZE`.
/// * Format-specific parse errors (`Json`, `Toml`, `Yaml`,
///   `InvalidUtf8`, `EmptyDocument`).
pub fn parse_file(path: &Path, format: Format) -> Result<ParsedComponent> {
    let content = std::fs::read(path).map_err(|source| ParseError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    parse_bytes(&content, format)
}

/// Parse `content` according to `format`.
///
/// Splits on format and delegates to the per-format parser. Always
/// computes the SHA-256 hash and the `size` regardless of which parser
/// runs - those fields are universal.
pub fn parse_bytes(content: &[u8], format: Format) -> Result<ParsedComponent> {
    let size = u64::try_from(content.len()).unwrap_or(u64::MAX);
    if size > MAX_PARSE_SIZE {
        return Err(ParseError::SizeExceeded {
            size,
            cap: MAX_PARSE_SIZE,
        });
    }

    let hash_hex = hash::sha256_hex(content);
    let raw = content.to_vec();

    let mut frontmatter = None;
    let mut body = None;
    let mut structured = None;
    let mut warnings = Vec::new();

    match format {
        Format::Json => {
            structured = Some(json::parse(content)?);
        }
        Format::Toml => {
            structured = Some(toml::parse(content)?);
        }
        Format::Yaml => {
            let outcome = yaml::parse(content)?;
            structured = Some(outcome.value);
            warnings.extend(outcome.warnings);
        }
        Format::Markdown | Format::MarkdownFrontmatter | Format::Mdc => {
            // We treat all three identically here: the parser
            // recognises a `---` block when present, and the
            // distinction only matters for the registry's choice
            // of where to look on disk. MDC is Cursor's marketing
            // name for the same shape.
            let doc = markdown::parse(content)?;
            frontmatter = doc.frontmatter;
            body = Some(doc.body);
            warnings.extend(doc.warnings);
        }
        Format::Jsonl | Format::Sqlite | Format::Binary => {
            // Out of scope for Phase 1.4. The IPC layer (Phase 1.6)
            // handles these via streaming readers (JSONL) and
            // library-specific access (rusqlite read-only). We
            // surface the request as a typed `UnsupportedFormat`
            // error so callers can distinguish "we don't handle
            // that here" from "malformed content".
            return Err(ParseError::UnsupportedFormat(format));
        }
    }

    Ok(ParsedComponent {
        frontmatter,
        body,
        structured,
        raw,
        hash: hash_hex,
        format,
        size,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use super::{parse_bytes, parse_file, ParseError, MAX_PARSE_SIZE};
    use crate::registry::types::Format;

    #[test]
    fn parse_file_dispatch_picks_right_parser_per_format() {
        // Each format routes to a different downstream parser; this
        // test pins the dispatch table by exercising one input per
        // supported format and checking the populated fields.
        let dir = tempfile::tempdir().expect("tmp");

        // JSON
        let json_path = dir.path().join("a.json");
        std::fs::write(&json_path, br#"{"hello": "world"}"#).unwrap();
        let parsed = parse_file(&json_path, Format::Json).expect("json parses");
        assert_eq!(parsed.structured.as_ref().unwrap()["hello"], "world");
        assert!(parsed.frontmatter.is_none());
        assert!(parsed.body.is_none());
        assert_eq!(parsed.format, Format::Json);

        // TOML
        let toml_path = dir.path().join("a.toml");
        std::fs::write(&toml_path, b"key = \"value\"\n").unwrap();
        let parsed = parse_file(&toml_path, Format::Toml).expect("toml parses");
        assert_eq!(parsed.structured.as_ref().unwrap()["key"], "value");
        assert_eq!(parsed.format, Format::Toml);

        // YAML
        let yaml_path = dir.path().join("a.yaml");
        std::fs::write(&yaml_path, b"key: value\n").unwrap();
        let parsed = parse_file(&yaml_path, Format::Yaml).expect("yaml parses");
        assert_eq!(parsed.structured.as_ref().unwrap()["key"], "value");
        assert_eq!(parsed.format, Format::Yaml);

        // Markdown (no frontmatter)
        let md_path = dir.path().join("a.md");
        std::fs::write(&md_path, b"# Title\n\nbody\n").unwrap();
        let parsed = parse_file(&md_path, Format::Markdown).expect("md parses");
        assert!(parsed.frontmatter.is_none());
        assert_eq!(parsed.body.as_deref(), Some("# Title\n\nbody\n"));

        // Markdown + frontmatter
        let mdfm_path = dir.path().join("b.md");
        std::fs::write(&mdfm_path, b"---\nname: x\n---\nbody\n").unwrap();
        let parsed = parse_file(&mdfm_path, Format::MarkdownFrontmatter).expect("md+fm parses");
        assert_eq!(parsed.frontmatter.as_ref().unwrap()["name"], "x");
        assert_eq!(parsed.body.as_deref(), Some("body\n"));

        // MDC routes through the same parser as MarkdownFrontmatter.
        let mdc_path = dir.path().join("a.mdc");
        std::fs::write(&mdc_path, b"---\ndescription: r\n---\nrule\n").unwrap();
        let parsed = parse_file(&mdc_path, Format::Mdc).expect("mdc parses");
        assert_eq!(parsed.frontmatter.as_ref().unwrap()["description"], "r");
        assert_eq!(parsed.body.as_deref(), Some("rule\n"));
        assert_eq!(parsed.format, Format::Mdc);
    }

    #[test]
    fn parse_file_size_cap_enforced() {
        // 6 MB > MAX_PARSE_SIZE (5 MB) - must reject before
        // attempting to parse the content.
        let dir = tempfile::tempdir().expect("tmp");
        let big_path = dir.path().join("big.json");

        // Stream the bytes out so we don't allocate twice in memory.
        let mut f = std::fs::File::create(&big_path).expect("create");
        let chunk = vec![b'a'; 1024];
        let chunks_needed = ((6 * 1024 * 1024) / chunk.len()) + 1;
        for _ in 0..chunks_needed {
            f.write_all(&chunk).expect("write");
        }
        drop(f);

        let err = parse_file(&big_path, Format::Json).expect_err("must reject");
        match err {
            ParseError::SizeExceeded { size, cap } => {
                assert!(size > MAX_PARSE_SIZE);
                assert_eq!(cap, MAX_PARSE_SIZE);
            }
            other => panic!("expected SizeExceeded, got {other:?}"),
        }
    }

    #[test]
    fn parse_bytes_hash_and_size_populated() {
        // Sanity check that hash + size are populated regardless of
        // the source format. SHA-256 of `{}` is the well-known
        // "44136fa..." value.
        let parsed = parse_bytes(b"{}", Format::Json).expect("parses");
        assert_eq!(parsed.size, 2);
        assert_eq!(
            parsed.hash,
            "44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a"
        );
        assert_eq!(parsed.raw, b"{}");
    }

    #[test]
    fn parse_file_io_error_typed() {
        // Path that does not exist - must come back as `Io`, not as
        // a downstream format error.
        let dir = tempfile::tempdir().expect("tmp");
        let missing = dir.path().join("nope.json");
        let err = parse_file(&missing, Format::Json).expect_err("must error");
        assert!(matches!(err, ParseError::Io { .. }));
    }

    #[test]
    fn parse_bytes_unsupported_format_typed() {
        // JSONL / SQLite / Binary are out of scope for Phase 1.4 -
        // dispatching them must come back as `UnsupportedFormat`.
        for fmt in [Format::Jsonl, Format::Sqlite, Format::Binary] {
            let err = parse_bytes(b"anything", fmt).expect_err("must reject");
            match err {
                ParseError::UnsupportedFormat(got) => assert_eq!(got, fmt),
                other => panic!("expected UnsupportedFormat for {fmt:?}, got {other:?}"),
            }
        }
    }
}

#[cfg(test)]
mod proptests {
    //! Round-trip property tests.
    //!
    //! `serde_yaml` emits YAML in canonical form (sorted keys, fixed
    //! quoting, two-space indent, ...). It is *not* a re-formatter:
    //! parse-then-emit is generally not byte-identical with arbitrary
    //! human input. The property we *can* assert is **structural
    //! identity** for YAML we ourselves produced: emit -> parse -> emit
    //! -> parse must round-trip the value, and the JSON projection
    //! must match.
    //!
    //! Inputs are kept small and well-typed (string / int / bool keys
    //! and values) so the proptest shrinker can produce useful
    //! counterexamples without combinatorial explosion.
    //!
    //! Phase 5.1 raises the case count from 64 to 256 across the four
    //! round-trip properties (markdown frontmatter, JSON, TOML, YAML)
    //! so we exercise the full body of the proptest default rather
    //! than the minimum-viable count Phase 1.4 ran.

    use proptest::prelude::*;

    use super::{parse_bytes, MAX_PARSE_SIZE};
    use crate::parser::{json as json_parser, markdown, toml as toml_parser};
    use crate::registry::types::Format;

    /// Strategy: small map of `key: value` where values are scalar.
    ///
    /// Keys avoid YAML reserved characters (`:`, `-`, `?`, `#`, ...) so
    /// we don't drift into testing `serde_yaml`'s quoting rules instead
    /// of our own dispatcher.
    fn small_yaml_map() -> impl Strategy<Value = serde_json::Value> {
        let key = "[a-z][a-z0-9_]{0,7}";
        let val = prop_oneof![
            any::<bool>().prop_map(serde_json::Value::Bool),
            any::<i32>().prop_map(|i| serde_json::Value::Number(i.into())),
            "[a-z0-9_ ]{0,16}".prop_map(serde_json::Value::String),
        ];
        prop::collection::btree_map(key, val, 0..6).prop_map(|m| {
            let mut obj = serde_json::Map::new();
            for (k, v) in m {
                obj.insert(k, v);
            }
            serde_json::Value::Object(obj)
        })
    }

    /// Strategy: small map suitable for TOML emission.
    ///
    /// TOML rejects bare integers that don't fit in `i64` and forbids
    /// the empty string as a bare key. We constrain values to
    /// `(bool | i32 | non-empty ascii string)` to stay inside what the
    /// `toml` crate's serialiser accepts, which means we are testing
    /// *our* parser, not the upstream emitter's edge cases.
    fn small_toml_map() -> impl Strategy<Value = serde_json::Value> {
        let key = "[a-z][a-z0-9_]{0,7}";
        let val = prop_oneof![
            any::<bool>().prop_map(serde_json::Value::Bool),
            any::<i32>().prop_map(|i| serde_json::Value::Number(i.into())),
            // TOML strings can hold arbitrary printable ASCII; we keep
            // it tame so quoting doesn't introduce ambiguity.
            "[a-z0-9_ ]{1,16}".prop_map(serde_json::Value::String),
        ];
        prop::collection::btree_map(key, val, 1..6).prop_map(|m| {
            let mut obj = serde_json::Map::new();
            for (k, v) in m {
                obj.insert(k, v);
            }
            serde_json::Value::Object(obj)
        })
    }

    proptest! {
        // 256 cases matches the proptest default. Phase 1.4 used 64;
        // Phase 5.1 raises this to give the shrinker more room while
        // still finishing under a second per property on a modern CPU.
        #![proptest_config(ProptestConfig::with_cases(256))]

        /// `serde_yaml::to_string` -> `parse(Format::Yaml)` must yield
        /// a value structurally equal to the input.
        #[test]
        fn proptest_yaml_roundtrip(value in small_yaml_map()) {
            let yaml = serde_yaml::to_string(&value).expect("emit");
            // Cap the input we feed to the parser at the configured
            // size limit; the strategy can never exceed it but the
            // assertion documents the invariant.
            prop_assume!(yaml.len() as u64 <= MAX_PARSE_SIZE);

            let parsed = parse_bytes(yaml.as_bytes(), Format::Yaml).expect("parse");
            let got = parsed.structured.expect("yaml has structured");
            prop_assert_eq!(got, value);
        }

        /// Markdown frontmatter round-trip: build a file from a
        /// frontmatter dict + a body, parse it, assert the
        /// frontmatter is structurally identical and the body is
        /// preserved verbatim.
        ///
        /// Note: byte-identity of the *file* is not the property -
        /// `serde_yaml` may renormalise the frontmatter on emit. The
        /// property is that the round-trip preserves *content*.
        #[test]
        fn proptest_markdown_frontmatter_roundtrip(
            fm in small_yaml_map(),
            // Body is plain ASCII without frontmatter delimiters
            // so we don't accidentally embed a second frontmatter
            // block in the middle of the document.
            body in "[a-z0-9 \\n]{0,64}",
        ) {
            let yaml = serde_yaml::to_string(&fm).expect("emit");
            // Compose the file shape: opening `---`, the YAML
            // frontmatter, closing `---`, body.
            let file = format!("---\n{yaml}---\n{body}");

            let doc = markdown::parse(file.as_bytes()).expect("parse");
            let got_fm = doc.frontmatter.expect("frontmatter present");
            prop_assert_eq!(got_fm, fm);
            prop_assert_eq!(doc.body, body);
        }

        /// JSON parse -> re-serialise -> parse: the value must be
        /// equal across both parses. We hand the parser the textual
        /// form `serde_json` produces, parse, re-emit, and parse
        /// again; both parsed values must be equal to the original.
        ///
        /// Tests *our* JSON dispatcher, not `serde_json` itself: any
        /// regression that swaps key ordering or normalises numbers
        /// would surface here.
        #[test]
        fn proptest_json_roundtrip(value in small_yaml_map()) {
            let text = serde_json::to_string(&value).expect("emit");
            prop_assume!(text.len() as u64 <= MAX_PARSE_SIZE);

            let first = json_parser::parse(text.as_bytes()).expect("first parse");
            prop_assert_eq!(first.clone(), value.clone());

            let reemit = serde_json::to_string(&first).expect("re-emit");
            let second = json_parser::parse(reemit.as_bytes()).expect("second parse");
            prop_assert_eq!(second, value);
        }

        /// TOML parse -> JSON projection -> JSON parse: the projection
        /// must round-trip through the JSON parser. This exercises
        /// `toml_to_json` (the conversion function inside the TOML
        /// parser) by composing it with our JSON parser.
        ///
        /// Equivalent to: emit TOML -> our TOML parser -> JSON value
        /// -> serde_json emit -> our JSON parser -> structurally
        /// equal value.
        #[test]
        fn proptest_toml_json_projection_roundtrip(value in small_toml_map()) {
            // Emit as TOML. The `toml` crate's `to_string` rejects
            // arrays of tables at the top level (which we don't
            // generate) and other shapes our strategy excludes.
            // If the upstream emitter refuses (extremely rare for this
            // strategy), drop the case rather than fail - we're testing
            // our parser, not the emitter.
            let Ok(text) = ::toml::to_string(&value) else {
                return Ok(());
            };
            prop_assume!(text.len() as u64 <= MAX_PARSE_SIZE);

            let toml_parsed = toml_parser::parse(text.as_bytes()).expect("toml parse");
            // Project through JSON: emit, re-parse with our JSON
            // dispatcher, compare to the original.
            let projected = serde_json::to_string(&toml_parsed).expect("project to json");
            let json_parsed = json_parser::parse(projected.as_bytes()).expect("json parse");
            prop_assert_eq!(json_parsed, value);
        }
    }
}
