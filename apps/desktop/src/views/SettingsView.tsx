import { useUi, type Theme, type Density, type McpProbingMode, type UpdateChannel } from "@/store/ui";
import { detectedToolsFixture } from "@/lib/fixtures";
import { DiagnosticsPanel } from "@/components/DiagnosticsPanel";

/**
 * Settings view (Phase 4.4).
 *
 * Sections mirror docs/06 F14 "Settings": General, Tools, Index, Health,
 * Privacy, Updates, Diagnostics.
 *
 * IPC reads/writes for the Tools section and the Index buttons land in
 * Phase 1.6 / 2.1. For now this view consumes the static fixture and the
 * Zustand store; mutating actions are wired to no-op handlers with a clear
 * TODO marker.
 */
/**
 * Build-time constant injected by Vite (`vite.config.ts::define`). Single
 * source of truth for the app version; the Diagnostics panel uses the
 * same value.
 */
const APP_VERSION = __APP_VERSION__;

/**
 * Default sidecar / index DB path per `docs/05-data-architecture.md`.
 * Hard-coded for the current platform until we wire `@tauri-apps/plugin-os`
 * (Phase 6.x). The macOS path is shown by default; Linux/Windows are listed
 * for context.
 */
const DB_PATHS = {
  macos: "~/Library/Application Support/AllSeeingEye/index.sqlite",
  linux: "$XDG_DATA_HOME/AllSeeingEye/index.sqlite",
  windows: "%APPDATA%/AllSeeingEye/index.sqlite",
} as const;

function detectPlatform(): keyof typeof DB_PATHS {
  if (typeof navigator === "undefined") return "macos";
  const platform = navigator.platform.toLowerCase();
  if (platform.includes("mac")) return "macos";
  if (platform.includes("win")) return "windows";
  return "linux";
}

function GeneralPane() {
  const theme = useUi((s) => s.theme);
  const setTheme = useUi((s) => s.setTheme);
  const density = useUi((s) => s.density);
  const setDensity = useUi((s) => s.setDensity);
  const reducedMotion = useUi((s) => s.reducedMotion);
  const setReducedMotion = useUi((s) => s.setReducedMotion);

  return (
    <section className="health-pane settings-pane" aria-labelledby="settings-general">
      <h3 id="settings-general">General</h3>

      <div className="settings-row">
        <div className="settings-row-label">
          <strong>Theme</strong>
          <small>Dark is the default. System follows your OS preference.</small>
        </div>
        <div className="segmented" role="radiogroup" aria-label="theme">
          {(["dark", "light", "system"] as const).map((t) => (
            <button
              key={t}
              type="button"
              role="radio"
              aria-checked={theme === t}
              className={theme === t ? "active" : ""}
              onClick={() => setTheme(t as Theme)}
            >
              {t}
            </button>
          ))}
        </div>
      </div>

      <div className="settings-row">
        <div className="settings-row-label">
          <strong>Density</strong>
          <small>Compact reduces row height to 40px.</small>
        </div>
        <div className="segmented" role="radiogroup" aria-label="density">
          {(["comfortable", "compact"] as const).map((d) => (
            <button
              key={d}
              type="button"
              role="radio"
              aria-checked={density === d}
              className={density === d ? "active" : ""}
              onClick={() => setDensity(d as Density)}
            >
              {d}
            </button>
          ))}
        </div>
      </div>

      <div className="settings-row">
        <div className="settings-row-label">
          <strong>Reduced motion</strong>
          <small>System defers to your OS preference.</small>
        </div>
        <div className="segmented" role="radiogroup" aria-label="reduced motion">
          {(["system", "on", "off"] as const).map((m) => (
            <button
              key={m}
              type="button"
              role="radio"
              aria-checked={reducedMotion === m}
              className={reducedMotion === m ? "active" : ""}
              onClick={() => setReducedMotion(m)}
            >
              {m}
            </button>
          ))}
        </div>
      </div>

      <div className="settings-row">
        <div className="settings-row-label">
          <strong>Dyslexia-friendly font</strong>
          <small className="settings-todo">
            Coming soon - bundled font + CSS variable still pending.
          </small>
        </div>
        <button type="button" className="text-button quiet" disabled>
          enable
        </button>
      </div>
    </section>
  );
}

function ToolsPane() {
  // TODO(phase-1.6): replace with `invoke<DetectedTool[]>('list_tools')` and
  // wire the `indexed` toggle to `invoke('set_tool_indexed', ...)`. Until
  // then we read from the static fixture.
  return (
    <section className="health-pane settings-pane" aria-labelledby="settings-tools">
      <h3 id="settings-tools">Tools</h3>
      <div className="settings-tools-list">
        {detectedToolsFixture.map((tool) => (
          <div key={tool.id} className="settings-tool-row">
            <span className={`tool-dot ${tool.dotClass}`} />
            <div>
              <strong>{tool.displayName}</strong>
              <div className="mono">{tool.rootPath}</div>
            </div>
            <span className={`health-pill ${tool.indexed ? "up" : "cold"}`}>
              {tool.indexed ? "indexed" : "skipped"}
            </span>
            <button
              type="button"
              className="text-button quiet"
              aria-pressed={tool.indexed}
              aria-label={`toggle indexing for ${tool.displayName}`}
              // TODO(phase-1.6): invoke('set_tool_indexed', { id, indexed })
            >
              {tool.indexed ? "skip" : "index"}
            </button>
          </div>
        ))}
      </div>
    </section>
  );
}

function IndexPane() {
  const platform = detectPlatform();
  const dbPath = DB_PATHS[platform];

  return (
    <section className="health-pane settings-pane" aria-labelledby="settings-index">
      <h3 id="settings-index">Index</h3>
      <div className="settings-row">
        <div className="settings-row-label">
          <strong>Database path</strong>
          <small className="mono">{dbPath}</small>
        </div>
      </div>
      <div className="settings-row">
        <div className="settings-row-label">
          <strong>Rebuild index</strong>
          <small>Re-parses every file in every detected root.</small>
        </div>
        <button
          type="button"
          className="text-button"
          // TODO(phase-1.6): invoke('rebuild_index')
        >
          rebuild
        </button>
      </div>
      <div className="settings-row">
        <div className="settings-row-label">
          <strong>Reset index</strong>
          <small>Drops all indexed components and starts fresh.</small>
        </div>
        <button
          type="button"
          className="text-button quiet"
          // TODO(phase-1.6): invoke('reset_index')
        >
          reset
        </button>
      </div>
    </section>
  );
}

function HealthPane() {
  const mcpProbing = useUi((s) => s.mcpProbing);
  const setMcpProbing = useUi((s) => s.setMcpProbing);

  return (
    <section className="health-pane settings-pane" aria-labelledby="settings-health">
      <h3 id="settings-health">Health</h3>
      <div className="settings-row">
        <div className="settings-row-label">
          <strong>MCP probing</strong>
          <small>
            Off by default per docs/05. Per-server lets you opt in selectively.
          </small>
        </div>
        <div className="segmented" role="radiogroup" aria-label="mcp probing default">
          {(["off", "per-server", "global"] as const).map((m) => (
            <button
              key={m}
              type="button"
              role="radio"
              aria-checked={mcpProbing === m}
              className={mcpProbing === m ? "active" : ""}
              onClick={() => setMcpProbing(m as McpProbingMode)}
            >
              {m}
            </button>
          ))}
        </div>
      </div>
    </section>
  );
}

function PrivacyPane() {
  function handleDiagnosticsExport(): void {
    // TODO(phase-4.2): write a sanitised JSON to disk via the dialog plugin.
    console.log("[settings] diagnostics export requested (phase 4.2 stub)");
  }

  return (
    <section className="health-pane settings-pane" aria-labelledby="settings-privacy">
      <h3 id="settings-privacy">Privacy</h3>
      <div className="settings-row">
        <div className="settings-row-label">
          <strong>Telemetry</strong>
          <small>Forced off in MVP. Ships post-MVP with explicit opt-in.</small>
        </div>
        <label className="settings-todo">
          <input type="checkbox" checked={false} disabled readOnly />{" "}
          disabled
        </label>
      </div>
      <div className="settings-row">
        <div className="settings-row-label">
          <strong>Diagnostics export</strong>
          <small>Saves a sanitised JSON snapshot for support.</small>
        </div>
        <button
          type="button"
          className="text-button"
          onClick={handleDiagnosticsExport}
        >
          export
        </button>
      </div>
    </section>
  );
}

function UpdatesPane() {
  const channel = useUi((s) => s.updateChannel);
  const setChannel = useUi((s) => s.setUpdateChannel);
  const autoCheck = useUi((s) => s.autoCheckUpdates);
  const setAutoCheck = useUi((s) => s.setAutoCheckUpdates);

  return (
    <section className="health-pane settings-pane" aria-labelledby="settings-updates">
      <h3 id="settings-updates">Updates</h3>
      <div className="settings-row">
        <div className="settings-row-label">
          <strong>Channel</strong>
          <small>Stable receives release builds. Beta receives pre-releases.</small>
        </div>
        <div className="segmented" role="radiogroup" aria-label="update channel">
          {(["stable", "beta"] as const).map((c) => (
            <button
              key={c}
              type="button"
              role="radio"
              aria-checked={channel === c}
              className={channel === c ? "active" : ""}
              onClick={() => setChannel(c as UpdateChannel)}
            >
              {c}
            </button>
          ))}
        </div>
      </div>
      <div className="settings-row">
        <div className="settings-row-label">
          <strong>Auto-check on launch</strong>
          <small>Checks the update channel each time the app starts.</small>
        </div>
        <label>
          <input
            type="checkbox"
            checked={autoCheck}
            onChange={(e) => setAutoCheck(e.target.checked)}
            aria-label="auto-check updates"
          />
        </label>
      </div>
      <div className="settings-row">
        <div className="settings-row-label">
          <strong>Current version</strong>
          <small className="mono">v{APP_VERSION}</small>
        </div>
      </div>
    </section>
  );
}

function DiagnosticsPane() {
  return (
    <section className="health-pane settings-pane" aria-labelledby="settings-diagnostics">
      <h3 id="settings-diagnostics">Diagnostics</h3>
      <DiagnosticsPanel />
    </section>
  );
}

export function SettingsView() {
  const view = useUi((s) => s.view);
  const isActive = view === "settings";

  return (
    <section
      className={`view${isActive ? " active" : ""}`}
      data-view-panel="settings"
      aria-labelledby="settings-heading"
      hidden={!isActive}
    >
      <div className="view-toolbar">
        <h2 id="settings-heading">Settings</h2>
      </div>

      <div className="settings-layout">
        <GeneralPane />
        <ToolsPane />
        <IndexPane />
        <HealthPane />
        <PrivacyPane />
        <UpdatesPane />
        <DiagnosticsPane />
      </div>
    </section>
  );
}
