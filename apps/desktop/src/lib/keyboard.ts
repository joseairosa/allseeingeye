import { useEffect } from "react";
import { useUi } from "@/store/ui";

/**
 * Global keyboard map.
 * Mirrors `design/app.js` shortcuts and the table in `docs/06-ux-design.md`:
 *   ⌘K              command palette
 *   ⌘1 / ⌘2 / ⌘3 / ⌘4 / ⌘5  inventory / map / editor / health / security
 *   ⌘,              settings view
 *   ⌘⇧.             panic mode toggle (instant secret mask)
 *   Esc             close palette / onboarding / quick look
 */
export function useGlobalKeyboard(): void {
  const togglePalette = useUi((s) => s.togglePalette);
  const toggleOnboarding = useUi((s) => s.toggleOnboarding);
  const toggleQuickLook = useUi((s) => s.toggleQuickLook);
  const togglePanicMode = useUi((s) => s.togglePanicMode);
  const setView = useUi((s) => s.setView);

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
  }, [togglePalette, toggleOnboarding, toggleQuickLook, togglePanicMode, setView]);
}
