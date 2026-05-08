import type { OnboardingActions } from "./types";

interface TourProps {
  actions: OnboardingActions;
}

interface Coachmark {
  title: string;
  body: string;
  hint: string;
}

const MARKS: readonly Coachmark[] = [
  {
    title: "Inventory",
    body: "Every component across your tools. Filter with chips or type/tool/scope prefixes.",
    hint: "Cmd-1",
  },
  {
    title: "Map",
    body: "Relationships and overlap between rules, skills, and MCP servers.",
    hint: "Cmd-2",
  },
  {
    title: "Command palette",
    body: "Jump to anything by name or run an action.",
    hint: "Cmd-K",
  },
];

/**
 * Step 5. Three flat coachmarks. v1 deliberately avoids overlay arrows on
 * the live UI - they're hard to position robustly and the explainer text
 * is enough at this stage.
 */
export function Tour({ actions }: TourProps) {
  return (
    <>
      <h2 id="onboarding-step-heading">A quick tour</h2>
      <p className="onboarding-lead">Three corners of the app you'll use most.</p>

      <ol className="onboarding-tour-list">
        {MARKS.map((m) => (
          <li key={m.title} className="onboarding-tour-item">
            <div className="onboarding-tour-title">{m.title}</div>
            <p>{m.body}</p>
            <kbd>{m.hint}</kbd>
          </li>
        ))}
      </ol>

      <div className="inline-actions">
        <button
          type="button"
          className="primary-button"
          onClick={actions.goNext}
        >
          continue
        </button>
        <button
          type="button"
          className="text-button quiet"
          onClick={actions.skip}
        >
          skip
        </button>
      </div>
    </>
  );
}
