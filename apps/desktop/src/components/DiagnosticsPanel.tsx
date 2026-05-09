/**
 * Diagnostics panel (Phase 4.2).
 *
 * Lives inside Settings. Renders six sub-sections:
 *   1. App + platform metadata
 *   2. Index stats (totals + per-(tool, kind))
 *   3. Tool detection
 *   4. Recent file events (in-memory ring)
 *   5. Recent parse errors (filtered subset of the ring)
 *   6. Watcher status (per-tool watch roots)
 *
 * Plus three controls:
 *   - "Copy diagnostics" - sanitised JSON to clipboard
 *   - "Panic mode" toggle (mirrors keyboard shortcut)
 *   - "Reset onboarding" - clears the persisted flag and re-opens the modal
 *
 * The panel never reads file contents; it only enumerates events that
 * already crossed the IPC boundary. The clipboard payload is run through
 * `sanitiseForClipboard` before it leaves the WebView.
 */
import { useCallback, useMemo, useState } from "react";
import { useUi } from "@/store/ui";
import { useTools, useHealthSummary } from "@/ipc/hooks";
import { useDiagnosticsEvents } from "@/lib/diagnosticsRing";
import {
  sanitiseForClipboard,
  type DiagnosticsParseError,
  type DiagnosticsToolEntry,
  type DiagnosticsToolKindCount,
} from "@/lib/diagnosticsSanitiser";
import {
  buildReport,
  detectPlatformLabel,
  toParseError,
} from "@/lib/diagnosticsReport";
import {
  resetOnboarding as clearOnboardingFlag,
} from "@/lib/onboarding";

/** Cap on the parse-errors sub-section. */
const PARSE_ERROR_LIMIT = 50;

/** Truncate file paths so a single event line never blows the layout. */
const PATH_DISPLAY_MAX = 80;

type CopyState = "idle" | "copied" | "fallback" | "error";

function truncatePath(path: string): string {
  if (path.length <= PATH_DISPLAY_MAX) return path;
  // Keep the suffix - the basename and parent are usually most useful.
  const tail = path.slice(-(PATH_DISPLAY_MAX - 3));
  return `…${tail}`;
}

function formatTimestamp(ms: number): string {
  return new Date(ms).toLocaleTimeString();
}

interface PanelHeadingProps {
  id: string;
  children: React.ReactNode;
}

function PanelHeading({ id, children }: PanelHeadingProps): React.ReactElement {
  return <h4 id={id}>{children}</h4>;
}

interface MetaItemProps {
  label: string;
  value: string;
}

function MetaItem({ label, value }: MetaItemProps): React.ReactElement {
  return (
    <div className="settings-row">
      <div className="settings-row-label">
        <strong>{label}</strong>
      </div>
      <span className="mono diagnostics-meta-value">{value}</span>
    </div>
  );
}

export function DiagnosticsPanel(): React.ReactElement {
  const panicMode = useUi((s) => s.panicMode);
  const panicLast = useUi((s) => s.panicModeLastToggledAt);
  const togglePanicMode = useUi((s) => s.togglePanicMode);
  const toggleOnboarding = useUi((s) => s.toggleOnboarding);

  const tools = useTools();
  const health = useHealthSummary();
  const events = useDiagnosticsEvents();

  const [copyState, setCopyState] = useState<CopyState>("idle");
  const [copyPayload, setCopyPayload] = useState<string>("");

  const parseErrors = useMemo<DiagnosticsParseError[]>(() => {
    const out: DiagnosticsParseError[] = [];
    for (const stamped of events) {
      const pe = toParseError(stamped);
      if (pe) out.push(pe);
      if (out.length >= PARSE_ERROR_LIMIT) break;
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

  const handleCopy = useCallback(async (): Promise<void> => {
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
    setCopyPayload(json);

    // Try the standard clipboard API first. Some Tauri builds gate this
    // behind a permission; we surface a textarea fallback when it fails.
    try {
      if (typeof navigator !== "undefined" && navigator.clipboard) {
        await navigator.clipboard.writeText(json);
        setCopyState("copied");
        return;
      }
      setCopyState("fallback");
    } catch {
      setCopyState("fallback");
    }
  }, [
    events,
    parseErrors,
    panicMode,
    panicLast,
    health.data,
    byToolKind,
    toolEntries,
  ]);

  const handleResetOnboarding = useCallback((): void => {
    clearOnboardingFlag();
    toggleOnboarding(true);
  }, [toggleOnboarding]);

  const handleTogglePanic = useCallback((): void => {
    togglePanicMode();
  }, [togglePanicMode]);

  const lastPanicLabel =
    panicLast === null ? "never toggled" : new Date(panicLast).toLocaleString();

  const platformLabel = detectPlatformLabel();

  return (
    <div className="diagnostics-panel" aria-label="diagnostics">
      <div className="diagnostics-actions">
        <button
          type="button"
          className="text-button"
          onClick={() => {
            void handleCopy();
          }}
          aria-label="copy diagnostics to clipboard"
        >
          copy diagnostics
        </button>
        <button
          type="button"
          className={`text-button ${panicMode ? "" : "quiet"}`}
          aria-pressed={panicMode}
          onClick={handleTogglePanic}
        >
          {panicMode ? "exit panic" : "enter panic"}
        </button>
        <button
          type="button"
          className="text-button quiet"
          onClick={handleResetOnboarding}
        >
          reset onboarding
        </button>
      </div>

      {copyState === "copied" ? (
        <p className="diagnostics-status" role="status" aria-live="polite">
          sanitised report copied to clipboard
        </p>
      ) : null}
      {copyState === "fallback" ? (
        <div className="diagnostics-fallback" role="status" aria-live="polite">
          <p>
            Clipboard access was denied. Select the text below and copy it
            manually.
          </p>
          <textarea
            readOnly
            className="diagnostics-fallback-text mono"
            aria-label="diagnostics report (fallback)"
            value={copyPayload}
            rows={8}
          />
        </div>
      ) : null}
      {copyState === "error" ? (
        <p className="diagnostics-status" role="status" aria-live="polite">
          could not produce diagnostics
        </p>
      ) : null}

      <section
        className="diagnostics-section"
        aria-labelledby="diagnostics-meta"
      >
        <PanelHeading id="diagnostics-meta">App + platform</PanelHeading>
        <MetaItem label="Version" value={`v${__APP_VERSION__}`} />
        <MetaItem label="Platform" value={platformLabel} />
        <div className="settings-row">
          <div className="settings-row-label">
            <strong>Panic mode</strong>
            <small>
              {panicMode ? "active" : "off"} · last toggle: {lastPanicLabel}
            </small>
          </div>
          {panicMode ? <span className="health-pill warn">on</span> : null}
        </div>
      </section>

      <section
        className="diagnostics-section"
        aria-labelledby="diagnostics-index"
      >
        <PanelHeading id="diagnostics-index">Index stats</PanelHeading>
        {health.isPending ? (
          <p className="settings-todo">loading…</p>
        ) : (
          <>
            <MetaItem
              label="Total components"
              value={String(health.data?.totalComponents ?? 0)}
            />
            <MetaItem
              label="Parse errors"
              value={String(health.data?.totalParseErrors ?? 0)}
            />
            <div className="diagnostics-table" role="table">
              <div className="diagnostics-table-row head" role="row">
                <span role="columnheader">tool</span>
                <span role="columnheader">kind</span>
                <span role="columnheader">count</span>
              </div>
              {byToolKind.length === 0 ? (
                <div className="diagnostics-table-row" role="row">
                  <span className="settings-todo" role="cell">
                    no rows yet
                  </span>
                </div>
              ) : (
                byToolKind.map((row) => (
                  <div
                    key={`${row.tool}-${row.kind}`}
                    className="diagnostics-table-row"
                    role="row"
                  >
                    <span role="cell">{row.tool}</span>
                    <span role="cell">{row.kind}</span>
                    <span role="cell">{row.count}</span>
                  </div>
                ))
              )}
            </div>
          </>
        )}
      </section>

      <section
        className="diagnostics-section"
        aria-labelledby="diagnostics-tools"
      >
        <PanelHeading id="diagnostics-tools">Tool detection</PanelHeading>
        {tools.isPending ? (
          <p className="settings-todo">loading…</p>
        ) : tools.data && tools.data.length > 0 ? (
          <ul className="diagnostics-list">
            {tools.data.map((tool) => (
              <li key={tool.id} className="diagnostics-list-row">
                <span
                  className={`health-pill ${tool.detected ? "up" : "cold"}`}
                  aria-label={tool.detected ? "detected" : "not detected"}
                >
                  {tool.detected ? "detected" : "absent"}
                </span>
                <div className="diagnostics-list-body">
                  <strong>{tool.displayName}</strong>
                  <small className="mono">
                    {tool.binary ?? "no binary"}
                    {tool.version ? ` · ${tool.version}` : ""}
                  </small>
                </div>
              </li>
            ))}
          </ul>
        ) : (
          <p className="settings-todo">no tools detected</p>
        )}
      </section>

      <section
        className="diagnostics-section"
        aria-labelledby="diagnostics-watchers"
      >
        <PanelHeading id="diagnostics-watchers">Watcher status</PanelHeading>
        {tools.data && tools.data.length > 0 ? (
          <ul className="diagnostics-list">
            {tools.data.map((tool) => (
              <li key={tool.id} className="diagnostics-list-row">
                <strong>{tool.displayName}</strong>
                {tool.existingRootPaths.length === 0 ? (
                  <small className="settings-todo">no watch roots</small>
                ) : (
                  <ul className="diagnostics-watch-roots">
                    {tool.existingRootPaths.map((root) => (
                      <li key={root} className="mono">
                        {root}
                      </li>
                    ))}
                  </ul>
                )}
              </li>
            ))}
          </ul>
        ) : (
          <p className="settings-todo">no tools detected</p>
        )}
      </section>

      <section
        className="diagnostics-section"
        aria-labelledby="diagnostics-events"
      >
        <PanelHeading id="diagnostics-events">
          Recent file events
          <small className="diagnostics-section-meta">
            {events.length === 0
              ? "none yet"
              : `${events.length} event${events.length === 1 ? "" : "s"}`}
          </small>
        </PanelHeading>
        {events.length === 0 ? (
          <p className="settings-todo">
            No events received in this session yet.
          </p>
        ) : (
          <ul className="diagnostics-events">
            {events.map((stamped, idx) => {
              const path =
                stamped.event.event === "parseError"
                  ? stamped.event.path
                  : null;
              return (
                <li
                  // Composite key keeps the same id reusable across event
                  // kinds without collision.
                  key={`${stamped.timestamp}-${idx}`}
                  className="diagnostics-event-row"
                >
                  <span className="diagnostics-event-kind mono">
                    {stamped.event.event}
                  </span>
                  <span className="diagnostics-event-time mono">
                    {formatTimestamp(stamped.timestamp)}
                  </span>
                  <span className="diagnostics-event-path mono">
                    {path ? truncatePath(path) : ""}
                  </span>
                </li>
              );
            })}
          </ul>
        )}
      </section>

      <section
        className="diagnostics-section"
        aria-labelledby="diagnostics-parse-errors"
      >
        <PanelHeading id="diagnostics-parse-errors">
          Recent parse errors
          <small className="diagnostics-section-meta">
            {parseErrors.length === 0
              ? "none"
              : `${parseErrors.length} of last ${PARSE_ERROR_LIMIT}`}
          </small>
        </PanelHeading>
        {parseErrors.length === 0 ? (
          <p className="settings-todo">
            No parse errors in this session.
          </p>
        ) : (
          <ul className="diagnostics-events">
            {parseErrors.map((err, idx) => (
              <li
                key={`${err.timestamp}-${idx}`}
                className="diagnostics-event-row"
              >
                <span className="diagnostics-event-time mono">
                  {formatTimestamp(err.timestamp)}
                </span>
                <span className="diagnostics-event-path mono">
                  {truncatePath(err.path)}
                </span>
              </li>
            ))}
          </ul>
        )}
      </section>
    </div>
  );
}
