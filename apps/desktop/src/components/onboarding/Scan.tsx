import { useEffect, useRef, useState } from "react";
import type { OnboardingStepProps } from "./types";

/**
 * Cap the simulated progress somewhere short of completion so the user
 * sees forward motion without us lying about being done. The remaining
 * delta jumps to 100% when the real `ScanReport` arrives.
 */
const PROGRESS_CAP = 85;
const PROGRESS_TICK_MS = 220;
const PROGRESS_INCREMENT = 4;

function prefersReducedMotion(): boolean {
  if (typeof window === "undefined" || !window.matchMedia) return false;
  return window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

/**
 * Step 4. The orchestrator triggers the scan on entry and pushes the
 * resulting `ScanReport` down via state. This component owns only the
 * UX bits: a progress bar that animates from 0 -> 85% while in flight,
 * snapping to 100% on completion.
 */
export function Scan({ state, actions }: OnboardingStepProps) {
  const inFlight = state.scanReport === null && state.scanError === null;
  const [progress, setProgress] = useState(0);
  const reduced = useRef(prefersReducedMotion());

  useEffect(() => {
    if (state.scanReport !== null) {
      setProgress(100);
      return;
    }
    if (state.scanError !== null) return;
    if (reduced.current) {
      // Single static jump - the label below carries the in-flight signal.
      setProgress(40);
      return;
    }

    const id = window.setInterval(() => {
      setProgress((prev) => Math.min(PROGRESS_CAP, prev + PROGRESS_INCREMENT));
    }, PROGRESS_TICK_MS);
    return () => window.clearInterval(id);
  }, [state.scanReport, state.scanError]);

  return (
    <>
      <h2 id="onboarding-step-heading">
        {state.scanReport !== null ? "Scan complete" : "Indexing"}
      </h2>
      <p className="onboarding-lead">
        {state.scanReport !== null
          ? "Your tools are ready to browse."
          : "Walking your tool roots and parsing every component."}
      </p>

      <div
        className="progress"
        role="progressbar"
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={progress}
        aria-label="scan progress"
      >
        <i style={{ width: `${progress}%` }} />
      </div>

      {inFlight && reduced.current ? (
        <p className="onboarding-progress-label" aria-live="polite">
          scanning...
        </p>
      ) : null}

      {state.scanReport !== null ? (
        <dl className="onboarding-scan-report">
          <div>
            <dt>tools scanned</dt>
            <dd>{state.scanReport.toolsScanned}</dd>
          </div>
          <div>
            <dt>components</dt>
            <dd>{state.scanReport.componentsSeen}</dd>
          </div>
          <div>
            <dt>parse errors</dt>
            <dd>{state.scanReport.parseErrors}</dd>
          </div>
        </dl>
      ) : null}

      {state.scanError !== null ? (
        <p className="onboarding-error" role="alert">
          {state.scanError}
        </p>
      ) : null}

      <div className="inline-actions">
        {state.scanError !== null ? (
          <>
            <button
              type="button"
              className="primary-button"
              onClick={actions.retryScan}
            >
              retry
            </button>
            <button
              type="button"
              className="text-button quiet"
              onClick={actions.goNext}
            >
              skip scan
            </button>
          </>
        ) : (
          <button
            type="button"
            className="primary-button"
            onClick={actions.goNext}
            disabled={state.scanReport === null}
          >
            continue
          </button>
        )}
      </div>
    </>
  );
}
