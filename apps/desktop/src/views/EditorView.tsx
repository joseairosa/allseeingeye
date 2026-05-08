/**
 * Editor view (Phase 3.3).
 *
 * Wires the form ↔ raw round-trip plus the save flow:
 *   * `useComponentWithRaw` opens both halves in one IPC call.
 *   * `EditState` reducer holds Monaco's buffer + the projected AST.
 *   * `FormPane` renders schema-driven inputs; edits flow through
 *     `setFormField`, project back to raw, and Monaco reflects the
 *     change on the next render.
 *   * `MonacoRawPane` writes through `setRaw`; the AST re-projects
 *     after a 250ms idle window to keep keystroke latency clean.
 *   * Cmd-S triggers the save mutation; `SaveOutcome` is mapped to
 *     toast / banner state.
 *
 * Monaco lives behind `React.lazy` so the main bundle never imports
 * `monaco-editor`. The first time the user opens the Editor view the
 * Monaco chunk loads; subsequent visits are cached.
 */
import {
  Suspense,
  lazy,
  useCallback,
  useEffect,
  useMemo,
  useReducer,
  useRef,
  useState,
  type ReactElement,
} from "react";
import { useUi } from "@/store/ui";
import { SaveIcon } from "@/components/icons";
import {
  useComponentWithRaw,
  useSaveComponent,
  useValidationSchema,
} from "@/ipc/hooks";
import { EDITOR_SAVE_EVENT } from "@/lib/keyboard";
import type { SaveOutcome, ValidationError } from "@aseye/shared-types";
import {
  editReducer,
  externalChangeFromOutcome,
  type EditState,
} from "./editor/EditState";
import { FormPane, parseSchema, type SchemaShape } from "./editor/FormPane";
import { projectorFor } from "./editor/serialise";

const MonacoRawPane = lazy(() => import("./editor/MonacoRawPane"));

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

function EditorEmpty(): ReactElement {
  return (
    <div className="editor-empty" role="status">
      Pick a component to start editing.
    </div>
  );
}

/** Toast shapes the EditorView surfaces above the layout. */
type Toast =
  | { kind: "success"; message: string }
  | { kind: "error"; message: string };

interface EditorBodyProps {
  state: EditState;
  schema: SchemaShape | null;
  format: string;
  onRawChange: (raw: string) => void;
  onFieldChange: (pointer: string, value: unknown) => void;
}

function EditorBody({
  state,
  schema,
  format,
  onRawChange,
  onFieldChange,
}: EditorBodyProps): ReactElement {
  const errors = state.validation?.errors ?? [];
  return (
    <div className="editor-layout">
      <FormPane
        ast={state.formAst}
        errors={errors}
        schema={schema}
        format={format}
        parseError={state.parseError}
        onFieldChange={onFieldChange}
      />
      <div className="raw-pane" aria-label="raw editor">
        <div className="pane-title">
          <span>raw view</span>
          {state.dirty ? <span className="health-pill warn">unsaved</span> : null}
        </div>
        <Suspense fallback={<MonacoSkeleton />}>
          <MonacoRawPane
            content={state.currentRaw}
            format={format as Parameters<typeof MonacoRawPane>[0]["format"]}
            onChange={onRawChange}
          />
        </Suspense>
      </div>
    </div>
  );
}

/** Banner rendered when the file changed under us between open and save. */
function ExternalChangeBanner({
  onReload,
  onForceSave,
}: {
  onReload: () => void;
  onForceSave: () => void;
}): ReactElement {
  return (
    <div className="validation-box" role="alert" aria-live="polite">
      <span>!</span>
      <div>
        <p>
          The file changed on disk while you were editing. Reload to discard
          your changes and pull the latest, or save anyway to overwrite the
          external edit.
        </p>
        <div className="editor-actions" style={{ marginTop: 8 }}>
          <button
            type="button"
            className="text-button quiet"
            onClick={onReload}
          >
            reload
          </button>
          <button
            type="button"
            className="primary-button"
            onClick={onForceSave}
          >
            save anyway
          </button>
        </div>
      </div>
    </div>
  );
}

/** Light toast banner shown above the editor body. */
function ToastBanner({ toast }: { toast: Toast }): ReactElement {
  const role = toast.kind === "error" ? "alert" : "status";
  return (
    <div
      className="validation-box"
      role={role}
      aria-live="polite"
      data-toast-kind={toast.kind}
    >
      <span>{toast.kind === "error" ? "!" : "✓"}</span>
      <p>{toast.message}</p>
    </div>
  );
}

export function EditorView(): ReactElement {
  const view = useUi((s) => s.view);
  const isActive = view === "editor";
  const selectedId = useUi((s) => s.selectedComponentId);

  // Single round-trip on open: detail + raw + hash.
  const componentQuery = useComponentWithRaw(selectedId);
  const detail = componentQuery.data ?? null;
  const tool = detail?.detail.tool ?? null;
  const kind = detail?.detail.kind ?? null;
  const format = detail?.detail.format ?? "markdown";

  const schemaQuery = useValidationSchema(tool, kind);
  const schema = useMemo(
    () => parseSchema(schemaQuery.data ?? null),
    [schemaQuery.data],
  );

  const projector = useMemo(() => projectorFor(format), [format]);
  const [state, dispatch] = useReducer(editReducer, null);
  const [toast, setToast] = useState<Toast | null>(null);
  const saveMutation = useSaveComponent();

  // Re-project on idle so keystroke latency stays clean. Resets the
  // timer on every raw change.
  const idleTimer = useRef<number | null>(null);

  // Re-init the reducer whenever a new component+raw payload lands.
  useEffect(() => {
    if (detail !== null) {
      dispatch({ type: "open", detail, project: projector.project });
      setToast(null);
    }
  }, [detail, projector.project]);

  // Pipeline-event-driven external change: when TanStack Query
  // invalidates the bundle and the new payload's hash differs from
  // the editor's snapshot, surface the banner. This catches
  // out-of-band edits while the editor is open.
  useEffect(() => {
    if (state === null) return;
    if (detail === null) return;
    if (detail.hash === state.originalHash) return;
    // Only flip when the editor is dirty - if the user has not
    // edited yet, we just re-open with the new content.
    if (!state.dirty) {
      dispatch({ type: "open", detail, project: projector.project });
      return;
    }
    dispatch({
      type: "noteExternalChange",
      payload: { currentHash: detail.hash, currentContent: detail.raw },
    });
  }, [detail, projector.project, state]);

  const handleRawChange = useCallback(
    (raw: string) => {
      // Quick path: update Monaco's mirror immediately so the buffer
      // is the source of truth without waiting for the idle timer.
      // The reducer also debounces the AST re-projection so a stream
      // of keystrokes doesn't burn CPU on each character.
      if (idleTimer.current !== null) {
        window.clearTimeout(idleTimer.current);
      }
      idleTimer.current = window.setTimeout(() => {
        dispatch({ type: "setRaw", raw, project: projector.project });
      }, 250);
      // Also dispatch synchronously with the latest text so the
      // dirty flag flips on the next render. The idle re-projection
      // will reconcile the AST a moment later.
      dispatch({ type: "setRaw", raw, project: projector.project });
    },
    [projector.project],
  );

  const handleFieldChange = useCallback(
    (pointer: string, value: unknown) => {
      dispatch({
        type: "setFormField",
        pointer,
        value,
        serialise: projector.serialise,
      });
    },
    [projector.serialise],
  );

  const handleDiscard = useCallback(() => {
    dispatch({ type: "discard" });
    setToast(null);
  }, []);

  const runSave = useCallback(
    (originalHash: string) => {
      if (state === null) return;
      saveMutation.mutate(
        { id: state.id, content: state.currentRaw, originalHash },
        {
          onSuccess: (outcome: SaveOutcome) => {
            switch (outcome.kind) {
              case "saved":
                dispatch({ type: "markSaved", newHash: outcome.newHash });
                setToast({ kind: "success", message: "saved" });
                break;
              case "externalChange":
                dispatch({
                  type: "noteExternalChange",
                  payload: externalChangeFromOutcome(outcome),
                });
                setToast(null);
                break;
              case "validationFailed":
                // Render errors next to fields; surface a top-level
                // toast so the user knows the save was rejected.
                dispatch({
                  type: "setValidation",
                  outcome: {
                    ok: false,
                    errors: outcome.errors as ValidationError[],
                    warnings: [],
                  },
                });
                setToast({
                  kind: "error",
                  message: `save blocked: ${outcome.errors.length} validation error${outcome.errors.length === 1 ? "" : "s"}`,
                });
                break;
              case "forbidden":
                setToast({
                  kind: "error",
                  message: `save refused: ${outcome.reason}`,
                });
                break;
            }
          },
          onError: (err) => {
            setToast({ kind: "error", message: err.message });
          },
        },
      );
    },
    [saveMutation, state],
  );

  const handleSave = useCallback(() => {
    if (state === null || !state.dirty) return;
    runSave(state.originalHash);
  }, [runSave, state]);

  const handleReload = useCallback(() => {
    if (state === null || state.externalChange === null) return;
    // Re-open the editor against the disk content. We synthesise a
    // new "open" so the AST + history reset cleanly.
    if (detail === null) return;
    dispatch({
      type: "open",
      detail: {
        ...detail,
        raw: state.externalChange.currentContent,
        hash: state.externalChange.currentHash,
      },
      project: projector.project,
    });
    setToast(null);
  }, [detail, projector.project, state]);

  const handleForceSave = useCallback(() => {
    if (state === null || state.externalChange === null) return;
    runSave(state.externalChange.currentHash);
  }, [runSave, state]);

  // Cmd-S routing: the keyboard layer dispatches a custom event when
  // the editor view is active.
  useEffect(() => {
    function onSave(): void {
      if (!isActive) return;
      handleSave();
    }
    window.addEventListener(EDITOR_SAVE_EVENT, onSave);
    return () => window.removeEventListener(EDITOR_SAVE_EVENT, onSave);
  }, [handleSave, isActive]);

  // Auto-clear the toast after a few seconds so it doesn't stick.
  useEffect(() => {
    if (toast === null) return;
    const id = window.setTimeout(() => setToast(null), 4000);
    return () => window.clearTimeout(id);
  }, [toast]);

  // Heading + path strip mirror Phase 3.1 conventions.
  let heading = "editor";
  let path = "";
  if (selectedId === null) {
    heading = "editor";
  } else if (componentQuery.isPending && !detail) {
    heading = "loading...";
  } else if (!detail) {
    heading = "component not found";
  } else {
    heading = `${detail.detail.kind}: ${detail.detail.name}`;
    path = detail.detail.path;
  }

  let body: ReactElement;
  if (selectedId === null) {
    body = <EditorEmpty />;
  } else if (componentQuery.isPending && !detail) {
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
  } else if (componentQuery.isError) {
    body = (
      <div className="editor-empty" role="alert">
        Could not open component: {componentQuery.error.message}
      </div>
    );
  } else if (!detail || state === null) {
    body = <EditorEmpty />;
  } else {
    body = (
      <EditorBody
        state={state}
        schema={schema}
        format={format}
        onRawChange={handleRawChange}
        onFieldChange={handleFieldChange}
      />
    );
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
            onClick={handleDiscard}
            disabled={state === null || !state.dirty}
            aria-label="discard"
          >
            discard
          </button>
          <button
            type="button"
            className="primary-button"
            onClick={handleSave}
            disabled={
              state === null ||
              !state.dirty ||
              saveMutation.isPending ||
              state.externalChange !== null
            }
            aria-label="save"
            title="save (Cmd-S)"
          >
            <SaveIcon />
            save
          </button>
        </div>
      </div>

      {state?.externalChange ? (
        <ExternalChangeBanner
          onReload={handleReload}
          onForceSave={handleForceSave}
        />
      ) : null}
      {toast ? <ToastBanner toast={toast} /> : null}

      {body}
    </section>
  );
}
