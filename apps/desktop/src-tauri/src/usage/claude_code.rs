//! JSONL parser for Claude Code session transcripts.
//!
//! Claude Code writes one JSONL file per session under
//! `~/.claude/projects/<encoded-cwd>/<session-uuid>.jsonl`. Each line is
//! a JSON object with a `type` discriminator. The shapes we care about
//! are the `assistant` turns whose `message.usage` block carries the
//! per-turn token counts.
//!
//! ## Shape (verified against real session files on 2026-05-08)
//!
//! ```json
//! {
//!   "type": "assistant",
//!   "cwd": "/Users/joseairosa/Development/allseeingeye",
//!   "timestamp": "2026-05-08T01:47:38.686Z",
//!   "sessionId": "790dfe08-66f8-41f0-b817-957a2bc8384e",
//!   "message": {
//!     "model": "claude-opus-4-7",
//!     "usage": {
//!       "input_tokens": 6,
//!       "cache_creation_input_tokens": 74204,
//!       "cache_read_input_tokens": 0,
//!       "output_tokens": 1005
//!     }
//!   }
//! }
//! ```
//!
//! Other line types (`last-prompt`, `permission-mode`, `system`,
//! `user`, `tool_use`, ...) are skipped. Lines without a `message.usage`
//! object are also skipped silently - they are not assistant
//! token-bearing turns.
//!
//! ## Robustness
//!
//! - JSONL is line-delimited, so a single malformed line skips itself
//!   without aborting the scan.
//! - Empty lines are tolerated (parser returns `None`).
//! - Unknown future fields are ignored (`#[serde(default)]` on the
//!   bits we read; the rest goes through `serde_json::Value`-style
//!   tolerance via untagged structs).
//! - The `cwd` field on the first line is the authoritative source
//!   for the project path; the encoded directory name is a fallback.

use std::path::PathBuf;

use serde::Deserialize;

use super::types::{TokenTurn, ToolKind};

/// Subset of the `assistant` line we read from a Claude Code JSONL.
///
/// We deserialise only the fields we use; serde tolerates unknown keys
/// by default. `Option<...>` everywhere lets us silently skip lines
/// that look like assistant turns but lack one of the three required
/// fields (`message.model`, `message.usage`, `timestamp`).
#[derive(Debug, Deserialize)]
struct ClaudeLine {
    #[serde(default)]
    #[serde(rename = "type")]
    line_type: Option<String>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    timestamp: Option<String>,
    #[serde(default)]
    #[serde(rename = "sessionId")]
    session_id: Option<String>,
    #[serde(default)]
    message: Option<ClaudeMessage>,
}

#[derive(Debug, Deserialize)]
struct ClaudeMessage {
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    usage: Option<ClaudeUsage>,
}

/// The token counters from a single assistant turn.
///
/// Field names mirror the on-disk JSON exactly so the deserialiser
/// does not need a `#[serde(rename = ...)]` per field. The repeated
/// `_tokens` postfix is intentional (vendor schema) so we silence
/// the `struct_field_names` lint locally.
#[allow(clippy::struct_field_names)]
#[derive(Debug, Deserialize)]
struct ClaudeUsage {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
    #[serde(default)]
    cache_read_input_tokens: u64,
    #[serde(default)]
    cache_creation_input_tokens: u64,
}

/// Find the `cwd` value carried by the first line that has one.
///
/// Claude Code writes `cwd` on every `system` / `user` / `assistant`
/// line, so we just take the first one we see. Returns `None` if no
/// line in the slice carries a cwd (extremely rare; only happens on a
/// session that wrote nothing but `last-prompt` markers).
///
/// Currently only invoked from tests; kept on the public surface so a
/// future caller (e.g. a "show project from session" affordance) can
/// reach it without re-deriving the field path.
#[allow(dead_code)]
#[must_use]
pub fn extract_cwd(text: &str) -> Option<PathBuf> {
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let Ok(parsed) = serde_json::from_str::<ClaudeLine>(line) else {
            continue;
        };
        if let Some(cwd) = parsed.cwd {
            if !cwd.is_empty() {
                return Some(PathBuf::from(cwd));
            }
        }
    }
    None
}

/// Parse a Claude Code JSONL slice into a sequence of token turns.
///
/// `text` is the bytes of one session file (or the tail since the
/// last watermark). Lines that do not look like assistant token
/// turns are silently skipped. Malformed lines are skipped too.
///
/// Each emitted [`TokenTurn`] carries:
/// - `tool` = `ToolKind::ClaudeCode`
/// - `project_path` = the line's own `cwd` (authoritative)
/// - `model` from `message.model`
/// - `day` derived from the line's UTC `timestamp`
/// - `session_id` from the line's `sessionId`
/// - the four token counters
///
/// The caller is expected to fold these into `token_usage` rows; see
/// [`super::aggregate`].
pub fn parse_turns(text: &str) -> Vec<TokenTurn> {
    let mut turns = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let Ok(parsed) = serde_json::from_str::<ClaudeLine>(line) else {
            continue;
        };

        // Only `assistant` turns carry billable usage.
        if parsed.line_type.as_deref() != Some("assistant") {
            continue;
        }
        let Some(message) = parsed.message else {
            continue;
        };
        let Some(usage) = message.usage else { continue };
        let Some(model) = message.model else { continue };
        let Some(timestamp) = parsed.timestamp else {
            continue;
        };
        let Some(cwd) = parsed.cwd else { continue };
        let session_id = parsed.session_id.unwrap_or_default();

        let Some(day) = super::types::day_from_iso8601(&timestamp) else {
            continue;
        };

        turns.push(TokenTurn {
            tool: ToolKind::ClaudeCode,
            project_path: cwd,
            model,
            day,
            session_id,
            input: usage.input_tokens,
            output: usage.output_tokens,
            cache_read: usage.cache_read_input_tokens,
            cache_create: usage.cache_creation_input_tokens,
        });
    }
    turns
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("../../tests/fixtures/usage/claude_sample.jsonl");

    #[test]
    fn parses_assistant_turns_only() {
        let turns = parse_turns(FIXTURE);
        // Fixture has 3 assistant turns. The non-assistant lines must
        // be ignored.
        assert_eq!(turns.len(), 3);
        for t in &turns {
            assert_eq!(t.tool, ToolKind::ClaudeCode);
            assert!(!t.model.is_empty());
            assert!(!t.day.is_empty());
        }
    }

    #[test]
    fn token_totals_match_fixture() {
        let turns = parse_turns(FIXTURE);
        let total_input: u64 = turns.iter().map(|t| t.input).sum();
        let total_output: u64 = turns.iter().map(|t| t.output).sum();
        let total_cache_create: u64 = turns.iter().map(|t| t.cache_create).sum();
        let total_cache_read: u64 = turns.iter().map(|t| t.cache_read).sum();
        // Fixture turns:
        //   turn 1: input 6 / output 1005 / cc 74204 / cr 0
        //   turn 2: input 6 / output 327  / cc 80154 / cr 0
        //   turn 3: input 4 / output 200  / cc 0     / cr 50000
        assert_eq!(total_input, 16);
        assert_eq!(total_output, 1532);
        assert_eq!(total_cache_create, 154_358);
        assert_eq!(total_cache_read, 50_000);
    }

    #[test]
    fn skips_malformed_lines() {
        let mixed = "not json\n{\"type\":\"assistant\"}\n";
        let turns = parse_turns(mixed);
        assert!(turns.is_empty());
    }

    #[test]
    fn skips_empty_lines() {
        let blank = "\n\n  \n\n";
        let turns = parse_turns(blank);
        assert!(turns.is_empty());
    }

    #[test]
    fn extract_cwd_returns_first_match() {
        let cwd = extract_cwd(FIXTURE).expect("fixture has cwd");
        assert_eq!(
            cwd,
            PathBuf::from("/Users/joseairosa/Development/allseeingeye")
        );
    }

    #[test]
    fn extract_cwd_handles_no_cwd_lines() {
        let text = "{\"type\":\"last-prompt\",\"sessionId\":\"x\"}\n";
        assert!(extract_cwd(text).is_none());
    }
}
