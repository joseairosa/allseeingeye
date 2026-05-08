//! Tool registry.
//!
//! Phase 1.1 fills this with declarative `ToolDescriptor` entries for
//! Claude Code, Codex, Cursor, and Antigravity, plus a runtime detection
//! probe that reports which tools are present on the local machine.

pub mod classify;
pub mod detect;
pub mod tools;
pub mod types;

use std::sync::OnceLock;

// Re-exports kept narrow on purpose: only the public API the rest of the
// crate calls through `registry::*`. Anything else lives behind
// `registry::types::*` so we don't accumulate dead `pub use` lines as
// later phases land.
pub use classify::classify_path;
pub use detect::detect_all;
pub use types::{DetectedTool, ToolDescriptor};

/// Borrow the static slice of `ToolDescriptor`s known at build time.
///
/// Initialised lazily via `OnceLock` so the descriptor allocations happen
/// once per process. The data is fixed for the lifetime of the binary.
#[must_use]
pub fn registry() -> &'static [ToolDescriptor] {
    static REGISTRY: OnceLock<Vec<ToolDescriptor>> = OnceLock::new();
    REGISTRY.get_or_init(tools::all_descriptors)
}
