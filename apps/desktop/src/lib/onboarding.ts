/**
 * Onboarding completion persistence (Phase 4.1).
 *
 * The first-launch flow lives in `components/Onboarding.tsx`. We persist a
 * single boolean in `localStorage` so subsequent launches skip the modal.
 * Settings (or a future dev menu) can call `resetOnboarding()` to replay
 * the tour without uninstalling.
 *
 * `localStorage` is intentional: the value is per-browser-profile and never
 * needs to leave the machine. Tauri exposes the same `window.localStorage`
 * the WebView uses, backed by the OS keychain-adjacent app data dir.
 *
 * All accessors guard against missing `window`/`localStorage` (Storybook
 * SSR setups and node-based vitest by default get a JSDOM window, but a
 * thrown SecurityError from a denied storage policy is still possible).
 */
const STORAGE_KEY = "aseye.onboarding.completed";

/**
 * `true` if the user completed (or skipped) onboarding on this profile.
 * Returns `false` on the first launch, when storage is unavailable, or
 * when the stored value is anything other than the literal "true".
 */
export function loadOnboardingCompleted(): boolean {
  try {
    if (typeof window === "undefined") return false;
    const value = window.localStorage.getItem(STORAGE_KEY);
    return value === "true";
  } catch {
    // localStorage can throw in private browsing modes / strict policies.
    // Treat as "not completed" so we degrade to showing the welcome screen
    // rather than locking the user out of onboarding altogether.
    return false;
  }
}

/** Persist the "user has completed onboarding" flag. */
export function markOnboardingCompleted(): void {
  try {
    if (typeof window === "undefined") return;
    window.localStorage.setItem(STORAGE_KEY, "true");
  } catch {
    // No-op. Worst case: the modal reappears next launch.
  }
}

/**
 * Clear the persisted flag so the next launch (or remount, if combined
 * with `useUi.toggleOnboarding(true)`) shows the flow again. Intended
 * for the Settings "replay onboarding" affordance and tests.
 */
export function resetOnboarding(): void {
  try {
    if (typeof window === "undefined") return;
    window.localStorage.removeItem(STORAGE_KEY);
  } catch {
    // No-op (see notes above).
  }
}
