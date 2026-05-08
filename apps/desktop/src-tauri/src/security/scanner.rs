//! Secret-scanning engine.
//!
//! Phase 7.1 - takes a `ParsedComponent` (parser output) or any text +
//! source label, runs the curated pattern set from [`super::patterns`],
//! and produces a vector of [`Finding`]s.
//!
//! Walking strategy:
//! * The markdown body (if any) is scanned as a single string with
//!   `source_label = "body"`. Line numbers are computed from byte
//!   offsets so the UI can hop the user to the right line.
//! * The structured value (if any) is walked recursively, depth-first.
//!   Each string leaf is scanned with `source_label` set to the JSON
//!   pointer to that leaf (`/mcpServers/github/env/GH_TOKEN`).
//!
//! Behaviour contracts:
//! * The scanner never panics. A malformed pattern bubbles up at
//!   compilation time (caught by the test in [`super::patterns`]),
//!   never at scan time.
//! * Bad UTF-8 surfaces as zero findings for the affected field with a
//!   debug log - the parser already rejects non-UTF-8 input so this is
//!   defensive only.
//! * Repeated scans of the same input produce identical finding ids
//!   (deterministic SHA-256 over `path || category || pattern_name ||
//!   range`).
//! * Placeholders (`${...}`, `<value>`, `*****`, literal
//!   `null`/`undefined`/`example`) never produce a finding.
//! * When two patterns match overlapping byte ranges, only the highest-
//!   severity match is kept.

use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;
use sha2::{Digest, Sha256};

use super::finding::{redact, Finding, Severity};
use super::patterns::{def_for, quick_scan, regex_for, EvidenceExtractor, PatternDef};
use crate::parser::ParsedComponent;

/// Scan a parsed component (markdown body + structured value) and
/// return every finding. The returned vector is order-stable: body
/// findings first (sorted by byte offset), then structured findings
/// (in JSON-pointer DFS order).
///
/// Use this from the upsert path. The scanner never reads the file
/// system; pass in a fully-parsed component.
#[must_use]
pub fn scan_parsed(parsed: &ParsedComponent) -> Vec<Finding> {
    let mut findings = Vec::new();
    if let Some(body) = parsed.body.as_deref() {
        findings.extend(scan_text(body, "body"));
    }
    if let Some(value) = parsed.structured.as_ref() {
        scan_value(value, &mut String::new(), &mut findings);
    }
    findings
}

/// Scan an arbitrary text blob with a caller-supplied source label.
/// Findings include the line number (1-based) of the matched span
/// inside the input.
#[must_use]
pub fn scan_text(text: &str, source_label: &str) -> Vec<Finding> {
    let hits = quick_scan(text);
    if hits.is_empty() {
        return Vec::new();
    }

    let mut raw: Vec<Match> = Vec::new();
    for idx in hits {
        let regex = regex_for(idx);
        let def = def_for(idx);
        for cap in regex.captures_iter(text) {
            let (range, value) = if let (EvidenceExtractor::CapturedValue, Some(group)) =
                (def.evidence, cap.name("value"))
            {
                (group.start()..group.end(), group.as_str())
            } else {
                let m = cap
                    .get(0)
                    .expect("captures_iter always yields at least group 0");
                (m.start()..m.end(), m.as_str())
            };
            if is_placeholder(value) {
                continue;
            }
            raw.push(Match {
                pattern_idx: idx,
                start: range.start,
                end: range.end,
                value: value.to_owned(),
            });
        }
    }

    // Highest-severity wins on overlapping byte ranges. We sort by
    // (severity DESC, start ASC) and accept a candidate iff it doesn't
    // overlap any already-accepted span. This is O(n^2) in worst case
    // but the pattern set is small (~14) and matches per file are
    // typically << 10.
    raw.sort_by(|a, b| {
        let sev_a = def_for(a.pattern_idx).severity.rank();
        let sev_b = def_for(b.pattern_idx).severity.rank();
        sev_b
            .cmp(&sev_a) // severity DESC
            .then_with(|| a.start.cmp(&b.start)) // earlier first on tie
    });

    let mut accepted: Vec<&Match> = Vec::new();
    for m in &raw {
        let overlaps = accepted
            .iter()
            .any(|a| !(m.end <= a.start || m.start >= a.end));
        if !overlaps {
            accepted.push(m);
        }
    }

    // Re-sort accepted findings by byte offset so the output is
    // deterministic and reads like the source.
    accepted.sort_by_key(|m| m.start);

    let now = unix_now_millis();
    accepted
        .into_iter()
        .map(|m| {
            let def = def_for(m.pattern_idx);
            let line = line_of(text, m.start);
            build_finding(def, source_label, &m.value, m.start, m.end, Some(line), now)
        })
        .collect()
}

/// Internal: a confirmed match before the de-duplicator runs.
struct Match {
    pattern_idx: usize,
    start: usize,
    end: usize,
    value: String,
}

/// Walk a `serde_json::Value` and feed every string leaf to the
/// pattern engine.
///
/// `pointer` is a mutable string that mirrors RFC 6901 JSON pointer
/// syntax (`/foo/0/bar`). We push and pop segments in place so the
/// recursion doesn't allocate a new string per node.
fn scan_value(value: &Value, pointer: &mut String, sink: &mut Vec<Finding>) {
    match value {
        Value::String(s) => {
            // The structured-value JSON pointer becomes the source
            // label. Lines mean nothing here (the value was parsed),
            // so we omit the line number.
            for f in scan_text(s, pointer) {
                sink.push(Finding { line: None, ..f });
            }
        }
        Value::Object(map) => {
            for (k, v) in map {
                let mark = pointer.len();
                pointer.push('/');
                pointer.push_str(&escape_pointer_segment(k));
                scan_value(v, pointer, sink);
                pointer.truncate(mark);
            }
        }
        Value::Array(arr) => {
            for (i, v) in arr.iter().enumerate() {
                let mark = pointer.len();
                pointer.push('/');
                pointer.push_str(&i.to_string());
                scan_value(v, pointer, sink);
                pointer.truncate(mark);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {
            // Scalars other than strings can't carry a credential.
        }
    }
}

/// Escape a JSON-pointer path segment per RFC 6901.
/// `~` -> `~0`, `/` -> `~1`. Everything else passes through.
fn escape_pointer_segment(segment: &str) -> String {
    if !segment.contains('~') && !segment.contains('/') {
        return segment.to_owned();
    }
    let mut out = String::with_capacity(segment.len());
    for ch in segment.chars() {
        match ch {
            '~' => out.push_str("~0"),
            '/' => out.push_str("~1"),
            other => out.push(other),
        }
    }
    out
}

/// Heuristic placeholder detection. The patterns themselves don't
/// know about placeholders; we filter at this layer so future
/// categories can reuse the helper.
///
/// Skips:
/// * Empty string.
/// * Angle-bracket placeholders: `<value>`, `<your-token>`.
/// * Env-var references: `${SECRET}`, `$SECRET`.
/// * Repeated masking characters: `*****`, `xxxxxxxxxx`.
/// * The literals `null`, `undefined`, `example`, `placeholder`,
///   `changeme` (case-insensitive).
fn is_placeholder(value: &str) -> bool {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return true;
    }
    if trimmed.starts_with('<') && trimmed.ends_with('>') {
        return true;
    }
    if trimmed.starts_with("${") || trimmed.starts_with('$') {
        // `$SECRET` is a shell-style env reference; we err on the
        // side of suppressing.
        return true;
    }
    if trimmed.chars().all(|c| c == '*') {
        return true;
    }
    // A run of identical alphanumeric chars is almost always a
    // placeholder (`xxxxxxxx`, `aaaaaaaa`, `00000000`).
    if trimmed.len() >= 8 {
        let first = trimmed.chars().next().unwrap_or(' ');
        if first.is_ascii_alphanumeric() && trimmed.chars().all(|c| c == first) {
            return true;
        }
    }
    let lowered = trimmed.to_ascii_lowercase();
    matches!(
        lowered.as_str(),
        "null" | "undefined" | "example" | "placeholder" | "changeme"
    )
}

/// 1-based line number containing the byte offset `pos` inside `text`.
fn line_of(text: &str, pos: usize) -> u32 {
    // `bytes()[..pos]` is safe even when `pos` lands inside a
    // multi-byte char because we only count newlines, not codepoints.
    let safe_pos = pos.min(text.len());
    let line = text[..safe_pos].bytes().filter(|b| *b == b'\n').count() + 1;
    u32::try_from(line).unwrap_or(u32::MAX)
}

/// Build a stable [`Finding`] from a confirmed match.
#[allow(clippy::too_many_arguments)]
fn build_finding(
    def: &PatternDef,
    source_label: &str,
    value: &str,
    start: usize,
    end: usize,
    line: Option<u32>,
    detected_at: i64,
) -> Finding {
    Finding {
        id: build_finding_id(source_label, def, start, end),
        component_id: None,
        category: def.category,
        pattern: def.name.to_owned(),
        severity: def.severity,
        source_label: source_label.to_owned(),
        line,
        redacted_preview: redact(value),
        detected_at,
    }
}

/// Stable id derived from `sha256(source_label || category || pattern
/// || start..end)`.
///
/// We use the source label (which is either `"body"` or the JSON
/// pointer) instead of a component path because the scanner doesn't
/// know the component identity. The upsert layer prefixes the URI
/// when it persists the row.
fn build_finding_id(source_label: &str, def: &PatternDef, start: usize, end: usize) -> String {
    let mut h = Sha256::new();
    h.update(source_label.as_bytes());
    h.update(b"\x00");
    h.update(def.category.as_str().as_bytes());
    h.update(b"\x00");
    h.update(def.name.as_bytes());
    h.update(b"\x00");
    h.update(start.to_le_bytes());
    h.update(end.to_le_bytes());
    let digest = h.finalize();
    let mut hex = String::with_capacity(16);
    for byte in &digest[..8] {
        // 8 bytes -> 16 hex chars, matching the docs/12 spec
        // (`aseye-finding-<sha256-prefix>`).
        use std::fmt::Write as _;
        // Writing into a preallocated `String` cannot fail; ignore
        // the error explicitly so clippy doesn't complain.
        let _ = write!(hex, "{byte:02x}");
    }
    format!("aseye-finding-{hex}")
}

/// `Severity` exposed at the scanner module so callers can check
/// rank without going through the parent module.
#[allow(dead_code)]
const _SEVERITY_REEXPORT_ANCHOR: Severity = Severity::Critical;

fn unix_now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_millis()).ok())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::{parse_bytes, ParsedComponent};
    use crate::registry::types::Format;

    fn parsed_md(body: &str) -> ParsedComponent {
        parse_bytes(body.as_bytes(), Format::Markdown).expect("parse md")
    }

    fn parsed_json(json: &str) -> ParsedComponent {
        parse_bytes(json.as_bytes(), Format::Json).expect("parse json")
    }

    #[test]
    fn detects_anthropic_key() {
        let body = "API_KEY=sk-ant-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n";
        let findings = scan_text(body, "body");
        assert!(findings.iter().any(|f| f.pattern == "anthropic-key"));
        let f = findings
            .iter()
            .find(|f| f.pattern == "anthropic-key")
            .unwrap();
        assert_eq!(f.severity, Severity::Critical);
        assert_eq!(f.source_label, "body");
        assert!(f.redacted_preview.contains('\u{2026}'));
    }

    #[test]
    fn detects_openai_key() {
        let body = "OPENAI=sk-projABCDEFGHIJKLMNOPQRSTUVWXYZ012345\n";
        let findings = scan_text(body, "body");
        assert!(
            findings
                .iter()
                .any(|f| f.pattern == "openai-key" || f.pattern == "openai-project-key"),
            "expected openai pattern, got {findings:?}"
        );
    }

    #[test]
    fn detects_github_pat() {
        let body = "TOKEN=ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789\n";
        let findings = scan_text(body, "body");
        assert!(findings.iter().any(|f| f.pattern == "github-pat-classic"));
    }

    #[test]
    fn detects_password_assignment() {
        let body = "password = SuperSecretValue123\n";
        let findings = scan_text(body, "body");
        assert!(
            findings.iter().any(|f| f.pattern == "bare-password"),
            "expected bare-password finding, got {findings:?}"
        );
    }

    #[test]
    fn skips_placeholder_value() {
        // GitHub-style env reference and angle-bracket placeholder
        // must not produce findings.
        let env_ref = scan_text("password = ${{ secrets.DB }}", "body");
        assert!(
            env_ref.is_empty(),
            "env ref should be suppressed: {env_ref:?}"
        );
        let angle = scan_text("password = <placeholder>", "body");
        assert!(
            angle.is_empty(),
            "angle placeholder should be suppressed: {angle:?}"
        );
        let masked = scan_text("password = ********", "body");
        assert!(
            masked.is_empty(),
            "masked value should be suppressed: {masked:?}"
        );
    }

    #[test]
    fn redacts_value() {
        // Long value: first 8 + ellipsis + last 4.
        let long = scan_text("password = SuperSecretSuperSecretSuperSecret", "body");
        let f = long
            .iter()
            .find(|f| f.pattern == "bare-password")
            .expect("bare-password fires");
        assert!(f.redacted_preview.starts_with("SuperSec"));
        assert!(f.redacted_preview.contains('\u{2026}'));
        // Last 4 chars of the value.
        assert!(f.redacted_preview.ends_with("cret"));

        // Short value (>=8 from the regex but <16 chars total):
        // first 4 + ellipsis + last 2.
        let short = scan_text("password = abcdef1234", "body");
        let f = short
            .iter()
            .find(|f| f.pattern == "bare-password")
            .expect("bare-password fires");
        assert!(f.redacted_preview.starts_with("abcd"));
        assert!(f.redacted_preview.ends_with("34"));
    }

    #[test]
    fn scans_structured_json_recursively() {
        let json = r#"{"mcpServers": {"github": {"env": {"GH_TOKEN": "ghp_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"}}}}"#;
        let parsed = parsed_json(json);
        let findings = scan_parsed(&parsed);
        let f = findings
            .iter()
            .find(|f| f.pattern == "github-pat-classic")
            .expect("expected github pat finding");
        assert_eq!(f.source_label, "/mcpServers/github/env/GH_TOKEN");
        assert!(
            f.line.is_none(),
            "structured findings should not carry a line number"
        );
    }

    #[test]
    fn scans_markdown_body_for_jwt() {
        // A JWT spans three base64url-ish chunks separated by `.`.
        let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c";
        let body = format!("token: {jwt}\n");
        let parsed = parsed_md(&body);
        let findings = scan_parsed(&parsed);
        assert!(
            findings.iter().any(|f| f.pattern == "jwt"),
            "expected jwt finding, got {findings:?}"
        );
    }

    #[test]
    fn pattern_priority_keeps_most_specific() {
        // `sk-ant-...` would also match the generic `secret = ...`
        // pattern and the openai `sk-...` pattern. The de-duplicator
        // must keep only the highest-severity (== anthropic) finding
        // for the overlapping span.
        let body = "secret = sk-ant-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n";
        let findings = scan_text(body, "body");
        // The anthropic pattern must fire.
        assert!(findings.iter().any(|f| f.pattern == "anthropic-key"));
        // The generic match's *value* span sits inside the anthropic
        // match's span, so it should be suppressed by the
        // de-duplicator. (The generic pattern's value group starts
        // where `sk-ant-...` starts.)
        let generic_count = findings
            .iter()
            .filter(|f| f.pattern == "generic-secret-assignment")
            .count();
        assert_eq!(
            generic_count, 0,
            "generic finding should be suppressed by anthropic, got {findings:?}"
        );
    }

    #[test]
    fn idempotent_scan() {
        let body = "ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789 token";
        let first = scan_text(body, "body");
        let second = scan_text(body, "body");
        assert_eq!(first.len(), second.len());
        for (a, b) in first.iter().zip(second.iter()) {
            // `id`, source_label, range, severity must match across
            // runs (only `detected_at` may differ).
            assert_eq!(a.id, b.id);
            assert_eq!(a.pattern, b.pattern);
            assert_eq!(a.source_label, b.source_label);
            assert_eq!(a.severity, b.severity);
            assert_eq!(a.redacted_preview, b.redacted_preview);
        }
    }

    #[test]
    fn line_number_tracks_newlines() {
        let body = "first line\nsecond line\nthird ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789\n";
        let findings = scan_text(body, "body");
        let f = findings
            .iter()
            .find(|f| f.pattern == "github-pat-classic")
            .unwrap();
        assert_eq!(f.line, Some(3));
    }

    #[test]
    fn json_pointer_escapes_special_chars() {
        // Per RFC 6901, `/` becomes `~1` and `~` becomes `~0` inside
        // a path segment. Ensure the escape helper kicks in.
        assert_eq!(escape_pointer_segment("normal"), "normal");
        assert_eq!(escape_pointer_segment("with/slash"), "with~1slash");
        assert_eq!(escape_pointer_segment("with~tilde"), "with~0tilde");
    }
}
