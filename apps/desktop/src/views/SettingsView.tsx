import { useEffect, useMemo, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { useUi, type Theme, type Density, type McpProbingMode, type UpdateChannel } from "@/store/ui";
import { DiagnosticsPanel } from "@/components/DiagnosticsPanel";
import {
  exportDiagnostics,
  rebuildIndex,
  resetIndex,
  startFullScan,
} from "@/ipc";
import {
  useBackupNow,
  useBackupSetAuto,
  useBackupStatus,
  useBackupVerify,
  useExcludedToolIds,
  useHealthSummary,
  useProjectMemoryRoots,
  useRestoreNow,
  useSetProjectMemoryRoots,
  useSetToolIndexed,
  useTools,
} from "@/ipc/hooks";
import { useDiagnosticsEvents } from "@/lib/diagnosticsRing";
import { buildReport } from "@/lib/diagnosticsReport";
import {
  sanitiseForClipboard,
  type DiagnosticsParseError,
  type DiagnosticsToolEntry,
  type DiagnosticsToolKindCount,
} from "@/lib/diagnosticsSanitiser";

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

      {/*
        Audit issue #13: a "Dyslexia-friendly font" row used to live
        here but the bundled font + CSS variable were never shipped.
        The row has been removed; it returns when an OpenDyslexic-
        compatible asset lands and `--font-ui` can switch on a setting.
      */}
    </section>
  );
}

/**
 * Map a `ToolId` to the design-system colour-dot class. Mirrors the
 * fixture list one-to-one - the fixture is going away once #2 lands
 * but the dot mapping itself is part of the design.
 */
function dotClassFor(toolId: string): string {
  switch (toolId) {
    case "claude-code":
      return "claude";
    case "codex":
      return "codex";
    case "cursor":
      return "cursor";
    case "antigravity":
      return "anti";
    default:
      return "claude";
  }
}

function ToolsPane() {
  // Audit issue #2 - the previous implementation read from a static
  // fixture and the toggle had no handler. Now both come from the
  // live IPC: detection drives the list, and `set_tool_indexed`
  // persists per-tool exclusions in `app_settings.excludedToolIds`.
  const tools = useTools();
  const excluded = useExcludedToolIds();
  const setIndexedMut = useSetToolIndexed();

  // Memoise the lookup so each row's `indexed` boolean is a stable
  // reference; otherwise the row re-renders on every parent tick.
  const excludedSet = useMemo<ReadonlySet<string>>(
    () => new Set(excluded.data ?? []),
    [excluded.data],
  );

  function handleToggle(toolId: string, currentlyIndexed: boolean): void {
    setIndexedMut.mutate({ toolId, indexed: !currentlyIndexed });
  }

  const isPending = tools.isPending || excluded.isPending;
  const isError = tools.isError;

  return (
    <section className="health-pane settings-pane" aria-labelledby="settings-tools">
      <h3 id="settings-tools">Tools</h3>
      {isPending ? (
        <p className="settings-todo" aria-live="polite">
          loading detected tools…
        </p>
      ) : null}
      {isError ? (
        <p className="settings-todo" role="alert">
          could not load tools; check the index process
        </p>
      ) : null}
      {!isPending && !isError && (tools.data?.length ?? 0) === 0 ? (
        <p className="settings-todo">no tools detected</p>
      ) : null}
      <div className="settings-tools-list">
        {(tools.data ?? []).map((tool) => {
          const indexed = !excludedSet.has(tool.id);
          const rootPath = tool.existingRootPaths[0] ?? "(no root detected)";
          return (
            <div key={tool.id} className="settings-tool-row">
              <span className={`tool-dot ${dotClassFor(tool.id)}`} />
              <div>
                <strong>{tool.displayName}</strong>
                <div className="mono">{rootPath}</div>
              </div>
              <span className={`health-pill ${indexed ? "up" : "cold"}`}>
                {indexed ? "indexed" : "skipped"}
              </span>
              <button
                type="button"
                className="text-button quiet"
                aria-pressed={indexed}
                aria-label={`toggle indexing for ${tool.displayName}`}
                onClick={() => handleToggle(tool.id, indexed)}
                disabled={setIndexedMut.isPending}
              >
                {indexed ? "skip" : "index"}
              </button>
            </div>
          );
        })}
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
  const [resetState, setResetState] = useState<IndexActionState>("idle");
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

  // Issue #7 - "Reset" is the destructive sibling: wipes everything,
  // including user preferences. The confirm copy is stronger than
  // rebuild's so a misclick is harder.
  async function handleReset(): Promise<void> {
    if (typeof window !== "undefined" && window.confirm) {
      const ok = window.confirm(
        "This will wipe ALL local index data including settings. Continue?",
      );
      if (!ok) return;
    }
    setResetState("running");
    try {
      await resetIndex();
      setResetState("done");
      void qc.invalidateQueries();
    } catch (err) {
      console.error("[settings] reset failed", err);
      setResetState("error");
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
          disabled={rebuildState === "running" || resetState === "running"}
        >
          {rebuildState === "running" ? "rebuilding…" : "rebuild"}
        </button>
      </div>
      <div className="settings-row">
        <div className="settings-row-label">
          <strong>Reset index</strong>
          <small>Drops all indexed components and starts fresh.</small>
          {resetState === "done" ? (
            <small className="settings-todo" role="status" aria-live="polite">
              reset complete
            </small>
          ) : null}
          {resetState === "error" ? (
            <small className="settings-todo" role="status" aria-live="polite">
              reset failed; see console
            </small>
          ) : null}
        </div>
        <button
          type="button"
          className="text-button quiet"
          onClick={() => {
            void handleReset();
          }}
          disabled={resetState === "running" || rebuildState === "running"}
        >
          {resetState === "running" ? "resetting…" : "reset"}
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

/**
 * Default file name for the Diagnostics export. Date-stamped so a
 * support thread with multiple snapshots stays orderable.
 */
function defaultDiagnosticsFileName(): string {
  const now = new Date();
  const yyyy = now.getFullYear();
  const mm = String(now.getMonth() + 1).padStart(2, "0");
  const dd = String(now.getDate()).padStart(2, "0");
  return `aseye-diagnostics-${yyyy}${mm}${dd}.json`;
}

type DiagnosticsExportState =
  | { kind: "idle" }
  | { kind: "running" }
  | { kind: "saved"; path: string }
  | { kind: "cancelled" }
  | { kind: "error"; message: string };

/**
 * Phase 15: encrypted local backup of every indexed component.
 *
 * The pane shows current status (manifest count, last run, storage
 * dir), exposes the manual "Backup now" action, an auto-backup
 * toggle, and a restore flow gated by a confirm dialog. The
 * cryptography is end-to-end at this point (`backup_now` is the
 * single IPC the user has to remember); the pane is purely a status
 * + control surface.
 */
function BackupPane() {
  const status = useBackupStatus();
  const backupNowMut = useBackupNow();
  const restoreNowMut = useRestoreNow();
  const verifyMut = useBackupVerify();
  const setAutoMut = useBackupSetAuto();

  type Toast =
    | { kind: "success"; message: string }
    | { kind: "error"; message: string };
  const [toast, setToast] = useState<Toast | null>(null);
  const [restoreConfirmOpen, setRestoreConfirmOpen] = useState(false);
  const [lastDryRun, setLastDryRun] = useState<{
    total: number;
    restored: number;
    skippedLocalNewer: number;
    errors: number;
    elapsedMs: bigint;
  } | null>(null);
  const [lastVerify, setLastVerify] = useState<{
    total: number;
    verified: number;
    errors: ReadonlyArray<{
      componentId: string;
      kind: string;
      message: string;
    }>;
    elapsedMs: bigint;
  } | null>(null);

  function pushSuccess(message: string): void {
    setToast({ kind: "success", message });
  }
  function pushError(message: string): void {
    setToast({ kind: "error", message });
  }

  // Dismiss the success toast after a few seconds; errors persist.
  useEffect(() => {
    if (toast === null) return;
    if (toast.kind === "error") return;
    const id = window.setTimeout(() => setToast(null), 4000);
    return () => window.clearTimeout(id);
  }, [toast]);

  function formatLastBackupAt(ts: bigint | null): string {
    if (ts === null) return "never";
    const seconds = Number(ts);
    if (!Number.isFinite(seconds) || seconds === 0) return "never";
    const now = Math.floor(Date.now() / 1000);
    const delta = now - seconds;
    if (delta < 60) return "just now";
    if (delta < 3600) {
      const m = Math.floor(delta / 60);
      return `${m} minute${m === 1 ? "" : "s"} ago`;
    }
    if (delta < 86_400) {
      const h = Math.floor(delta / 3600);
      return `${h} hour${h === 1 ? "" : "s"} ago`;
    }
    const d = Math.floor(delta / 86_400);
    return `${d} day${d === 1 ? "" : "s"} ago`;
  }

  async function handleBackup(): Promise<void> {
    setToast(null);
    try {
      const report = await backupNowMut.mutateAsync();
      const errs = report.errors.length;
      const baseMsg =
        `Backed up ${report.encrypted} of ${report.total} (${report.skippedUnchanged} unchanged)`;
      if (errs > 0) {
        pushError(`${baseMsg}, ${errs} error${errs === 1 ? "" : "s"}`);
      } else {
        pushSuccess(`${baseMsg} in ${report.elapsedMs}ms`);
      }
    } catch (err) {
      pushError(err instanceof Error ? err.message : String(err));
    }
  }

  async function handleDryRun(): Promise<void> {
    setToast(null);
    try {
      const report = await restoreNowMut.mutateAsync(true);
      setLastDryRun({
        total: report.total,
        restored: report.restored,
        skippedLocalNewer: report.skippedLocalNewer,
        errors: report.errors.length,
        elapsedMs: report.elapsedMs,
      });
    } catch (err) {
      pushError(err instanceof Error ? err.message : String(err));
    }
  }

  async function handleRestoreConfirm(): Promise<void> {
    setRestoreConfirmOpen(false);
    setToast(null);
    try {
      const report = await restoreNowMut.mutateAsync(false);
      const errs = report.errors.length;
      const baseMsg =
        `Restored ${report.restored} of ${report.total}` +
        ` (${report.skippedLocalNewer} skipped because local was newer)`;
      if (errs > 0) {
        pushError(`${baseMsg}, ${errs} error${errs === 1 ? "" : "s"}`);
      } else {
        pushSuccess(`${baseMsg} in ${report.elapsedMs}ms`);
      }
    } catch (err) {
      pushError(err instanceof Error ? err.message : String(err));
    }
  }

  function handleAutoToggle(next: boolean): void {
    setAutoMut.mutate(next, {
      onError: (err) => {
        pushError(err instanceof Error ? err.message : String(err));
      },
    });
  }

  async function handleVerify(): Promise<void> {
    setToast(null);
    try {
      const report = await verifyMut.mutateAsync();
      setLastVerify({
        total: report.total,
        verified: report.verified,
        errors: report.errors.map((e) => ({
          componentId: e.componentId,
          kind: String(e.kind),
          message: e.message,
        })),
        elapsedMs: report.elapsedMs,
      });
      const errs = report.errors.length;
      if (errs > 0) {
        pushError(
          `Verified ${report.verified} of ${report.total}; ${errs} integrity issue${errs === 1 ? "" : "s"} found`,
        );
      } else {
        pushSuccess(
          `Verified ${report.verified} of ${report.total} in ${report.elapsedMs}ms; all integrity checks passed`,
        );
      }
    } catch (err) {
      pushError(err instanceof Error ? err.message : String(err));
    }
  }

  const data = status.data;
  const ipcBusy =
    backupNowMut.isPending ||
    verifyMut.isPending ||
    restoreNowMut.isPending ||
    setAutoMut.isPending ||
    status.isFetching;

  return (
    <section
      className="health-pane settings-pane"
      aria-labelledby="settings-backup"
    >
      <h3 id="settings-backup">Backup</h3>

      {toast ? (
        <div
          className="validation-box"
          role={toast.kind === "error" ? "alert" : "status"}
          aria-live="polite"
          data-toast-kind={toast.kind}
        >
          <span>{toast.kind === "error" ? "!" : "✓"}</span>
          <p>{toast.message}</p>
          {toast.kind === "error" ? (
            <button
              type="button"
              className="text-button quiet"
              onClick={() => setToast(null)}
              aria-label="dismiss"
              style={{ marginLeft: "auto" }}
            >
              dismiss
            </button>
          ) : null}
        </div>
      ) : null}

      <div className="settings-row settings-row-stacked">
        <div className="settings-row-label">
          <strong>Status</strong>
          {status.isPending ? (
            <small className="settings-todo">loading…</small>
          ) : status.isError ? (
            <small className="settings-todo" role="alert">
              could not load backup status: {status.error.message}
            </small>
          ) : data ? (
            <>
              <small>
                Backed up <strong>{data.manifestCount}</strong> components
              </small>
              <small>
                Last backup: <strong>{formatLastBackupAt(data.lastBackupAt)}</strong>
              </small>
              <small>
                Storage: <span className="mono">{data.backupDir}</span>
              </small>
              <small>
                Encryption: device-bound X25519 + AES-256-GCM. The private
                key is held in your OS keychain and never leaves this Mac.
              </small>
              {!data.keyPresent ? (
                <small className="settings-todo" role="status">
                  Key not yet generated. Click "Backup now" to create it on
                  first use.
                </small>
              ) : null}
            </>
          ) : null}
        </div>
      </div>

      <div className="settings-row settings-row-stacked">
        <div className="settings-row-label">
          <strong>Actions</strong>
          <small>
            Backup runs locally, idempotent on unchanged files. Restore
            never overwrites a file whose local mtime is newer than the
            backup.
          </small>
        </div>
        <div className="settings-row-actions">
          <button
            type="button"
            className="primary-button"
            onClick={() => {
              void handleBackup();
            }}
            disabled={ipcBusy}
            aria-busy={backupNowMut.isPending}
          >
            {backupNowMut.isPending ? "Backing up…" : "Backup now"}
          </button>
          <button
            type="button"
            className="text-button"
            onClick={() => {
              void handleDryRun();
            }}
            disabled={ipcBusy || (data?.manifestCount ?? 0) === 0}
            title={
              (data?.manifestCount ?? 0) === 0
                ? "No backups yet to preview"
                : "Show what restore would do without writing anything"
            }
          >
            {restoreNowMut.isPending && lastDryRun === null
              ? "Previewing…"
              : "Preview restore"}
          </button>
          <button
            type="button"
            className="text-button"
            onClick={() => {
              void handleVerify();
            }}
            disabled={ipcBusy || (data?.manifestCount ?? 0) === 0}
            title={
              (data?.manifestCount ?? 0) === 0
                ? "No backups yet to verify"
                : "Re-read every blob and check ciphertext + plaintext hashes"
            }
            aria-busy={verifyMut.isPending}
          >
            {verifyMut.isPending ? "Verifying…" : "Verify integrity"}
          </button>
          <button
            type="button"
            className="text-button quiet"
            onClick={() => setRestoreConfirmOpen(true)}
            disabled={ipcBusy || (data?.manifestCount ?? 0) === 0}
          >
            Restore now…
          </button>
        </div>
      </div>

      {lastVerify !== null ? (
        <div className="settings-row settings-row-stacked">
          <div className="settings-row-label">
            <strong>Integrity check</strong>
            <small>
              Verified <strong>{lastVerify.verified}</strong> of{" "}
              <strong>{lastVerify.total}</strong> blobs in{" "}
              {String(lastVerify.elapsedMs)}ms.
              {lastVerify.errors.length > 0
                ? ` ${lastVerify.errors.length} integrity issue${lastVerify.errors.length === 1 ? "" : "s"} found.`
                : " All ciphertext + plaintext hashes match."}
            </small>
            {lastVerify.errors.length > 0 ? (
              <details className="settings-todo">
                <summary>
                  Show {lastVerify.errors.length} issue
                  {lastVerify.errors.length === 1 ? "" : "s"}
                </summary>
                <ul style={{ marginTop: 6 }}>
                  {lastVerify.errors.map((err) => (
                    <li key={err.componentId}>
                      <span className="mono">{err.componentId}</span>
                      {" - "}
                      <strong>{err.kind}</strong>
                      {": "}
                      {err.message}
                    </li>
                  ))}
                </ul>
              </details>
            ) : null}
          </div>
          <div className="settings-row-actions">
            <button
              type="button"
              className="text-button quiet"
              onClick={() => setLastVerify(null)}
            >
              clear report
            </button>
          </div>
        </div>
      ) : null}

      {lastDryRun !== null ? (
        <div className="settings-row settings-row-stacked">
          <div className="settings-row-label">
            <strong>Restore preview</strong>
            <small>
              Would restore <strong>{lastDryRun.restored}</strong> of{" "}
              <strong>{lastDryRun.total}</strong>;{" "}
              <strong>{lastDryRun.skippedLocalNewer}</strong> skipped because
              local copies are newer than the backup
              {lastDryRun.errors > 0 ? (
                <>
                  {" "}
                  ({lastDryRun.errors} would error)
                </>
              ) : null}
              .
            </small>
          </div>
          <div className="settings-row-actions">
            <button
              type="button"
              className="text-button quiet"
              onClick={() => setLastDryRun(null)}
            >
              clear preview
            </button>
          </div>
        </div>
      ) : null}

      <div className="settings-row">
        <div className="settings-row-label">
          <strong>Auto-backup</strong>
          <small>
            Run a backup pass automatically after edits, debounced 5s so
            a stream of saves coalesces into one pass.
          </small>
        </div>
        <label
          className="settings-toggle"
          aria-label="auto-backup after edits"
        >
          <input
            type="checkbox"
            checked={data?.autoBackupEnabled ?? false}
            disabled={ipcBusy}
            onChange={(e) => handleAutoToggle(e.target.checked)}
          />
          <span>{data?.autoBackupEnabled ? "on" : "off"}</span>
        </label>
      </div>

      <p className="settings-todo" role="note">
        Cross-device restore is not supported in v0. The private key is
        device-bound. See <span className="mono">docs/15</span> for the
        threat model and the production migration path.
      </p>

      {restoreConfirmOpen ? (
        <div
          className="validation-box"
          role="alertdialog"
          aria-labelledby="restore-confirm-title"
          aria-describedby="restore-confirm-body"
        >
          <span>!</span>
          <div>
            <p id="restore-confirm-title">
              <strong>Restore from backup?</strong>
            </p>
            <p id="restore-confirm-body">
              This will overwrite local files that are older than their
              backup. Files newer than their backup are skipped server-side.
              {" "}
              <strong>This cannot be undone.</strong>
            </p>
            <div className="settings-row-actions" style={{ marginTop: 8 }}>
              <button
                type="button"
                className="text-button quiet"
                onClick={() => setRestoreConfirmOpen(false)}
              >
                cancel
              </button>
              <button
                type="button"
                className="primary-button"
                onClick={() => {
                  void handleRestoreConfirm();
                }}
                disabled={restoreNowMut.isPending}
              >
                {restoreNowMut.isPending ? "Restoring…" : "Restore"}
              </button>
            </div>
          </div>
        </div>
      ) : null}
    </section>
  );
}

function PrivacyPane() {
  const panicMode = useUi((s) => s.panicMode);
  const panicLast = useUi((s) => s.panicModeLastToggledAt);

  // Same data the DiagnosticsPanel reads. We re-query rather than
  // share state so the export can be triggered from the Privacy pane
  // without scrolling to the diagnostics card.
  const tools = useTools();
  const health = useHealthSummary();
  const events = useDiagnosticsEvents();

  const parseErrors = useMemo<DiagnosticsParseError[]>(() => {
    const out: DiagnosticsParseError[] = [];
    for (const stamped of events) {
      if (stamped.event.event !== "parseError") continue;
      out.push({
        timestamp: stamped.timestamp,
        id: stamped.event.id,
        path: stamped.event.path,
      });
    }
    return out;
  }, [events]);

  const toolEntries = useMemo<DiagnosticsToolEntry[]>(() => {
    if (!tools.data) return [];
    return tools.data.map((t) => ({
      id: t.id,
      displayName: t.displayName,
      detected: t.detected,
      binary: t.binary,
      version: t.version,
      watchRoots: t.existingRootPaths,
    }));
  }, [tools.data]);

  const byToolKind = useMemo<DiagnosticsToolKindCount[]>(() => {
    if (!health.data) return [];
    return health.data.byToolKind.map((row) => ({
      tool: row.tool,
      kind: row.kind,
      count: row.count,
    }));
  }, [health.data]);

  const [exportState, setExportState] = useState<DiagnosticsExportState>({
    kind: "idle",
  });

  // Issue #9 - the previous handler only console.log'd. Now: build
  // the report, sanitise it, ask the user where to save through the
  // Tauri dialog plugin, then write atomically through the
  // `export_diagnostics` IPC.
  async function handleDiagnosticsExport(): Promise<void> {
    setExportState({ kind: "running" });
    try {
      const report = buildReport({
        appVersion: __APP_VERSION__,
        events,
        parseErrors,
        panicActive: panicMode,
        panicLastToggledAt: panicLast,
        totalComponents: health.data?.totalComponents ?? 0,
        totalParseErrors: health.data?.totalParseErrors ?? 0,
        byToolKind,
        tools: toolEntries,
      });
      const sanitised = sanitiseForClipboard(report);
      const json = JSON.stringify(sanitised, null, 2);

      const target = await saveDialog({
        title: "Save diagnostics snapshot",
        defaultPath: defaultDiagnosticsFileName(),
        filters: [{ name: "JSON", extensions: ["json"] }],
      });
      if (typeof target !== "string" || target.length === 0) {
        setExportState({ kind: "cancelled" });
        return;
      }

      await exportDiagnostics(target, json);
      setExportState({ kind: "saved", path: target });
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      console.error("[settings] diagnostics export failed", err);
      setExportState({ kind: "error", message });
    }
  }

  return (
    <section className="health-pane settings-pane" aria-labelledby="settings-privacy">
      <h3 id="settings-privacy">Privacy</h3>
      <div className="settings-row">
        <div className="settings-row-label">
          <strong>Telemetry</strong>
          <small>
            Forced off in MVP per <span className="mono">docs/12</span>.
            Ships post-MVP with an explicit opt-in toggle.
          </small>
        </div>
        {/*
          Audit issue #15: this used to render as a permanently disabled
          checkbox, which read as "you don't have permission to toggle
          this" rather than "the feature is intentionally absent". A
          status pill says exactly what's true: telemetry is off, and
          there's nothing to toggle.
        */}
        <span
          className="health-pill ok"
          role="status"
          aria-label="telemetry is off in this build"
          title="Telemetry will return as a real opt-in once it ships post-MVP"
        >
          off in MVP
        </span>
      </div>
      <div className="settings-row">
        <div className="settings-row-label">
          <strong>Diagnostics export</strong>
          <small>Saves a sanitised JSON snapshot for support.</small>
          {exportState.kind === "saved" ? (
            <small className="settings-todo" role="status" aria-live="polite">
              saved to <span className="mono">{exportState.path}</span>
            </small>
          ) : null}
          {exportState.kind === "cancelled" ? (
            <small className="settings-todo" role="status" aria-live="polite">
              export cancelled
            </small>
          ) : null}
          {exportState.kind === "error" ? (
            <small className="settings-todo" role="status" aria-live="polite">
              export failed: {exportState.message}
            </small>
          ) : null}
        </div>
        <button
          type="button"
          className="text-button"
          onClick={() => {
            void handleDiagnosticsExport();
          }}
          disabled={exportState.kind === "running"}
        >
          {exportState.kind === "running" ? "exporting…" : "export"}
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
        <BackupPane />
        <PrivacyPane />
        <UpdatesPane />
        <DiagnosticsPane />
      </div>
    </section>
  );
}
