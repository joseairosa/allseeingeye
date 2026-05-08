import { useEffect } from "react";
import { useUi } from "@/store/ui";

/**
 * Global keyboard map.
 * Mirrors `design/app.js` shortcuts:
 *   ⌘K            command palette
 *   ⌘1 / ⌘2 / ⌘3  inventory / map / editor
 *   Esc           close palette / onboarding / quick look
 */
export function useGlobalKeyboard() {
  const togglePalette = useUi((s) => s.togglePalette);
  const toggleOnboarding = useUi((s) => s.toggleOnboarding);
  const toggleQuickLook = useUi((s) => s.toggleQuickLook);
  const setView = useUi((s) => s.setView);

  useEffect(() => {
    function onKey(event: KeyboardEvent) {
      const mod = event.metaKey || event.ctrlKey;

      if (mod && event.key.toLowerCase() === "k") {
        event.preventDefault();
        togglePalette();
        return;
      }

      if (mod && ["1", "2", "3", "4"].includes(event.key)) {
        event.preventDefault();
        const map = { "1": "inventory", "2": "map", "3": "editor", "4": "health" } as const;
        setView(map[event.key as "1" | "2" | "3" | "4"]);
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
  }, [togglePalette, toggleOnboarding, toggleQuickLook, setView]);
}
