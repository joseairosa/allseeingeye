import { useMemo } from "react";
import type { OnboardingStepProps } from "./types";

/**
 * macOS Full Disk Access pane. The user lands here from System Settings
 * after we open the deep link. Linux/Windows do not need this.
 */
const MAC_FDA_DEEP_LINK =
  "x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles";

/**
 * Placeholder reachability check. The Tauri shell command that probes
 * `fs::metadata` per path lands in a later phase; for now we assume the
 * user can read everything inside their own home directory.
 *
 * TODO(phase-x): replace with a Rust-side `is_path_readable` IPC.
 */
function isPathReadable(_path: string): boolean {
  return true;
}

function isMac(): boolean {
  if (typeof navigator === "undefined") return false;
  return navigator.platform.toLowerCase().includes("mac");
}

/**
 * Step 3. Lists every directory we'll read, grouped by enabled tool. The
 * "Grant access" button opens System Settings on macOS so the user can
 * flip Full Disk Access on All Seeing Eye.
 */
export function Permission({ state, actions, tools }: OnboardingStepProps) {
  const paths = useMemo(() => {
    const acc = new Set<string>();
    for (const tool of tools) {
      if (!state.enabledTools[tool.id]) continue;
      for (const p of tool.existingRootPaths) acc.add(p);
    }
    return [...acc].sort();
  }, [tools, state.enabledTools]);

  const allReadable = paths.every(isPathReadable);
  const onMac = isMac();

  function handleGrant(): void {
    if (!onMac) return;
    // Tauri's WebView allows `window.open` of the deep link; Settings
    // intercepts the URL scheme and opens the right pane.
    if (typeof window !== "undefined") {
      window.open(MAC_FDA_DEEP_LINK, "_blank");
    }
  }

  return (
    <>
      <h2 id="onboarding-step-heading">Allow read access</h2>
      <p className="onboarding-lead">
        All Seeing Eye reads files in these locations. Nothing leaves your
        machine.
      </p>

      <div className="permission-list" role="list">
        {paths.length === 0 ? (
          <span className="mono">(no paths - enable a tool to continue)</span>
        ) : (
          paths.map((p) => (
            <span key={p} className="mono" role="listitem">
              {p}
            </span>
          ))
        )}
      </div>

      {!allReadable ? (
        <p className="onboarding-error" role="alert">
          Without access, only metadata-detected tools can be indexed.
        </p>
      ) : null}

      <div className="inline-actions">
        {onMac && !allReadable ? (
          <button
            type="button"
            className="primary-button"
            onClick={handleGrant}
          >
            grant access
          </button>
        ) : (
          <button
            type="button"
            className="primary-button"
            onClick={actions.goNext}
            disabled={paths.length === 0}
          >
            continue
          </button>
        )}
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
