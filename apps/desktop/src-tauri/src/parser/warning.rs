//! Non-fatal parse warnings.
//!
//! A `ParseWarning` is the parser's way of saying "I parsed something but
//! you should know about it". These never fail the parse; they ride along
//! in `ParsedComponent::warnings` for the IPC layer to surface to the UI.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Discriminator for the kinds of warnings the parser layer emits.
///
/// Kept small and deliberately stable - each variant becomes a string
/// in the TS binding the UI may pattern-match on.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../bindings/parser/ParseWarningKind.ts")]
#[ts(rename_all = "camelCase")]
pub enum ParseWarningKind {
    /// A YAML input contained more than one `---`-separated document.
    /// We only parsed the first.
    MultipleYamlDocuments,

    /// A Markdown file opened with a `---` frontmatter delimiter on the
    /// first line but no closing `---` was found before EOF. We treated
    /// the whole file as Markdown body.
    UnclosedFrontmatter,

    /// File exceeded the parser size cap. Emitted by upstream callers
    /// when they choose to surface the cap as a warning rather than an
    /// error. The parser itself returns `ParseError::SizeExceeded`; this
    /// variant exists so the IPC layer can convert it.
    SizeExceeded,
}

/// A non-fatal warning produced while parsing a file.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../bindings/parser/ParseWarning.ts")]
#[ts(rename_all = "camelCase")]
pub struct ParseWarning {
    pub kind: ParseWarningKind,
    pub message: String,
    /// 1-based line number when known. `None` when the warning is
    /// document-wide rather than positional.
    pub line: Option<u32>,
}
