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

let didConfigure = false;

/**
 * Configure the Monaco loader the first time the Editor view is shown.
 * Subsequent calls are no-ops.
 */
export function setupMonaco(): void {
  if (didConfigure) return;
  didConfigure = true;

  // Hand the loader the bundled Monaco instance. Without this the
  // loader would race-fetch a different Monaco copy from `cdn.jsdelivr`
  // at runtime - bad for offline use AND a memory leak (two Monacos in
  // the same window). See: https://github.com/suren-atoyan/monaco-loader.
  loader.config({ monaco });
}
