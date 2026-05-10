import { useEffect } from "react";
import { useUi } from "@/store/ui";
import { useGlobalKeyboard } from "@/lib/keyboard";
import { TitleBar } from "@/components/TitleBar";
import { Sidebar } from "@/components/Sidebar";
import { MainHeader } from "@/components/MainHeader";
import { Statusbar } from "@/components/Statusbar";
import { QuickLook } from "@/components/QuickLook";
import { CommandPalette } from "@/components/CommandPalette";
import { Onboarding } from "@/components/Onboarding";
import { InventoryView } from "@/views/InventoryView";
import { MapView } from "@/views/MapView";
import { EditorView } from "@/views/EditorView";
import { HealthView } from "@/views/HealthView";
import { CostView } from "@/views/CostView";
import { ProjectsView } from "@/views/ProjectsView";
import { SecurityView } from "@/views/SecurityView";
import { SettingsView } from "@/views/SettingsView";
import { useHealthSummary, usePipelineEventInvalidator } from "@/ipc/hooks";
import { useDiagnosticsRing } from "@/lib/diagnosticsRing";
import { loadOnboardingCompleted } from "@/lib/onboarding";

/**
 * Resolve the effective theme, honouring the user's `system` selection.
 */
function useResolvedTheme(): void {
  const theme = useUi((s) => s.theme);
  useEffect(() => {
    function apply(): void {
      const prefersLight =
        theme === "light" ||
        (theme === "system" &&
          typeof window !== "undefined" &&
          window.matchMedia("(prefers-color-scheme: light)").matches);
      document.body.classList.toggle("light", prefersLight);
    }
    apply();
    if (theme !== "system") return;
    const mql = window.matchMedia("(prefers-color-scheme: light)");
    mql.addEventListener("change", apply);
    return () => mql.removeEventListener("change", apply);
  }, [theme]);
}

function useDensityClass(): void {
  const density = useUi((s) => s.density);
  useEffect(() => {
    document.body.classList.toggle("compact", density === "compact");
  }, [density]);
}

/**
 * Apply the user's reduced-motion override on top of the OS preference.
 * The CSS already short-circuits transitions when `prefers-reduced-motion`
 * is set; here we just toggle a body class so a future stylesheet rule can
 * force the same behaviour when the user opts in manually.
 */
function useReducedMotionOverride(): void {
  const mode = useUi((s) => s.reducedMotion);
  useEffect(() => {
    document.body.classList.toggle("reduced-motion", mode === "on");
  }, [mode]);
}

function usePanicBodyClass(): void {
  const panicMode = useUi((s) => s.panicMode);
  useEffect(() => {
    document.body.classList.toggle("panic", panicMode);
  }, [panicMode]);
}

/**
 * Module-level guard so re-mounts (HMR, StrictMode double-invoke) don't
 * pop the onboarding modal back open after the user has dismissed it
 * within the current session.
 */
let onboardingAutoOpenAttempted = false;

/**
 * Detect a real Tauri runtime. Storybook + raw `vite preview` should
 * never auto-open onboarding; only the packaged desktop shell or
 * `tauri dev` injects the IPC bridge under `window.__TAURI_INTERNALS__`.
 */
function isTauriRuntime(): boolean {
  if (typeof window === "undefined") return false;
  const w = window as unknown as { __TAURI_INTERNALS__?: unknown };
  return typeof w.__TAURI_INTERNALS__ !== "undefined";
}

function useFirstLaunchOnboarding(): void {
  const toggleOnboarding = useUi((s) => s.toggleOnboarding);
  useEffect(() => {
    if (onboardingAutoOpenAttempted) return;
    onboardingAutoOpenAttempted = true;
    if (!isTauriRuntime()) return;
    if (loadOnboardingCompleted()) return;
    toggleOnboarding(true);
  }, [toggleOnboarding]);
}

export function App() {
  useResolvedTheme();
  useDensityClass();
  useReducedMotionOverride();
  usePanicBodyClass();
  useGlobalKeyboard();
  usePipelineEventInvalidator();
  useDiagnosticsRing();
  useFirstLaunchOnboarding();

  const panicMode = useUi((s) => s.panicMode);
  const health = useHealthSummary();
  const totalComponents = health.data?.totalComponents ?? 0;

  return (
    <div className="app-shell" data-density="comfortable">
      <TitleBar />
      {panicMode ? (
        <div
          className="panic-badge"
          role="status"
          aria-live="polite"
          style={{ position: "fixed", top: 8, right: 12, zIndex: 50 }}
        >
          panic
        </div>
      ) : null}
      <Sidebar />
      <main className="main-area">
        <MainHeader />
        <InventoryView />
        <MapView />
        <EditorView />
        <HealthView />
        <CostView />
        <ProjectsView />
        <SecurityView />
        <SettingsView />
        <Statusbar resultCount={totalComponents} />
      </main>
      <QuickLook />
      <CommandPalette />
      <Onboarding />
    </div>
  );
}
