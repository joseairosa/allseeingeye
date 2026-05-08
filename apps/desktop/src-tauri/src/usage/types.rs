//! Shared types for the token usage analytics module.
//!
//! Splits IPC wire types from internal types. Wire types derive `TS`
//! and live next to the IPC commands via the `usage_query` /
//! `usage_refresh` handlers. Internal types stay private to the
//! module.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Which tool produced a usage row. The serialised string lands in
/// the `token_usage.tool` column verbatim.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export, export_to = "../bindings/usage/UsageToolKind.ts")]
#[ts(rename_all = "kebab-case")]
pub enum ToolKind {
    ClaudeCode,
    Codex,
}

impl ToolKind {
    /// Database-side string representation. Stable.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            ToolKind::ClaudeCode => "claude-code",
            ToolKind::Codex => "codex",
        }
    }
}

/// One assistant turn, post-parse, pre-aggregation.
///
/// The aggregator folds turns into `token_usage` rows by
/// `(tool, project_path, model, day)`.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct TokenTurn {
    pub tool: ToolKind,
    pub project_path: String,
    pub model: String,
    pub day: String,
    pub session_id: String,
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_create: u64,
}

/// Folded token totals for an arbitrary slice (one row, one project,
/// one day, ...). Carries the four usage buckets the UI displays.
#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../bindings/usage/TokenTotals.ts")]
#[ts(rename_all = "camelCase")]
pub struct TokenTotals {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_create: u64,
}

impl TokenTotals {
    /// Sum of all four buckets. Useful for "5.4M tokens / 30d" KPI.
    /// Currently only exercised by tests; the IPC layer will surface
    /// this in the Cost view header once 14C-frontend lands.
    #[allow(dead_code)]
    #[must_use]
    pub fn total(&self) -> u64 {
        self.input
            .saturating_add(self.output)
            .saturating_add(self.cache_read)
            .saturating_add(self.cache_create)
    }
}

/// Convert an ISO 8601 timestamp string to a `YYYY-MM-DD` day in UTC.
///
/// Accepts the formats Claude Code and Codex emit:
/// - `"2026-05-08T01:47:38.686Z"` (Claude)
/// - `"2026-03-30T15:37:30.977Z"` (Codex)
///
/// Returns `None` if the string is shorter than 10 characters or the
/// first 10 characters do not look like a date.
///
/// We intentionally do NOT pull in `chrono` for this; the format is
/// fixed and the first 10 chars are already ISO date in UTC. This
/// keeps the dependency surface tight.
#[must_use]
pub fn day_from_iso8601(ts: &str) -> Option<String> {
    if ts.len() < 10 {
        return None;
    }
    let bytes = ts.as_bytes();
    // `YYYY-MM-DD` shape sanity check: digits at 0-3, 5-6, 8-9; dashes
    // at 4 and 7.
    let is_digit = |i: usize| bytes[i].is_ascii_digit();
    let is_dash = |i: usize| bytes[i] == b'-';
    if !is_digit(0)
        || !is_digit(1)
        || !is_digit(2)
        || !is_digit(3)
        || !is_dash(4)
        || !is_digit(5)
        || !is_digit(6)
        || !is_dash(7)
        || !is_digit(8)
        || !is_digit(9)
    {
        return None;
    }
    Some(ts[..10].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn day_from_iso_typical() {
        assert_eq!(
            day_from_iso8601("2026-05-08T01:47:38.686Z").as_deref(),
            Some("2026-05-08")
        );
    }

    #[test]
    fn day_from_iso_codex() {
        assert_eq!(
            day_from_iso8601("2026-03-30T15:37:30.977Z").as_deref(),
            Some("2026-03-30")
        );
    }

    #[test]
    fn day_from_iso_short_input() {
        assert!(day_from_iso8601("2026-05").is_none());
        assert!(day_from_iso8601("").is_none());
    }

    #[test]
    fn day_from_iso_malformed() {
        assert!(day_from_iso8601("not-a-real-stamp").is_none());
    }

    #[test]
    fn token_totals_sum() {
        let t = TokenTotals {
            input: 1,
            output: 2,
            cache_read: 4,
            cache_create: 8,
        };
        assert_eq!(t.total(), 15);
    }

    #[test]
    fn tool_kind_strings() {
        assert_eq!(ToolKind::ClaudeCode.as_str(), "claude-code");
        assert_eq!(ToolKind::Codex.as_str(), "codex");
    }
}
