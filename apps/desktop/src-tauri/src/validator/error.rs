//! Errors raised by the validator module.
//!
//! Phase 3.2 - validation itself never fails; an unparseable schema is a
//! programmer-error trapped at compile time (the test harness asserts
//! every bundled schema compiles). The error type exists for the IPC
//! command path that re-runs validation by component id, where `SQLite`
//! lookups, missing rows, or malformed cached `parsed_json` need a
//! discriminable shape rather than `String`.

use thiserror::Error;

/// Errors returned by the validator module's public surface.
#[derive(Debug, Error)]
pub enum ValidatorError {
    /// `SQLite` lookup failed (read pool error, connection drop, ...).
    /// Surfaces from the IPC command that loads the parsed JSON cached
    /// in `component.parsed_json` for re-validation.
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    /// Index-layer error (pool, migration, IO). Surfaces from
    /// [`crate::index::IndexHandle::read`] in the by-id IPC path.
    /// Wrapping the typed enum lets the IPC layer propagate the
    /// original cause string rather than collapse it into "sqlite
    /// error".
    #[error("index error: {0}")]
    Index(#[from] crate::index::IndexError),

    /// The component id supplied to [`crate::validator::validate_by_id`]
    /// has no row in the index. Surfaced separately from `Sqlite` so the
    /// IPC layer can map it to a 404-equivalent UI state.
    #[error("component not found: {0}")]
    NotFound(String),

    /// `parsed_json` for a component was not valid JSON. Surfaces only
    /// when the upsert layer wrote bad cached JSON, which would itself
    /// be a programmer error. Carries the underlying `serde_json` cause.
    #[error("invalid cached parsed_json: {0}")]
    InvalidCachedJson(#[from] serde_json::Error),

    /// The component row exists but has a `tool` or `type` value the
    /// validator cannot map to its enums. Indicates the index is from
    /// a future schema version; we surface the raw values for the IPC
    /// layer to log.
    #[error(
        "unknown tool or component type for component {id}: tool={tool}, type={component_type}"
    )]
    UnknownComponentClassification {
        id: String,
        tool: String,
        component_type: String,
    },
}

/// Convenience alias for module-internal `Result`s.
pub type Result<T> = std::result::Result<T, ValidatorError>;
