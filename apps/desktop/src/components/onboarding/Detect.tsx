import type { ToolId } from "@aseye/shared-types";
import type { OnboardingStepProps } from "./types";

const TOOL_DOT: Record<ToolId, "claude" | "codex" | "cursor" | "anti"> = {
  "claude-code": "claude",
  codex: "codex",
  cursor: "cursor",
  antigravity: "anti",
};

/**
 * Per-tool install hints surfaced in the "tools not found" subsection.
 * Static copy is fine here - the URLs are public marketing pages, not
 * config the user can override.
 */
const INSTALL_HINTS: Record<ToolId, string> = {
  "claude-code": "https://docs.anthropic.com/en/docs/agents/claude-code",
  codex: "https://github.com/openai/codex",
  cursor: "https://cursor.sh",
  antigravity: "https://gemini.google.com/app",
};

/**
 * Step 2. Lists the result of `list_tools`. Each detected tool becomes a
 * row with an "indexed" toggle. Undetected tools collapse into a quieter
 * subsection with "Install" affordances.
 */
export function Detect({ state, actions, tools }: OnboardingStepProps) {
  const detected = tools.filter((t) => t.detected);
  const missing = tools.filter((t) => !t.detected);

  return (
    <>
      <h2 id="onboarding-step-heading">
        Detected {detected.length} tool{detected.length === 1 ? "" : "s"}
      </h2>
      <p className="onboarding-lead">
        Pick which ones to index. You can change this later in Settings.
      </p>

      <div className="onboarding-tool-list" role="list">
        {detected.map((tool) => {
          const enabled = state.enabledTools[tool.id] ?? false;
          const path = tool.existingRootPaths[0] ?? "(no path)";
          return (
            <label
              key={tool.id}
              className="onboarding-tool-row"
              role="listitem"
            >
              <span className={`tool-dot ${TOOL_DOT[tool.id]}`} />
              <span className="onboarding-tool-name">{tool.displayName}</span>
              <span className="mono onboarding-tool-path">{path}</span>
              <input
                type="checkbox"
                checked={enabled}
                onChange={(e) =>
                  actions.toggleToolEnabled(tool.id, e.target.checked)
                }
                aria-label={`index ${tool.displayName}`}
              />
            </label>
          );
        })}
      </div>

      {missing.length > 0 ? (
        <div className="onboarding-missing" aria-labelledby="missing-label">
          <div className="side-label" id="missing-label">
            tools not found
          </div>
          <div className="onboarding-tool-list">
            {missing.map((tool) => (
              <div key={tool.id} className="onboarding-tool-row quiet">
                <span className={`tool-dot ${TOOL_DOT[tool.id]}`} />
                <span className="onboarding-tool-name">{tool.displayName}</span>
                <a
                  className="text-button quiet"
                  href={INSTALL_HINTS[tool.id]}
                  target="_blank"
                  rel="noreferrer noopener"
                >
                  install {tool.displayName}
                </a>
              </div>
            ))}
          </div>
        </div>
      ) : null}

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
          onClick={actions.goBack}
        >
          back
        </button>
      </div>
    </>
  );
}
