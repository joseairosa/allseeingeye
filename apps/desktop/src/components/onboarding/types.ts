import type { DetectedTool, ScanReport, ToolId } from "@aseye/shared-types";

/**
 * Linear progression of the first-launch flow. Steps may be skipped
 * forward (Skip tour) but never re-entered after Done.
 */
export type OnboardingStep =
  | "welcome"
  | "detect"
  | "permission"
  | "scan"
  | "tour"
  | "done";

/**
 * Per-tool toggles the user controls in the Detect step. Defaults to ON
 * for tools where `detected: true`, OFF otherwise.
 */
export type EnabledTools = Record<ToolId, boolean>;

/**
 * Aggregate state owned by the orchestrator and threaded into each sub-
 * step via props. Sub-steps never call hooks against the live IPC
 * directly; they receive ready-resolved data + dispatch callbacks.
 */
export interface OnboardingState {
  step: OnboardingStep;
  enabledTools: EnabledTools;
  scanReport: ScanReport | null;
  scanProgress: number;
  scanError: string | null;
}

export interface OnboardingActions {
  goNext: () => void;
  goBack: () => void;
  skip: () => void;
  finish: () => void;
  toggleToolEnabled: (id: ToolId, enabled: boolean) => void;
  retryScan: () => void;
}

export interface OnboardingStepProps {
  state: OnboardingState;
  actions: OnboardingActions;
  tools: DetectedTool[];
}
