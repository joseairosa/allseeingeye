//! Markdown + YAML frontmatter parser.
//!
//! Recognises the de-facto frontmatter convention shared by Claude Code,
//! Cursor (`.mdc`), Antigravity, Windsurf, Copilot, and others (per
//! `docs/04-data-sources.md`):
//!
//! ```text
//! ---
//! description: Build, debug, and optimise.
//! globs: ["src/**/*.rs"]
//! ---
//! body content here, parsed as opaque Markdown
//! ```
//!
//! Rules (kept deliberately strict - matches the spec used by `gray-matter`,
//! `front-matter` and the various JS implementations):
//! * The opening `---` MUST be the first line of the document. No leading
//!   whitespace, no BOM. (BOM stripping is the parser's job in upstream
//!   tools; we follow suit.)
//! * The closing `---` MUST appear on its own line.
//! * If the closing delimiter is missing, we emit
//!   `ParseWarningKind::UnclosedFrontmatter` and treat the entire file
//!   as Markdown body. We never silently truncate.

use crate::parser::error::{ParseError, Result};
use crate::parser::warning::{ParseWarning, ParseWarningKind};
use crate::parser::yaml;

/// Result of parsing a Markdown document.
///
/// `frontmatter` is `None` when the document has no `---` block at the
/// top. `body` is always present (may be empty if the file is purely
/// frontmatter + closing delimiter).
pub struct MarkdownDoc {
    pub frontmatter: Option<serde_json::Value>,
    pub body: String,
    /// 1-based line number of the closing `---`, when one was found.
    /// `None` when there was no frontmatter or it was unclosed.
    ///
    /// Surfaced for the future schema-aware editor (Phase 2.x) which
    /// needs the offset to highlight frontmatter / body regions in the
    /// raw view. `dead_code` lint allowed at the field level because
    /// no in-crate consumer reads it yet.
    #[allow(
        dead_code,
        reason = "exposed for the schema-aware editor in a later phase"
    )]
    pub frontmatter_end_line: Option<u32>,
    /// Non-fatal warnings (e.g. unclosed frontmatter) surfaced to the
    /// caller for inclusion in `ParsedComponent::warnings`.
    pub warnings: Vec<ParseWarning>,
}

/// Parse `content` as Markdown with optional YAML frontmatter.
pub fn parse(content: &[u8]) -> Result<MarkdownDoc> {
    let text = std::str::from_utf8(content).map_err(|source| ParseError::InvalidUtf8 { source })?;

    // Empty file is a valid Markdown doc with no frontmatter and no body.
    // We do *not* return `EmptyDocument` here because Markdown is
    // semantically text and an empty body is meaningful.
    if text.is_empty() {
        return Ok(MarkdownDoc {
            frontmatter: None,
            body: String::new(),
            frontmatter_end_line: None,
            warnings: Vec::new(),
        });
    }

    // Frontmatter must start with `---` on the first line. Anything
    // else is treated as plain Markdown.
    if let Some(frontmatter_block) = find_frontmatter_block(text) {
        // Parse the YAML block; any error there is fatal - we'd
        // rather refuse to index a half-broken skill than silently
        // drop the metadata.
        let outcome = yaml::parse_with_origin(frontmatter_block.yaml.as_bytes(), "frontmatter")?;
        return Ok(MarkdownDoc {
            frontmatter: Some(outcome.value),
            body: frontmatter_block.body,
            frontmatter_end_line: Some(frontmatter_block.end_line),
            warnings: outcome.warnings,
        });
    }

    // Either no frontmatter, or an unclosed frontmatter we treated
    // as plain body. Detect the unclosed case so we can warn.
    let mut warnings = Vec::new();
    if starts_with_frontmatter_delimiter(text) {
        warnings.push(ParseWarning {
            kind: ParseWarningKind::UnclosedFrontmatter,
            message: "document opens with `---` but no closing delimiter was found".to_owned(),
            line: Some(1),
        });
    }

    Ok(MarkdownDoc {
        frontmatter: None,
        body: text.to_owned(),
        frontmatter_end_line: None,
        warnings,
    })
}

/// Internal representation of a frontmatter block extracted from the
/// document.
struct FrontmatterBlock {
    /// YAML body (everything between the two `---` delimiters).
    yaml: String,
    /// Markdown body (everything after the closing delimiter, with the
    /// trailing newline preserved verbatim).
    body: String,
    /// 1-based line number of the closing delimiter.
    end_line: u32,
}

/// Return `true` when `text` opens with a frontmatter delimiter line.
///
/// The delimiter is a line whose only contents are `---`. Trailing
/// whitespace is allowed (matches what `gray-matter` accepts).
fn starts_with_frontmatter_delimiter(text: &str) -> bool {
    // First "line" runs from start to the first `\n` (or EOF).
    let first_line_end = text.find('\n').unwrap_or(text.len());
    let first_line = text[..first_line_end].trim_end_matches('\r');
    first_line.trim_end() == "---"
}

/// Locate and split off a `---`-delimited YAML frontmatter block.
///
/// Returns `None` when the document does not open with `---` or the
/// closing delimiter is missing. The body retains its original trailing
/// content verbatim - we do *not* trim leading whitespace or newlines.
fn find_frontmatter_block(text: &str) -> Option<FrontmatterBlock> {
    if !starts_with_frontmatter_delimiter(text) {
        return None;
    }

    // Skip the opening `---` line (and its `\n`, if present).
    let after_open = match text.find('\n') {
        Some(idx) => &text[idx + 1..],
        // Document is literally just `---` with no newline. No
        // frontmatter we can extract.
        None => return None,
    };

    // Walk lines from `after_open` until we find a `---` on its own.
    // We track byte offsets relative to `text` so the caller can
    // reconstruct line numbers and slice the body cleanly.
    let opening_line_count = 1u32; // the opening `---` line itself
    let mut offset_in_after = 0usize;
    let mut line_count = opening_line_count;

    while offset_in_after < after_open.len() {
        let remaining = &after_open[offset_in_after..];
        let line_end = remaining.find('\n').map_or(remaining.len(), |i| i);
        let line_with_cr = &remaining[..line_end];
        let line = line_with_cr.trim_end_matches('\r');
        line_count += 1;

        if line.trim_end() == "---" {
            // Found the closing delimiter. The YAML payload is the
            // slice from the start of `after_open` to the start of
            // this line.
            let yaml = after_open[..offset_in_after].to_owned();

            // Body starts right after the newline that terminates
            // this `---` line. If the closing line was the last
            // line of the file with no trailing newline, body is
            // empty.
            let consume_to = offset_in_after + line_end + usize::from(line_end < remaining.len());
            let body = after_open[consume_to..].to_owned();

            return Some(FrontmatterBlock {
                yaml,
                body,
                end_line: line_count,
            });
        }

        // Move past this line + its trailing `\n` (if present).
        offset_in_after += line_end + usize::from(line_end < remaining.len());
    }

    // Reached EOF without finding the closing `---`. The caller will
    // emit `UnclosedFrontmatter` and treat the whole file as body.
    None
}

#[cfg(test)]
mod tests {
    use super::parse;
    use crate::parser::warning::ParseWarningKind;

    #[test]
    fn markdown_no_frontmatter() {
        let src = b"# Hello\n\nthis is markdown.\n";
        let doc = parse(src).expect("must parse");
        assert!(doc.frontmatter.is_none());
        assert_eq!(doc.body, "# Hello\n\nthis is markdown.\n");
        assert!(doc.warnings.is_empty());
        assert!(doc.frontmatter_end_line.is_none());
    }

    #[test]
    fn markdown_with_frontmatter() {
        let src = b"---\nname: spec\ndescription: TDD runner\n---\n# Body\n\nstuff\n";
        let doc = parse(src).expect("must parse");
        let fm = doc.frontmatter.expect("frontmatter must be present");
        assert_eq!(fm["name"], "spec");
        assert_eq!(fm["description"], "TDD runner");
        assert_eq!(doc.body, "# Body\n\nstuff\n");
        assert_eq!(doc.frontmatter_end_line, Some(4));
        assert!(doc.warnings.is_empty());
    }

    #[test]
    fn markdown_with_unclosed_frontmatter() {
        // No closing `---`. We emit a warning and treat the file as
        // plain body verbatim. (The file is still well-formed; we
        // don't want to lose content.)
        let src = b"---\nname: spec\nstuff that is not closed\n";
        let doc = parse(src).expect("must parse");
        assert!(doc.frontmatter.is_none());
        assert_eq!(doc.body, "---\nname: spec\nstuff that is not closed\n");
        assert_eq!(doc.warnings.len(), 1);
        assert!(matches!(
            doc.warnings[0].kind,
            ParseWarningKind::UnclosedFrontmatter
        ));
    }

    #[test]
    fn markdown_empty_body_after_frontmatter() {
        // Frontmatter with no body afterwards must still parse cleanly.
        let src = b"---\nname: spec\n---\n";
        let doc = parse(src).expect("must parse");
        let fm = doc.frontmatter.expect("frontmatter must be present");
        assert_eq!(fm["name"], "spec");
        assert!(doc.body.is_empty());
    }

    #[test]
    fn markdown_crlf_frontmatter() {
        // Editors on Windows emit CRLF endings. Frontmatter must still
        // open and close correctly.
        let src = b"---\r\nname: spec\r\n---\r\nbody\r\n";
        let doc = parse(src).expect("must parse");
        let fm = doc.frontmatter.expect("frontmatter must be present");
        assert_eq!(fm["name"], "spec");
        assert_eq!(doc.body, "body\r\n");
    }

    #[test]
    fn markdown_empty_file() {
        // Empty body, no frontmatter; not an error (Markdown can be
        // empty - skill bodies in particular are sometimes pure
        // frontmatter pointing at scripts).
        let doc = parse(b"").expect("must parse");
        assert!(doc.frontmatter.is_none());
        assert_eq!(doc.body, "");
        assert!(doc.warnings.is_empty());
    }

    #[test]
    fn markdown_three_dashes_inside_body_is_not_closing() {
        // A `---` mid-body must not be treated as the closing
        // delimiter when the document had no opening `---`.
        let src = b"# Title\n\n---\n\nrest\n";
        let doc = parse(src).expect("must parse");
        assert!(doc.frontmatter.is_none());
        assert_eq!(doc.body, "# Title\n\n---\n\nrest\n");
    }
}
