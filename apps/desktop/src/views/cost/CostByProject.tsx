/**
 * By-project horizontal bar list.
 *
 * Pure HTML/CSS bars - no SVG, no chart library. Each row's fill is a
 * percentage of the leader's spend. The leader stays visually dominant
 * which sets the eye scale for the rest of the list at a glance.
 */
import type { ByProjectRow } from "@aseye/shared-types";
import { formatUsd, shortenProjectPath } from "./format";

interface CostByProjectProps {
  rows: ReadonlyArray<ByProjectRow>;
  isLoading: boolean;
  /** Cap shown to keep the list scannable. Excess rows roll into the footer. */
  limit?: number;
}

const DEFAULT_LIMIT = 10;

export function CostByProject({
  rows,
  isLoading,
  limit = DEFAULT_LIMIT,
}: CostByProjectProps): React.ReactElement {
  if (isLoading && rows.length === 0) {
    return (
      <section className="cost-pane" aria-labelledby="cost-by-project-heading">
        <h3 id="cost-by-project-heading">By project</h3>
        <div className="cost-bar-list" aria-busy="true">
          {[0, 1, 2, 3].map((k) => (
            <div className="cost-bar-row" key={k}>
              <span className="cost-bar-label">
                <span className="skeleton-block" style={{ width: "60%" }} />
              </span>
              <span className="cost-bar-track" aria-hidden="true">
                <span className="skeleton-block" style={{ width: "40%" }} />
              </span>
              <span className="cost-bar-value">
                <span className="skeleton-block" style={{ width: "70%" }} />
              </span>
            </div>
          ))}
        </div>
      </section>
    );
  }

  if (rows.length === 0) {
    return (
      <section className="cost-pane" aria-labelledby="cost-by-project-heading">
        <h3 id="cost-by-project-heading">By project</h3>
        <p className="settings-todo">No project spend in the last 30 days.</p>
      </section>
    );
  }

  const max = rows.reduce((acc, r) => Math.max(acc, r.costUsd), 0);
  const visible = rows.slice(0, limit);
  const overflow = rows.length - visible.length;

  return (
    <section className="cost-pane" aria-labelledby="cost-by-project-heading">
      <h3 id="cost-by-project-heading">By project</h3>
      <ol className="cost-bar-list" aria-label="cost per project">
        {visible.map((row) => {
          const pct = max > 0 ? Math.max(2, (row.costUsd / max) * 100) : 0;
          const label = shortenProjectPath(row.project);
          return (
            <li className="cost-bar-row" key={row.project}>
              <span className="cost-bar-label" title={row.project}>
                {label}
              </span>
              <span
                className="cost-bar-track"
                role="img"
                aria-label={`${label}: ${formatUsd(row.costUsd)}`}
              >
                <span
                  className="cost-bar-fill"
                  style={{ width: `${pct.toFixed(1)}%` }}
                />
              </span>
              <span className="cost-bar-value">{formatUsd(row.costUsd)}</span>
            </li>
          );
        })}
      </ol>
      {overflow > 0 ? (
        <p className="cost-bar-footnote">
          +{overflow} more project{overflow === 1 ? "" : "s"} not shown
        </p>
      ) : null}
    </section>
  );
}
