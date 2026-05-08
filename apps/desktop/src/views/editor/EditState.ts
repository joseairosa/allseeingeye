/**
 * Editor edit-state reducer.
 *
 * Owns the data model for ONE component being edited:
 *   * the snapshot the editor opened with (`originalRaw` + `originalHash`),
 *   * Monaco's live buffer (`currentRaw`),
 *   * the parsed AST projected from `currentRaw` (`formAst`),
 *   * derived flags (`dirty`),
 *   * validator outcome (`validation`),
 *   * external-change banner state (`externalChange`),
 *   * a small AST history for undo within the form pane.
 *
 * The reducer is the single source of truth - the EditorView wires
 * Monaco edits to `setRaw`, form edits to `setFormField`, save to
 * `markSaved`, and the pipeline-event invalidator to
 * `noteExternalChange`.
 *
 * Phase 3.3 - we keep the AST in JSON-pointer-addressable form so
 * the validator's error pointers map 1:1 onto form field paths.
 */
import type {
  ComponentDetailWithRaw,
  SaveOutcome,
  ValidationOutcome,
} from "@aseye/shared-types";
import { setAtPointer } from "./jsonPointer";

/** A flat JSON-shaped record - matches frontmatter / structured payloads. */
export type FormAst = Record<string, unknown>;

/** Payload pulled out of `SaveOutcome::ExternalChange` for the banner. */
export interface ExternalChangePayload {
  currentHash: string;
  currentContent: string;
}

/** Reducer state. `null` means "no component open". */
export interface EditState {
  /** `aseye://` URI of the component the editor is mounted on. */
  id: string;
  /** Hash of the content the editor opened with - the external-change guard. */
  originalHash: string;
  /** Raw on-disk text at open time. The "Discard" button reverts to this. */
  originalRaw: string;
  /** Monaco's live buffer. */
  currentRaw: string;
  /**
   * AST projected from `currentRaw`. Markdown components project the
   * frontmatter; pure-data components project the structured value.
   * Stays at the last successful parse when a re-parse fails.
   */
  formAst: FormAst;
  /** Last parse error message, if any. `null` when the last re-parse succeeded. */
  parseError: string | null;
  /** True when `currentRaw !== originalRaw`. */
  dirty: boolean;
  /** Last validator outcome the UI surfaces under each field. */
  validation: ValidationOutcome | null;
  /** External-change banner state, populated when the watcher invalidates underneath. */
  externalChange: ExternalChangePayload | null;
  /** Small AST history for the form-pane Cmd-Z (capped at HISTORY_LIMIT). */
  history: FormAst[];
  /** Cursor into `history`. `-1` means "no history yet". */
  historyIndex: number;
}

/** Cap for the form-pane undo stack. Matches the task brief (50). */
export const HISTORY_LIMIT = 50;

/** Discriminated reducer actions. */
export type EditAction =
  | { type: "open"; detail: ComponentDetailWithRaw; project: AstProjector }
  | { type: "setRaw"; raw: string; project: AstProjector }
  | { type: "setFormField"; pointer: string; value: unknown; serialise: AstSerialiser }
  | { type: "discard" }
  | { type: "markSaved"; newHash: string }
  | { type: "noteExternalChange"; payload: ExternalChangePayload }
  | { type: "clearExternalChange" }
  | { type: "setValidation"; outcome: ValidationOutcome | null }
  | { type: "undoFormChange"; serialise: AstSerialiser };

/**
 * Project a raw string into a form AST. Markdown components return
 * the frontmatter object; JSON / TOML / YAML components return the
 * top-level structured value as an object.
 *
 * Returns either a parsed AST or an error message; the reducer keeps
 * the last good AST when the projector fails so live-typing in the
 * raw pane doesn't blow away the form pane.
 */
export type AstProjector = (
  raw: string,
) => { ok: true; ast: FormAst } | { ok: false; error: string };

/**
 * Serialise a form AST back into raw text. Format-specific - the
 * caller (EditorView) wires the right serialiser based on the
 * component's format.
 */
export type AstSerialiser = (ast: FormAst, original: string) => string;

/** Initial state when the editor mounts with no component selected. */
export const EMPTY_EDIT_STATE: EditState | null = null;

/**
 * Pure reducer. No side effects - the EditorView dispatches actions
 * and re-renders from the returned state.
 */
export function editReducer(
  state: EditState | null,
  action: EditAction,
): EditState | null {
  switch (action.type) {
    case "open":
      return openState(action.detail, action.project);
    case "setRaw":
      return state ? applySetRaw(state, action.raw, action.project) : state;
    case "setFormField":
      return state
        ? applySetFormField(state, action.pointer, action.value, action.serialise)
        : state;
    case "discard":
      return state ? applyDiscard(state) : state;
    case "markSaved":
      return state ? applyMarkSaved(state, action.newHash) : state;
    case "noteExternalChange":
      return state
        ? { ...state, externalChange: { ...action.payload } }
        : state;
    case "clearExternalChange":
      return state ? { ...state, externalChange: null } : state;
    case "setValidation":
      return state ? { ...state, validation: action.outcome } : state;
    case "undoFormChange":
      return state ? applyUndoFormChange(state, action.serialise) : state;
    default:
      return state;
  }
}

// ─── Reducer branches ───────────────────────────────────────────────

function openState(detail: ComponentDetailWithRaw, project: AstProjector): EditState {
  const projected = project(detail.raw);
  const formAst = projected.ok ? projected.ast : {};
  const parseError = projected.ok ? null : projected.error;
  return {
    id: detail.detail.id,
    originalHash: detail.hash,
    originalRaw: detail.raw,
    currentRaw: detail.raw,
    formAst,
    parseError,
    dirty: false,
    validation: null,
    externalChange: null,
    history: [formAst],
    historyIndex: 0,
  };
}

function applySetRaw(
  state: EditState,
  raw: string,
  project: AstProjector,
): EditState {
  if (raw === state.currentRaw) return state;
  const projected = project(raw);
  if (projected.ok) {
    return {
      ...state,
      currentRaw: raw,
      formAst: projected.ast,
      parseError: null,
      dirty: raw !== state.originalRaw,
    };
  }
  // Parse failed: keep the last good `formAst` so the form pane stays
  // usable. Surface the error message so the user knows why the
  // form is stale.
  return {
    ...state,
    currentRaw: raw,
    parseError: projected.error,
    dirty: raw !== state.originalRaw,
  };
}

function applySetFormField(
  state: EditState,
  pointer: string,
  value: unknown,
  serialise: AstSerialiser,
): EditState {
  const next = setAtPointer(state.formAst, pointer, value) as FormAst;
  if (next === state.formAst) return state;
  const newRaw = serialise(next, state.currentRaw);
  const history = pushHistory(state.history, state.historyIndex, next);
  return {
    ...state,
    formAst: next,
    currentRaw: newRaw,
    parseError: null,
    dirty: newRaw !== state.originalRaw,
    history,
    historyIndex: history.length - 1,
  };
}

function applyDiscard(state: EditState): EditState {
  // Re-project the original raw so the form mirrors the on-disk
  // shape. We can't use `state.formAst` as-is because the form may
  // have diverged from the original through partial edits.
  return {
    ...state,
    currentRaw: state.originalRaw,
    // Leave validation alone - the next render will refresh it on
    // demand. Clearing it here prevents a stale red badge from
    // sticking after a discard.
    validation: null,
    parseError: null,
    dirty: false,
  };
}

function applyMarkSaved(state: EditState, newHash: string): EditState {
  return {
    ...state,
    originalHash: newHash,
    originalRaw: state.currentRaw,
    dirty: false,
    externalChange: null,
  };
}

function applyUndoFormChange(state: EditState, serialise: AstSerialiser): EditState {
  if (state.historyIndex <= 0) return state;
  const prevIndex = state.historyIndex - 1;
  const prev = state.history[prevIndex];
  if (prev === undefined) return state;
  const newRaw = serialise(prev, state.currentRaw);
  return {
    ...state,
    formAst: prev,
    currentRaw: newRaw,
    parseError: null,
    dirty: newRaw !== state.originalRaw,
    historyIndex: prevIndex,
  };
}

function pushHistory(
  history: FormAst[],
  index: number,
  next: FormAst,
): FormAst[] {
  // Drop any redo-frontier entries when a new edit lands after an
  // undo. Standard editor undo-stack semantics.
  const trimmed = history.slice(0, index + 1);
  trimmed.push(next);
  if (trimmed.length > HISTORY_LIMIT) {
    return trimmed.slice(trimmed.length - HISTORY_LIMIT);
  }
  return trimmed;
}

// ─── External-change helper exposed for the IPC bridge ──────────────

/**
 * Build an `ExternalChangePayload` from a `SaveOutcome::ExternalChange`
 * variant. Centralised so the EditorView and any future consumer
 * use the same field-name conventions.
 */
export function externalChangeFromOutcome(
  outcome: Extract<SaveOutcome, { kind: "externalChange" }>,
): ExternalChangePayload {
  return {
    currentHash: outcome.currentHash,
    currentContent: outcome.currentContent,
  };
}
