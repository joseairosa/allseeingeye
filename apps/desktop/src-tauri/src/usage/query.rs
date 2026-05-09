//! Read-side queries backing the `usage_query` IPC command.
//!
//! Four query shapes:
//!
//! - `summary`         - 30-day totals + top project (the headline KPI).
//! - `by-project`      - one row per project, cost desc.
//! - `by-day`          - one row per day, ascending date order (sparkline).
//! - `recommendations` - the [`super::recommend`] heuristics.
//!
//! All queries are scoped to the last 30 days unless we add range
//! parameters in v2. Today's cutoff is computed once per call so the
//! handler is deterministic.

use rusqlite::params;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::recommend::CostRec;
use super::types::TokenTotals;
use super::unix_now_secs;
use crate::index::{IndexHandle, Result as IndexResult};

/// Cutoff window for all summary queries (days). Hardcoded for v1.
const WINDOW_DAYS: i64 = 30;

/// IPC discriminator. Mirrors the spec's `CostQuery` literal union.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../bindings/usage/CostQuery.ts")]
#[ts(rename_all = "camelCase")]
pub enum CostQuery {
    Summary,
    ByProject,
    ByDay,
    Recommendations,
}

/// Headline summary returned by `kind = "summary"`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../bindings/usage/SummaryResponse.ts")]
#[ts(rename_all = "camelCase")]
pub struct SummaryResponse {
    pub tokens_30d: TokenTotals,
    pub cost_usd_30d: f64,
    pub top_project: String,
    pub top_project_cost: f64,
    /// `refreshed_at` of the most-recent row read. 0 when the table
    /// is empty.
    pub refreshed_at: i64,
    /// Snapshot identifier of the price table used to compute
    /// `cost_usd_30d`. Surfaced verbatim in the Cost view footer so
    /// the UI cannot drift from the actual prices that were applied.
    pub price_table_version: String,
}

/// One row in `kind = "byProject"`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../bindings/usage/ByProjectRow.ts")]
#[ts(rename_all = "camelCase")]
pub struct ByProjectRow {
    pub project: String,
    pub cost_usd: f64,
    pub tokens: TokenTotals,
}

/// Per-project sub-summary used by [`by_project`].
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct ProjectSummary {
    pub project: String,
    pub cost_usd: f64,
    pub tokens: TokenTotals,
}

/// One row in `kind = "byDay"`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../bindings/usage/ByDayRow.ts")]
#[ts(rename_all = "camelCase")]
pub struct ByDayRow {
    pub day: String,
    pub cost_usd: f64,
    pub tokens: TokenTotals,
}

/// Tagged union returned by `usage_query`. Mirrors the spec's
/// `CostResponse` shape.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[ts(export, export_to = "../bindings/usage/CostResponse.ts")]
#[ts(rename_all = "camelCase")]
pub enum CostResponse {
    Summary(SummaryResponse),
    ByProject { rows: Vec<ByProjectRow> },
    ByDay { rows: Vec<ByDayRow> },
    Recommendations { recs: Vec<CostRec> },
}

/// Compute the YYYY-MM-DD cutoff string for the 30-day window.
fn cutoff_day(today_unix: i64) -> String {
    let cutoff = today_unix - WINDOW_DAYS * 86_400;
    super::recommend::day_string_for_test(cutoff)
}

/// `kind = "summary"` handler.
pub fn summary(index: &IndexHandle, today_unix: i64) -> IndexResult<SummaryResponse> {
    let cutoff = cutoff_day(today_unix);
    index.read(|conn| {
        // Headline totals (single aggregate row).
        let totals_row = conn.query_row(
            "SELECT COALESCE(SUM(input), 0),
                    COALESCE(SUM(output), 0),
                    COALESCE(SUM(cache_read), 0),
                    COALESCE(SUM(cache_create), 0),
                    COALESCE(SUM(est_cost_usd), 0.0),
                    COALESCE(MAX(refreshed_at), 0)
               FROM token_usage
              WHERE day >= ?1",
            params![cutoff],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, f64>(4)?,
                    row.get::<_, i64>(5)?,
                ))
            },
        )?;
        let (input, output, cache_read, cache_create, total_cost, refreshed_at) = totals_row;

        // Top project.
        let top: Option<(String, f64)> = conn
            .query_row(
                "SELECT project_path, SUM(est_cost_usd) AS s
                   FROM token_usage
                  WHERE day >= ?1
                  GROUP BY project_path
                  ORDER BY s DESC
                  LIMIT 1",
                params![cutoff],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?)),
            )
            .ok();
        let (top_project, top_project_cost) = top.unwrap_or_default();

        Ok(SummaryResponse {
            tokens_30d: TokenTotals {
                #[allow(clippy::cast_sign_loss)]
                input: input.max(0) as u64,
                #[allow(clippy::cast_sign_loss)]
                output: output.max(0) as u64,
                #[allow(clippy::cast_sign_loss)]
                cache_read: cache_read.max(0) as u64,
                #[allow(clippy::cast_sign_loss)]
                cache_create: cache_create.max(0) as u64,
            },
            cost_usd_30d: total_cost,
            top_project,
            top_project_cost,
            refreshed_at,
            price_table_version: super::pricing::PRICE_TABLE_VERSION.to_owned(),
        })
    })
}

/// `kind = "byProject"` handler. Rows ordered by cost desc.
pub fn by_project(index: &IndexHandle, today_unix: i64) -> IndexResult<Vec<ByProjectRow>> {
    let cutoff = cutoff_day(today_unix);
    index.read(|conn| {
        let mut stmt = conn.prepare(
            "SELECT project_path,
                    SUM(input), SUM(output), SUM(cache_read), SUM(cache_create),
                    SUM(est_cost_usd)
               FROM token_usage
              WHERE day >= ?1
              GROUP BY project_path
              ORDER BY SUM(est_cost_usd) DESC",
        )?;
        let rows = stmt.query_map(params![cutoff], |row| {
            Ok(ByProjectRow {
                project: row.get::<_, String>(0)?,
                tokens: TokenTotals {
                    #[allow(clippy::cast_sign_loss)]
                    input: row.get::<_, i64>(1)?.max(0) as u64,
                    #[allow(clippy::cast_sign_loss)]
                    output: row.get::<_, i64>(2)?.max(0) as u64,
                    #[allow(clippy::cast_sign_loss)]
                    cache_read: row.get::<_, i64>(3)?.max(0) as u64,
                    #[allow(clippy::cast_sign_loss)]
                    cache_create: row.get::<_, i64>(4)?.max(0) as u64,
                },
                cost_usd: row.get::<_, f64>(5)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    })
}

/// `kind = "byDay"` handler. Rows ordered by day asc.
pub fn by_day(index: &IndexHandle, today_unix: i64) -> IndexResult<Vec<ByDayRow>> {
    let cutoff = cutoff_day(today_unix);
    index.read(|conn| {
        let mut stmt = conn.prepare(
            "SELECT day,
                    SUM(input), SUM(output), SUM(cache_read), SUM(cache_create),
                    SUM(est_cost_usd)
               FROM token_usage
              WHERE day >= ?1
              GROUP BY day
              ORDER BY day ASC",
        )?;
        let rows = stmt.query_map(params![cutoff], |row| {
            Ok(ByDayRow {
                day: row.get::<_, String>(0)?,
                tokens: TokenTotals {
                    #[allow(clippy::cast_sign_loss)]
                    input: row.get::<_, i64>(1)?.max(0) as u64,
                    #[allow(clippy::cast_sign_loss)]
                    output: row.get::<_, i64>(2)?.max(0) as u64,
                    #[allow(clippy::cast_sign_loss)]
                    cache_read: row.get::<_, i64>(3)?.max(0) as u64,
                    #[allow(clippy::cast_sign_loss)]
                    cache_create: row.get::<_, i64>(4)?.max(0) as u64,
                },
                cost_usd: row.get::<_, f64>(5)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    })
}

/// Run the recommendations engine - just a thin wrapper for symmetry
/// with the other query handlers.
pub fn recommendations(index: &IndexHandle, today_unix: i64) -> IndexResult<Vec<CostRec>> {
    super::recommend::recommend(index, today_unix)
}

/// Convenience: dispatch an enum query to the right handler.
pub fn dispatch(index: &IndexHandle, query: CostQuery) -> IndexResult<CostResponse> {
    let now = unix_now_secs();
    Ok(match query {
        CostQuery::Summary => CostResponse::Summary(summary(index, now)?),
        CostQuery::ByProject => CostResponse::ByProject {
            rows: by_project(index, now)?,
        },
        CostQuery::ByDay => CostResponse::ByDay {
            rows: by_day(index, now)?,
        },
        CostQuery::Recommendations => CostResponse::Recommendations {
            recs: recommendations(index, now)?,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixed_today() -> i64 {
        // 2026-05-08 UTC.
        1_778_198_400
    }

    fn install_row(
        index: &IndexHandle,
        project: &str,
        day: &str,
        input: i64,
        output: i64,
        cost: f64,
    ) {
        index
            .write(|c| {
                c.execute(
                    "INSERT INTO token_usage (tool, project_path, model, day,
                         sessions, turns, input, output, cache_read, cache_create,
                         est_cost_usd, refreshed_at)
                     VALUES ('claude-code', ?1, 'claude-sonnet-4-7', ?2,
                             1, 10, ?3, ?4, 0, 0, ?5, 100)",
                    params![project, day, input, output, cost],
                )?;
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn summary_aggregates_window() {
        let index = IndexHandle::open_in_memory().unwrap();
        install_row(&index, "/proj-a", "2026-05-01", 1000, 500, 5.0);
        install_row(&index, "/proj-b", "2026-05-02", 2000, 800, 8.0);
        // Out of window - 31 days before fixed_today.
        install_row(&index, "/proj-old", "2026-04-01", 9999, 9999, 100.0);

        let s = summary(&index, fixed_today()).unwrap();
        // Expected: 5 + 8 = 13 (proj-old excluded).
        assert!(
            (s.cost_usd_30d - 13.0).abs() < 0.001,
            "cost = {}",
            s.cost_usd_30d
        );
        assert_eq!(s.tokens_30d.input, 3000);
        assert_eq!(s.tokens_30d.output, 1300);
        assert_eq!(s.top_project, "/proj-b");
    }

    #[test]
    fn by_project_orders_by_cost_desc() {
        let index = IndexHandle::open_in_memory().unwrap();
        install_row(&index, "/proj-a", "2026-05-01", 100, 100, 5.0);
        install_row(&index, "/proj-b", "2026-05-01", 100, 100, 20.0);
        install_row(&index, "/proj-c", "2026-05-01", 100, 100, 10.0);

        let rows = by_project(&index, fixed_today()).unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].project, "/proj-b");
        assert_eq!(rows[1].project, "/proj-c");
        assert_eq!(rows[2].project, "/proj-a");
    }

    #[test]
    fn by_day_orders_by_day_asc() {
        let index = IndexHandle::open_in_memory().unwrap();
        install_row(&index, "/p", "2026-05-03", 100, 100, 1.0);
        install_row(&index, "/p", "2026-05-01", 100, 100, 1.0);
        install_row(&index, "/p", "2026-05-02", 100, 100, 1.0);

        let rows = by_day(&index, fixed_today()).unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].day, "2026-05-01");
        assert_eq!(rows[1].day, "2026-05-02");
        assert_eq!(rows[2].day, "2026-05-03");
    }

    #[test]
    fn empty_index_returns_zero_summary() {
        let index = IndexHandle::open_in_memory().unwrap();
        let s = summary(&index, fixed_today()).unwrap();
        // Float exact-zero compare via abs() < EPSILON keeps clippy
        // happy. SUM(...) on an empty table returns the COALESCE
        // default (0.0) without accumulation, so the exact compare
        // would also be safe here.
        assert!(
            s.cost_usd_30d.abs() < f64::EPSILON,
            "expected zero cost on empty index, got {}",
            s.cost_usd_30d
        );
        assert_eq!(s.top_project, "");
        assert_eq!(s.refreshed_at, 0);
    }
}
