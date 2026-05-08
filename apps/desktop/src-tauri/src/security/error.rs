//! Errors raised by the security module.
//!
//! Phase 7.1 - the secret-detection engine never propagates a hard
//! failure to its callers (parsing a malformed value yields zero
//! findings, never panics). The error type exists for the persistence
//! layer (writing `security_finding` rows during upsert) where a SQL
//! failure must surface to the caller.
//!
//! Variants are kept narrow and named after the failure they describe so
//! upsert callers can map them to user-facing messages without
//! inspecting nested causes.

/// Errors returned by the security module's public surface.
#[derive(Debug, thiserror::Error)]
pub enum SecurityError {
    /// Underlying rusqlite error - covers IO, busy, constraint, etc.
    /// Surfaces when persisting findings into `security_finding` fails.
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    /// Internal serialisation / corruption error. Used when an evidence
    /// blob fails to round-trip through `serde_json` (only the
    /// persistence layer touches the JSON, so a serialisation failure
    /// always points at programmer error rather than user input).
    #[error("internal security error: {0}")]
    Internal(String),
}

/// Convenience alias for module-internal `Result`s.
pub type Result<T> = std::result::Result<T, SecurityError>;
