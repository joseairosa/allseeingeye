//! Per-project actions: discovery + memory analysis + worktree audit
//! + docs reorganisation.
//!
//! Spec lives at `docs/17-projects-and-actions.md`. The module is
//! organised the same way `backup/` is - one file per concern, a
//! shared `types.rs` for cross-IPC payloads.
//!
//! All actions that mutate the filesystem run as **dry-run by
//! default**. The caller passes `dry_run: false` to actually apply.

pub mod analyze_memory;
pub mod discover;
pub mod types;

#[allow(unused_imports)]
pub use analyze_memory::{
    analyze_memory, AnalyzeError, MemoryAnalysisReport, MemoryRecommendation,
    MemoryRecommendationKind,
};
#[allow(unused_imports)]
pub use discover::list_projects;
#[allow(unused_imports)]
pub use types::{MemoryFileSummary, ProjectSummary};
