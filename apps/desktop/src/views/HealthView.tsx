/**
 * Health view (Phase 4.2 wiring).
 *
 * Three panes:
 *   - MCP servers: real list from `useComponents({ kind: "mcp" })`. Status
 *     is hard-coded to `unprobed` for MVP because the Rust side does not
 *     yet probe MCP endpoints (v1).
 *   - Drift: empty state pointing at the roadmap.
 *   - Usage: live totals from `useHealthSummary()`.
 *
 * The view consumes IPC via the existing TanStack Query hooks; the
 * pipeline-event invalidator in `App.tsx` keeps these queries fresh.
 */
import { useMemo } from "react";
import { useUi } from "@/store/ui";
import { useComponents, useHealthSummary } from "@/ipc/hooks";
import { formatRelativeTime } from "@/lib/relativeTime";
import { PlusIcon } from "@/components/icons";
import type { ComponentFilter, ComponentSummary } from "@aseye/shared-types";

const MCP_FILTER: ComponentFilter = {
  toolId: null,
  kind: "mcp",
  scope: null,
  query: null,
  tag: null,
  limit: 100,
  offset: 0,
};

interface UsageStat {
  value: string;
  label: string;
}

function McpRow({ row }: { row: ComponentSummary }): React.ReactElement {
  return (
    <div className="health-row">
      <span>{row.displayName?.trim() || row.name}</span>
      <span>
        <span
          className="health-pill unprobed"
          aria-label="MCP probing not yet implemented"
        >
          unprobed
        </span>
      </span>
      <span>{formatRelativeTime(row.mtime)}</span>
      <span>{row.tool}</span>
    </div>
  );
}

function McpPane(): React.ReactElement {
  const mcps = useComponents(MCP_FILTER);

  return (
    <section className="health-pane" aria-labelledby="mcp-heading">
      <h3 id="mcp-heading">MCP servers</h3>
      <div className="health-table">
        <div className="health-row head">
          <span>server</span>
          <span>status</span>
          <span>last seen</span>
          <span>tool</span>
        </div>
        {mcps.isPending ? (
          <div className="health-row" aria-live="polite">
            <span className="settings-todo">loading…</span>
          </div>
        ) : mcps.data && mcps.data.length > 0 ? (
          mcps.data.map((row) => <McpRow key={row.id} row={row} />)
        ) : (
          <div className="health-row">
            <span className="settings-todo">no MCP servers indexed yet</span>
          </div>
        )}
      </div>
    </section>
  );
}

function DriftPane(): React.ReactElement {
  return (
    <section className="health-pane" aria-labelledby="drift-heading">
      <h3 id="drift-heading">Drift</h3>
      <p className="settings-todo">
        Drift detection lands in v1; see docs/10-roadmap.md.
      </p>
    </section>
  );
}

function UsagePane(): React.ReactElement {
  const health = useHealthSummary();

  const stats = useMemo<UsageStat[]>(() => {
    const total = health.data?.totalComponents ?? 0;
    const errors = health.data?.totalParseErrors ?? 0;
    const tools = new Set(
      (health.data?.byToolKind ?? []).map((r) => r.tool),
    ).size;
    const kinds = new Set(
      (health.data?.byToolKind ?? []).map((r) => r.kind),
    ).size;
    return [
      { value: String(total), label: "components indexed" },
      { value: String(errors), label: "parse errors" },
      { value: String(tools), label: "tools tracked" },
      { value: String(kinds), label: "component kinds" },
    ];
  }, [health.data]);

  return (
    <section className="health-pane stats-pane" aria-labelledby="usage-heading">
      <h3 id="usage-heading">Usage</h3>
      <div className="stat-grid">
        {stats.map((s) => (
          <div key={s.label}>
            <strong>{s.value}</strong>
            <span>{s.label}</span>
          </div>
        ))}
      </div>
    </section>
  );
}

export function HealthView(): React.ReactElement {
  const view = useUi((s) => s.view);
  const isActive = view === "health";

  return (
    <section
      className={`view${isActive ? " active" : ""}`}
      data-view-panel="health"
      aria-labelledby="health-heading"
      hidden={!isActive}
    >
      <div className="view-toolbar">
        <h2 id="health-heading">Health</h2>
        <button type="button" className="text-button" disabled>
          <PlusIcon />
          probe selected
        </button>
      </div>

      <div className="health-layout">
        <McpPane />
        <DriftPane />
        <UsagePane />
      </div>
    </section>
  );
}
