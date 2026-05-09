/**
 * 30-day sparkline. Inline SVG; no chart library.
 *
 * The strip is decorative - the headline numbers live in the KPI strip
 * up top. The sparkline conveys shape (spike vs steady) at a glance.
 * For accessibility the SVG carries an `aria-label` that summarises
 * the range and peak day so screen readers don't get a meaningless
 * `<svg>` announcement.
 */
import { useMemo } from "react";
import type { ByDayRow } from "@aseye/shared-types";
import { buildSparklinePoints, formatUsd } from "./format";

interface CostByDayProps {
  rows: ReadonlyArray<ByDayRow>;
  isLoading: boolean;
}

const VIEWBOX_WIDTH = 300;
const VIEWBOX_HEIGHT = 40;

export function CostByDay({
  rows,
  isLoading,
}: CostByDayProps): React.ReactElement {
  const points = useMemo<string>(
    () => buildSparklinePoints(rows, VIEWBOX_WIDTH, VIEWBOX_HEIGHT),
    [rows],
  );

  const peak = useMemo<ByDayRow | null>(() => {
    let acc: ByDayRow | null = null;
    for (const row of rows) {
      if (!acc || row.costUsd > acc.costUsd) acc = row;
    }
    return acc;
  }, [rows]);

  const ariaLabel = useMemo<string>(() => {
    if (rows.length === 0) return "no spend in the last 30 days";
    const range = `${rows[0]?.day ?? "?"} to ${rows[rows.length - 1]?.day ?? "?"}`;
    if (!peak) return `daily spend, ${range}`;
    return `daily spend, ${range}; peak ${formatUsd(peak.costUsd)} on ${peak.day}`;
  }, [rows, peak]);

  if (isLoading && rows.length === 0) {
    return (
      <section className="cost-pane cost-day-pane" aria-labelledby="cost-by-day-heading">
        <h3 id="cost-by-day-heading">Last 30 days</h3>
        <div className="cost-sparkline" aria-busy="true">
          <span className="skeleton-block" style={{ height: 40 }} />
        </div>
      </section>
    );
  }

  if (rows.length === 0) {
    return (
      <section className="cost-pane cost-day-pane" aria-labelledby="cost-by-day-heading">
        <h3 id="cost-by-day-heading">Last 30 days</h3>
        <p className="settings-todo">No daily spend recorded.</p>
      </section>
    );
  }

  return (
    <section className="cost-pane cost-day-pane" aria-labelledby="cost-by-day-heading">
      <h3 id="cost-by-day-heading">Last 30 days</h3>
      <svg
        className="cost-sparkline"
        viewBox={`0 0 ${VIEWBOX_WIDTH} ${VIEWBOX_HEIGHT}`}
        preserveAspectRatio="none"
        role="img"
        aria-label={ariaLabel}
      >
        <polyline
          fill="none"
          stroke="var(--accent-2)"
          strokeWidth={1.5}
          strokeLinejoin="round"
          strokeLinecap="round"
          points={points}
        />
      </svg>
      <div className="cost-day-meta" aria-hidden="true">
        <span>{rows[0]?.day ?? ""}</span>
        <span>peak {peak ? formatUsd(peak.costUsd) : "-"}</span>
        <span>{rows[rows.length - 1]?.day ?? ""}</span>
      </div>
    </section>
  );
}
