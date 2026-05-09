/**
 * Health view (Phase 4.2 wiring + Phase 14B "Bloated memory" pane).
 *
 * Four panes:
 *   - MCP servers: real list from `useComponents({ kind: "mcp" })`. Status
 *     is hard-coded to `unprobed` for MVP because the Rust side does not
 *     yet probe MCP endpoints (v1).
 *   - Drift: empty state pointing at the roadmap.
 *   - Bloated memory (14B): memory components above the size threshold.
 *     Client-side filter on the full memory list keeps the IPC surface
 *     unchanged - the dataset is small (<200 rows on real machines).
 *   - Usage: live totals from `useHealthSummary()`.
 *
 * The view consumes IPC via the existing TanStack Query hooks; the
 * pipeline-event invalidator in `App.tsx` keeps these queries fresh.
 */
import { useMemo } from "react";
import { useUi } from "@/store/ui";
import { useComponents, useHealthSummary } from "@/ipc/hooks";
import { formatRelativeTime } from "@/lib/relativeTime";
import {
  estimateTokens,
  formatBytes,
  formatTokensK,
  OVERSIZED_MEMORY_BYTES,
} from "@/lib/tokens";
import { NavEditorIcon, PlusIcon, ShieldCheckIcon } from "@/components/icons";
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

/**
 * Pull every memory component without paginating - the developer's
 * universe of memory files is bounded to ~200 even on a heavy machine
 * (one per project), so the full list is cheap to fetch.
 */
const MEMORY_FILTER: ComponentFilter = {
  toolId: null,
  kind: "memory",
  scope: null,
  query: null,
  tag: null,
  limit: 500,
  offset: 0,
};

/**
 * Last path segment of the parent directory - used as the "project"
 * label in the Bloated memory list. Falls back to the empty string for
 * memory files at the filesystem root (theoretical edge case).
 */
function projectLabel(path: string): string {
  const parts = path.split("/").filter((s) => s.length > 0);
  if (parts.length < 2) return "";
  return parts[parts.length - 2] ?? "";
}

/**
 * Coerce a `bigint | number` to a plain `number` for comparisons.
 * `ts-rs` emits `i64`/`u64` columns as `bigint`, but Number arithmetic
 * is enough at the byte magnitudes we deal with.
 */
function sizeAsNumber(size: bigint | number): number {
  return typeof size === "bigint" ? Number(size) : size;
}

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

/**
 * Phase 14B - bloated-memory list. Surfaces memory components above
 * the `OVERSIZED_MEMORY_BYTES` threshold so the user can see at a
 * glance which preamble files are inflating every-turn cost. The
 * dataset is filtered client-side on the full memory list - the
 * spec carved out a `health::bloated_memory()` IPC but the full
 * memory list is bounded enough that adding a server-side query is
 * unwarranted.
 */
function BloatedMemoryPane(): React.ReactElement {
  const memories = useComponents(MEMORY_FILTER);
  const selectComponent = useUi((s) => s.selectComponent);
  const setView = useUi((s) => s.setView);

  // Stable rows reference so dependent memos don't tear down on every
  // render of the parent.
  const data = memories.data;
  const oversized = useMemo<ComponentSummary[]>(() => {
    if (!data) return [];
    return data
      .filter((row) => sizeAsNumber(row.size) > OVERSIZED_MEMORY_BYTES)
      .sort((a, b) => sizeAsNumber(b.size) - sizeAsNumber(a.size));
  }, [data]);

  function handleOpenEditor(id: string): void {
    selectComponent(id);
    setView("editor");
  }

  return (
    <section className="health-pane" aria-labelledby="bloat-heading">
      <h3 id="bloat-heading">Bloated memory</h3>
      <p className="settings-todo">
        Memory files larger than {formatBytes(OVERSIZED_MEMORY_BYTES)} are
        loaded on every turn and add real cost.
      </p>
      <div className="health-table">
        <div className="health-row head bloat-row">
          <span>memory file</span>
          <span>project</span>
          <span>size</span>
          <span>tokens</span>
          <span aria-label="actions" />
        </div>
        {memories.isPending ? (
          <div className="health-row bloat-row" aria-live="polite">
            <span className="settings-todo">loading…</span>
          </div>
        ) : oversized.length > 0 ? (
          oversized.map((row) => {
            const tokens = estimateTokens(row.size);
            const project = projectLabel(row.path);
            return (
              <div key={row.id} className="health-row bloat-row">
                <span title={row.path}>{row.displayName?.trim() || row.name}</span>
                <span className="mono">{project || "-"}</span>
                <span>{formatBytes(row.size)}</span>
                <span title={`approx ${tokens.toLocaleString("en-US")} tokens`}>
                  ~{formatTokensK(tokens)} tok
                </span>
                <button
                  type="button"
                  className="text-button quiet"
                  onClick={() => handleOpenEditor(row.id)}
                  aria-label={`open ${row.name} in editor`}
                >
                  <NavEditorIcon />
                  open
                </button>
              </div>
            );
          })
        ) : (
          <div className="health-row bloat-row bloat-empty" aria-live="polite">
            <span className="bloat-empty-icon" aria-hidden="true">
              <ShieldCheckIcon />
            </span>
            <span className="settings-todo">
              No oversized memory files. Your context preamble is lean.
            </span>
          </div>
        )}
      </div>
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
        <BloatedMemoryPane />
        <UsagePane />
      </div>
    </section>
  );
}
