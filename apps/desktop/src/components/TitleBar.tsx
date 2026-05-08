import { useUi } from "@/store/ui";
import { DensityIcon, ThemeIcon } from "./icons";

export function TitleBar() {
  const toggleTheme = useUi((s) => s.toggleTheme);
  const toggleDensity = useUi((s) => s.toggleDensity);

  return (
    <header className="titlebar" aria-label="window chrome" data-tauri-drag-region>
      <div className="traffic" aria-hidden="true">
        <span className="traffic-dot close" />
        <span className="traffic-dot minimize" />
        <span className="traffic-dot zoom" />
      </div>
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
