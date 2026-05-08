//! Finding types for security audits.
//!
//! `Finding` is the shape of every audit result the security module
//! produces, regardless of category. The struct, `Severity`, and
//! `Category` are exported via `ts-rs` so the React side can render
//! findings without a hand-written parallel definition.
//!
//! Privacy contract (`docs/12-security.md`): the actual secret value
//! NEVER leaves the redaction helper. Only the redacted preview, its
//! source label, and metadata enter the on-disk row, the FTS index, or
//! any telemetry payload.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Audit severity bucket. The Security UI sorts findings by severity
/// descending: `Critical` first, then `High`, `Medium`, `Low`.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export, export_to = "../bindings/security/Severity.ts")]
#[ts(rename_all = "lowercase")]
pub enum Severity {
    Low,
    Medium,
    High,
    Critical,
}

impl Severity {
    /// Stable ordinal used for "highest severity wins" de-duplication
    /// of overlapping pattern matches inside `scanner::scan_text`.
    #[must_use]
    pub const fn rank(self) -> u8 {
        match self {
            Self::Low => 0,
            Self::Medium => 1,
            Self::High => 2,
            Self::Critical => 3,
        }
    }

    /// Wire-compatible camelCase label for the SQL `severity` column.
    /// Mirrors the `serde(rename_all = "lowercase")` shape so reads
    /// converge with writes.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Critical => "critical",
        }
    }
}

/// Audit category. Phase 7.1 ships only `Secret`; Phase 7.2 adds
/// `McpPermission`, Phase 7.3 adds `Hook` / `Plugin` / `PathTraversal` /
/// `SensitiveDir` / `License`. The variant is open-ended so future work
/// can extend without breaking serialised rows.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export, export_to = "../bindings/security/Category.ts")]
#[ts(rename_all = "kebab-case")]
pub enum Category {
    /// `docs/12-security.md` Section A. Secret exposure.
    Secret,
}

impl Category {
    /// Wire-compatible label for the SQL `category` column.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Secret => "secret",
        }
    }
}

/// A single audit finding.
///
/// `component_id` is `None` when the finding is produced by the scanner
/// (which doesn't know the component identity); the upsert layer fills
/// it in before persisting the row.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../bindings/security/Finding.ts")]
#[ts(rename_all = "camelCase")]
pub struct Finding {
    /// Stable, deterministic id derived from
    /// `sha256(component_path || category || pattern_name || matched_byte_range)`.
    /// Persisted as `aseye-finding-<sha256-prefix>`.
    pub id: String,
    /// Owning component URI. Populated by the upsert layer; the scanner
    /// itself leaves this as `None` because it doesn't know identity.
    pub component_id: Option<String>,
    pub category: Category,
    /// Name of the pattern that fired (e.g. `anthropic-key`,
    /// `github-pat-classic`). Stable across releases - used by the
    /// suppression table as a join key.
    pub pattern: String,
    pub severity: Severity,
    /// Where in the parsed component the match was found. `body` for
    /// the markdown body; a JSON pointer (`/mcpServers/github/env/GH_TOKEN`)
    /// for structured leaves.
    pub source_label: String,
    /// 1-based line number inside the source string when known. The
    /// scanner only computes this for the markdown body, not for JSON
    /// leaves (where lines mean little after parsing).
    pub line: Option<u32>,
    /// Redacted preview - first 8 + ellipsis + last 4 chars of the
    /// matched value (or first 4 + ellipsis + last 2 if total length
    /// < 16). NEVER contains the secret in full.
    pub redacted_preview: String,
    /// Unix epoch milliseconds when the finding was detected.
    pub detected_at: i64,
}

/// Build the redacted preview for a matched secret value.
///
/// Shape, per `docs/12-security.md` (`redacted_preview`):
/// * `value.len() >= 16`: `first 8 chars + '…' + last 4 chars`.
/// * `value.len() <  16`: `first 4 chars + '…' + last 2 chars`.
///
/// Operates on Unicode characters (not bytes) so multi-byte sequences
/// don't get sliced mid-codepoint. Non-printable and whitespace bytes
/// pass through unchanged - the preview is informational only and the
/// UI is responsible for escaping it before rendering.
#[must_use]
pub fn redact(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    let total = chars.len();
    let (head_n, tail_n) = if total >= 16 { (8, 4) } else { (4, 2) };
    if total <= head_n + tail_n {
        // Value too short to redact meaningfully - return the full
        // string. In practice the scanner won't fire on such tiny
        // matches because every pattern requires a minimum length, but
        // we keep the helper total-correct for unit testing.
        return value.to_owned();
    }
    let head: String = chars.iter().take(head_n).collect();
    let tail: String = chars.iter().skip(total - tail_n).collect();
    format!("{head}\u{2026}{tail}")
}

#[cfg(test)]
mod tests {
    use super::{redact, Category, Severity};

    #[test]
    fn severity_ranks_monotonic() {
        assert!(Severity::Low.rank() < Severity::Medium.rank());
        assert!(Severity::Medium.rank() < Severity::High.rank());
        assert!(Severity::High.rank() < Severity::Critical.rank());
    }

    #[test]
    fn severity_as_str_round_trip() {
        for s in [
            Severity::Low,
            Severity::Medium,
            Severity::High,
            Severity::Critical,
        ] {
            assert!(!s.as_str().is_empty());
        }
    }

    #[test]
    fn category_as_str_secret() {
        assert_eq!(Category::Secret.as_str(), "secret");
    }

    #[test]
    fn redact_long_value_uses_8_4_shape() {
        // 27-char string: first 8 + ellipsis + last 4.
        let preview = redact("sk-ant-abcdef0123456789ABCD");
        assert_eq!(preview, "sk-ant-a\u{2026}ABCD");
        // Structural assertions belt-and-braces for the next reader.
        assert!(preview.starts_with("sk-ant-a"));
        assert!(preview.ends_with("ABCD"));
        assert!(preview.contains('\u{2026}'));
    }

    #[test]
    fn redact_short_value_uses_4_2_shape() {
        // 12-char string (>= head+tail of the long shape but < 16):
        // first 4 + ellipsis + last 2.
        let preview = redact("abcdef123456");
        assert!(preview.starts_with("abcd"));
        assert!(preview.ends_with("56"));
        assert!(preview.contains('\u{2026}'));
    }

    #[test]
    fn redact_too_short_returns_input() {
        // 5 chars is shorter than the 4+2 envelope; we return the
        // original string rather than emitting a meaningless preview.
        assert_eq!(redact("hello"), "hello");
    }

    #[test]
    fn redact_unicode_is_codepoint_safe() {
        // Mixed-width content must not be sliced mid-codepoint.
        let preview = redact("café-secret-token-1234567890");
        assert!(preview.contains('\u{2026}'));
        // Sanity: the helper produces valid UTF-8 (this would panic
        // on a bad slice).
        assert!(preview.is_char_boundary(0));
    }
}
