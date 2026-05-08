//! Error types for the `parser` module.
//!
//! `ParseError` is the unified error returned by every public function in
//! this module. Variants are deliberately granular so callers (and tests)
//! can distinguish *which* format / phase failed without having to parse
//! nested error messages.

use std::io;
use std::path::PathBuf;

use thiserror::Error;

use crate::registry::types::Format;

/// Errors emitted by the parser dispatch layer.
#[derive(Error, Debug)]
pub enum ParseError {
    /// Reading the file from disk failed (does not exist, permission
    /// denied, EIO, ...).
    #[error("failed to read `{path}`: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    /// File exceeded the 5 MB cap declared in `docs/05-data-architecture.md`.
    /// The IPC layer surfaces this as a parse warning to the UI rather
    /// than indexing partial content.
    #[error("file size {size} bytes exceeds parser cap {cap} bytes")]
    SizeExceeded {
        /// Actual size of the input in bytes.
        size: u64,
        /// Configured cap (5 MB at the moment).
        cap: u64,
    },

    /// Document was zero bytes (or otherwise structurally empty).
    /// JSON in particular rejects empty input outright.
    #[error("document is empty")]
    EmptyDocument,

    /// Bytes were not valid UTF-8 - relevant for Markdown / YAML / TOML
    /// where we need a textual representation to split frontmatter or
    /// serialise back.
    #[error("input is not valid UTF-8: {source}")]
    InvalidUtf8 {
        #[source]
        source: std::str::Utf8Error,
    },

    /// `serde_json` rejected the document. Carries line/column when
    /// available.
    #[error("invalid JSON at line {line}, column {column}: {message}")]
    Json {
        message: String,
        line: u32,
        column: u32,
    },

    /// `toml::de` rejected the document.
    #[error("invalid TOML: {message}")]
    Toml {
        message: String,
        /// 1-based line number when the TOML parser reported a span.
        line: Option<u32>,
    },

    /// `serde_yaml` rejected the document.
    #[error("invalid YAML: {message}")]
    Yaml {
        message: String,
        line: Option<u32>,
        column: Option<u32>,
    },

    /// Caller asked the parser dispatcher to handle a format that is
    /// out of scope for Phase 1.4 (`Jsonl`, `Sqlite`, `Binary`). The
    /// IPC layer (Phase 1.6) handles those via streaming readers
    /// (JSONL) and library-specific access (`rusqlite` read-only).
    /// Surfaced as a typed variant so callers can distinguish "not
    /// our content" from "malformed content".
    #[error("format {0:?} is not handled by the parser dispatcher")]
    UnsupportedFormat(Format),
}

impl ParseError {
    /// Build a `ParseError::Json` from a `serde_json::Error`, carrying its
    /// 1-based line / column when present.
    pub(crate) fn from_json(err: &serde_json::Error) -> Self {
        Self::Json {
            message: err.to_string(),
            // `serde_json` exposes 1-based line / column via the public
            // `line()` / `column()` accessors; both return `0` when no
            // location is available, which we forward verbatim.
            line: u32::try_from(err.line()).unwrap_or(0),
            column: u32::try_from(err.column()).unwrap_or(0),
        }
    }

    /// Build a `ParseError::Toml` from a `toml::de::Error`.
    pub(crate) fn from_toml(err: &toml::de::Error) -> Self {
        // toml's `span()` is byte-offset based and not always present.
        // Extracting a line number requires re-walking the input; we
        // surface the raw message which already names line/column for
        // the human reader.
        Self::Toml {
            message: err.to_string(),
            line: None,
        }
    }

    /// Build a `ParseError::Yaml` from a `serde_yaml::Error`.
    pub(crate) fn from_yaml(err: &serde_yaml::Error) -> Self {
        let location = err.location();
        Self::Yaml {
            message: err.to_string(),
            line: location
                .as_ref()
                .map(serde_yaml::Location::line)
                .and_then(|n| u32::try_from(n).ok()),
            column: location
                .as_ref()
                .map(serde_yaml::Location::column)
                .and_then(|n| u32::try_from(n).ok()),
        }
    }
}

/// Convenience alias for module-internal `Result`s.
pub type Result<T> = std::result::Result<T, ParseError>;
