import { useUi } from "@/store/ui";
import { DensityIcon, ThemeIcon } from "./icons";

export function TitleBar() {
  const toggleTheme = useUi((s) => s.toggleTheme);
  const toggleDensity = useUi((s) => s.toggleDensity);

  return (
    <header className="titlebar" aria-label="window chrome" data-tauri-drag-region>
      {/* macOS draws the native traffic lights via `titleBarStyle: "Transparent"`.
          We reserve space for them via the `.titlebar` left-padding so this
          section is purely the centred logo + name. On Linux / Windows the OS
          chrome lives outside the WebView, so the same left-padding is
          harmless padding. */}
      <div className="titlebar-center">
        <img src="/assets/eye-mark.svg" alt="" className="titlebar-logo" />
        <span>all seeing eye</span>
      </div>
      <div className="titlebar-actions">
        <button
          className="icon-button"
          type="button"
          onClick={toggleDensity}
          aria-label="toggle density"
          title="Density"
        >
          <DensityIcon />
        </button>
        <button
          className="icon-button"
          type="button"
          onClick={toggleTheme}
          aria-label="toggle theme"
          title="Theme"
        >
          <ThemeIcon />
        </button>
      </div>
    </header>
  );
}
