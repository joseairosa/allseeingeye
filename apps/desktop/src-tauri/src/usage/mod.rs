//! Token usage analytics (Phase 14C - backend half).
//!
//! Reads Claude Code and Codex JSONL session transcripts, folds
//! assistant turns into per-day rollups in `token_usage`, and offers
//! cost-reduction recommendations.
//!
//! Public surface:
//! - [`refresh`] - run a single aggregation pass against `~/.claude`
//!   and `~/.codex`. Idempotent.
//! - [`query::*`] - read-side queries that back the IPC `usage_query`
//!   command (summary / by project / by day / recommendations).
//! - [`pricing::PRICE_TABLE_VERSION`] - version stamp for the price
//!   table the UI footnotes.
//!
//! See `docs/14-cost-and-memory.md` section 14C for the data-source
//! contract and recommendation heuristics.

pub mod aggregate;
pub mod claude_code;
pub mod codex;
pub mod cwd_decode;
pub mod pricing;
pub mod query;
pub mod recommend;
pub mod types;

pub use aggregate::{refresh_from_home, RefreshError, RefreshOutcome};
// Re-exports below form the module's public API. Some are used only
// transitively through the IPC dispatch layer or the ts-rs bindings,
// hence the `#[allow(unused_imports)]` to keep the surface explicit
// without nagging on internal-only consumers.
#[allow(unused_imports)]
pub use pricing::{estimate_cost_usd, lookup_price, PriceLookup, PRICE_TABLE_VERSION};
#[allow(unused_imports)]
pub use query::{
    by_day, by_project, summary, ByDayRow, ByProjectRow, CostQuery, CostResponse, ProjectSummary,
    SummaryResponse,
};
#[allow(unused_imports)]
pub use recommend::{recommend, CostRec, CostRecKind};
#[allow(unused_imports)]
pub use types::{TokenTotals, ToolKind};

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::index::IndexHandle;

/// Run a refresh pass against the user's home directory (resolved via
/// `dirs::home_dir`). Returns the refresh outcome.
///
/// Errors if the home dir cannot be resolved or the database write
/// fails. Per-file IO errors are absorbed (the aggregator skips and
/// moves on).
pub fn refresh(index: &IndexHandle) -> Result<RefreshOutcome, RefreshError> {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    refresh_from_home(index, &home)
}

/// Unix epoch seconds. Centralised so callers don't repeat the
/// boilerplate.
#[must_use]
pub fn unix_now_secs() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| {
        #[allow(clippy::cast_possible_wrap)]
        let s = d.as_secs() as i64;
        s
    })
}
