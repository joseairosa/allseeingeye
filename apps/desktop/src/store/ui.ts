import { create } from "zustand";
import { persist, createJSONStorage } from "zustand/middleware";

export type ViewId =
  | "inventory"
  | "map"
  | "editor"
  | "health"
  | "cost"
  | "security"
  | "settings";
export type Density = "comfortable" | "compact";
/**
 * `system` defers to `prefers-color-scheme`; `dark` and `light` force.
 * The `body.light` class is applied by `App.tsx` based on the resolved theme.
 */
export type Theme = "dark" | "light" | "system";
export type UpdateChannel = "stable" | "beta";
export type McpProbingMode = "off" | "per-server" | "global";

export interface UiState {
  view: ViewId;
  theme: Theme;
  density: Density;
  search: string;
  selectedComponentId: string | null;
  paletteOpen: boolean;
  quickLookOpen: boolean;
  onboardingOpen: boolean;
  /**
   * Panic mode: forces every `SecretField` to mask, closes Quick Look,
   * the command palette, and onboarding. Toggled with Cmd-Shift-.
   */
  panicMode: boolean;
  /**
   * `Date.now()` at the moment panic mode was last toggled (on or off),
   * surfaced by the Diagnostics panel so support can see whether the user
   * has been in panic mode recently. `null` until the first toggle.
   */
  panicModeLastToggledAt: number | null;
  /**
   * User override for `prefers-reduced-motion`. `system` defers to OS.
   */
  reducedMotion: "system" | "on" | "off";
  /**
   * Default MCP probing posture. Off by default per docs/06 F14.
   */
  mcpProbing: McpProbingMode;
  /**
   * Telemetry must remain off in MVP (docs/12). The setting exists but is
   * read-only in the UI.
   */
  telemetryEnabled: false;
  updateChannel: UpdateChannel;
  autoCheckUpdates: boolean;
  /**
   * Audit issue #18: Sidebar Health rows ("Drift", "MCP issues") set
   * this when navigating to Health so the view can scroll to and
   * briefly highlight the matching pane. Cleared automatically by the
   * pane that consumes it after the highlight animation ends.
   */
  healthFocus: "drift" | "mcp" | null;

  setView: (view: ViewId) => void;
  setTheme: (theme: Theme) => void;
  toggleTheme: () => void;
  setDensity: (density: Density) => void;
  toggleDensity: () => void;
  setReducedMotion: (mode: UiState["reducedMotion"]) => void;
  setMcpProbing: (mode: McpProbingMode) => void;
  setUpdateChannel: (channel: UpdateChannel) => void;
  setAutoCheckUpdates: (value: boolean) => void;
  setSearch: (search: string) => void;
  selectComponent: (id: string | null) => void;
  togglePalette: (force?: boolean) => void;
  toggleQuickLook: (force?: boolean) => void;
  toggleOnboarding: (force?: boolean) => void;
  togglePanicMode: (force?: boolean) => void;
  setHealthFocus: (focus: UiState["healthFocus"]) => void;
}

/**
 * Fields persisted to `localStorage` via Zustand's `persist` middleware.
 * The shape is an explicit allowlist of user preferences; everything
 * else is session state and intentionally resets each launch (e.g.
 * `view`, `search`, `selectedComponentId`, `paletteOpen`, `panicMode`).
 *
 * Adding a new persisted field: extend `PersistedUiState` AND
 * `partialize` below. Adding a session-only field needs no change.
 */
type PersistedUiState = Pick<
  UiState,
  | "theme"
  | "density"
  | "reducedMotion"
  | "mcpProbing"
  | "updateChannel"
  | "autoCheckUpdates"
>;

const PERSIST_KEY = "aseye:ui";
/**
 * Bumped when the persisted shape changes. The `migrate` callback runs
 * for any older snapshot found in storage; we treat unknown shapes as
 * "discard, fall back to defaults" rather than guessing because the
 * partition is small.
 */
const PERSIST_VERSION = 1;

export const useUi = create<UiState>()(
  persist(
    (set) => ({
      view: "inventory",
      theme: "dark",
      density: "comfortable",
      search: "type:skill tool:claude-code",
      selectedComponentId: "spec",
      paletteOpen: false,
      quickLookOpen: false,
      onboardingOpen: false,
      panicMode: false,
      panicModeLastToggledAt: null,
      reducedMotion: "system",
      mcpProbing: "off",
      telemetryEnabled: false,
      updateChannel: "stable",
      autoCheckUpdates: true,
      healthFocus: null,

      setView: (view) => set({ view }),
      setTheme: (theme) => set({ theme }),
      toggleTheme: () =>
        set((s) => ({
          theme:
            s.theme === "dark"
              ? "light"
              : s.theme === "light"
                ? "system"
                : "dark",
        })),
      setDensity: (density) => set({ density }),
      toggleDensity: () =>
        set((s) => ({
          density: s.density === "comfortable" ? "compact" : "comfortable",
        })),
      setReducedMotion: (reducedMotion) => set({ reducedMotion }),
      setMcpProbing: (mcpProbing) => set({ mcpProbing }),
      setUpdateChannel: (updateChannel) => set({ updateChannel }),
      setAutoCheckUpdates: (autoCheckUpdates) => set({ autoCheckUpdates }),
      setSearch: (search) => set({ search }),
      selectComponent: (id) =>
        set({ selectedComponentId: id, quickLookOpen: id !== null }),
      togglePalette: (force) =>
        set((s) => ({ paletteOpen: force ?? !s.paletteOpen })),
      toggleQuickLook: (force) =>
        set((s) => ({ quickLookOpen: force ?? !s.quickLookOpen })),
      toggleOnboarding: (force) =>
        set((s) => ({ onboardingOpen: force ?? !s.onboardingOpen })),
      setHealthFocus: (focus) => set({ healthFocus: focus }),
      togglePanicMode: (force) =>
        set((s) => {
          const next = force ?? !s.panicMode;
          // No-op if the requested state matches the current state - we
          // don't bump `lastToggledAt` for a redundant toggle.
          if (next === s.panicMode) return {};
          const panicModeLastToggledAt = Date.now();
          // Activating panic mode forcibly closes any open surface that could
          // be revealing a secret or distracting the user.
          if (next) {
            return {
              panicMode: true,
              panicModeLastToggledAt,
              quickLookOpen: false,
              paletteOpen: false,
              onboardingOpen: false,
            };
          }
          return { panicMode: false, panicModeLastToggledAt };
        }),
    }),
    {
      name: PERSIST_KEY,
      version: PERSIST_VERSION,
      storage: createJSONStorage(() => localStorage),
      partialize: (state): PersistedUiState => ({
        theme: state.theme,
        density: state.density,
        reducedMotion: state.reducedMotion,
        mcpProbing: state.mcpProbing,
        updateChannel: state.updateChannel,
        autoCheckUpdates: state.autoCheckUpdates,
      }),
    },
  ),
);
