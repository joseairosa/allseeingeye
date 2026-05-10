import { useMemo } from "react";
import { useUi, type ViewId } from "@/store/ui";
import { useHealthSummary, useSecuritySummary, useTools } from "@/ipc/hooks";
import type { ComponentType, ToolId } from "@aseye/shared-types";
import {
  NavCostIcon,
  NavEditorIcon,
  NavHealthIcon,
  NavInventoryIcon,
  NavMapIcon,
  NavProjectsIcon,
  TypeIcon,
  type TypeIconId,
} from "./icons";

interface NavBtnProps {
  view: ViewId;
  label: string;
  count?: number;
  alert?: number;
  icon: React.ReactNode;
}

function NavButton({ view, label, count, alert, icon }: NavBtnProps) {
  const active = useUi((s) => s.view === view);
  const setView = useUi((s) => s.setView);
  return (
    <button
      type="button"
      className={`nav-item${active ? " active" : ""}`}
      onClick={() => setView(view)}
      aria-current={active ? "page" : undefined}
    >
      <span className="nav-glyph">{icon}</span>
      <span>{label}</span>
      {typeof count === "number" ? <span className="nav-count">{count}</span> : null}
      {typeof alert === "number" ? <span className="nav-alert">{alert}</span> : null}
    </button>
  );
}

const TOOL_DOT: Record<ToolId, "claude" | "codex" | "cursor" | "anti"> = {
  "claude-code": "claude",
  codex: "codex",
  cursor: "cursor",
  antigravity: "anti",
};

interface TypeMeta {
  id: ComponentType;
  displayName: string;
  iconId: TypeIconId;
}

/**
 * The seven first-class types that surface in the sidebar's TYPES
 * group. The remaining `ComponentType` variants exist on the wire but
 * are infrastructural (settings, sessions, statusline, ...) and would
 * crowd the sidebar without earning their place.
 */
const TYPES_IN_SIDEBAR: readonly TypeMeta[] = [
  { id: "skill", displayName: "Skills", iconId: "icon-skill" },
  { id: "agent", displayName: "Agents", iconId: "icon-agent" },
  { id: "command", displayName: "Commands", iconId: "icon-command" },
  { id: "mcp", displayName: "MCP servers", iconId: "icon-mcp" },
  { id: "rule", displayName: "Rules", iconId: "icon-rule" },
  { id: "memory", displayName: "Memory", iconId: "icon-memory" },
  { id: "hook", displayName: "Hooks", iconId: "icon-hook" },
] as const;

function ToolsGroup() {
  const setSearch = useUi((s) => s.setSearch);
  const setView = useUi((s) => s.setView);
  const { data: tools, isPending } = useTools();
  const { data: health } = useHealthSummary();

  const countsByTool = useMemo<Record<string, number>>(() => {
    if (!health) return {};
    const acc: Record<string, number> = {};
    for (const row of health.byToolKind) {
      acc[row.tool] = (acc[row.tool] ?? 0) + row.count;
    }
    return acc;
  }, [health]);

  return (
    <section className="side-group" aria-labelledby="tools-label">
      <div className="side-label" id="tools-label">tools</div>
      {isPending && !tools ? (
        <button type="button" className="side-row quiet" disabled aria-busy="true">
          <span className="side-icon">·</span>
          <span>loading</span>
        </button>
      ) : null}
      {(tools ?? []).map((tool) => {
        const count = countsByTool[tool.id];
        // Undetected tools are truly disabled: filtering to a tool that
        // is not on disk would yield an empty inventory with no
        // explanation. The `quiet` class shows the row dimmed; the
        // `disabled` attribute keeps screen readers, click, and
        // keyboard interaction in sync (the audit found `aria-disabled`
        // alone left clicks reachable).
        return (
          <button
            key={tool.id}
            type="button"
            className={`side-row${tool.detected ? "" : " quiet"}`}
            onClick={() => {
              setSearch(`tool:${tool.id}`);
              setView("inventory");
            }}
            disabled={!tool.detected}
            title={
              tool.detected
                ? undefined
                : `${tool.displayName} not detected on this machine`
            }
          >
            <span className={`tool-dot ${TOOL_DOT[tool.id]}`} />
            <span>{tool.displayName}</span>
            <span className="side-count">
              {tool.detected ? (typeof count === "number" ? count : "-") : "-"}
            </span>
          </button>
        );
      })}
      {/*
        Audit issue #16: an "Add tool" row used to live here but the
        registry is hardcoded - there is no path to register a custom
        tool root at runtime. Rather than tease the affordance, the
        row has been removed. A future "Add custom tool" feature would
        add it back and route to a Settings -> Tools section that owns
        the custom-root list.
      */}
    </section>
  );
}

function TypesGroup() {
  const setSearch = useUi((s) => s.setSearch);
  const setView = useUi((s) => s.setView);
  const { data: health } = useHealthSummary();

  const countsByKind = useMemo<Record<string, number>>(() => {
    if (!health) return {};
    const acc: Record<string, number> = {};
    for (const row of health.byToolKind) {
      acc[row.kind] = (acc[row.kind] ?? 0) + row.count;
    }
    return acc;
  }, [health]);

  return (
    <section className="side-group" aria-labelledby="types-label">
      <div className="side-label" id="types-label">types</div>
      {TYPES_IN_SIDEBAR.map((t) => {
        const count = countsByKind[t.id] ?? 0;
        return (
          <button
            key={t.id}
            type="button"
            className="side-row"
            onClick={() => {
              setSearch(`type:${t.id}`);
              setView("inventory");
            }}
          >
            <TypeIcon id={t.iconId} className="type-mini" />
            <span>{t.displayName}</span>
            <span className="side-count">{count}</span>
          </button>
        );
      })}
    </section>
  );
}

/**
 * Audit issue #18: the Drift / MCP rows used to ship hardcoded "-"
 * counts and just `setView("health")` without telling the view which
 * pane to surface. The Cold row was removed wholesale because a Cold
 * pane does not exist in HealthView. The remaining two rows now show
 * real counts derived from live IPC data and set `healthFocus` so
 * HealthView can scroll to and briefly highlight the matching pane.
 */
type HealthFocusId = "drift" | "mcp";
interface HealthRowMeta {
  id: HealthFocusId;
  label: string;
  ring: "warn" | "error" | "cold";
}

const HEALTH_ROWS: readonly HealthRowMeta[] = [
  { id: "drift", label: "Drift", ring: "warn" },
  { id: "mcp", label: "MCP servers", ring: "cold" },
] as const;

/**
 * Pick the highest-severity status ring colour for the security row -
 * red for any critical, amber for any high, grey otherwise. Mirrors
 * the contract spelt out in `docs/12-security.md` ("Sidebar Health
 * group" bullet) so the row signals severity at a glance without
 * relying on numbers alone.
 */
function pickSecurityRing(
  summary: ReturnType<typeof useSecuritySummary>["data"],
): "error" | "warn" | "cold" {
  if (!summary) return "cold";
  if (summary.bySeverity.critical > 0) return "error";
  if (summary.bySeverity.high > 0) return "error";
  if (summary.bySeverity.medium > 0) return "warn";
  return "cold";
}

function HealthGroup() {
  // Drift detection ships in v1 - until then the row reports "-" and
  // navigating to it focuses the Drift pane (which itself shows the
  // "lands in v1" notice). MCP count is the per-tool-kind aggregate
  // across all detected tools, taken from the existing health summary.
  const setView = useUi((s) => s.setView);
  const setHealthFocus = useUi((s) => s.setHealthFocus);
  const { data: securitySummary } = useSecuritySummary();
  const { data: health } = useHealthSummary();
  const mcpCount = (health?.byToolKind ?? [])
    .filter((row) => row.kind === "mcp")
    .reduce((acc, row) => acc + row.count, 0);
  const countByRow: Record<HealthFocusId, string> = {
    drift: "-",
    mcp: mcpCount > 0 ? String(mcpCount) : "-",
  };
  const securityCount = securitySummary?.total ?? 0;
  const securityRing = pickSecurityRing(securitySummary);
  return (
    <section className="side-group" aria-labelledby="health-label">
      <div className="side-label" id="health-label">health</div>
      {HEALTH_ROWS.map((h) => (
        <button
          key={h.id}
          type="button"
          className="side-row"
          onClick={() => {
            setHealthFocus(h.id);
            setView("health");
          }}
        >
          <span className={`status-ring ${h.ring}`} />
          <span>{h.label}</span>
          <span className="side-count">{countByRow[h.id]}</span>
        </button>
      ))}
      <button
        type="button"
        className="side-row"
        onClick={() => setView("security")}
        aria-label={`Security issues (${securityCount})`}
      >
        <span className={`status-ring ${securityRing}`} />
        <span>Security issues</span>
        <span className="side-count">{securityCount}</span>
      </button>
    </section>
  );
}

export function Sidebar() {
  const toggleOnboarding = useUi((s) => s.toggleOnboarding);
  const setView = useUi((s) => s.setView);
  const { data: health } = useHealthSummary();
  const totalComponents = health?.totalComponents ?? 0;

  return (
    <aside className="sidebar" aria-label="primary navigation">
      <div className="brand-lockup">
        <img src="/assets/eye-logo.svg" alt="" className="brand-logo" />
        <div>
          <div className="brand-name">All Seeing Eye</div>
          <div className="brand-meta">local index online</div>
        </div>
      </div>

      <nav className="nav-section" aria-label="views">
        <NavButton
          view="inventory"
          label="Inventory"
          count={totalComponents}
          icon={<NavInventoryIcon />}
        />
        <NavButton view="map" label="Map" icon={<NavMapIcon />} />
        <NavButton view="editor" label="Editor" icon={<NavEditorIcon />} />
        <NavButton view="health" label="Health" icon={<NavHealthIcon />} />
        <NavButton view="cost" label="Cost" icon={<NavCostIcon />} />
        <NavButton view="projects" label="Projects" icon={<NavProjectsIcon />} />
      </nav>

      <div className="sidebar-scroll">
        <ToolsGroup />
        <TypesGroup />
        <HealthGroup />
      </div>

      <footer className="sidebar-footer">
        <button
          type="button"
          className="footer-action"
          onClick={() => toggleOnboarding(true)}
        >
          tour
        </button>
        <button
          type="button"
          className="footer-action"
          onClick={() => setView("settings")}
        >
          settings
        </button>
        <span>v0.0.1</span>
      </footer>
    </aside>
  );
}
