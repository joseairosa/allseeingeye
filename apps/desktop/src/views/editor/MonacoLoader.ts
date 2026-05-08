/**
 * One-time Monaco-editor configuration.
 *
 * `@monaco-editor/react` resolves Monaco lazily via its own loader.
 * The loader's default behaviour is to fetch Monaco from a public CDN
 * at runtime, which would break the offline-first guarantee in
 * `docs/02-prd.md` H4 ("Works offline"). We override that by passing
 * the locally-bundled `monaco-editor` module to `loader.config({...})`,
 * which makes the loader resolve to our copy instead of touching the
 * network.
 *
 * Monaco itself is imported via a side-effect import so Vite pulls the
 * editor + language tokens for the languages we touch (JSON / YAML /
 * Markdown) into THIS module's chunk. Combined with `React.lazy` in
 * `EditorView.tsx`, the entire Monaco footprint stays out of the main
 * `index-*.js` bundle and only loads when the Editor view first
 * mounts.
 *
 * Phase 3.2 layers `monaco-yaml` on top to drive schema-aware
 * validation; Phase 3.3 wires save and adds language workers proper.
 */
import { loader } from "@monaco-editor/react";
import * as monaco from "monaco-editor";
import editorWorker from "monaco-editor/esm/vs/editor/editor.worker?worker";
import jsonWorker from "monaco-editor/esm/vs/language/json/json.worker?worker";
// monaco-editor doesn't ship a YAML worker; monaco-yaml does, but until
// we wire it (Phase 3.2 schema validation) we route YAML through the
// generic editor worker - syntax highlighting works, schema validation
// is opt-in.

// Hand the loader the bundled Monaco instance synchronously, at module
// load time. Calling `loader.config({ monaco })` from inside a
// `useEffect` is too late: `<Editor>` mounts on the same render that
// scheduled the effect and triggers `loader.init()` immediately, which
// then races to fetch a separate Monaco copy from `cdn.jsdelivr`. Our
// Tauri CSP (`connect-src 'self' ipc:`) blocks that fetch, init never
// resolves, and `@monaco-editor/react` shows "Loading..." forever.
//
// Side-effect import order matters: this file is imported by
// `MonacoRawPane.tsx`, which is itself reached only via `React.lazy`.
// The lazy chunk evaluates this module before the component function
// runs, so the loader is configured before any `<Editor>` renders.
loader.config({ monaco });

// Monaco language services (JSON validation, formatters, TS) run in
// dedicated web workers. `@monaco-editor/react`'s default behaviour is
// to fetch worker scripts from the CDN, which our CSP blocks. Vite's
// `?worker` import returns a `Worker`-constructing class that the
// Monaco runtime can hand back from `MonacoEnvironment.getWorker`.
self.MonacoEnvironment = {
  getWorker(_workerId: string, label: string): Worker {
    if (label === "json") return new jsonWorker();
    return new editorWorker();
  },
};

let didConfigure = false;

/**
 * Idempotent hook for components to call on mount. The actual loader
 * configuration runs at module top-level (above) - this remains as a
 * no-op shim so callers don't need to change their import sites and so
 * the side effect of importing this module is preserved even under
 * tree-shaking.
 */
export function setupMonaco(): void {
  if (didConfigure) return;
  didConfigure = true;
  // Loader is already configured at module top. Touching `monaco` here
  // keeps the import alive so a future bundler doesn't decide the
  // top-level side effects are dead code.
  void monaco;
}
