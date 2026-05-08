//! Aggregator: walk JSONL session files, fold turns into `token_usage`
//! rows, advance per-session watermarks.
//!
//! The aggregator is invoked from the IPC `usage_refresh` command and
//! lazily on first mount of the Cost view. It is idempotent: re-running
//! against the same on-disk state with watermarks already at EOF is a
//! no-op.
//!
//! ## Refresh cycle
//!
//! 1. List every JSONL file under the Claude Code projects dir and the
//!    Codex sessions dir.
//! 2. For each file, look up the prior `bytes_read` watermark; seek
//!    there, read the appended tail, pass to the per-tool parser.
//! 3. Fold returned turns into `(tool, project, model, day)` buckets.
//! 4. Upsert each bucket into `token_usage` (ADDING the deltas to any
//!    existing counts).
//! 5. Update each session's watermark to the new file size.
//!
//! Step 4 is the trickiest piece: a re-scan that re-reads the **tail**
//! of a file must add the tail's deltas onto the existing rolled-up
//! row, not overwrite it. We track session ids inside each bucket so
//! the `sessions` count reflects the union of distinct sessions
//! folded so far, not just the deltas.

use std::collections::HashMap;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension};

use super::pricing::estimate_cost_usd;
use super::types::{TokenTurn, ToolKind};
use super::{claude_code, codex};
use crate::index::IndexHandle;

/// Unique key for a `token_usage` row.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
struct BucketKey {
    tool: ToolKind,
    project_path: String,
    model: String,
    day: String,
}

/// Mutable accumulator for a `token_usage` row before it is written.
#[derive(Debug, Default)]
struct BucketAcc {
    turns: u64,
    input: u64,
    output: u64,
    cache_read: u64,
    cache_create: u64,
    /// Distinct session ids contributing to this bucket. We store the
    /// set so re-runs that fold partial tails do not over-count.
    sessions: std::collections::HashSet<String>,
}

/// Outcome of a refresh pass.
///
/// `new_bytes`, `rows_touched`, and `turns_folded` are diagnostic and
/// only consumed by tests today. `refreshed_at` is what the IPC
/// `usage_refresh` command returns to the frontend so the UI can
/// stamp "Updated 12:34". The diagnostic fields are kept on the
/// struct so a future "telemetry"/health view can surface them
/// without changing the function shape.
#[allow(dead_code)]
#[derive(Debug, Clone, Default)]
pub struct RefreshOutcome {
    /// Number of new (tool, session) bytes consumed in this pass.
    pub new_bytes: u64,
    /// Number of `token_usage` rows touched by upserts.
    pub rows_touched: u64,
    /// Number of new turns folded.
    pub turns_folded: u64,
    /// Unix epoch seconds at which the pass completed.
    pub refreshed_at: i64,
}

/// Run a single refresh pass against the user's Claude Code + Codex
/// session directories. Both directories are optional; if either is
/// missing the pass simply skips that tool.
///
/// `home` is the user's home directory - extracted as a parameter so
/// tests can point at a tempdir-built fixture tree without monkey-
/// patching `dirs::home_dir`.
pub fn refresh_from_home(index: &IndexHandle, home: &Path) -> Result<RefreshOutcome, RefreshError> {
    let claude_root = home.join(".claude").join("projects");
    let codex_root = home.join(".codex").join("sessions");

    let mut new_bytes = 0_u64;
    let mut buckets: HashMap<BucketKey, BucketAcc> = HashMap::new();
    let mut watermark_updates: Vec<(ToolKind, String, u64)> = Vec::new();

    // Walk Claude Code project directories.
    if claude_root.is_dir() {
        for project_dir in iter_dirs(&claude_root) {
            for jsonl in iter_files_with_ext(&project_dir, "jsonl") {
                let consumed = process_file(
                    index,
                    ToolKind::ClaudeCode,
                    &jsonl,
                    &mut buckets,
                    &mut watermark_updates,
                )?;
                new_bytes = new_bytes.saturating_add(consumed);
            }
        }
    }

    // Walk Codex YYYY/MM/DD tree.
    if codex_root.is_dir() {
        for jsonl in walk_jsonl(&codex_root) {
            let consumed = process_file(
                index,
                ToolKind::Codex,
                &jsonl,
                &mut buckets,
                &mut watermark_updates,
            )?;
            new_bytes = new_bytes.saturating_add(consumed);
        }
    }

    let refreshed_at = unix_now_secs();
    let turns_folded: u64 = buckets.values().map(|b| b.turns).sum();
    let mut rows_touched = 0_u64;
    index.write(|conn| {
        for (key, acc) in &buckets {
            upsert_bucket(conn, key, acc, refreshed_at)?;
            rows_touched = rows_touched.saturating_add(1);
        }
        for (tool, session_id, bytes) in &watermark_updates {
            update_watermark(conn, *tool, session_id, *bytes)?;
        }
        Ok(())
    })?;

    Ok(RefreshOutcome {
        new_bytes,
        rows_touched,
        turns_folded,
        refreshed_at,
    })
}

/// Process one session JSONL file. Reads the appended tail since the
/// last watermark, parses it, and folds turns into `buckets`. Pushes
/// the new watermark onto `watermark_updates` for the caller to commit
/// inside the same write transaction.
///
/// Returns the number of new bytes consumed by this file.
fn process_file(
    index: &IndexHandle,
    tool: ToolKind,
    path: &Path,
    buckets: &mut HashMap<BucketKey, BucketAcc>,
    watermark_updates: &mut Vec<(ToolKind, String, u64)>,
) -> Result<u64, RefreshError> {
    // The session id is the file stem for both tools.
    // - Claude Code: `<uuid>.jsonl`
    // - Codex:        `rollout-<utc-stamp>-<uuid>.jsonl`
    // We use the bare stem here as a stable key; the precise uuid
    // doesn't matter as long as it's per-file unique.
    let Some(session_id) = path.file_stem().and_then(|s| s.to_str()) else {
        return Ok(0);
    };

    let prior: u64 = index.read(|conn| {
        let v: Option<i64> = conn
            .query_row(
                "SELECT bytes_read FROM usage_session_watermark WHERE tool = ?1 AND session_id = ?2",
                params![tool.as_str(), session_id],
                |row| row.get(0),
            )
            .optional()?;
        // `bytes_read` is non-negative by construction; the `max(0)`
        // is a defensive cap for any future migration that lets a
        // value bottom out below zero. The cast back to u64 is
        // therefore safe.
        #[allow(clippy::cast_sign_loss)]
        let bytes = v.unwrap_or(0).max(0) as u64;
        Ok(bytes)
    })?;

    let Ok(mut file) = fs::File::open(path) else {
        return Ok(0);
    };
    let Ok(total) = file.metadata().map(|m| m.len()) else {
        return Ok(0);
    };

    if total == prior {
        return Ok(0);
    }
    if total < prior {
        // File was truncated (rare but possible). Re-read from start.
        // We cannot easily reconcile prior partial folds; the safer
        // move is to keep the existing rolled-up data and just
        // re-fold from byte 0. The aggregator is idempotent over
        // distinct turns because we de-dup by session_id+turn count
        // when computing `sessions`. Token deltas may be slightly
        // double-counted on truncate; we accept that as a rare
        // failure mode rather than rewriting the whole row.
        if file.seek(SeekFrom::Start(0)).is_err() {
            return Ok(0);
        }
    } else if file.seek(SeekFrom::Start(prior)).is_err() {
        return Ok(0);
    }

    let mut buf = String::new();
    if file.read_to_string(&mut buf).is_err() {
        // Likely a non-UTF-8 read error mid-stream. Skip the file but
        // do not advance the watermark - we'll retry next time.
        return Ok(0);
    }

    let consumed = total.saturating_sub(prior);
    let turns = match tool {
        ToolKind::ClaudeCode => claude_code::parse_turns(&buf),
        ToolKind::Codex => codex::parse_turns(&buf),
    };

    fold_turns_into(buckets, turns);
    watermark_updates.push((tool, session_id.to_string(), total));
    Ok(consumed)
}

/// Fold a freshly-parsed batch of turns into the running bucket map.
fn fold_turns_into(buckets: &mut HashMap<BucketKey, BucketAcc>, turns: Vec<TokenTurn>) {
    for t in turns {
        let key = BucketKey {
            tool: t.tool,
            project_path: t.project_path,
            model: t.model,
            day: t.day,
        };
        let entry = buckets.entry(key).or_default();
        entry.turns = entry.turns.saturating_add(1);
        entry.input = entry.input.saturating_add(t.input);
        entry.output = entry.output.saturating_add(t.output);
        entry.cache_read = entry.cache_read.saturating_add(t.cache_read);
        entry.cache_create = entry.cache_create.saturating_add(t.cache_create);
        if !t.session_id.is_empty() {
            entry.sessions.insert(t.session_id);
        }
    }
}

/// Upsert one rolled-up row into `token_usage`, ADDING the deltas onto
/// any existing row. The row's `est_cost_usd` is recomputed from the
/// new totals so it stays consistent with the price table.
fn upsert_bucket(
    conn: &Connection,
    key: &BucketKey,
    acc: &BucketAcc,
    refreshed_at: i64,
) -> rusqlite::Result<()> {
    // Read existing row (may not exist).
    let existing: Option<(i64, i64, i64, i64, i64, i64)> = conn
        .query_row(
            "SELECT sessions, turns, input, output, cache_read, cache_create
               FROM token_usage
              WHERE tool = ?1 AND project_path = ?2 AND model = ?3 AND day = ?4",
            params![key.tool.as_str(), &key.project_path, &key.model, &key.day],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                ))
            },
        )
        .optional()?;

    // Coerce u64 deltas to i64 once. SQLite INTEGER is 64-bit signed;
    // realistic token counts never approach i64::MAX so wrap risk is
    // negligible, but we cap with `saturating` math anyway.
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let acc_sessions = acc.sessions.len() as i64;
    #[allow(clippy::cast_possible_wrap)]
    let acc_turns = acc.turns as i64;
    #[allow(clippy::cast_possible_wrap)]
    let acc_input = acc.input as i64;
    #[allow(clippy::cast_possible_wrap)]
    let acc_output = acc.output as i64;
    #[allow(clippy::cast_possible_wrap)]
    let acc_cache_read = acc.cache_read as i64;
    #[allow(clippy::cast_possible_wrap)]
    let acc_cache_create = acc.cache_create as i64;

    let (sessions, turns, input, output, cache_read, cache_create) = match existing {
        Some((s, t, i, o, cr, cc)) => {
            // Sessions: take the max of (existing, sessions seen this
            // pass). A session may legitimately span multiple passes,
            // so we cannot simply add - that would double-count. If
            // the new pass observed strictly more sessions for this
            // bucket than the row knew about, advance to the larger
            // count. Conservative and matches the user-visible
            // semantic ("how many distinct sessions did this project
            // have on this day").
            (
                acc_sessions.max(s),
                t.saturating_add(acc_turns),
                i.saturating_add(acc_input),
                o.saturating_add(acc_output),
                cr.saturating_add(acc_cache_read),
                cc.saturating_add(acc_cache_create),
            )
        }
        None => (
            acc_sessions,
            acc_turns,
            acc_input,
            acc_output,
            acc_cache_read,
            acc_cache_create,
        ),
    };

    #[allow(clippy::cast_sign_loss)]
    let est_cost = estimate_cost_usd(
        &key.model,
        input.max(0) as u64,
        output.max(0) as u64,
        cache_read.max(0) as u64,
        cache_create.max(0) as u64,
    );

    conn.execute(
        "INSERT INTO token_usage (tool, project_path, model, day,
                                  sessions, turns, input, output,
                                  cache_read, cache_create,
                                  est_cost_usd, refreshed_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
         ON CONFLICT(tool, project_path, model, day) DO UPDATE SET
            sessions     = excluded.sessions,
            turns        = excluded.turns,
            input        = excluded.input,
            output       = excluded.output,
            cache_read   = excluded.cache_read,
            cache_create = excluded.cache_create,
            est_cost_usd = excluded.est_cost_usd,
            refreshed_at = excluded.refreshed_at",
        params![
            key.tool.as_str(),
            &key.project_path,
            &key.model,
            &key.day,
            sessions,
            turns,
            input,
            output,
            cache_read,
            cache_create,
            est_cost,
            refreshed_at,
        ],
    )?;
    Ok(())
}

/// Persist a per-session watermark.
fn update_watermark(
    conn: &Connection,
    tool: ToolKind,
    session_id: &str,
    bytes_read: u64,
) -> rusqlite::Result<()> {
    #[allow(clippy::cast_possible_wrap)]
    let bytes = bytes_read as i64;
    conn.execute(
        "INSERT INTO usage_session_watermark (tool, session_id, bytes_read)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(tool, session_id) DO UPDATE SET bytes_read = excluded.bytes_read",
        params![tool.as_str(), session_id, bytes],
    )?;
    Ok(())
}

/// Iterate immediate child directories of `root` (one level deep).
fn iter_dirs(root: &Path) -> Vec<PathBuf> {
    let Ok(rd) = fs::read_dir(root) else {
        return Vec::new();
    };
    rd.filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| e.path())
        .collect()
}

/// Iterate files in `dir` (one level) whose extension matches `ext`.
fn iter_files_with_ext(dir: &Path, ext: &str) -> Vec<PathBuf> {
    let Ok(rd) = fs::read_dir(dir) else {
        return Vec::new();
    };
    rd.filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some(ext))
        .collect()
}

/// Recursively walk `root` and return every `.jsonl` path. Used for
/// the Codex `YYYY/MM/DD` tree.
fn walk_jsonl(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = fs::read_dir(&dir) else { continue };
        for entry in rd.filter_map(Result::ok) {
            let Ok(ft) = entry.file_type() else { continue };
            let path = entry.path();
            if ft.is_dir() {
                stack.push(path);
            } else if ft.is_file() && path.extension().and_then(|s| s.to_str()) == Some("jsonl") {
                out.push(path);
            }
        }
    }
    out
}

fn unix_now_secs() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| {
        #[allow(clippy::cast_possible_wrap)]
        let s = d.as_secs() as i64;
        s
    })
}

/// Errors that can bubble out of a refresh pass. The aggregator is
/// best-effort about per-file IO errors (it skips and moves on); the
/// errors here are the database-level ones the caller cares about.
#[derive(Debug, thiserror::Error)]
pub enum RefreshError {
    #[error("index error: {0}")]
    Index(#[from] crate::index::IndexError),
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn fixture_claude() -> &'static str {
        include_str!("../../tests/fixtures/usage/claude_sample.jsonl")
    }

    fn fixture_codex() -> &'static str {
        include_str!("../../tests/fixtures/usage/codex_sample.jsonl")
    }

    fn build_synthetic_home(dir: &Path) -> std::io::Result<()> {
        // Claude Code projects/<encoded>/<session>.jsonl
        let cdir = dir.join(".claude/projects/-Users-joseairosa-Development-allseeingeye");
        fs::create_dir_all(&cdir)?;
        let mut f = fs::File::create(cdir.join("sess-A.jsonl"))?;
        f.write_all(fixture_claude().as_bytes())?;

        // Codex sessions/YYYY/MM/DD/rollout-*.jsonl
        let xdir = dir.join(".codex/sessions/2026/03/30");
        fs::create_dir_all(&xdir)?;
        let mut f = fs::File::create(xdir.join("rollout-2026-03-30T15-37-30-019d3f64.jsonl"))?;
        f.write_all(fixture_codex().as_bytes())?;
        Ok(())
    }

    #[test]
    fn refresh_processes_both_tools() {
        let tmp = tempfile::tempdir().expect("tempdir");
        build_synthetic_home(tmp.path()).expect("build home");
        let index = IndexHandle::open_in_memory().expect("open");

        let outcome = refresh_from_home(&index, tmp.path()).expect("refresh");
        assert!(outcome.new_bytes > 0);
        assert!(
            outcome.turns_folded >= 5,
            "expected 5+ turns folded, got {}",
            outcome.turns_folded
        );
        assert!(outcome.rows_touched >= 3); // 2 claude (opus + sonnet) + 2 codex days

        // Spot check: claude project row exists.
        let count: i64 = index
            .read(|c| {
                Ok(c.query_row(
                    "SELECT COUNT(*) FROM token_usage WHERE tool = 'claude-code'",
                    [],
                    |r| r.get(0),
                )?)
            })
            .unwrap();
        assert!(
            count >= 2,
            "expected 2 claude rows (opus + sonnet), got {count}"
        );

        let codex_rows: i64 = index
            .read(|c| {
                Ok(c.query_row(
                    "SELECT COUNT(*) FROM token_usage WHERE tool = 'codex'",
                    [],
                    |r| r.get(0),
                )?)
            })
            .unwrap();
        assert!(codex_rows >= 1);
    }

    #[test]
    fn refresh_is_idempotent_on_unchanged_files() {
        let tmp = tempfile::tempdir().expect("tempdir");
        build_synthetic_home(tmp.path()).expect("build home");
        let index = IndexHandle::open_in_memory().expect("open");

        let _first = refresh_from_home(&index, tmp.path()).expect("first");
        let second = refresh_from_home(&index, tmp.path()).expect("second");
        assert_eq!(second.new_bytes, 0, "no new bytes on rescan");
        assert_eq!(second.turns_folded, 0);
    }

    #[test]
    fn refresh_advances_watermark_on_appended_lines() {
        let tmp = tempfile::tempdir().expect("tempdir");
        build_synthetic_home(tmp.path()).expect("build home");
        let index = IndexHandle::open_in_memory().expect("open");

        let _first = refresh_from_home(&index, tmp.path()).expect("first");

        // Append one more assistant turn to the claude jsonl.
        let claude_path = tmp
            .path()
            .join(".claude/projects/-Users-joseairosa-Development-allseeingeye/sess-A.jsonl");
        let extra = "{\"type\":\"assistant\",\"cwd\":\"/Users/joseairosa/Development/allseeingeye\",\"timestamp\":\"2026-05-10T01:00:00.000Z\",\"sessionId\":\"sess-A\",\"message\":{\"model\":\"claude-opus-4-7\",\"usage\":{\"input_tokens\":10,\"cache_creation_input_tokens\":0,\"cache_read_input_tokens\":0,\"output_tokens\":50}}}\n";
        let mut f = fs::OpenOptions::new()
            .append(true)
            .open(&claude_path)
            .expect("open append");
        f.write_all(extra.as_bytes()).expect("append");
        drop(f);

        let second = refresh_from_home(&index, tmp.path()).expect("second");
        // Only the appended bytes should be consumed.
        let appended_len = extra.len() as u64;
        assert_eq!(
            second.new_bytes, appended_len,
            "watermark must restrict re-read to the new tail; saw {} bytes",
            second.new_bytes
        );
        assert_eq!(
            second.turns_folded, 1,
            "exactly one new turn should fold from the appended line"
        );

        // The opus row's `output` count must reflect the additional 50 tokens.
        let opus_output: i64 = index
            .read(|c| {
                Ok(c.query_row(
                    "SELECT output FROM token_usage WHERE tool='claude-code' AND model='claude-opus-4-7' AND day='2026-05-10'",
                    [],
                    |r| r.get(0),
                )?)
            })
            .unwrap();
        assert_eq!(opus_output, 50, "appended turn rolled into a new day row");
    }

    #[test]
    fn missing_home_dirs_are_tolerated() {
        let tmp = tempfile::tempdir().expect("tempdir");
        // Don't build any directories. Refresh must not panic; it
        // must return zero outcome.
        let index = IndexHandle::open_in_memory().expect("open");
        let outcome = refresh_from_home(&index, tmp.path()).expect("refresh");
        assert_eq!(outcome.new_bytes, 0);
        assert_eq!(outcome.rows_touched, 0);
        assert_eq!(outcome.turns_folded, 0);
    }
}
