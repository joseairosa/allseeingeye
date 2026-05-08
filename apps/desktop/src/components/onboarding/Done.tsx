import { useUi } from "@/store/ui";
import type { OnboardingActions } from "./types";

interface DoneProps {
  actions: OnboardingActions;
}

/**
 * Final step. Closes onboarding and routes the user into Inventory.
 */
export function Done({ actions }: DoneProps) {
  const setView = useUi((s) => s.setView);

  function openInventory(): void {
    setView("inventory");
    actions.finish();
  }

  function openSettings(): void {
    setView("settings");
    actions.finish();
  }

  return (
    <>
      <img src="/assets/eye-logo.svg" alt="" />
      <h2 id="onboarding-step-heading">You&apos;re in</h2>
      <p className="onboarding-lead">
        Everything is indexed locally. Open the Inventory to start browsing.
      </p>
      <div className="inline-actions">
        <button
          type="button"
          className="primary-button"
          onClick={openInventory}
          autoFocus
        >
          open inventory
        </button>
        <button
          type="button"
          className="text-button quiet"
          onClick={openSettings}
        >
          settings
        </button>
      </div>
    </>
  );
}
