import { useUi } from "@/store/ui";
import { PlusIcon } from "@/components/icons";

interface McpRow {
  server: string;
  status: "up" | "warn" | "error";
  statusLabel: string;
  latency: string;
  calls: string;
}

const MCP_ROWS: McpRow[] = [
  { server: "github", status: "warn", statusLabel: "degraded", latency: "142 ms", calls: "1,432" },
  { server: "stripe", status: "up", statusLabel: "up", latency: "89 ms", calls: "204" },
  { server: "playwright", status: "up", statusLabel: "up", latency: "1.2 s", calls: "312" },
  { server: "sentry", status: "error", statusLabel: "down", latency: "timeout", calls: "7" },
];

export function HealthView() {
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
        <button type="button" className="text-button">
          <PlusIcon />
          probe selected
        </button>
      </div>

      <div className="health-layout">
        <section className="health-pane" aria-labelledby="mcp-heading">
          <h3 id="mcp-heading">MCP servers</h3>
          <div className="health-table">
            <div className="health-row head">
              <span>server</span>
              <span>status</span>
              <span>latency</span>
              <span>calls</span>
            </div>
            {MCP_ROWS.map((r) => (
              <div key={r.server} className="health-row">
                <span>{r.server}</span>
                <span><span className={`health-pill ${r.status}`}>{r.statusLabel}</span></span>
                <span>{r.latency}</span>
                <span>{r.calls}</span>
              </div>
            ))}
          </div>
        </section>

        <section className="health-pane" aria-labelledby="drift-heading">
          <h3 id="drift-heading">Drift</h3>
          <div className="drift-pair">
            <div>
              <strong>Memory</strong>
              <p className="mono">~/.claude/CLAUDE.md</p>
            </div>
            <span className="drift-meter">
              <i style={{ width: "32%" }} />
            </span>
            <div>
              <strong>32% diverged</strong>
              <p className="mono">~/.cursor/AGENTS.md</p>
            </div>
          </div>
          <div className="diff-preview">
            <p><span>-</span> Use frontend-design skill for every web view.</p>
            <p><span>+</span> Apply design tokens before creating React components.</p>
            <p><span>+</span> Preserve Tauri file-system boundaries in UI copy.</p>
          </div>
          <div className="inline-actions">
            <button type="button" className="text-button">merge</button>
            <button type="button" className="text-button quiet">adopt left</button>
            <button type="button" className="text-button quiet">ignore 30d</button>
          </div>
        </section>

        <section className="health-pane stats-pane" aria-labelledby="usage-heading">
          <h3 id="usage-heading">Usage</h3>
          <div className="stat-grid">
            <div><strong>42</strong><span>skills used 7d</span></div>
            <div><strong>57</strong><span>skills used 30d</span></div>
            <div><strong>18</strong><span>cold 90d</span></div>
            <div><strong>5</strong><span>plugins indexed</span></div>
          </div>
        </section>
      </div>
    </section>
  );
}
