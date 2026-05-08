import { useUi } from "@/store/ui";
import { CommandSearchIcon, RefreshIcon } from "./icons";

const TITLES: Record<string, string> = {
  inventory: "Inventory",
  map: "Map",
  editor: "Editor",
  health: "Health",
};

export function MainHeader() {
  const view = useUi((s) => s.view);
  const togglePalette = useUi((s) => s.togglePalette);

  return (
    <div className="main-header">
      <div>
        <div className="breadcrumb">local / user roots / live index</div>
        <h1>{TITLES[view] ?? "Inventory"}</h1>
      </div>
      <div className="header-actions">
        <button
          type="button"
          className="command-button"
          onClick={() => togglePalette(true)}
          aria-label="open command palette"
        >
          <CommandSearchIcon />
          <span>Search or command</span>
          <kbd>⌘K</kbd>
        </button>
        <button
          type="button"
          className="icon-button"
          aria-label="refresh index"
          title="Refresh index"
        >
          <RefreshIcon />
        </button>
      </div>
    </div>
  );
}
