//! Soak tests for the `SQLite` + FTS5 index.
//!
//! Phase 5.1 - exercises the index machinery at scale that the default
//! suite cannot afford to run on every CI build. All tests in this
//! module are gated behind `#[ignore]` so they only run when invoked
//! explicitly via `cargo test --release -- --ignored`.
//!
//! Why a dedicated module instead of folding into `upsert::tests`?
//! These tests touch SQL directly (bypassing `upsert_component` so we
//! can hit a 10 000-row corpus without driving the parser through
//! every iteration). Keeping the bypass code isolated makes the intent
//! clear and prevents leakage into the production write path.

use std::time::{Duration, Instant};

use rusqlite::params;

use super::IndexHandle;

/// Insert N synthetic component rows directly via SQL. We bypass
/// `upsert_component` so the soak measures *index* throughput, not
/// parser cost. Rows have realistic shape (markdown skill, claude-code
/// tool, user scope) so the FTS5 tokeniser sees the same content
/// distribution it would in production.
fn insert_synthetic_rows(handle: &IndexHandle, count: usize) {
    handle
        .write(|conn| {
            // Single transaction keeps fsync amortised across the
            // whole batch - 10 000 individual commits would dwarf the
            // actual work and skew the timings.
            conn.execute("BEGIN", [])?;
            for i in 0..count {
                let id = format!("aseye://claude-code/user/skill/soak-{i}");
                let name = format!("soak-{i}");
                let path = format!("/tmp/soak/skills/{i}/SKILL.md");
                // Vary the body so FTS has > 1 unique token per row.
                let body = format!(
                    "soak skill number {i} performs a synthetic rotation of \
                     index entries to exercise FTS5 token cardinality"
                );
                conn.execute(
                    "INSERT INTO component (
                        id, type, tool, scope, origin, name, display_name,
                        description, path, format, size, mtime, ctime,
                        enabled, hash, updated_at
                     ) VALUES (
                        ?1, 'skill', 'claude-code', 'user', 'userCreated',
                        ?2, ?2, 'soak skill', ?3, 'markdownFrontmatter',
                        128, 0, 0, 1, ?4, 0
                     )",
                    params![id, name, path, format!("hash-{i}")],
                )?;
                conn.execute(
                    "INSERT INTO component_fts (id, name, description, body)
                     VALUES (?1, ?2, 'soak skill', ?3)",
                    params![id, name, body],
                )?;
            }
            conn.execute("COMMIT", [])?;
            Ok(())
        })
        .expect("synthetic inserts");
}

/// Bounded p50 latency for the FTS query. Empirically the populated
/// 10 000-row corpus answers a single-token MATCH in << 50 ms on a
/// modern desktop; the gate exists to catch a future regression that
/// drops FTS indexes or changes the tokeniser to a slow variant. Set
/// a 50 ms ceiling rather than a tight number so intermittent CPU
/// pressure on shared CI runners does not flap.
const P50_MAX: Duration = Duration::from_millis(50);

/// 10 000-row insert + 100-query soak.
///
/// Inserts 10 k synthetic rows in a single transaction, then runs 100
/// random single-token MATCH queries and asserts the median latency
/// stays below `P50_MAX`. Run via:
///
/// ```ignore
/// cargo test --release -p aseye-desktop -- --ignored \
///   index::soak::soak_ten_thousand_row_insert
/// ```
#[test]
#[ignore = "long-running soak; run with --ignored"]
fn soak_ten_thousand_row_insert() {
    let handle = IndexHandle::open_in_memory().expect("open");
    insert_synthetic_rows(&handle, 10_000);

    // Sanity check the corpus made it in.
    let count: i64 = handle
        .read(|c| Ok(c.query_row("SELECT COUNT(*) FROM component", [], |r| r.get(0))?))
        .expect("count");
    assert_eq!(count, 10_000);

    // Tiny xorshift PRNG - same shape as the existing atomic-write
    // soak so we keep dev-deps tight (no `rand` dep).
    let mut state: u64 = 0xABCD_1234_5678_9ABC;
    let mut latencies = Vec::with_capacity(100);

    for _ in 0..100 {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        // Pick one of a handful of tokens that appear in our synthetic
        // bodies; the tokeniser is FTS5's default, which lower-cases
        // and splits on word boundaries. The query has 1+ hits in the
        // corpus by construction.
        let token = match state % 5 {
            0 => "soak",
            1 => "skill",
            2 => "synthetic",
            3 => "rotation",
            _ => "fts5",
        };

        let started = Instant::now();
        let hit_count: i64 = handle
            .read(|c| {
                Ok(c.query_row(
                    "SELECT COUNT(*) FROM component_fts WHERE component_fts MATCH ?1",
                    params![token],
                    |r| r.get(0),
                )?)
            })
            .expect("fts query");
        latencies.push(started.elapsed());
        assert!(hit_count > 0, "every token must hit at least one row");
    }

    latencies.sort();
    let p50 = latencies[latencies.len() / 2];
    assert!(
        p50 <= P50_MAX,
        "FTS p50 latency exceeded budget: {p50:?} > {P50_MAX:?}",
    );
}
