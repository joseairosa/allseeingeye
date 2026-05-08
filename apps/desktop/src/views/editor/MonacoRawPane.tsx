/**
 * Lazy-loaded Monaco editor pane.
 *
 * This module is the deliberate code-split target: importing it from
 * `EditorView` via `React.lazy` keeps `monaco-editor` (~2-3 MB before
 * gzip) out of the main chunk. The bundle splits at the dynamic import
 * boundary; everything Monaco needs is reachable only through this
 * file.
 *
 * Phase 3.1 ships read+write capability without save - the parent
 * view feeds raw text in via props. Save / discard wires up in Phase
 * 3.3 once the AST round-trip lands.
 *
 * Design constraints (docs/06 + docs/11):
 *   - Theme follows the resolved app theme (`body.light` => `vs-light`).
 *   - Reduced-motion users get smooth-scrolling disabled.
 *   - Tabs render as 2 spaces; soft-tabs only.
 *   - Language inferred from the component's `Format`. MDC routes to
 *     Markdown.
 *   - `automaticLayout: true` lets Monaco reflow when its host resizes.
 */
import { useEffect, useMemo, type ReactElement } from "react";
import Editor, { type OnMount } from "@monaco-editor/react";
import type * as Monaco from "monaco-editor";
import type { Format } from "@aseye/shared-types";
import { useUi } from "@/store/ui";
import { setupMonaco } from "./MonacoLoader";

export interface MonacoRawPaneProps {
  /** Raw on-disk text - the source of truth until Phase 3.3. */
  content: string;
  /** Drives the language mode + read-only fallback. */
  format: Format;
  /** Phase 3.3 will wire save; until then onChange is informational. */
  onChange?: (next: string) => void;
}

/**
 * Map our cross-tool `Format` to a Monaco language id. Anything we
 * don't have a first-class mode for (jsonl, sqlite, binary) falls back
 * to plaintext so the editor still loads.
 */
function languageFor(format: Format): string {
  switch (format) {
    case "json":
      return "json";
    case "yaml":
      return "yaml";
    case "toml":
      // Monaco doesn't ship a TOML language out of the box; "ini" gives
      // workable highlighting until we register a proper TOML grammar.
      return "ini";
    case "markdown":
    case "markdownfrontmatter":
    case "mdc":
      return "markdown";
    case "jsonl":
    case "sqlite":
    case "binary":
    default:
      return "plaintext";
  }
}

/** Detect the user's effective reduced-motion preference. */
function prefersReducedMotion(): boolean {
  if (typeof window === "undefined") return false;
  return window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

/**
 * Snapshot of the body class at mount. We re-evaluate via subscription
 * inside the component so theme flips re-render Monaco's options.
 */
function isLightTheme(): boolean {
  if (typeof document === "undefined") return false;
  return document.body.classList.contains("light");
}

export default function MonacoRawPane({
  content,
  format,
  onChange,
}: MonacoRawPaneProps): ReactElement {
  // Configure the loader exactly once, the first time this lazy chunk
  // is mounted. Idempotent on re-mount.
  useEffect(() => {
    setupMonaco();
  }, []);

  // Subscribe to the user's reduced-motion override so toggling the
  // setting flips smooth-scrolling without a remount.
  const reducedMotion = useUi((s) => s.reducedMotion);
  const reduced = reducedMotion === "on" || prefersReducedMotion();

  // The light/dark theme lives on `document.body` and is updated by
  // `App.tsx`. We don't have an event for that, but the user's
  // `theme` setting is a Zustand value the parent already touches when
  // it flips - subscribing to it forces a re-render with the right
  // Monaco theme when the user toggles.
  const appTheme = useUi((s) => s.theme);
  const theme = useMemo(() => {
    // `appTheme === "system"` resolves at the body-class layer, so we
    // read the actual class rather than re-implementing the resolver.
    void appTheme;
    return isLightTheme() ? "vs-light" : "vs-dark";
  }, [appTheme]);

  const language = languageFor(format);

  const handleMount: OnMount = (editor, monaco) => {
    // Editor-wide visual options (whitespace, smooth scrolling, ...).
    const opts: Monaco.editor.IEditorOptions = {
      smoothScrolling: !reduced,
      cursorSmoothCaretAnimation: reduced ? "off" : "explicit",
      // The `automaticLayout` option makes Monaco watch its container
      // size. Combined with our flex parent it removes the need for a
      // ResizeObserver on the React side.
      automaticLayout: true,
      minimap: { enabled: false },
      // Word wrap at the column boundary so frontmatter blocks don't
      // force horizontal scroll inside a narrow pane.
      wordWrap: "on",
      renderWhitespace: "selection",
    };
    editor.updateOptions(opts);
    // Tab settings live on the model; force soft-tabs of width 2 for
    // every model the editor mounts. The model preserves any tab
    // characters already present in the file, but new edits insert
    // spaces.
    const model = editor.getModel();
    if (model) {
      model.updateOptions({ tabSize: 2, insertSpaces: true });
    }
    void monaco;
  };

  return (
    <div className="editor-monaco-host" data-language={language}>
      <Editor
        height="100%"
        defaultLanguage={language}
        language={language}
        value={content}
        theme={theme}
        onMount={handleMount}
        onChange={(next) => {
          if (typeof next === "string") onChange?.(next);
        }}
        options={{
          smoothScrolling: !reduced,
          minimap: { enabled: false },
          wordWrap: "on",
          automaticLayout: true,
          // Monaco's read-only mode is per-instance; Phase 3.1 keeps
          // edits live in the buffer but the host disables Save until
          // Phase 3.3 lands.
          readOnly: false,
        }}
      />
    </div>
  );
}
