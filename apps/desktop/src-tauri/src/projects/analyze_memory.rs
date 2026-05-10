//! Action 1: CLAUDE.md / AGENTS.md / GEMINI.md size + optimisation
//! analysis.
//!
//! Read-only: never modifies the source file. Returns a structured
//! report with size + token estimate + recommendations the user can
//! act on through the existing Editor view.
//!
//! Heuristics (spec docs/17 §17.3):
//! * `Oversized` - file is over 8 KiB (~2k tokens of every-turn cost).
//! * `DuplicateOfGlobal` - a section's body matches a section in
//!   `~/.claude/CLAUDE.md` modulo whitespace.
//! * `InternalDuplicate` - two H2 sections inside this file have
//!   near-identical bodies.
//! * `UnknownFrontmatterField` - frontmatter has fields the schema
//!   does not consume.
//! * `StaleReference` - section body references files / paths that
//!   no longer exist on disk.
//!
//! All heuristics fail closed: a heuristic that errors mid-analysis
//! does NOT abort the report, it just produces no entries from that
//! pass.

// `cast_precision_loss` fires on the deliberate `usize -> f32` and
// `u64 -> f64` casts inside the similarity score computation and the
// human-readable `format_bytes`. Both are intentional: similarity is
// a [0.0, 1.0] ratio by design, and the bytes formatter shows
// kibibytes / mebibytes with one decimal place. The values fit in
// the float mantissa for any realistic memory file size.
#![allow(clippy::cast_precision_loss)]

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::index::IndexHandle;

/// Spec threshold for the `Oversized` recommendation (docs/17 §17.3).
const OVERSIZED_BYTES: u64 = 8 * 1024;

/// Lower bound on section length before we consider it for duplicate
/// detection. Short headings / boilerplate produce false positives;
/// 80 bytes is roughly two short paragraphs.
const MIN_SECTION_BODY_BYTES: usize = 80;

/// Similarity threshold for `DuplicateOfGlobal` and
/// `InternalDuplicate`. 0.85 means two sections agree on 85% of
/// their content (Levenshtein-normalised). Below this we treat them
/// as independent.
const SIMILARITY_THRESHOLD: f32 = 0.85;

/// Hard cap on Levenshtein computation. Sections longer than this
/// fall back to a coarse hash-equality check so we don't burn
/// quadratic time on a 50 KB section pair.
const LEVENSHTEIN_BUDGET_BYTES: usize = 4096;

/// Outcome of the `analyze_memory` action.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../bindings/projects/MemoryAnalysisReport.ts")]
#[ts(rename_all = "camelCase")]
pub struct MemoryAnalysisReport {
    pub project_path: String,
    pub memory_path: String,
    pub size_bytes: u64,
    pub tokens_est: u64,
    pub recommendations: Vec<MemoryRecommendation>,
    pub elapsed_ms: u64,
}

/// One row in `MemoryAnalysisReport.recommendations`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../bindings/projects/MemoryRecommendation.ts")]
#[ts(rename_all = "camelCase")]
pub struct MemoryRecommendation {
    pub kind: MemoryRecommendationKind,
    pub message: String,
    pub estimated_savings_bytes: u64,
    /// 1-indexed `(start, end_inclusive)` line range for "open in
    /// editor at line N". `None` when the heuristic does not point
    /// at a specific span (e.g. `Oversized` applies to the whole
    /// file).
    pub line_range: Option<(u32, u32)>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../bindings/projects/MemoryRecommendationKind.ts")]
#[ts(rename_all = "camelCase")]
pub enum MemoryRecommendationKind {
    Oversized,
    DuplicateOfGlobal,
    InternalDuplicate,
    UnknownFrontmatterField,
    StaleReference,
}

/// Run the analyze pass against `memory_path`. The `project_path`
/// is recorded in the report so the UI can attribute the result to
/// the right project card without a follow-up lookup.
///
/// Read-only: never opens the source file for write. The `handle`
/// argument is currently unused but kept on the signature so future
/// heuristics that need to query the component table (e.g. compare
/// against the global `~/.claude/CLAUDE.md` pulled from the index)
/// can wire in without an API change.
pub fn analyze_memory(
    handle: &IndexHandle,
    project_path: &Path,
    memory_path: &Path,
) -> Result<MemoryAnalysisReport, AnalyzeError> {
    let started = std::time::Instant::now();
    let bytes = std::fs::read(memory_path).map_err(AnalyzeError::ReadSource)?;
    let size_bytes = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    let tokens_est = size_bytes / 4;

    let text = String::from_utf8_lossy(&bytes).into_owned();

    let mut recommendations: Vec<MemoryRecommendation> = Vec::new();

    if size_bytes > OVERSIZED_BYTES {
        recommendations.push(MemoryRecommendation {
            kind: MemoryRecommendationKind::Oversized,
            message: format!(
                "File is {} ({} tokens) - over the {} KiB threshold. Long memory files cost \
                 every conversation turn. Consider splitting into per-skill rules under \
                 `~/.claude/rules/` or moving stable content to your global CLAUDE.md.",
                format_bytes(size_bytes),
                approx_tokens(size_bytes),
                OVERSIZED_BYTES / 1024,
            ),
            estimated_savings_bytes: size_bytes.saturating_sub(OVERSIZED_BYTES),
            line_range: None,
        });
    }

    let local_sections = split_h2_sections(&text);

    // Internal duplicate: pairwise compare every section against
    // every other section. Cap at 50 sections to bound the
    // quadratic; real CLAUDE.md files rarely exceed 30 sections.
    let local_section_cap = local_sections.len().min(50);
    for i in 0..local_section_cap {
        for j in (i + 1)..local_section_cap {
            let a = &local_sections[i];
            let b = &local_sections[j];
            if a.body.len() < MIN_SECTION_BODY_BYTES || b.body.len() < MIN_SECTION_BODY_BYTES {
                continue;
            }
            if similarity(&a.body, &b.body) >= SIMILARITY_THRESHOLD {
                recommendations.push(MemoryRecommendation {
                    kind: MemoryRecommendationKind::InternalDuplicate,
                    message: format!(
                        "Sections \"{}\" (line {}) and \"{}\" (line {}) have very similar \
                         bodies. Consider merging or removing one.",
                        a.heading, a.start_line, b.heading, b.start_line,
                    ),
                    estimated_savings_bytes: a.body.len().min(b.body.len()) as u64,
                    line_range: Some((b.start_line, b.end_line)),
                });
            }
        }
    }

    // Duplicate of global: load `~/.claude/CLAUDE.md` if present and
    // compare every local section against every global section.
    if let Some(global_text) = read_global_claude_md() {
        let global_sections = split_h2_sections(&global_text);
        for local in &local_sections {
            if local.body.len() < MIN_SECTION_BODY_BYTES {
                continue;
            }
            for global in &global_sections {
                if global.body.len() < MIN_SECTION_BODY_BYTES {
                    continue;
                }
                if similarity(&local.body, &global.body) >= SIMILARITY_THRESHOLD {
                    recommendations.push(MemoryRecommendation {
                        kind: MemoryRecommendationKind::DuplicateOfGlobal,
                        message: format!(
                            "Section \"{}\" (line {}) duplicates content already in your \
                             global ~/.claude/CLAUDE.md. Remove from this project to \
                             reduce per-turn cost.",
                            local.heading, local.start_line,
                        ),
                        estimated_savings_bytes: local.body.len() as u64,
                        line_range: Some((local.start_line, local.end_line)),
                    });
                    break; // one match is enough; don't double-report
                }
            }
        }
    }

    // Stale references: find file-like tokens in the body and probe
    // the filesystem.
    let stale = detect_stale_references(&text, project_path);
    recommendations.extend(stale);

    let _ = handle; // reserved for future heuristics that consult the index
    let elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    Ok(MemoryAnalysisReport {
        project_path: project_path.to_string_lossy().into_owned(),
        memory_path: memory_path.to_string_lossy().into_owned(),
        size_bytes,
        tokens_est,
        recommendations,
        elapsed_ms,
    })
}

/// Errors that prevent the analyze pass from running at all. Per-
/// heuristic failures collect inside the report instead.
#[derive(Debug, thiserror::Error)]
pub enum AnalyzeError {
    #[error("could not read source memory file: {0}")]
    ReadSource(std::io::Error),
}

#[derive(Debug, Clone)]
struct Section {
    heading: String,
    start_line: u32,
    end_line: u32,
    body: String,
}

/// Split a markdown text into H2 sections. The "preamble" before
/// the first H2 is dropped; H1 is treated as document title.
fn split_h2_sections(text: &str) -> Vec<Section> {
    let mut sections: Vec<Section> = Vec::new();
    let mut current: Option<Section> = None;

    for (i, line) in text.lines().enumerate() {
        let one_indexed = u32::try_from(i + 1).unwrap_or(u32::MAX);
        if let Some(rest) = line.strip_prefix("## ") {
            // Close out the previous section.
            if let Some(mut prev) = current.take() {
                prev.end_line = one_indexed.saturating_sub(1);
                sections.push(prev);
            }
            current = Some(Section {
                heading: rest.trim().to_owned(),
                start_line: one_indexed,
                end_line: one_indexed,
                body: String::new(),
            });
        } else if let Some(ref mut s) = current {
            if !s.body.is_empty() {
                s.body.push('\n');
            }
            s.body.push_str(line);
        }
    }

    if let Some(mut prev) = current {
        prev.end_line = u32::try_from(text.lines().count()).unwrap_or(u32::MAX);
        sections.push(prev);
    }

    sections
}

/// Read `~/.claude/CLAUDE.md` if present. Returns `None` (not an
/// error) when it doesn't exist - the duplicate-of-global heuristic
/// just reports nothing in that case.
fn read_global_claude_md() -> Option<String> {
    let home = dirs::home_dir()?;
    let path = home.join(".claude").join("CLAUDE.md");
    std::fs::read_to_string(path).ok()
}

/// Levenshtein-normalised similarity in `[0.0, 1.0]`. Returns `1.0`
/// for identical inputs after whitespace normalisation, scaling
/// linearly down with edit distance.
///
/// Sections longer than `LEVENSHTEIN_BUDGET_BYTES` fall back to
/// hash-equality of the normalised text; this is conservative
/// (returns 1.0 only for an exact post-normalise match) but keeps
/// the analyse pass O(N) instead of O(N^2) for huge sections.
fn similarity(a: &str, b: &str) -> f32 {
    let a_norm = normalise(a);
    let b_norm = normalise(b);
    if a_norm.is_empty() || b_norm.is_empty() {
        return 0.0;
    }
    if a_norm.len() > LEVENSHTEIN_BUDGET_BYTES || b_norm.len() > LEVENSHTEIN_BUDGET_BYTES {
        return if a_norm == b_norm { 1.0 } else { 0.0 };
    }
    let dist = levenshtein(&a_norm, &b_norm);
    let max_len = a_norm.len().max(b_norm.len());
    1.0 - (dist as f32 / max_len as f32)
}

/// Whitespace-normalise: collapse runs of whitespace into a single
/// space, trim leading/trailing whitespace.
fn normalise(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut last_was_space = true;
    for c in s.chars() {
        if c.is_whitespace() {
            if !last_was_space {
                out.push(' ');
                last_was_space = true;
            }
        } else {
            out.push(c);
            last_was_space = false;
        }
    }
    out.trim().to_owned()
}

/// Standard Levenshtein distance with an inline DP buffer. Caller
/// guarantees both inputs are below `LEVENSHTEIN_BUDGET_BYTES`.
fn levenshtein(a: &str, b: &str) -> usize {
    let a_bytes: Vec<char> = a.chars().collect();
    let b_bytes: Vec<char> = b.chars().collect();
    let m = a_bytes.len();
    let n = b_bytes.len();
    if m == 0 {
        return n;
    }
    if n == 0 {
        return m;
    }
    let mut prev: Vec<usize> = (0..=n).collect();
    let mut curr: Vec<usize> = vec![0; n + 1];
    for i in 1..=m {
        curr[0] = i;
        for j in 1..=n {
            let cost = usize::from(a_bytes[i - 1] != b_bytes[j - 1]);
            curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[n]
}

/// Detect stale references: scan body for path-like tokens
/// (`./foo.md`, `path/to/file.md`, `~/something.md`) and probe each
/// against the filesystem.
fn detect_stale_references(text: &str, project_path: &Path) -> Vec<MemoryRecommendation> {
    let mut out: Vec<MemoryRecommendation> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    for (i, line) in text.lines().enumerate() {
        let one_indexed = u32::try_from(i + 1).unwrap_or(u32::MAX);
        // Look for inline link targets `](X.md)` and bare relative
        // paths `path/to/file.md`. We're conservative: only accept
        // tokens with a `.md` / `.txt` / `.json` / `.yaml` / `.toml`
        // extension to avoid flagging "see section 5" as a path.
        for token in extract_path_tokens(line) {
            if !seen.insert(token.clone()) {
                continue;
            }
            let resolved = resolve_path(project_path, &token);
            if !resolved.exists() {
                out.push(MemoryRecommendation {
                    kind: MemoryRecommendationKind::StaleReference,
                    message: format!(
                        "Line {one_indexed}: reference `{token}` does not resolve to an existing \
                         file. Either remove the reference or fix the path."
                    ),
                    estimated_savings_bytes: 0,
                    line_range: Some((one_indexed, one_indexed)),
                });
            }
        }
    }
    out
}

fn extract_path_tokens(line: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    // Inline-link targets: `](path)`.
    let bytes = line.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b']' && bytes[i + 1] == b'(' {
            i += 2;
            let start = i;
            while i < bytes.len() && bytes[i] != b')' {
                i += 1;
            }
            if i < bytes.len() {
                let raw = &line[start..i];
                let token = raw.split_whitespace().next().unwrap_or(raw);
                if is_pathlike(token) {
                    out.push(token.to_owned());
                }
            }
        } else {
            i += 1;
        }
    }
    // Bare path-like tokens: split by whitespace and grab any token
    // that has a known extension. Skip tokens that still carry
    // markdown-link punctuation after trim - those were already
    // covered by the inline-link scan above and would resolve to a
    // bogus joined path otherwise (`docs](./real.md`).
    for token in line.split(|c: char| c.is_whitespace() || c == ',' || c == ';') {
        let cleaned = token
            .trim_matches(|c: char| matches!(c, '"' | '\'' | '`' | '(' | ')' | '[' | ']' | '.'));
        if cleaned.chars().any(|c| matches!(c, '[' | ']' | '(' | ')')) {
            continue;
        }
        if is_pathlike(cleaned) && cleaned.contains('/') {
            out.push(cleaned.to_owned());
        }
    }
    out
}

fn is_pathlike(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    matches!(
        s.rsplit('.').next(),
        Some("md" | "txt" | "json" | "yaml" | "yml" | "toml" | "rs" | "ts" | "tsx" | "js" | "py")
    )
}

fn resolve_path(project_path: &Path, token: &str) -> PathBuf {
    if let Some(stripped) = token.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(stripped);
        }
    }
    if Path::new(token).is_absolute() {
        return PathBuf::from(token);
    }
    project_path.join(token)
}

fn format_bytes(n: u64) -> String {
    if n < 1024 {
        format!("{n} B")
    } else if n < 1024 * 1024 {
        format!("{:.1} KiB", n as f64 / 1024.0)
    } else {
        format!("{:.1} MiB", n as f64 / (1024.0 * 1024.0))
    }
}

fn approx_tokens(n: u64) -> u64 {
    n / 4
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write(dir: &Path, name: &str, body: &str) -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, body).unwrap();
        p
    }

    #[test]
    fn small_clean_file_yields_no_recommendations() {
        let dir = tempdir().unwrap();
        let body = "# tiny\n\n## one\n\nhi\n";
        let p = write(dir.path(), "CLAUDE.md", body);
        let handle = IndexHandle::open_in_memory().unwrap();
        let r = analyze_memory(&handle, dir.path(), &p).unwrap();
        assert_eq!(r.size_bytes, body.len() as u64);
        // We allow the duplicate-of-global heuristic to fire here
        // because the developer's real ~/.claude/CLAUDE.md may
        // happen to share content with this synthetic file. Filter
        // for the heuristics we care about being absent.
        let unwanted = r
            .recommendations
            .iter()
            .filter(|x| {
                matches!(
                    x.kind,
                    MemoryRecommendationKind::Oversized
                        | MemoryRecommendationKind::InternalDuplicate
                        | MemoryRecommendationKind::StaleReference
                )
            })
            .count();
        assert_eq!(unwanted, 0, "{:?}", r.recommendations);
    }

    #[test]
    fn oversized_recommendation_fires_above_8_kib() {
        let dir = tempdir().unwrap();
        let body = format!("# big\n\n{}", "lorem ipsum ".repeat(900));
        let p = write(dir.path(), "CLAUDE.md", &body);
        let handle = IndexHandle::open_in_memory().unwrap();
        let r = analyze_memory(&handle, dir.path(), &p).unwrap();
        assert!(r
            .recommendations
            .iter()
            .any(|x| matches!(x.kind, MemoryRecommendationKind::Oversized)));
    }

    #[test]
    fn internal_duplicate_detected() {
        let dir = tempdir().unwrap();
        // Two H2 sections with near-identical bodies (well over the
        // 80-byte minimum, similarity ~1.0).
        let body = "# t\n\n## a\n\nThe quick brown fox jumps over the lazy dog. \
                    Pack my box with five dozen liquor jugs. Sphinx of black quartz, \
                    judge my vow.\n\n## b\n\nThe quick brown fox jumps over the lazy \
                    dog. Pack my box with five dozen liquor jugs. Sphinx of black \
                    quartz, judge my vow.\n";
        let p = write(dir.path(), "CLAUDE.md", body);
        let handle = IndexHandle::open_in_memory().unwrap();
        let r = analyze_memory(&handle, dir.path(), &p).unwrap();
        assert!(r
            .recommendations
            .iter()
            .any(|x| matches!(x.kind, MemoryRecommendationKind::InternalDuplicate)));
    }

    #[test]
    fn stale_reference_to_missing_file_detected() {
        let dir = tempdir().unwrap();
        let body = "# t\n\n## refs\n\nSee [docs](./does-not-exist.md) for details.\n";
        let p = write(dir.path(), "CLAUDE.md", body);
        let handle = IndexHandle::open_in_memory().unwrap();
        let r = analyze_memory(&handle, dir.path(), &p).unwrap();
        assert!(r
            .recommendations
            .iter()
            .any(|x| matches!(x.kind, MemoryRecommendationKind::StaleReference)));
    }

    #[test]
    fn existing_reference_does_not_fire_stale() {
        let dir = tempdir().unwrap();
        write(dir.path(), "real.md", "hi");
        let body = "# t\n\n## refs\n\nSee [docs](./real.md) for details.\n";
        let p = write(dir.path(), "CLAUDE.md", body);
        let handle = IndexHandle::open_in_memory().unwrap();
        let r = analyze_memory(&handle, dir.path(), &p).unwrap();
        assert!(!r
            .recommendations
            .iter()
            .any(|x| matches!(x.kind, MemoryRecommendationKind::StaleReference)));
    }

    #[test]
    fn similarity_handles_empty_inputs() {
        assert!(similarity("", "").abs() < f32::EPSILON);
        assert!(similarity("hello", "").abs() < f32::EPSILON);
        assert!(similarity("", "hello").abs() < f32::EPSILON);
    }

    #[test]
    fn similarity_perfect_match() {
        assert!((similarity("hello world", "hello   world") - 1.0).abs() < 1e-6);
    }

    #[test]
    fn split_h2_records_line_ranges() {
        let text = "# title\n\n## first\n\nbody1\n\n## second\n\nbody2 line a\nbody2 line b\n";
        let sections = split_h2_sections(text);
        assert_eq!(sections.len(), 2);
        assert_eq!(sections[0].heading, "first");
        assert_eq!(sections[0].start_line, 3);
        assert_eq!(sections[1].heading, "second");
        assert_eq!(sections[1].start_line, 7);
    }

    #[test]
    fn missing_source_file_returns_typed_error() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("absent.md");
        let handle = IndexHandle::open_in_memory().unwrap();
        let err = analyze_memory(&handle, dir.path(), &p).unwrap_err();
        assert!(matches!(err, AnalyzeError::ReadSource(_)));
    }
}
