import { create } from "zustand";

export type ViewId = "inventory" | "map" | "editor" | "health";
export type Density = "comfortable" | "compact";
export type Theme = "dark" | "light";

export interface UiState {
  view: ViewId;
  theme: Theme;
  density: Density;
  search: string;
  selectedComponentId: string | null;
  paletteOpen: boolean;
  quickLookOpen: boolean;
  onboardingOpen: boolean;

  setView: (view: ViewId) => void;
  toggleTheme: () => void;
  toggleDensity: () => void;
  setSearch: (search: string) => void;
  selectComponent: (id: string | null) => void;
  togglePalette: (force?: boolean) => void;
  toggleQuickLook: (force?: boolean) => void;
  toggleOnboarding: (force?: boolean) => void;
}

export const useUi = create<UiState>((set) => ({
  view: "inventory",
  theme: "dark",
  density: "comfortable",
  search: "type:skill tool:claude-code",
  selectedComponentId: "spec",
  paletteOpen: false,
  quickLookOpen: false,
  onboardingOpen: false,

  setView: (view) => set({ view }),
  toggleTheme: () =>
    set((s) => ({ theme: s.theme === "dark" ? "light" : "dark" })),
  toggleDensity: () =>
    set((s) => ({
      density: s.density === "comfortable" ? "compact" : "comfortable",
    })),
  setSearch: (search) => set({ search }),
  selectComponent: (id) =>
    set({ selectedComponentId: id, quickLookOpen: id !== null }),
  togglePalette: (force) =>
    set((s) => ({ paletteOpen: force ?? !s.paletteOpen })),
  toggleQuickLook: (force) =>
    set((s) => ({ quickLookOpen: force ?? !s.quickLookOpen })),
  toggleOnboarding: (force) =>
    set((s) => ({ onboardingOpen: force ?? !s.onboardingOpen })),
}));
