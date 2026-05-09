/**
 * Cost view (Phase 14C-frontend).
 *
 * Layout - mirrors the ASCII mock in docs/14-cost-and-memory.md:
 *
 *   ┌──────────────────────────────────────────────────────────────┐
 *   │ Cost                                              refresh    │
 *   │ <KPI strip: $30d / tokens30d / top project>                  │
 *   ├──────────────────────────┬───────────────────────────────────┤
 *   │ <by-project bar list>    │ <by-day sparkline>                │
 *   │                          │ <recommendations panel>           │
 *   └──────────────────────────┴───────────────────────────────────┘
 *
 * IPC: every read goes through the four `useCost*` hooks; the refresh
 * button fires `useCostRefresh` which invalidates the cost cache. The
 * backend lives behind `usage_query` / `usage_refresh` in
 * `apps/desktop/src-tauri/src/ipc/commands.rs`.
 */
import { useUi } from "@/store/ui";
import {
  useCostByDay,
  useCostByProject,
  useCostRecommendations,
  useCostRefresh,
  useCostSummary,
} from "@/ipc/hooks";
import { RefreshIcon } from "@/components/icons";
import { CostKpiStrip } from "./cost/CostKpiStrip";
import { CostByProject } from "./cost/CostByProject";
import { CostByDay } from "./cost/CostByDay";
import { CostRecommendations } from "./cost/CostRecommendations";
import { formatRefreshedAgo } from "./cost/format";

/**
 * Fallback rendered before the first summary payload lands. The
 * authoritative version comes from `summary.data.priceTableVersion`
 * (sourced from `apps/desktop/src-tauri/src/usage/pricing.rs::
 * PRICE_TABLE_VERSION`). The fallback only ever shows for the brief
 * window between mount and first IPC response.
 */
const FALLBACK_PRICE_TABLE_VERSION = "loading";

export function CostView(): React.ReactElement {
  const view = useUi((s) => s.view);
  const isActive = view === "cost";

  const summary = useCostSummary();
  const byProject = useCostByProject();
  const byDay = useCostByDay();
  const recs = useCostRecommendations();
  const refresh = useCostRefresh();

  // Show the refresh button as busy when EITHER the imperative refresh
  // mutation is in flight OR any of the four read queries is fetching;
  // either case means the user just asked for a refresh and the data
  // hasn't fully landed yet.
  const isRefreshing =
    refresh.isPending ||
    summary.isFetching ||
    byProject.isFetching ||
    byDay.isFetching ||
    recs.isFetching;

  const refreshedAtSec = summary.data?.refreshedAt ?? null;

  // Empty state: backend returns refreshedAt=0 when the table is empty.
  // We also fall back to "no spend" if every read came back empty after
  // a successful first pass.
  const hasAnyData =
    summary.data !== undefined &&
    summary.data.refreshedAt !== BigInt(0) &&
    summary.data.costUsd30d > 0;

  const isFirstLoad =
    summary.isPending && !summary.data;

  return (
    <section
      className={`view${isActive ? " active" : ""}`}
      data-view-panel="cost"
      aria-labelledby="cost-heading"
      hidden={!isActive}
    >
      <div className="view-toolbar cost-toolbar">
        <h2 id="cost-heading">Cost</h2>
        <span className="cost-refreshed-meta" aria-live="polite">
          last refreshed {formatRefreshedAgo(refreshedAtSec)}
        </span>
        <button
          type="button"
          className="text-button"
          onClick={() => refresh.mutate()}
          disabled={isRefreshing}
          aria-label="refresh cost analytics"
        >
          <RefreshIcon />
          {isRefreshing ? "refreshing…" : "refresh"}
        </button>
      </div>

      {isFirstLoad ? (
        <CostKpiStrip summary={undefined} isLoading />
      ) : !hasAnyData ? (
        <CostEmptyState />
      ) : (
        <>
          <CostKpiStrip
            summary={summary.data}
            isLoading={summary.isPending}
          />
          <div className="cost-layout">
            <CostByProject
              rows={byProject.data?.rows ?? []}
              isLoading={byProject.isPending}
            />
            <div className="cost-right-column">
              <CostByDay
                rows={byDay.data?.rows ?? []}
                isLoading={byDay.isPending}
              />
              <CostRecommendations
                recs={recs.data?.recs ?? []}
                isLoading={recs.isPending}
              />
            </div>
          </div>
          <p className="cost-pricing-footnote" role="note">
            Pricing snapshot:{" "}
            {summary.data?.priceTableVersion ?? FALLBACK_PRICE_TABLE_VERSION}.
            Verify before quoting.
          </p>
        </>
      )}
    </section>
  );
}

/**
 * Render when the rollup table is empty - either the user has no
 * Claude Code / Codex history yet, or the first refresh hasn't run.
 * Shows the two source tools so users know what feeds the view.
 */
function CostEmptyState(): React.ReactElement {
  return (
    <section className="cost-empty" aria-labelledby="cost-empty-heading">
      <h3 id="cost-empty-heading">No usage data found</h3>
      <p>
        Cost analytics needs at least one Claude Code or Codex session.
      </p>
      <ul className="cost-empty-tools" aria-label="supported tools">
        <li>
          <span className="tool-dot claude" aria-hidden="true" />
          <span>Claude Code</span>
          <small>~/.claude/projects/.../*.jsonl</small>
        </li>
        <li>
          <span className="tool-dot codex" aria-hidden="true" />
          <span>Codex</span>
          <small>~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl</small>
        </li>
      </ul>
    </section>
  );
}
