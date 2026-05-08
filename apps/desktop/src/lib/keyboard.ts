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
