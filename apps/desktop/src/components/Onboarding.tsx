import { useUi } from "@/store/ui";

export function Onboarding() {
  const open = useUi((s) => s.onboardingOpen);
  const toggle = useUi((s) => s.toggleOnboarding);

  return (
    <div
      className={`onboarding-backdrop${open ? " open" : ""}`}
      aria-hidden={!open}
      onClick={(e) => {
        if (e.target === e.currentTarget) toggle(false);
      }}
    >
      <div className="onboarding-panel" role="dialog" aria-modal="true" aria-label="onboarding">
        <img src="/assets/eye-logo.svg" alt="" />
        <h2>Detected 4 tools</h2>
        <div className="detected-grid">
          <span><span className="tool-dot claude" /> Claude Code</span>
          <span><span className="tool-dot codex" /> Codex</span>
          <span><span className="tool-dot cursor" /> Cursor</span>
          <span><span className="tool-dot anti" /> Antigravity</span>
        </div>
        <div className="permission-list">
          <span className="mono">~/.claude</span>
          <span className="mono">~/.codex</span>
          <span className="mono">~/.cursor</span>
          <span className="mono">~/.gemini</span>
        </div>
        <div className="progress"><i style={{ width: "82%" }} /></div>
        <div className="inline-actions">
          <button
            type="button"
            className="primary-button"
            onClick={() => toggle(false)}
          >
            continue
          </button>
          <button
            type="button"
            className="text-button quiet"
            onClick={() => toggle(false)}
          >
            skip
          </button>
        </div>
      </div>
    </div>
  );
}
