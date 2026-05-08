import { useUi, type ViewId } from "@/store/ui";
import {
  componentTypes,
  healthSummaries,
  tools,
} from "@/lib/fixtures";
import {
  NavEditorIcon,
  NavHealthIcon,
  NavInventoryIcon,
  NavMapIcon,
  TypeIcon,
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

function ToolsGroup() {
  const setSearch = useUi((s) => s.setSearch);
  const setView = useUi((s) => s.setView);
  return (
    <section className="side-group" aria-labelledby="tools-label">
      <div className="side-label" id="tools-label">tools</div>
      {tools.map((tool, idx) => (
        <button
          key={tool.id}
          type="button"
          className={`side-row${idx === 0 ? " selected" : ""}`}
          onClick={() => {
            setSearch(`tool:${tool.id}`);
            setView("inventory");
          }}
        >
          <span className={`tool-dot ${tool.dotClass}`} />
          <span>{tool.displayName}</span>
          <span className="side-count">{tool.count}</span>
        </button>
      ))}
      <button type="button" className="side-row quiet">
        <span className="side-icon">+</span>
        <span>Add tool</span>
      </button>
    </section>
  );
}

function TypesGroup() {
  const setSearch = useUi((s) => s.setSearch);
  const setView = useUi((s) => s.setView);
  return (
    <section className="side-group" aria-labelledby="types-label">
      <div className="side-label" id="types-label">types</div>
      {componentTypes.map((t) => (
        <button
          key={t.id}
          type="button"
          className={`side-row${t.hasIssue ? " has-issue" : ""}`}
          onClick={() => {
            setSearch(`type:${t.id}`);
            setView("inventory");
          }}
        >
          <TypeIcon id={t.iconId as Parameters<typeof TypeIcon>[0]["id"]} className="type-mini" />
          <span>{t.displayName}</span>
          <span className="side-count">{t.count}</span>
        </button>
      ))}
    </section>
  );
}

function HealthGroup() {
  const setView = useUi((s) => s.setView);
  return (
    <section className="side-group" aria-labelledby="health-label">
      <div className="side-label" id="health-label">health</div>
      {healthSummaries.map((h) => (
        <button
          key={h.id}
          type="button"
          className="side-row"
          onClick={() => setView("health")}
        >
          <span className={`status-ring ${h.ring}`} />
          <span>{h.label}</span>
          <span className="side-count">{h.count}</span>
        </button>
      ))}
    </section>
  );
}

export function Sidebar() {
  const toggleOnboarding = useUi((s) => s.toggleOnboarding);

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
        <NavButton view="inventory" label="Inventory" count={237} icon={<NavInventoryIcon />} />
        <NavButton view="map" label="Map" icon={<NavMapIcon />} />
        <NavButton view="editor" label="Editor" icon={<NavEditorIcon />} />
        <NavButton view="health" label="Health" alert={2} icon={<NavHealthIcon />} />
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
        <button type="button" className="footer-action">settings</button>
        <span>v0.0.1</span>
      </footer>
    </aside>
  );
}
