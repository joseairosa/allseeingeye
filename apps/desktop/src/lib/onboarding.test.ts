import { afterEach, beforeAll, beforeEach, describe, expect, it } from "vitest";
import {
  loadOnboardingCompleted,
  markOnboardingCompleted,
  resetOnboarding,
} from "./onboarding";

const STORAGE_KEY = "aseye.onboarding.completed";

/**
 * Vitest's default `node` environment does not expose `window`. We shim a
 * minimal `window.localStorage` so the production helpers exercise their
 * normal code path without us needing to add jsdom as a dependency.
 */
function installWindowShim(): void {
  const store = new Map<string, string>();
  const localStorage = {
    getItem: (key: string): string | null => store.get(key) ?? null,
    setItem: (key: string, value: string): void => {
      store.set(key, value);
    },
    removeItem: (key: string): void => {
      store.delete(key);
    },
    clear: (): void => {
      store.clear();
    },
  };
  // Cast through unknown so we don't widen the global type.
  (globalThis as unknown as { window: { localStorage: typeof localStorage } }).window =
    { localStorage };
}

describe("onboarding persistence", () => {
  beforeAll(() => {
    if (typeof (globalThis as unknown as { window?: unknown }).window === "undefined") {
      installWindowShim();
    }
  });

  beforeEach(() => {
    window.localStorage.removeItem(STORAGE_KEY);
  });

  afterEach(() => {
    window.localStorage.removeItem(STORAGE_KEY);
  });

  it("returns false when no value has ever been written", () => {
    expect(loadOnboardingCompleted()).toBe(false);
  });

  it("persists completion and reads it back", () => {
    markOnboardingCompleted();
    expect(loadOnboardingCompleted()).toBe(true);
    expect(window.localStorage.getItem(STORAGE_KEY)).toBe("true");
  });

  it("resetOnboarding clears the flag", () => {
    markOnboardingCompleted();
    resetOnboarding();
    expect(loadOnboardingCompleted()).toBe(false);
    expect(window.localStorage.getItem(STORAGE_KEY)).toBeNull();
  });

  it("treats unrelated stored values as not-completed", () => {
    window.localStorage.setItem(STORAGE_KEY, "yes");
    expect(loadOnboardingCompleted()).toBe(false);
  });
});
