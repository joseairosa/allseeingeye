import { useMemo } from "react";
import { useQuery } from "@tanstack/react-query";
import { checkPathReadable } from "@/ipc";
import type { OnboardingStepProps } from "./types";

/**
 * macOS Full Disk Access pane. The user lands here from System Settings
 * after we open the deep link. Linux/Windows do not need this.
 */
const MAC_FDA_DEEP_LINK =
  "x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles";

/** Long stale time - readability rarely changes within an onboarding session. */
const STALE_PERMISSION_MS = 60_000;

function isMac(): boolean {
  if (typeof navigator === "undefined") return false;
  return navigator.platform.toLowerCase().includes("mac");
}

/**
 * Probe each candidate path through the `check_path_readable` Tauri
 * command. Batched into a single `useQuery` (one IPC roundtrip per
 * path, but a single React re-render) so onboarding doesn't fan out
 * into N tiny queries.
 */
async function probePaths(paths: string[]): Promise<Record<string, boolean>> {
  const entries = await Promise.all(
    paths.map(async (path) => [path, await checkPathReadable(path)] as const),
  );
  return Object.fromEntries(entries);
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

  // Phase audit / issue #17 - the previous placeholder always returned
  // `true`, which made the macOS Full Disk Access deep link
  // unreachable. Probe each path through the Rust-side IPC so the
  // "grant access" button surfaces when at least one path is not
  // readable. Default to `true` while the probe is in flight so the
  // continue button is not disabled on a fast first paint.
  const probe = useQuery({
    queryKey: ["onboarding", "permission", paths] as const,
    queryFn: () => probePaths(paths),
    enabled: paths.length > 0,
    staleTime: STALE_PERMISSION_MS,
  });

  const allReadable = useMemo(() => {
    if (paths.length === 0) return true;
    if (!probe.data) return true;
    return paths.every((p) => probe.data[p] !== false);
  }, [paths, probe.data]);
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
