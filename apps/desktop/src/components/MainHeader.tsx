import { useState } from "react";
import { useUi } from "@/store/ui";
import { startFullScan } from "@/ipc";
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
  const [scanning, setScanning] = useState(false);

  async function handleRefresh(): Promise<void> {
    if (scanning) return;
    setScanning(true);
    try {
      await startFullScan();
    } catch (err) {
      console.error("[main-header] refresh failed", err);
    } finally {
      setScanning(false);
    }
  }

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
          aria-label={scanning ? "refreshing index" : "refresh index"}
          aria-busy={scanning}
          title={scanning ? "Refreshing..." : "Refresh index"}
          onClick={() => {
            void handleRefresh();
          }}
          disabled={scanning}
        >
          <RefreshIcon />
        </button>
      </div>
    </div>
  );
}
