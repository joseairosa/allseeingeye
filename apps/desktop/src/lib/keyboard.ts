import { useEffect } from "react";
import { useUi } from "@/store/ui";

/**
 * Global keyboard map.
 * Mirrors `design/app.js` shortcuts and the table in `docs/06-ux-design.md`:
 *   ⌘K              command palette
 *   ⌘1 / ⌘2 / ⌘3 / ⌘4 / ⌘5  inventory / map / editor / health / security
 *   ⌘,              settings view
 *   ⌘⇧.             panic mode toggle (instant secret mask)
 *   ⌘S              save the open editor (Phase 3.3) - dispatched as a
 *                   global custom event so the EditorView can react
 *                   without a circular hook dependency
 *   Esc             close palette / onboarding / quick look
 */

/**
 * Custom-event name dispatched on `window` when the user presses
 * Cmd-S while the Editor view is active. The EditorView listens for
 * this event and routes it to the save mutation. Using a custom
 * event keeps the keyboard layer decoupled from Editor-specific
 * state, mirroring how the panic-mode toggle works through the UI
 * store.
 */
export const EDITOR_SAVE_EVENT = "aseye:editor-save";

/**
 * Custom-event name for Cmd-Z inside the form pane. Same decoupling
 * pattern as `EDITOR_SAVE_EVENT`. We only fire the event when the
 * keystroke originates outside Monaco (which has its own internal
 * undo stack the user expects to win for raw-pane edits) and the
 * Editor view is active.
 */
export const EDITOR_UNDO_EVENT = "aseye:editor-undo";

/**
 * Decide whether a Cmd-Z keystroke should route through the
 * form-pane undo or fall through to Monaco / the platform default.
 * Monaco mounts a `data-language` attribute on its host and traps
 * its own input focus; an active element inside that subtree means
 * Monaco's undo should win.
 */
function shouldUndoFormPane(target: EventTarget | null): boolean {
  if (!(target instanceof Element)) return false;
  // The Monaco wrapper renders `<div class="editor-monaco-host" ...>`;
  // any focus inside that container belongs to Monaco's undo stack.
  if (target.closest(".editor-monaco-host")) return false;
  // Only fire when focus is inside the form pane. We check the
  // class the FormPane renders (`form-pane`) rather than asserting
  // a specific input tag so future field types stay covered.
  return target.closest(".form-pane") !== null;
}

export function useGlobalKeyboard(): void {
  const togglePalette = useUi((s) => s.togglePalette);
  const toggleOnboarding = useUi((s) => s.toggleOnboarding);
  const toggleQuickLook = useUi((s) => s.toggleQuickLook);
  const togglePanicMode = useUi((s) => s.togglePanicMode);
  const setView = useUi((s) => s.setView);
  const view = useUi((s) => s.view);

  useEffect(() => {
    function onKey(event: KeyboardEvent): void {
      const mod = event.metaKey || event.ctrlKey;

      // Panic mode (Cmd-Shift-.) - check before plain comma to avoid eating
      // the modifier-Shift combination.
      if (mod && event.shiftKey && event.key === ".") {
        event.preventDefault();
        togglePanicMode();
        return;
      }

      // Cmd-S inside the Editor view fires the save dispatcher event.
      // Outside the Editor it falls through so the browser's default
      // (no-op for our shell, but harmless) wins. We deliberately do
      // NOT swallow Cmd-S unless the Editor is active so users in
      // other views can use OS-level shortcuts that share the
      // chord.
      if (mod && !event.shiftKey && event.key.toLowerCase() === "s") {
        if (view === "editor") {
          event.preventDefault();
          window.dispatchEvent(new CustomEvent(EDITOR_SAVE_EVENT));
        }
        return;
      }

      // Cmd-Z inside the form pane fires the undo dispatcher event.
      // Monaco's own undo wins when its editor has focus; the form
      // pane's reducer history advances by one when focus is on a
      // schema-driven field. Cmd-Shift-Z (redo) is not yet
      // implemented in the reducer so we let the platform default
      // through for now.
      if (
        mod &&
        !event.shiftKey &&
        event.key.toLowerCase() === "z" &&
        view === "editor" &&
        shouldUndoFormPane(event.target)
      ) {
        event.preventDefault();
        window.dispatchEvent(new CustomEvent(EDITOR_UNDO_EVENT));
        return;
      }

      if (mod && event.key.toLowerCase() === "k") {
        event.preventDefault();
        togglePalette();
        return;
      }

      // Cmd-, opens Settings (also accepts the `<` key which some layouts
      // emit for Shift+comma; only fire when shift is NOT held).
      if (mod && !event.shiftKey && event.key === ",") {
        event.preventDefault();
        setView("settings");
        return;
      }

      if (mod && ["1", "2", "3", "4", "5"].includes(event.key)) {
        event.preventDefault();
        const map = {
          "1": "inventory",
          "2": "map",
          "3": "editor",
          "4": "health",
          "5": "security",
        } as const;
        setView(map[event.key as "1" | "2" | "3" | "4" | "5"]);
        return;
      }

      if (event.key === "Escape") {
        togglePalette(false);
        toggleOnboarding(false);
        toggleQuickLook(false);
      }
    }
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [togglePalette, toggleOnboarding, toggleQuickLook, togglePanicMode, setView, view]);
}
