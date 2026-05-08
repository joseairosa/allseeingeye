//! JSONL parser for Codex CLI session transcripts.
//!
//! Codex writes one JSONL file per session under
//! `~/.codex/sessions/YYYY/MM/DD/rollout-<utc-stamp>-<session-uuid>.jsonl`.
//!
//! ## Verified shape (sampled 2026-05-08 from real session files)
//!
//! Codex's JSONL has more line variants than Claude Code's. The only
//! ones this parser cares about are:
//!
//! 1. `session_meta` (always the first line) - carries `payload.cwd`
//!    and the session id.
//!
//!    ```json
//!    { "type": "session_meta",
//!      "payload": {
//!        "id": "019d3f64-9a4b-7250-8f5d-ff3b5709f209",
//!        "timestamp": "2026-03-30T15:37:30.977Z",
//!        "cwd": "/Users/joseairosa/Development/f4f/retooldb-migrations"
//!      }
//!    }
//!    ```
//!
//! 2. `turn_context` - carries the active model id. Codex emits one
//!    per turn; the model id can change mid-session if the user
//!    reconfigures.
//!
//!    ```json
//!    { "type": "turn_context",
//!      "payload": { "model": "openai/gpt-5", ... } }
//!    ```
//!
//! 3. `event_msg` with `payload.type = "token_count"` - the actual
//!    billing events. We use the per-turn delta in
//!    `payload.info.last_token_usage` (NOT `total_token_usage`, which
//!    is cumulative and would double-count if folded across turns).
//!
//!    ```json
//!    { "timestamp": "2026-03-30T05:58:30.670Z",
//!      "type": "event_msg",
//!      "payload": {
//!        "type": "token_count",
//!        "info": {
//!          "last_token_usage": {
//!            "input_tokens": 13929,
//!            "cached_input_tokens": 0,
//!            "output_tokens": 171,
//!            "reasoning_output_tokens": 128,
//!            "total_tokens": 14100
//!          }
//!        }
//!      }
//!    }
//!    ```
//!
//! ### Spec deviations
//!
//! The spec (`docs/14-cost-and-memory.md` section 14C) describes Codex
//! as `event_type: "token_count"` at the top level. The real format is
//! `type: "event_msg"` with `payload.type: "token_count"` underneath.
//! The spec also lists `payload.{model, usage}` on `token_count`; the
//! real payload nests usage under
//! `payload.info.{last,total}_token_usage` and carries no model field.
//! The model is taken from the most recent `turn_context` line that
//! preceded the `token_count` event.
//!
//! ### What we map
//!
//! Codex's token names do not line up 1-to-1 with Anthropic's:
//! - Codex `input_tokens` -> our `input` (uncached portion)
//! - Codex `cached_input_tokens` -> our `cache_read`
//! - Codex `output_tokens` + `reasoning_output_tokens` -> our `output`
//!   (reasoning tokens are billable as output on `OpenAI`'s side)
//! - Codex has no equivalent of `cache_creation_input_tokens` -> we
//!   record `0`.

use std::path::PathBuf;

use serde::Deserialize;

use super::types::{TokenTurn, ToolKind};

/// Top-level Codex line - we only inspect the discriminator and the
/// payload shape.
#[derive(Debug, Deserialize)]
struct CodexLine {
    #[serde(default)]
    timestamp: Option<String>,
    #[serde(default)]
    #[serde(rename = "type")]
    line_type: Option<String>,
    #[serde(default)]
    payload: Option<serde_json::Value>,
}

/// Subset of `session_meta.payload` we care about.
#[derive(Debug, Deserialize)]
struct SessionMetaPayload {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    cwd: Option<String>,
}

/// Subset of `turn_context.payload` we care about.
#[derive(Debug, Deserialize)]
struct TurnContextPayload {
    #[serde(default)]
    model: Option<String>,
}

/// Subset of `event_msg.payload` for `token_count` events.
#[derive(Debug, Deserialize)]
struct TokenCountPayload {
    #[serde(default)]
    #[serde(rename = "type")]
    inner_type: Option<String>,
    #[serde(default)]
    info: Option<TokenCountInfo>,
}

#[derive(Debug, Deserialize)]
struct TokenCountInfo {
    #[serde(default)]
    last_token_usage: Option<CodexUsage>,
}

/// The repeated `_tokens` postfix mirrors the vendor JSON; the lint
/// is silenced locally to keep the wire shape obvious.
#[allow(clippy::struct_field_names)]
#[derive(Debug, Deserialize)]
struct CodexUsage {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    cached_input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
    #[serde(default)]
    reasoning_output_tokens: u64,
}

/// Parse a Codex JSONL slice into a sequence of token turns.
///
/// Maintains running state: the most recent `cwd`, `model`, and
/// `session_id`. When a `token_count` event with `last_token_usage`
/// arrives, emits a turn carrying that running state. Lines before
/// any `session_meta` (extremely unusual) are dropped.
pub fn parse_turns(text: &str) -> Vec<TokenTurn> {
    let mut turns = Vec::new();
    let mut cwd: Option<String> = None;
    let mut session_id: Option<String> = None;
    let mut model: Option<String> = None;

    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let Ok(parsed) = serde_json::from_str::<CodexLine>(line) else {
            continue;
        };

        match parsed.line_type.as_deref() {
            Some("session_meta") => {
                if let Some(payload) = parsed.payload {
                    if let Ok(meta) = serde_json::from_value::<SessionMetaPayload>(payload) {
                        if let Some(c) = meta.cwd {
                            cwd = Some(c);
                        }
                        if let Some(id) = meta.id {
                            session_id = Some(id);
                        }
                    }
                }
            }
            Some("turn_context") => {
                if let Some(payload) = parsed.payload {
                    if let Ok(ctx) = serde_json::from_value::<TurnContextPayload>(payload) {
                        if let Some(m) = ctx.model {
                            model = Some(m);
                        }
                    }
                }
            }
            Some("event_msg") => {
                let Some(payload) = parsed.payload else {
                    continue;
                };
                let Ok(tc) = serde_json::from_value::<TokenCountPayload>(payload) else {
                    continue;
                };
                if tc.inner_type.as_deref() != Some("token_count") {
                    continue;
                }
                let Some(info) = tc.info else { continue };
                let Some(usage) = info.last_token_usage else {
                    continue;
                };
                // Need at least timestamp + cwd + model to emit. If
                // the session never set them, drop the event.
                let Some(ts) = parsed.timestamp.as_deref() else {
                    continue;
                };
                let Some(day) = super::types::day_from_iso8601(ts) else {
                    continue;
                };
                let Some(project_path) = cwd.clone() else {
                    continue;
                };
                let Some(model_id) = model.clone() else {
                    continue;
                };

                turns.push(TokenTurn {
                    tool: ToolKind::Codex,
                    project_path,
                    model: model_id,
                    day,
                    session_id: session_id.clone().unwrap_or_default(),
                    input: usage.input_tokens,
                    output: usage
                        .output_tokens
                        .saturating_add(usage.reasoning_output_tokens),
                    cache_read: usage.cached_input_tokens,
                    cache_create: 0,
                });
            }
            _ => {}
        }
    }
    turns
}

/// Convenience: extract just the cwd from the first `session_meta`
/// line. Used by callers that need to derive a project path before any
/// usage rows are present in the file. Currently only exercised by
/// tests; kept on the public surface for symmetry with the Claude
/// Code parser and future "show project from session" affordances.
#[allow(dead_code)]
#[must_use]
pub fn extract_cwd(text: &str) -> Option<PathBuf> {
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let Ok(parsed) = serde_json::from_str::<CodexLine>(line) else {
            continue;
        };
        if parsed.line_type.as_deref() == Some("session_meta") {
            if let Some(payload) = parsed.payload {
                if let Ok(meta) = serde_json::from_value::<SessionMetaPayload>(payload) {
                    if let Some(cwd) = meta.cwd {
                        return Some(PathBuf::from(cwd));
                    }
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("../../tests/fixtures/usage/codex_sample.jsonl");

    #[test]
    fn parses_token_count_events() {
        let turns = parse_turns(FIXTURE);
        // Fixture has 2 token_count events with last_token_usage; one
        // earlier event with `info: null` must be skipped.
        assert_eq!(turns.len(), 2);
        for t in &turns {
            assert_eq!(t.tool, ToolKind::Codex);
            assert!(!t.model.is_empty());
            assert_eq!(t.cache_create, 0, "Codex has no cache_create concept");
        }
    }

    #[test]
    fn token_totals_match_fixture() {
        let turns = parse_turns(FIXTURE);
        // Turn 1: input 13929 / output 171 + reasoning 128 = 299 / cache_read 0
        // Turn 2: input 200 / output 50 + reasoning 0 = 50 / cache_read 4096
        let total_input: u64 = turns.iter().map(|t| t.input).sum();
        let total_output: u64 = turns.iter().map(|t| t.output).sum();
        let total_cache_read: u64 = turns.iter().map(|t| t.cache_read).sum();
        assert_eq!(total_input, 13929 + 200);
        assert_eq!(total_output, 171 + 128 + 50);
        assert_eq!(total_cache_read, 4096);
    }

    #[test]
    fn project_path_comes_from_session_meta() {
        let turns = parse_turns(FIXTURE);
        let p = &turns[0].project_path;
        assert_eq!(p, "/Users/joseairosa/Development/f4f/retooldb-migrations");
    }

    #[test]
    fn model_comes_from_turn_context() {
        let turns = parse_turns(FIXTURE);
        // Both turns happen after the single turn_context; model
        // should be the openai/gpt-5 stamped there.
        assert_eq!(turns[0].model, "openai/gpt-5");
    }

    #[test]
    fn extract_cwd_finds_first_meta() {
        let cwd = extract_cwd(FIXTURE).unwrap();
        assert_eq!(
            cwd,
            PathBuf::from("/Users/joseairosa/Development/f4f/retooldb-migrations")
        );
    }

    #[test]
    fn skips_malformed_lines() {
        let bad = "not json\n{ \"type\": \"event_msg\", \"payload\": {} }\n";
        let turns = parse_turns(bad);
        assert!(turns.is_empty());
    }
}
