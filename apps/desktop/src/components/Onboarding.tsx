/**
 * Onboarding orchestrator (Phase 4.1).
 *
 * Multi-step first-launch flow:
 *   welcome -> detect -> permission -> scan -> tour -> done
 *
 * The orchestrator owns the step machine, fetches `useTools()` for the
 * detection list, drives `start_full_scan` on entry to the scan step,
 * and persists the completed flag via `lib/onboarding.ts`.
 *
 * Sub-step components live in `components/onboarding/*` and receive
 * `state` + `actions` as props - they never call IPC directly. This
 * keeps each step under 150 lines and easy to render in Storybook.
 */
import {
  useCallback,
  useEffect,
  useMemo,
  useReducer,
  useRef,
  type FocusEvent,
  type KeyboardEvent,
  type ReactNode,
} from "react";
import type {
  DetectedTool,
  ScanReport,
  ToolId,
} from "@aseye/shared-types";
import { useUi } from "@/store/ui";
import { useTools } from "@/ipc/hooks";
import { startFullScan } from "@/ipc/index";
import { markOnboardingCompleted } from "@/lib/onboarding";

import { Welcome } from "./onboarding/Welcome";
import { Detect } from "./onboarding/Detect";
import { Permission } from "./onboarding/Permission";
import { Scan } from "./onboarding/Scan";
import { Tour } from "./onboarding/Tour";
import { Done } from "./onboarding/Done";
import type {
  EnabledTools,
  OnboardingActions,
  OnboardingState,
  OnboardingStep,
  OnboardingStepProps,
} from "./onboarding/types";

const STEP_ORDER: readonly OnboardingStep[] = [
  "welcome",
  "detect",
  "permission",
  "scan",
  "tour",
  "done",
] as const;

/**
 * The single non-skippable step is the implicit close at "done". A user
 * can always Skip from `welcome` and `tour`, jumping straight to `done`.
 */
const STEP_TITLES: Record<OnboardingStep, string> = {
  welcome: "Welcome",
  detect: "Detect",
  permission: "Permission",
  scan: "Scan",
  tour: "Tour",
  done: "Done",
};

type ReducerAction =
  | { type: "next" }
  | { type: "back" }
  | { type: "skip" }
  | { type: "toggleTool"; id: ToolId; enabled: boolean }
  | { type: "seedTools"; tools: DetectedTool[] }
  | { type: "scanStarted" }
  | { type: "scanProgress"; pct: number }
  | { type: "scanCompleted"; report: ScanReport }
  | { type: "scanFailed"; message: string };

function nextStep(step: OnboardingStep): OnboardingStep {
  const idx = STEP_ORDER.indexOf(step);
  // STEP_ORDER is non-empty and `step` always appears in it; the bounded
  // index access is safe but still narrowed for `noUncheckedIndexedAccess`.
  const peek = STEP_ORDER[idx + 1];
  return peek ?? "done";
}

function prevStep(step: OnboardingStep): OnboardingStep {
  const idx = STEP_ORDER.indexOf(step);
  const peek = idx <= 0 ? STEP_ORDER[0] : STEP_ORDER[idx - 1];
  return peek ?? "welcome";
}

function initialEnabledTools(tools: DetectedTool[]): EnabledTools {
  const acc: EnabledTools = {
    "claude-code": false,
    codex: false,
    cursor: false,
    antigravity: false,
  };
  for (const t of tools) acc[t.id] = t.detected;
  return acc;
}

function reducer(state: OnboardingState, action: ReducerAction): OnboardingState {
  switch (action.type) {
    case "next":
      return { ...state, step: nextStep(state.step) };
    case "back":
      return { ...state, step: prevStep(state.step) };
    case "skip":
      return { ...state, step: "done" };
    case "toggleTool":
      return {
        ...state,
        enabledTools: { ...state.enabledTools, [action.id]: action.enabled },
      };
    case "seedTools":
      return { ...state, enabledTools: initialEnabledTools(action.tools) };
    case "scanStarted":
      return { ...state, scanReport: null, scanError: null, scanProgress: 0 };
    case "scanProgress":
      return { ...state, scanProgress: action.pct };
    case "scanCompleted":
      return {
        ...state,
        scanReport: action.report,
        scanError: null,
        scanProgress: 100,
      };
    case "scanFailed":
      return { ...state, scanError: action.message };
    default:
      return state;
  }
}

const INITIAL_STATE: OnboardingState = {
  step: "welcome",
  enabledTools: {
    "claude-code": false,
    codex: false,
    cursor: false,
    antigravity: false,
  },
  scanReport: null,
  scanProgress: 0,
  scanError: null,
};

/**
 * Resolve the focusable elements inside the panel for a tab-trap. We
 * scope to buttons/inputs/links because that's all our steps render and
 * a generic `:focusable` query would also catch decorative elements.
 */
function focusableInside(root: HTMLElement | null): HTMLElement[] {
  if (!root) return [];
  const nodes = root.querySelectorAll<HTMLElement>(
    'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])',
  );
  return Array.from(nodes);
}

export function Onboarding() {
  const open = useUi((s) => s.onboardingOpen);
  const toggleOnboarding = useUi((s) => s.toggleOnboarding);
  const panelRef = useRef<HTMLDivElement | null>(null);
  const lastActiveRef = useRef<HTMLElement | null>(null);

  const tools = useTools();
  const [state, dispatch] = useReducer(reducer, INITIAL_STATE);

  const liveTools = useMemo<DetectedTool[]>(
    () => tools.data ?? [],
    [tools.data],
  );

  // Seed enabledTools whenever the live list arrives (or refreshes).
  useEffect(() => {
    if (liveTools.length === 0) return;
    dispatch({ type: "seedTools", tools: liveTools });
  }, [liveTools]);

  // ---- step entry side-effects ----

  const runScan = useCallback(async () => {
    dispatch({ type: "scanStarted" });
    try {
      // Defer to the next macrotask so the progress bar mounts before the
      // (potentially) blocking Tauri call is dispatched. Without this the
      // animation never gets a chance to paint a 0% frame.
      await new Promise<void>((resolve) => window.setTimeout(resolve, 0));
      const report = await startFullScan();
      dispatch({ type: "scanCompleted", report });
    } catch (err) {
      const message = err instanceof Error ? err.message : "scan failed";
      dispatch({ type: "scanFailed", message });
    }
  }, []);

  // Fire scan once when the user enters the scan step, and never again
  // unless they explicitly retry.
  const scanFiredRef = useRef(false);
  useEffect(() => {
    if (state.step !== "scan") {
      scanFiredRef.current = false;
      return;
    }
    if (scanFiredRef.current) return;
    scanFiredRef.current = true;
    void runScan();
  }, [state.step, runScan]);

  // ---- close + focus management ----

  const finish = useCallback(() => {
    markOnboardingCompleted();
    toggleOnboarding(false);
  }, [toggleOnboarding]);

  const handleEsc = useCallback(() => {
    // Ask before throwing away an in-flight scan; otherwise close.
    if (state.step === "scan" && state.scanReport === null && state.scanError === null) {
      const ok = window.confirm("Close onboarding while a scan is running?");
      if (!ok) return;
    }
    finish();
  }, [state.step, state.scanReport, state.scanError, finish]);

  // Capture/restore focus across open transitions.
  useEffect(() => {
    if (!open) return;
    lastActiveRef.current = document.activeElement as HTMLElement | null;
    // Defer until the panel is rendered.
    const id = window.setTimeout(() => {
      const focusables = focusableInside(panelRef.current);
      focusables[0]?.focus();
    }, 0);
    return () => {
      window.clearTimeout(id);
      lastActiveRef.current?.focus?.();
    };
  }, [open]);

  // The global keyboard hook (`useGlobalKeyboard`) already handles plain
  // Esc by calling `toggleOnboarding(false)` directly. We add the
  // mid-scan confirmation guard at the panel level because the global
  // hook is intentionally cheap and unaware of step state.
  useEffect(() => {
    if (!open) return;
    function onKey(event: globalThis.KeyboardEvent): void {
      if (event.key !== "Escape") return;
      if (
        state.step === "scan" &&
        state.scanReport === null &&
        state.scanError === null
      ) {
        // Block the global Esc handler so the modal stays open until the
        // user confirms.
        event.preventDefault();
        event.stopPropagation();
        handleEsc();
      }
    }
    document.addEventListener("keydown", onKey, { capture: true });
    return () => document.removeEventListener("keydown", onKey, { capture: true });
  }, [open, state.step, state.scanReport, state.scanError, handleEsc]);

  // ---- bound action callbacks ----

  const actions = useMemo<OnboardingActions>(
    () => ({
      goNext: () => dispatch({ type: "next" }),
      goBack: () => dispatch({ type: "back" }),
      skip: () => {
        dispatch({ type: "skip" });
        markOnboardingCompleted();
        toggleOnboarding(false);
      },
      finish,
      toggleToolEnabled: (id, enabled) =>
        dispatch({ type: "toggleTool", id, enabled }),
      retryScan: () => void runScan(),
    }),
    [finish, runScan, toggleOnboarding],
  );

  // ---- focus trap ----

  function handleKeyDown(event: KeyboardEvent<HTMLDivElement>): void {
    if (event.key !== "Tab") return;
    const focusables = focusableInside(panelRef.current);
    if (focusables.length === 0) return;
    const first = focusables[0];
    const last = focusables[focusables.length - 1];
    if (!first || !last) return;
    const active = document.activeElement as HTMLElement | null;
    if (event.shiftKey && active === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && active === last) {
      event.preventDefault();
      first.focus();
    }
  }

  // Clicking outside the panel intentionally does nothing - per the
  // spec, only Skip / Continue / Done close onboarding. Same for any
  // sidebar click underneath; the backdrop swallows pointer events.
  function handleBackdropClick(): void {
    /* intentionally empty */
  }

  // Focus restoration when the user tabs out of the panel (defensive -
  // the trap above should make this unreachable).
  function onBlur(_event: FocusEvent<HTMLDivElement>): void {
    /* no-op; the focus trap re-enters on Tab */
  }

  const stepProps: OnboardingStepProps = { state, actions, tools: liveTools };
  const currentIndex = STEP_ORDER.indexOf(state.step);
  const totalCount = STEP_ORDER.length;

  let body: ReactNode;
  switch (state.step) {
    case "welcome":
      body = <Welcome actions={actions} />;
      break;
    case "detect":
      if (tools.isError) {
        body = (
          <ErrorPane
            message="Could not detect tools."
            onRetry={() => void tools.refetch()}
            onSkip={actions.skip}
          />
        );
        break;
      }
      if (tools.isPending) {
        body = <LoadingPane label="Detecting tools..." />;
        break;
      }
      body = <Detect {...stepProps} />;
      break;
    case "permission":
      body = <Permission {...stepProps} />;
      break;
    case "scan":
      body = <Scan {...stepProps} />;
      break;
    case "tour":
      body = <Tour actions={actions} />;
      break;
    case "done":
      body = <Done actions={actions} />;
      break;
  }

  return (
    <div
      className={`onboarding-backdrop${open ? " open" : ""}`}
      aria-hidden={!open}
      onClick={handleBackdropClick}
    >
      <div
        ref={panelRef}
        className="onboarding-panel"
        role="dialog"
        aria-modal="true"
        aria-labelledby="onboarding-step-heading"
        onKeyDown={handleKeyDown}
        onBlur={onBlur}
      >
        <div className="onboarding-step-indicator" aria-hidden="true">
          step {currentIndex + 1} of {totalCount}: {STEP_TITLES[state.step]}
        </div>
        {body}
      </div>
    </div>
  );
}

interface ErrorPaneProps {
  message: string;
  onRetry: () => void;
  onSkip: () => void;
}

function ErrorPane({ message, onRetry, onSkip }: ErrorPaneProps) {
  return (
    <>
      <h2 id="onboarding-step-heading">Something went wrong</h2>
      <p className="onboarding-error" role="alert">
        {message}
      </p>
      <div className="inline-actions">
        <button type="button" className="primary-button" onClick={onRetry}>
          retry
        </button>
        <button type="button" className="text-button quiet" onClick={onSkip}>
          skip onboarding
        </button>
      </div>
    </>
  );
}

function LoadingPane({ label }: { label: string }) {
  return (
    <>
      <h2 id="onboarding-step-heading">{label}</h2>
      <div
        className="progress"
        role="progressbar"
        aria-valuemin={0}
        aria-valuemax={100}
        aria-label="loading"
      >
        <i style={{ width: "30%" }} />
      </div>
    </>
  );
}
