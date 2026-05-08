/**
 * Editor view (Phase 3.1).
 *
 * Phase 3.1 ships the editor SHELL plus a lazy-loaded Monaco raw pane
 * so the user can view and edit the on-disk text. Schema-aware form
 * inputs (Phase 3.2) and AST round-trip + save (Phase 3.3) are
 * deliberately stubbed:
 *   - The save / discard buttons are disabled.
 *   - The form pane is a static placeholder populated with a few
 *     read-only frontmatter fields when the parser exposed them.
 *
 * Monaco lives behind `React.lazy` so the main bundle never imports
 * `monaco-editor`. The first time the user opens the Editor view the
 * Monaco chunk loads; subsequent visits are cached.
 */
import { Suspense, lazy, useMemo, type ReactElement } from "react";
import { useUi } from "@/store/ui";
import { SaveIcon } from "@/components/icons";
import { useComponent, useComponentRaw } from "@/ipc/hooks";
import type { ComponentDetail } from "@aseye/shared-types";

// Code-split target. The dynamic `import()` is what Vite's Rollup
// config keys off to emit a separate `MonacoRawPane-*.js` chunk that
// pulls `@monaco-editor/react` + `monaco-editor` along with it.
const MonacoRawPane = lazy(() => import("./editor/MonacoRawPane"));

/**
 * Pull a small, safe set of frontmatter fields out of the parsed JSON
 * the Rust parser already produced. We render them read-only as a
 * placeholder for the schema-driven form that lands in Phase 3.2.
 */
function frontmatterFields(detail: ComponentDetail): Record<string, string> {
  if (!detail.parsedJson) return {};
  try {
    const parsed: unknown = JSON.parse(detail.parsedJson);
    if (parsed === null || typeof parsed !== "object") return {};
    const fm = (parsed as Record<string, unknown>)["frontmatter"];
    if (fm === null || typeof fm !== "object" || Array.isArray(fm)) return {};
    const out: Record<string, string> = {};
    for (const [k, v] of Object.entries(fm as Record<string, unknown>)) {
      if (typeof v === "string") out[k] = v;
      else if (typeof v === "number" || typeof v === "boolean") out[k] = String(v);
      // Arrays / objects skipped - Phase 3.2 handles structured fields.
    }
    return out;
  } catch {
    return {};
  }
}

/** Skeleton mirrors Monaco's footprint so the layout doesn't jump. */
function MonacoSkeleton(): ReactElement {
  return (
    <div className="editor-monaco-host">
      <div
        className="skeleton-block"
        aria-label="loading editor"
        style={{ height: "100%", borderRadius: 0 }}
      />
    </div>
  );
}

interface EditorBodyProps {
  detail: ComponentDetail;
  raw: string;
}

function EditorBody({ detail, raw }: EditorBodyProps): ReactElement {
  const fields = useMemo(() => frontmatterFields(detail), [detail]);

  return (
    <div className="editor-layout">
      <form
        className="form-pane"
        aria-label="schema form"
        onSubmit={(e) => e.preventDefault()}
      >
        <div className="pane-title">
          <span>form view</span>
          <span className="health-pill up">phase 3.2</span>
        </div>
        <p className="settings-todo">
          Form view lands in Phase 3.2. Until then the fields below mirror
          the parsed frontmatter and stay read-only.
        </p>
        {Object.entries(fields).map(([name, value]) => (
          <label key={name} className="field">
            <span>{name}</span>
            <input value={value} readOnly />
          </label>
        ))}
        {Object.keys(fields).length === 0 ? (
          <label className="field">
            <span>frontmatter</span>
            <input value="(none parsed)" readOnly />
          </label>
        ) : null}
      </form>

      <div className="raw-pane" aria-label="raw editor">
        <div className="pane-title">
          <span>raw view</span>
          <span className="mono">{detail.format}</span>
        </div>
        <Suspense fallback={<MonacoSkeleton />}>
          <MonacoRawPane content={raw} format={detail.format} />
        </Suspense>
      </div>
    </div>
  );
}

function EditorEmpty(): ReactElement {
  return (
    <div className="editor-empty" role="status">
      Pick a component to start editing.
    </div>
  );
}

export function EditorView(): ReactElement {
  const view = useUi((s) => s.view);
  const isActive = view === "editor";
  const selectedId = useUi((s) => s.selectedComponentId);

  // The detail query feeds the form pane; the raw query feeds Monaco.
  // Both are gated on `selectedId !== null` inside the hook.
  const detailQuery = useComponent(selectedId);
  const rawQuery = useComponentRaw(selectedId);

  let heading = "editor";
  let path = "";
  if (selectedId === null) {
    heading = "editor";
  } else if (detailQuery.isPending && !detailQuery.data) {
    heading = "loading...";
  } else if (!detailQuery.data) {
    heading = "component not found";
  } else {
    heading = `${detailQuery.data.kind}: ${detailQuery.data.name}`;
    path = detailQuery.data.path;
  }

  let body: ReactElement;
  if (selectedId === null) {
    body = <EditorEmpty />;
  } else if (detailQuery.isPending && !detailQuery.data) {
    // Skeleton: empty layout while the first round-trip lands.
    body = (
      <div className="editor-layout">
        <div className="form-pane">
          <div className="skeleton-block" aria-hidden="true">
            &nbsp;
          </div>
        </div>
        <div className="raw-pane">
          <MonacoSkeleton />
        </div>
      </div>
    );
  } else if (!detailQuery.data) {
    body = <EditorEmpty />;
  } else if (rawQuery.isError) {
    // Surface the typed IPC error verbatim so the user sees why we
    // didn't open the file (oversize, missing, not UTF-8, ...).
    body = (
      <div className="editor-empty" role="alert">
        Could not read file: {rawQuery.error.message}
      </div>
    );
  } else {
    const raw = rawQuery.data ?? "";
    body = <EditorBody detail={detailQuery.data} raw={raw} />;
  }

  return (
    <section
      className={`view${isActive ? " active" : ""}`}
      data-view-panel="editor"
      aria-labelledby="editor-heading"
      hidden={!isActive}
    >
      <div className="editor-topline">
        <div>
          <h2 id="editor-heading">{heading}</h2>
          {path ? <p className="mono">{path}</p> : null}
        </div>
        <div className="editor-actions">
          <button
            type="button"
            className="text-button quiet"
            disabled
            aria-label="discard (Phase 3.3)"
            title="Discard wires up in Phase 3.3"
          >
            discard
          </button>
          <button
            type="button"
            className="primary-button"
            disabled
            aria-label="save (Phase 3.3)"
            title="Save wires up in Phase 3.3"
          >
            <SaveIcon />
            save
          </button>
        </div>
      </div>

      {body}
    </section>
  );
}
