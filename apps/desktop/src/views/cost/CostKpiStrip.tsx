/**
 * Top KPI strip for the Cost view.
 *
 * Renders three headline figures - 30d $, 30d tokens, top project. The
 * strip is the calling card for the entire view; if these numbers are
 * wrong the user will not trust the rest of the layout.
 */
import { useMemo } from "react";
import type { SummaryResponse } from "@aseye/shared-types";
import {
  formatTokenCount,
  formatUsd,
  shortenProjectPath,
  totalTokens,
} from "./format";

interface CostKpiStripProps {
  summary: SummaryResponse | undefined;
  isLoading: boolean;
}

export function CostKpiStrip({
  summary,
  isLoading,
}: CostKpiStripProps): React.ReactElement {
  const tokens = useMemo<string>(
    () => (summary ? formatTokenCount(totalTokens(summary.tokens30d)) : "-"),
    [summary],
  );

  if (isLoading && !summary) {
    return (
      <section className="cost-kpi-strip" aria-labelledby="cost-kpi-heading">
        <h3 id="cost-kpi-heading" className="visually-hidden">
          Headline cost figures (last 30 days)
        </h3>
        <div className="stat-grid cost-kpi-grid">
          <CostKpiSkeleton label="30d cost" />
          <CostKpiSkeleton label="30d tokens" />
          <CostKpiSkeleton label="top project" />
        </div>
      </section>
    );
  }

  return (
    <section className="cost-kpi-strip" aria-labelledby="cost-kpi-heading">
      <h3 id="cost-kpi-heading" className="visually-hidden">
        Headline cost figures (last 30 days)
      </h3>
      <div className="stat-grid cost-kpi-grid">
        <div>
          <strong>{formatUsd(summary?.costUsd30d ?? 0)}</strong>
          <span>30d cost (approx.)</span>
        </div>
        <div>
          <strong>{tokens}</strong>
          <span>30d tokens</span>
        </div>
        <div>
          <strong title={summary?.topProject}>
            {summary?.topProject
              ? shortenProjectPath(summary.topProject)
              : "-"}
          </strong>
          <span>
            top project · {formatUsd(summary?.topProjectCost ?? 0)}
          </span>
        </div>
      </div>
    </section>
  );
}

function CostKpiSkeleton({ label }: { label: string }): React.ReactElement {
  return (
    <div aria-busy="true">
      <strong>
        <span className="skeleton-block" style={{ width: "60%" }} />
      </strong>
      <span>{label}</span>
    </div>
  );
}
