import { useEffect, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useUi, type Theme, type Density, type McpProbingMode, type UpdateChannel } from "@/store/ui";
import { detectedToolsFixture } from "@/lib/fixtures";
import { DiagnosticsPanel } from "@/components/DiagnosticsPanel";
import { rebuildIndex, startFullScan } from "@/ipc";
import {
  useProjectMemoryRoots,
  useSetProjectMemoryRoots,
} from "@/ipc/hooks";

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

/**
 * Phase 14B - default value for the project-memory-roots textarea
 * mirrors `DEFAULT_PROJECT_MEMORY_ROOTS` in
 * `apps/desktop/src-tauri/src/index/settings.rs`. Kept in sync by
 * convention; if the backend list grows, update this constant too.
 */
const DEFAULT_PROJECT_MEMORY_ROOTS = ["~/Development", "~"] as const;

function rootsToText(roots: ReadonlyArray<string>): string {
  return roots.join("\n");
}

function textToRoots(text: string): string[] {
  return text
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line.length > 0);
}

/**
 * State machine for an asynchronous index-write button (rebuild,
 * reset, full re-scan). Centralised because every button on the Index
 * pane uses the same shape: idle -> running -> done|error, with the
 * `done` and `error` states giving a small status line for screen
 * readers.
 */
type IndexActionState = "idle" | "running" | "done" | "error";

function IndexPane() {
  const platform = detectPlatform();
  const dbPath = DB_PATHS[platform];
  const qc = useQueryClient();

  // Phase 14B - project memory roots. The textarea is the editing
  // surface; persistence rounds through the `get_project_memory_roots`
  // / `set_project_memory_roots` Tauri commands. The walker reads the
  // same `app_settings.projectMemoryRoots` row on every scan, so a
  // saved change takes effect on the next "re-scan now".
  const rootsQuery = useProjectMemoryRoots();
  const setRootsMutation = useSetProjectMemoryRoots();
  const [rootsText, setRootsText] = useState<string>("");
  const [rootsTouched, setRootsTouched] = useState(false);
  const [rescanState, setRescanState] = useState<IndexActionState>("idle");
  const [rebuildState, setRebuildState] = useState<IndexActionState>("idle");
  const [saveError, setSaveError] = useState<string | null>(null);

  // Sync the textarea with the persisted value when it loads, but never
  // overwrite an in-progress edit.
  useEffect(() => {
    if (rootsQuery.data && !rootsTouched) {
      setRootsText(rootsToText(rootsQuery.data));
    }
  }, [rootsQuery.data, rootsTouched]);

  const persistedText = rootsQuery.data
    ? rootsToText(rootsQuery.data)
    : rootsToText(DEFAULT_PROJECT_MEMORY_ROOTS);
  const dirty = rootsTouched && rootsText !== persistedText;

  async function handleSave(): Promise<void> {
    const cleaned = textToRoots(rootsText);
    setSaveError(null);
    try {
      await setRootsMutation.mutateAsync(cleaned);
      setRootsTouched(false);
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      setSaveError(msg);
    }
  }

  async function handleRescan(): Promise<void> {
    setRescanState("running");
    try {
      await startFullScan();
      setRescanState("done");
    } catch (err) {
      console.error("[settings] rescan failed", err);
      setRescanState("error");
    }
  }

  // Issue #5 - "Rebuild" wipes the indexed content but keeps user
  // preferences and re-runs a full scan. The double-click confirm is a
  // window.confirm() because Tauri's dialog plugin is opt-in per call
  // and the existing onboarding / panic flows already use the native
  // browser prompt.
  async function handleRebuild(): Promise<void> {
    if (typeof window !== "undefined" && window.confirm) {
      const ok = window.confirm(
        "This will wipe and rebuild the local index. Components stay on disk. Continue?",
      );
      if (!ok) return;
    }
    setRebuildState("running");
    try {
      await rebuildIndex();
      setRebuildState("done");
      // Every cache could have changed; a blanket invalidation is
      // simpler than enumerating each query key here.
      void qc.invalidateQueries();
    } catch (err) {
      console.error("[settings] rebuild failed", err);
      setRebuildState("error");
    }
  }

  return (
    <section className="health-pane settings-pane" aria-labelledby="settings-index">
      <h3 id="settings-index">Index</h3>
      <div className="settings-row">
        <div className="settings-row-label">
          <strong>Database path</strong>
          <small className="mono">{dbPath}</small>
        </div>
      </div>

      <div className="settings-row settings-row-stacked">
        <div className="settings-row-label">
          <strong>Project memory roots</strong>
          <small>
            Where to look for project-level CLAUDE.md / AGENTS.md / GEMINI.md
            files. One path per line. Tilde expansion supported.
          </small>
        </div>
        <label className="field" aria-label="project memory roots">
          <span className="sr-only">project memory roots</span>
          <textarea
            value={rootsText}
            rows={4}
            spellCheck={false}
            onChange={(e) => {
              setRootsText(e.target.value);
              setRootsTouched(true);
            }}
            aria-describedby="settings-memory-roots-help"
          />
          <small id="settings-memory-roots-help" className="settings-todo">
            Defaults: {DEFAULT_PROJECT_MEMORY_ROOTS.join(", ")}
          </small>
        </label>
        <div className="settings-row-actions">
          <button
            type="button"
            className="text-button"
            onClick={() => {
              void handleSave();
            }}
            disabled={!dirty || setRootsMutation.isPending}
          >
            {setRootsMutation.isPending ? "saving…" : "save"}
          </button>
          <button
            type="button"
            className="text-button"
            onClick={() => {
              void handleRescan();
            }}
            disabled={rescanState === "running" || dirty}
            title={dirty ? "save changes before re-scanning" : undefined}
          >
            {rescanState === "running" ? "scanning…" : "re-scan now"}
          </button>
          {rescanState === "done" && !dirty ? (
            <small className="settings-todo">scan completed</small>
          ) : null}
          {rescanState === "error" ? (
            <small className="settings-todo">scan failed; see console</small>
          ) : null}
          {saveError ? (
            <small className="settings-todo">save failed: {saveError}</small>
          ) : null}
        </div>
      </div>

      <div className="settings-row">
        <div className="settings-row-label">
          <strong>Rebuild index</strong>
          <small>Re-parses every file in every detected root.</small>
          {rebuildState === "done" ? (
            <small className="settings-todo" role="status" aria-live="polite">
              rebuild complete
            </small>
          ) : null}
          {rebuildState === "error" ? (
            <small className="settings-todo" role="status" aria-live="polite">
              rebuild failed; see console
            </small>
          ) : null}
        </div>
        <button
          type="button"
          className="text-button"
          onClick={() => {
            void handleRebuild();
          }}
          disabled={rebuildState === "running"}
        >
          {rebuildState === "running" ? "rebuilding…" : "rebuild"}
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
