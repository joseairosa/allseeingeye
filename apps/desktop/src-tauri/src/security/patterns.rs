//! Curated regex patterns for the secret-detection engine.
//!
//! Phase 7.1 - mirrors `docs/12-security.md` Section A. Secret exposure.
//! Each pattern is keyed by a stable `name` (used as the join key in the
//! `security_finding_suppression` table) plus a `Severity` and an
//! `evidence_extractor` that tells the scanner which bytes inside the
//! match represent the secret value (so the redacted preview reflects
//! the actual credential, not the surrounding `password = ...`
//! boilerplate).
//!
//! Compilation strategy:
//! * Patterns are stored in a `&'static [PatternDef]` table.
//! * On first use, `regex_set()` compiles every regex into a single
//!   `RegexSet` (one DFA pass over the input - cheap to scan).
//! * For each `RegexSet` hit, `regexes()` returns the matching `Regex`
//!   so the scanner can call `find_iter` and extract spans.
//! * Both lookups are cached in `OnceLock`, so compilation happens once
//!   per process. The scanner never recompiles.
//!
//! All patterns were tightened from the docs/12 table so they don't
//! match obvious placeholders (`<...>`, `${...}`, `*****`, repeated
//! `x`s). The placeholder filter lives in the scanner, not here, so
//! pattern definitions stay readable and shareable across categories.

use std::sync::OnceLock;

use regex::{Regex, RegexSet};

use super::finding::{Category, Severity};

/// How the scanner extracts the *value* portion from a match.
///
/// Most patterns match the whole secret and we use [`Self::Whole`].
/// For patterns shaped like `password = "<value>"` we want the
/// redacted preview to reflect the value alone, so the regex provides
/// a single named group `value` and the scanner extracts it via
/// [`Self::CapturedValue`].
#[derive(Debug, Clone, Copy)]
pub enum EvidenceExtractor {
    /// The match itself is the secret value.
    Whole,
    /// The match contains a `value` named capture group; that group is
    /// the secret value while the surrounding text is `key = ...`
    /// boilerplate the redaction helper should not preview.
    CapturedValue,
}

/// One detection rule.
///
/// `name` is stable across releases (used by suppression rows and
/// finding ids). `pattern` is the source regex string. We compile from
/// the source string instead of building `Regex` directly so the
/// `RegexSet` and the per-pattern `Regex` pool both come from the same
/// authoritative input - no chance of a typo on one side that the other
/// silently masks.
#[derive(Debug, Clone, Copy)]
pub struct PatternDef {
    pub name: &'static str,
    pub pattern: &'static str,
    pub severity: Severity,
    pub category: Category,
    pub evidence: EvidenceExtractor,
}

/// The curated rule set.
///
/// Order matters for the "highest severity wins on overlapping spans"
/// de-duplication: the scanner uses the rank from `Severity`, not array
/// position, but listing specific patterns before generic ones keeps
/// the table itself easy to audit.
pub const PATTERNS: &[PatternDef] = &[
    // ----- Vendor-specific API keys (critical) -----
    PatternDef {
        name: "anthropic-key",
        pattern: r"sk-ant-[a-zA-Z0-9_-]{40,}",
        severity: Severity::Critical,
        category: Category::Secret,
        evidence: EvidenceExtractor::Whole,
    },
    PatternDef {
        name: "openai-project-key",
        // OpenAI project keys carry the `sk-proj-` prefix and a longer
        // body. Listed before the legacy `sk-` rule below so the more
        // specific match wins on de-dup. We require a minimum body
        // length so this doesn't fire on `sk-proj-` literal text.
        pattern: r"sk-proj-[a-zA-Z0-9_-]{20,}",
        severity: Severity::Critical,
        category: Category::Secret,
        evidence: EvidenceExtractor::Whole,
    },
    PatternDef {
        name: "openai-key",
        // Negative lookahead for `ant-` and `proj-` would be cleaner
        // but the `regex` crate doesn't support lookaround. Instead we
        // rely on the de-duplicator to drop this generic match in
        // favour of the more specific anthropic / project matches when
        // both fire on the same byte range.
        pattern: r"sk-[a-zA-Z0-9]{20,}",
        severity: Severity::Critical,
        category: Category::Secret,
        evidence: EvidenceExtractor::Whole,
    },
    // ----- GitHub tokens (critical) -----
    PatternDef {
        name: "github-pat-classic",
        pattern: r"ghp_[A-Za-z0-9]{36}",
        severity: Severity::Critical,
        category: Category::Secret,
        evidence: EvidenceExtractor::Whole,
    },
    PatternDef {
        name: "github-pat-fine-grained",
        pattern: r"github_pat_[A-Za-z0-9_]{82}",
        severity: Severity::Critical,
        category: Category::Secret,
        evidence: EvidenceExtractor::Whole,
    },
    PatternDef {
        name: "github-oauth",
        pattern: r"gho_[A-Za-z0-9]{36}",
        severity: Severity::Critical,
        category: Category::Secret,
        evidence: EvidenceExtractor::Whole,
    },
    // ----- Slack tokens (critical) -----
    PatternDef {
        name: "slack-token",
        // `xox[baprs]-` covers bot, app, admin, personal, service, and
        // refresh prefixes per Slack's docs.
        pattern: r"xox[baprs]-[A-Za-z0-9-]{10,}",
        severity: Severity::Critical,
        category: Category::Secret,
        evidence: EvidenceExtractor::Whole,
    },
    // ----- AWS credentials (critical) -----
    PatternDef {
        name: "aws-access-key-id",
        pattern: r"AKIA[0-9A-Z]{16}",
        severity: Severity::Critical,
        category: Category::Secret,
        evidence: EvidenceExtractor::Whole,
    },
    PatternDef {
        name: "aws-secret-access-key",
        // Captures the value alone via the `value` group so the
        // redacted preview reflects the credential, not
        // `aws_secret_access_key = "..."`.
        pattern: r#"(?i)aws_secret_access_key\s*[:=]\s*["']?(?P<value>[A-Za-z0-9/+=]{40})["']?"#,
        severity: Severity::Critical,
        category: Category::Secret,
        evidence: EvidenceExtractor::CapturedValue,
    },
    // ----- JWT bearer (high) -----
    PatternDef {
        name: "jwt",
        pattern: r"eyJ[A-Za-z0-9_-]+\.eyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+",
        severity: Severity::High,
        category: Category::Secret,
        evidence: EvidenceExtractor::Whole,
    },
    // ----- Private key blocks (critical) -----
    PatternDef {
        name: "private-key-block",
        pattern: r"-----BEGIN (?:RSA |EC |DSA |OPENSSH |)PRIVATE KEY-----",
        severity: Severity::Critical,
        category: Category::Secret,
        evidence: EvidenceExtractor::Whole,
    },
    // ----- Authorization header (high) -----
    PatternDef {
        name: "authorization-bearer",
        pattern: r"(?i)Authorization:\s*Bearer\s+(?P<value>[A-Za-z0-9._-]{20,})",
        severity: Severity::High,
        category: Category::Secret,
        evidence: EvidenceExtractor::CapturedValue,
    },
    // ----- Bare password assignment (high) -----
    //
    // The placeholder filter (env var refs, angle-bracket placeholders,
    // literal `null`/`undefined`/`example`) lives in the scanner, not
    // in the regex itself - keeping the regex readable.
    PatternDef {
        name: "bare-password",
        pattern: r#"(?i)password\s*[:=]\s*["']?(?P<value>[^\s"']{8,})"#,
        severity: Severity::High,
        category: Category::Secret,
        evidence: EvidenceExtractor::CapturedValue,
    },
    // ----- Generic secret-shaped value (medium) -----
    //
    // Last in the table because it is the catch-all - if a more
    // specific pattern (anthropic, openai, github, ...) fires on the
    // same span, the de-duplicator keeps the higher-severity finding.
    PatternDef {
        name: "generic-secret-assignment",
        pattern: r#"(?i)(?:api[_-]?key|secret|token)\s*[:=]\s*["']?(?P<value>[A-Za-z0-9_-]{16,})["']?"#,
        severity: Severity::Medium,
        category: Category::Secret,
        evidence: EvidenceExtractor::CapturedValue,
    },
];

/// Compile-once cache for the `RegexSet` first-pass scanner.
fn regex_set() -> &'static RegexSet {
    static SET: OnceLock<RegexSet> = OnceLock::new();
    SET.get_or_init(|| {
        let sources: Vec<&str> = PATTERNS.iter().map(|p| p.pattern).collect();
        // `expect` here is the right call: a malformed pattern is a
        // build-time bug in this file, not a runtime condition. The
        // unit test `patterns_compile` exercises this path on every
        // CI run so a bad regex fails the suite, not the desktop app.
        RegexSet::new(&sources).expect("PATTERNS contain a malformed regex")
    })
}

/// Compile-once cache for the per-pattern `Regex`es. Indexed by the
/// same position as `PATTERNS` and `regex_set()`.
fn regexes() -> &'static [Regex] {
    static REGEXES: OnceLock<Vec<Regex>> = OnceLock::new();
    REGEXES.get_or_init(|| {
        PATTERNS
            .iter()
            .map(|p| Regex::new(p.pattern).expect("PATTERNS contain a malformed regex"))
            .collect()
    })
}

/// Returns the indexes of patterns whose regex matches `input` at least
/// once. Cheap first-pass filter the scanner uses to skip the
/// per-pattern `find_iter` walk when the input is clean.
pub fn quick_scan(input: &str) -> Vec<usize> {
    regex_set().matches(input).into_iter().collect()
}

/// Returns the compiled regex for `pattern_index`. Indexes are stable
/// because [`PATTERNS`] is `&'static`.
#[must_use]
pub fn regex_for(pattern_index: usize) -> &'static Regex {
    &regexes()[pattern_index]
}

/// Returns the pattern definition for `pattern_index`. Indexes are
/// stable because [`PATTERNS`] is `&'static`.
#[must_use]
pub fn def_for(pattern_index: usize) -> &'static PatternDef {
    &PATTERNS[pattern_index]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patterns_compile() {
        // Touch both caches so a malformed regex fails the test
        // instead of hiding until first runtime use.
        assert!(!regex_set().is_empty());
        assert_eq!(regexes().len(), PATTERNS.len());
    }

    #[test]
    fn pattern_names_are_unique() {
        // Names are the join key for the suppression table - duplicates
        // would silently merge unrelated findings. Catch this at test
        // time rather than letting a Phase 7.x patch trip over it.
        let mut names: Vec<&str> = PATTERNS.iter().map(|p| p.name).collect();
        names.sort_unstable();
        let before = names.len();
        names.dedup();
        assert_eq!(before, names.len(), "duplicate pattern name(s) in PATTERNS");
    }

    #[test]
    fn quick_scan_finds_anthropic_key() {
        let hits = quick_scan("token = sk-ant-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        // At least one pattern (anthropic-key) must match.
        assert!(!hits.is_empty());
        // The anthropic-key index must be among the hits.
        let anthropic_idx = PATTERNS
            .iter()
            .position(|p| p.name == "anthropic-key")
            .unwrap();
        assert!(hits.contains(&anthropic_idx));
    }

    #[test]
    fn quick_scan_clean_input_yields_no_hits() {
        assert!(quick_scan("just some prose without any secrets").is_empty());
    }
}
