import type { OnboardingActions } from "./types";

interface WelcomeProps {
  actions: OnboardingActions;
}

/**
 * Step 1. Hero + one-paragraph pitch + primary action. The "Skip tour"
 * link short-circuits the whole flow and marks onboarding completed.
 */
export function Welcome({ actions }: WelcomeProps) {
  return (
    <>
      <img src="/assets/eye-logo.svg" alt="" />
      <h2 id="onboarding-step-heading">All Seeing Eye</h2>
      <p className="onboarding-lead">
        A local-first inventory of every skill, agent, command, MCP server,
        and rule across your AI tools. We index in place, never copy out, and
        keep secrets masked by default.
      </p>
      <div className="inline-actions">
        <button
          type="button"
          className="primary-button"
          onClick={actions.goNext}
          autoFocus
        >
          start
        </button>
        <button
          type="button"
          className="text-button quiet"
          onClick={actions.skip}
        >
          skip tour
        </button>
      </div>
    </>
  );
}
