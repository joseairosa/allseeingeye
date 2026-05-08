//! Cost recommendation engine.
//!
//! Three v1 heuristics:
//!
//! 1. `BloatedMemory`  - oversized memory components in projects with
//!    real spend. Estimated savings: trim cost on every input turn.
//! 2. `LowCacheHitRate` - projects with > $20 / 30d that cache <40% of
//!    their input. Estimated savings: bridge the gap to a 0.7 baseline.
//! 3. `OldModelOnHotProject` - top-3 spend project that uses Opus for
//!    > 50% of turns when Sonnet would suffice.
//!
//! Heuristics fire only when their trigger condition is met. Returned
//! list is sorted by `estimated_savings_usd_30d` descending and
//! capped at 5 items.
//!
//! ### Phase 14A coupling
//!
//! `BloatedMemory` queries the `component` table for memory rows
//! larger than 8kB. That table is populated by the Phase 14A walker.
//! If 14A has not yet merged, the query returns zero rows and the
//! heuristic fires zero recommendations - that is the intended
//! graceful degradation.

use std::collections::HashMap;

use rusqlite::params;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::pricing::lookup_price;
use crate::index::{IndexHandle, Result as IndexResult};

/// Three recommendation flavours. Serialised as camelCase strings so
/// the UI can switch on a stable discriminator.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../bindings/usage/CostRecKind.ts")]
#[ts(rename_all = "camelCase")]
pub enum CostRecKind {
    BloatedMemory,
    LowCacheHitRate,
    OldModelOnHotProject,
}

/// One recommendation - what to fix, why, and an estimated 30-day
/// savings figure.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../bindings/usage/CostRec.ts")]
#[ts(rename_all = "camelCase")]
pub struct CostRec {
    pub kind: CostRecKind,
    pub title: String,
    pub rationale: String,
    pub estimated_savings_usd_30d: f64,
    /// Component IDs the UI should expose as "open in editor" links.
    pub related_components: Vec<String>,
    /// The project these savings are tied to (decoded cwd).
    pub project_path: String,
}

/// Window threshold (days) used by every heuristic. Hard-coded to 30
/// to match the spec's "$X / 30d" semantics.
const WINDOW_DAYS: i64 = 30;

/// `BloatedMemory` triggers above 8KiB - the same threshold the docs
/// 14B "Health view" uses for `is_oversized`.
const MEMORY_BYTES_THRESHOLD: i64 = 8192;
/// `BloatedMemory` only fires if the project also spent > $5 / 30d.
const BLOATED_MIN_SPEND: f64 = 5.0;

/// `LowCacheHitRate` only fires above this 30-day spend.
const LOW_CACHE_MIN_SPEND: f64 = 20.0;
/// Cache-hit ratio cutoff. Below this we recommend.
const LOW_CACHE_THRESHOLD: f64 = 0.4;
/// Baseline ratio used to estimate savings from raising cache use.
const CACHE_BASELINE: f64 = 0.7;

/// `OldModelOnHotProject` requires > 50% of project turns on Opus.
const OPUS_TURN_THRESHOLD: f64 = 0.5;

/// Hard cap on returned recommendations (per spec "up to 5").
const MAX_RECS: usize = 5;

/// Compute recommendations against the current `token_usage` table.
///
/// `today_unix` lets tests pin "now" deterministically. In production
/// callers pass `unix_now_secs()`.
pub fn recommend(index: &IndexHandle, today_unix: i64) -> IndexResult<Vec<CostRec>> {
    let cutoff = today_unix - WINDOW_DAYS * 86_400;
    // Day-string cutoff: take the YYYY-MM-DD of `cutoff` so the SQL
    // filter is a simple string comparison. We don't need to be exact
    // about timezones here - the heuristics are coarse.
    let cutoff_day = day_string(cutoff);

    let mut recs = Vec::new();

    let project_aggs = index.read(|conn| Ok(collect_project_aggregates(conn, &cutoff_day)?))?;

    // ─── BloatedMemory ──────────────────────────────────────────────
    let bloated = index.read(|conn| Ok(collect_bloated_memory(conn, &project_aggs)?))?;
    for rec in bloated {
        recs.push(rec);
    }

    // ─── LowCacheHitRate ────────────────────────────────────────────
    for (project, agg) in &project_aggs {
        if agg.cost_30d <= LOW_CACHE_MIN_SPEND {
            continue;
        }
        let denom = agg.input.saturating_add(agg.cache_read);
        if denom == 0 {
            continue;
        }
        #[allow(clippy::cast_precision_loss)]
        let ratio = (agg.cache_read as f64) / (denom as f64);
        if ratio >= LOW_CACHE_THRESHOLD {
            continue;
        }
        // Savings: bringing cache hit ratio from `ratio` to baseline
        // moves `(baseline - ratio) * total_input` tokens from the
        // input bucket to the cache_read bucket. Per-token saving is
        // (input_per_m - cache_read_per_m) / 1e6.
        // Use the project's dominant model for pricing.
        let model = &agg.dominant_model;
        let p = lookup_price(model);
        #[allow(clippy::cast_precision_loss)]
        let total_input_tokens = denom as f64;
        let saved_tokens = (CACHE_BASELINE - ratio).max(0.0) * total_input_tokens;
        let savings = saved_tokens / 1_000_000.0 * (p.input_per_m - p.cache_read_per_m).max(0.0);
        if savings <= 0.0 {
            continue;
        }
        recs.push(CostRec {
            kind: CostRecKind::LowCacheHitRate,
            title: format!("Raise cache hit rate on {}", short_project_name(project)),
            rationale: format!(
                "Project caches only {:.0}% of input tokens; baseline is 70%. \
                 Spend in last 30 days: ${:.2}.",
                ratio * 100.0,
                agg.cost_30d
            ),
            estimated_savings_usd_30d: savings,
            related_components: Vec::new(),
            project_path: project.clone(),
        });
    }

    // ─── OldModelOnHotProject ───────────────────────────────────────
    // Find top 3 projects by spend, then check if Opus dominates.
    let mut top_projects: Vec<(&String, &ProjectAgg)> = project_aggs.iter().collect();
    top_projects.sort_by(|a, b| {
        b.1.cost_30d
            .partial_cmp(&a.1.cost_30d)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    for (project, agg) in top_projects.iter().take(3) {
        if agg.opus_turns == 0 {
            continue;
        }
        let total_turns = agg.total_turns.max(1);
        #[allow(clippy::cast_precision_loss)]
        let opus_share = (agg.opus_turns as f64) / (total_turns as f64);
        if opus_share <= OPUS_TURN_THRESHOLD {
            continue;
        }
        // Savings: if every Opus turn ran on Sonnet instead, savings =
        // (opus_input * (opus_input_per_m - sonnet_input_per_m) +
        //  opus_output * (opus_output_per_m - sonnet_output_per_m)) / 1e6.
        let opus = lookup_price("claude-opus");
        let sonnet = lookup_price("claude-sonnet");
        #[allow(clippy::cast_precision_loss)]
        let in_diff = (agg.opus_input as f64) / 1_000_000.0
            * (opus.input_per_m - sonnet.input_per_m).max(0.0);
        #[allow(clippy::cast_precision_loss)]
        let out_diff = (agg.opus_output as f64) / 1_000_000.0
            * (opus.output_per_m - sonnet.output_per_m).max(0.0);
        #[allow(clippy::cast_precision_loss)]
        let cr_diff = (agg.opus_cache_read as f64) / 1_000_000.0
            * (opus.cache_read_per_m - sonnet.cache_read_per_m).max(0.0);
        let savings = in_diff + out_diff + cr_diff;
        if savings <= 0.0 {
            continue;
        }
        recs.push(CostRec {
            kind: CostRecKind::OldModelOnHotProject,
            title: format!(
                "Switch {} from Opus to Sonnet for routine work",
                short_project_name(project)
            ),
            rationale: format!(
                "{:.0}% of turns in this project ran on Opus over the last 30 days \
                 (${:.2} spent).",
                opus_share * 100.0,
                agg.cost_30d
            ),
            estimated_savings_usd_30d: savings,
            related_components: Vec::new(),
            project_path: (*project).clone(),
        });
    }

    // Sort by estimated savings desc, cap at MAX_RECS.
    recs.sort_by(|a, b| {
        b.estimated_savings_usd_30d
            .partial_cmp(&a.estimated_savings_usd_30d)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    recs.truncate(MAX_RECS);
    Ok(recs)
}

/// Per-project rollup over the last 30 days.
struct ProjectAgg {
    cost_30d: f64,
    input: u64,
    cache_read: u64,
    total_turns: u64,
    opus_turns: u64,
    opus_input: u64,
    opus_output: u64,
    opus_cache_read: u64,
    /// Most-used model id in this project (by token volume). Used to
    /// price the `LowCacheHitRate` savings.
    dominant_model: String,
}

fn collect_project_aggregates(
    conn: &rusqlite::Connection,
    cutoff_day: &str,
) -> rusqlite::Result<HashMap<String, ProjectAgg>> {
    let mut stmt = conn.prepare(
        "SELECT project_path, model, turns, input, output, cache_read, est_cost_usd
           FROM token_usage
          WHERE day >= ?1",
    )?;
    let rows = stmt.query_map(params![cutoff_day], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, i64>(2)?,
            row.get::<_, i64>(3)?,
            row.get::<_, i64>(4)?,
            row.get::<_, i64>(5)?,
            row.get::<_, f64>(6)?,
        ))
    })?;

    let mut out: HashMap<String, ProjectAgg> = HashMap::new();
    let mut model_volume: HashMap<String, HashMap<String, u64>> = HashMap::new();

    for row in rows {
        let (project, model, turns, input, output, cache_read, cost) = row?;
        #[allow(clippy::cast_sign_loss)]
        let turns_u = turns.max(0) as u64;
        #[allow(clippy::cast_sign_loss)]
        let input_u = input.max(0) as u64;
        #[allow(clippy::cast_sign_loss)]
        let output_u = output.max(0) as u64;
        #[allow(clippy::cast_sign_loss)]
        let cache_read_u = cache_read.max(0) as u64;

        let entry = out.entry(project.clone()).or_insert_with(|| ProjectAgg {
            cost_30d: 0.0,
            input: 0,
            cache_read: 0,
            total_turns: 0,
            opus_turns: 0,
            opus_input: 0,
            opus_output: 0,
            opus_cache_read: 0,
            dominant_model: String::new(),
        });
        entry.cost_30d += cost;
        entry.input = entry.input.saturating_add(input_u);
        entry.cache_read = entry.cache_read.saturating_add(cache_read_u);
        entry.total_turns = entry.total_turns.saturating_add(turns_u);

        if is_opus(&model) {
            entry.opus_turns = entry.opus_turns.saturating_add(turns_u);
            entry.opus_input = entry.opus_input.saturating_add(input_u);
            entry.opus_output = entry.opus_output.saturating_add(output_u);
            entry.opus_cache_read = entry.opus_cache_read.saturating_add(cache_read_u);
        }

        let vol = input_u
            .saturating_add(output_u)
            .saturating_add(cache_read_u);
        let proj_models = model_volume.entry(project).or_default();
        *proj_models.entry(model).or_insert(0) += vol;
    }

    // Pick the dominant model per project (highest volume).
    for (project, models) in model_volume {
        if let Some((m, _)) = models.iter().max_by_key(|(_, v)| **v) {
            if let Some(agg) = out.get_mut(&project) {
                m.clone_into(&mut agg.dominant_model);
            }
        }
    }

    Ok(out)
}

fn collect_bloated_memory(
    conn: &rusqlite::Connection,
    project_aggs: &HashMap<String, ProjectAgg>,
) -> rusqlite::Result<Vec<CostRec>> {
    // Pull memory components > threshold. The component table's `path`
    // is absolute; we want to match it against the project_path strings
    // we have aggregates for. We use a starts_with check in Rust
    // rather than SQL because SQLite's `LIKE` is awkward for prefix +
    // path-separator semantics.
    let mut stmt = conn.prepare(
        "SELECT id, name, path, size FROM component
          WHERE type = 'memory' AND size IS NOT NULL AND size > ?1",
    )?;
    let rows = stmt.query_map(params![MEMORY_BYTES_THRESHOLD], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, i64>(3)?,
        ))
    })?;

    let mut out = Vec::new();
    for row in rows {
        let (id, name, path, size) = row?;
        // Find the project this memory file belongs to: longest project
        // path that prefixes the component path.
        let project = project_aggs
            .keys()
            .filter(|p| path.starts_with(p.as_str()))
            .max_by_key(|p| p.len());
        let Some(project) = project else { continue };
        let Some(agg) = project_aggs.get(project) else {
            continue;
        };
        if agg.cost_30d <= BLOATED_MIN_SPEND {
            continue;
        }
        let oversize_bytes = (size - MEMORY_BYTES_THRESHOLD).max(0);
        if oversize_bytes <= 0 {
            continue;
        }
        // Each input turn re-sends the memory file. Estimated savings:
        // oversize_tokens * turns_30d * input_price.
        #[allow(clippy::cast_precision_loss)]
        let oversize_tokens = (oversize_bytes as f64) / 4.0;
        let model = if agg.dominant_model.is_empty() {
            "claude-sonnet".to_string()
        } else {
            agg.dominant_model.clone()
        };
        let p = lookup_price(&model);
        #[allow(clippy::cast_precision_loss)]
        let savings = (oversize_tokens * (agg.total_turns as f64) / 1_000_000.0) * p.input_per_m;
        if savings <= 0.0 {
            continue;
        }
        out.push(CostRec {
            kind: CostRecKind::BloatedMemory,
            title: format!("Trim oversized memory file {name}"),
            rationale: format!(
                "Memory file is {} bytes ({} over threshold). Project spent ${:.2} \
                 in last 30 days; trimming saves on every input turn.",
                size, oversize_bytes, agg.cost_30d
            ),
            estimated_savings_usd_30d: savings,
            related_components: vec![id],
            project_path: project.clone(),
        });
    }
    Ok(out)
}

fn is_opus(model: &str) -> bool {
    let m = model.to_ascii_lowercase();
    m.starts_with("claude-opus") || m.contains("/claude-opus")
}

fn short_project_name(path: &str) -> String {
    path.rsplit('/')
        .find(|s| !s.is_empty())
        .unwrap_or(path)
        .to_string()
}

/// Convert unix-seconds to a `YYYY-MM-DD` UTC string. Self-contained
/// algorithm that handles the proleptic Gregorian calendar; no
/// `chrono` dependency. Days-since-1970-01-01 -> y/m/d via Howard
/// Hinnant's algorithm.
///
/// Crate-public re-export so the query module can compute its own
/// 30-day cutoff without duplicating the math.
#[must_use]
pub(crate) fn day_string_for_test(unix_secs: i64) -> String {
    day_string(unix_secs)
}

fn day_string(unix_secs: i64) -> String {
    let secs_per_day = 86_400_i64;
    let days = unix_secs.div_euclid(secs_per_day);
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Howard Hinnant's `civil_from_days`. Returns (year, month, day)
/// for `days` since 1970-01-01. The internal subtractions are
/// guaranteed non-negative by the algorithm's preconditions, so the
/// `i64 -> u64` cast in the `doe` line cannot lose sign in practice.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    // `(z - era * 146_097)` is in `[0, 146096]` per the algorithm.
    #[allow(clippy::cast_sign_loss)]
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    #[allow(clippy::cast_possible_wrap)]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    let y = if m <= 2 { y + 1 } else { y };
    #[allow(clippy::cast_possible_truncation)]
    let m_u = m as u32;
    #[allow(clippy::cast_possible_truncation)]
    let d_u = d as u32;
    (y, m_u, d_u)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    // Test helper - allowed to take many parameters because each one
    // maps directly to a `token_usage` column we want to vary across
    // cases. Splitting into a struct would obscure the call sites.
    #[allow(clippy::too_many_arguments)]
    fn install_token_row(
        conn: &Connection,
        tool: &str,
        project: &str,
        model: &str,
        day: &str,
        turns: i64,
        input: i64,
        output: i64,
        cache_read: i64,
        cost: f64,
    ) {
        conn.execute(
            "INSERT INTO token_usage (tool, project_path, model, day,
                 sessions, turns, input, output, cache_read, cache_create,
                 est_cost_usd, refreshed_at)
             VALUES (?1, ?2, ?3, ?4, 1, ?5, ?6, ?7, ?8, 0, ?9, 0)",
            params![tool, project, model, day, turns, input, output, cache_read, cost],
        )
        .unwrap();
    }

    fn install_memory_component(conn: &Connection, id: &str, name: &str, path: &str, size: i64) {
        conn.execute(
            "INSERT INTO component (id, type, tool, scope, origin, name, path, format, size, mtime, hash, updated_at)
             VALUES (?1, 'memory', 'claude-code', 'project', 'fs', ?2, ?3, 'markdown', ?4, 0, 'h', 0)",
            params![id, name, path, size],
        ).unwrap();
    }

    fn fixed_today() -> i64 {
        // 2026-05-08 UTC = 1778198400 unix seconds (computed
        // independently for the test). Pin the date so day_string()
        // produces a deterministic cutoff regardless of when the
        // test runs.
        1_778_198_400
    }

    #[test]
    fn day_string_round_trips_known_dates() {
        // 2026-05-08 = 1778198400.
        assert_eq!(day_string(1_778_198_400), "2026-05-08");
        // 1970-01-01 = 0.
        assert_eq!(day_string(0), "1970-01-01");
        // 2000-02-29 (leap day) - precomputed: 951782400.
        assert_eq!(day_string(951_782_400), "2000-02-29");
    }

    #[test]
    fn no_recommendations_for_empty_index() {
        let index = IndexHandle::open_in_memory().unwrap();
        let recs = recommend(&index, fixed_today()).unwrap();
        assert!(recs.is_empty());
    }

    #[test]
    fn bloated_memory_fires_only_when_project_has_real_spend() {
        let index = IndexHandle::open_in_memory().unwrap();
        index
            .write(|c| {
                // Below-threshold memory file - must NOT fire.
                install_memory_component(c, "id-small", "small.md", "/p1/CLAUDE.md", 4_000);
                // Above-threshold memory file but project has zero spend - must NOT fire.
                install_memory_component(
                    c,
                    "id-poor",
                    "poor.md",
                    "/poor-project/CLAUDE.md",
                    20_000,
                );
                // Above-threshold memory file in a project with real spend - MUST fire.
                install_memory_component(
                    c,
                    "id-rich",
                    "rich.md",
                    "/rich-project/CLAUDE.md",
                    50_000,
                );
                install_token_row(
                    c,
                    "claude-code",
                    "/rich-project",
                    "claude-sonnet-4-7",
                    "2026-05-01",
                    100,
                    100_000,
                    20_000,
                    0,
                    25.0,
                );
                Ok(())
            })
            .unwrap();
        let recs = recommend(&index, fixed_today()).unwrap();
        let bloated: Vec<_> = recs
            .iter()
            .filter(|r| r.kind == CostRecKind::BloatedMemory)
            .collect();
        assert_eq!(
            bloated.len(),
            1,
            "only the rich project's memory should fire, got {recs:#?}"
        );
        assert_eq!(bloated[0].related_components, vec!["id-rich".to_string()]);
        assert!(bloated[0].estimated_savings_usd_30d > 0.0);
    }

    #[test]
    fn low_cache_hit_rate_fires_only_above_spend_threshold() {
        let index = IndexHandle::open_in_memory().unwrap();
        index
            .write(|c| {
                // Project A: $25 spend, 20% cache hit rate -> MUST fire.
                install_token_row(
                    c,
                    "claude-code",
                    "/proj-a",
                    "claude-sonnet-4-7",
                    "2026-05-01",
                    50,
                    800_000,
                    50_000,
                    200_000,
                    25.0,
                );
                // Project B: $10 spend, 10% cache hit rate -> NOT fire (below spend).
                install_token_row(
                    c,
                    "claude-code",
                    "/proj-b",
                    "claude-sonnet-4-7",
                    "2026-05-01",
                    50,
                    900_000,
                    50_000,
                    100_000,
                    10.0,
                );
                // Project C: $50 spend, 80% cache hit rate -> NOT fire (above baseline).
                install_token_row(
                    c,
                    "claude-code",
                    "/proj-c",
                    "claude-sonnet-4-7",
                    "2026-05-01",
                    50,
                    200_000,
                    50_000,
                    800_000,
                    50.0,
                );
                Ok(())
            })
            .unwrap();
        let recs = recommend(&index, fixed_today()).unwrap();
        let low_cache: Vec<_> = recs
            .iter()
            .filter(|r| r.kind == CostRecKind::LowCacheHitRate)
            .collect();
        assert_eq!(
            low_cache.len(),
            1,
            "only proj-a should fire LowCacheHitRate, got {recs:#?}"
        );
        assert_eq!(low_cache[0].project_path, "/proj-a");
    }

    #[test]
    fn old_model_on_hot_project_fires_only_for_top_three_with_opus_majority() {
        let index = IndexHandle::open_in_memory().unwrap();
        index
            .write(|c| {
                // Top spender, 100% Opus: MUST fire.
                install_token_row(
                    c,
                    "claude-code",
                    "/hot-project",
                    "claude-opus-4-7",
                    "2026-05-01",
                    100,
                    500_000,
                    100_000,
                    50_000,
                    100.0,
                );
                // Smaller project, 100% Opus, NOT in top 3 -> would still
                // fire because the top-3 cap is generous. We add three
                // larger Sonnet projects to push it out.
                for i in 0..3 {
                    install_token_row(
                        c,
                        "claude-code",
                        &format!("/sonnet-bigger-{i}"),
                        "claude-sonnet-4-7",
                        "2026-05-01",
                        100,
                        500_000,
                        100_000,
                        50_000,
                        90.0,
                    );
                }
                install_token_row(
                    c,
                    "claude-code",
                    "/tiny-opus",
                    "claude-opus-4-7",
                    "2026-05-01",
                    10,
                    50_000,
                    10_000,
                    5_000,
                    10.0,
                );
                Ok(())
            })
            .unwrap();
        let recs = recommend(&index, fixed_today()).unwrap();
        let opus: Vec<_> = recs
            .iter()
            .filter(|r| r.kind == CostRecKind::OldModelOnHotProject)
            .collect();
        assert_eq!(
            opus.len(),
            1,
            "only the hot opus project should fire (top spender), got {recs:#?}"
        );
        assert_eq!(opus[0].project_path, "/hot-project");
    }

    #[test]
    fn recommendations_capped_at_five_and_sorted_by_savings() {
        let index = IndexHandle::open_in_memory().unwrap();
        index
            .write(|c| {
                // Build several LowCacheHitRate-eligible projects with
                // varying spend. The capped output must be the top 5
                // sorted by savings desc.
                for i in 0..7 {
                    let spend = 30.0 + f64::from(i) * 10.0;
                    let path = format!("/p-{i}");
                    install_token_row(
                        c,
                        "claude-code",
                        &path,
                        "claude-sonnet-4-7",
                        "2026-05-01",
                        100,
                        1_000_000 + i64::from(i) * 100_000,
                        100_000,
                        50_000,
                        spend,
                    );
                }
                Ok(())
            })
            .unwrap();
        let recs = recommend(&index, fixed_today()).unwrap();
        assert!(recs.len() <= 5, "must cap at 5 recs, got {}", recs.len());
        // Sorted descending.
        for window in recs.windows(2) {
            assert!(
                window[0].estimated_savings_usd_30d >= window[1].estimated_savings_usd_30d,
                "recs not sorted: {:?} then {:?}",
                window[0].estimated_savings_usd_30d,
                window[1].estimated_savings_usd_30d
            );
        }
    }
}
