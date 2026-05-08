import { useEffect } from "react";
import { useUi } from "@/store/ui";
import { useGlobalKeyboard } from "@/lib/keyboard";
import { TitleBar } from "@/components/TitleBar";
import { Sidebar } from "@/components/Sidebar";
import { MainHeader } from "@/components/MainHeader";
import { Statusbar } from "@/components/Statusbar";
import { QuickLook } from "@/components/QuickLook";
import { CommandPalette } from "@/components/CommandPalette";
import { Onboarding } from "@/components/Onboarding";
import { InventoryView } from "@/views/InventoryView";
import { MapView } from "@/views/MapView";
import { EditorView } from "@/views/EditorView";
import { HealthView } from "@/views/HealthView";
import { inventoryRows } from "@/lib/fixtures";

function useBodyClasses() {
  const theme = useUi((s) => s.theme);
  const density = useUi((s) => s.density);
  useEffect(() => {
    document.body.classList.toggle("light", theme === "light");
  }, [theme]);
  useEffect(() => {
    document.body.classList.toggle("compact", density === "compact");
  }, [density]);
}

export function App() {
  useBodyClasses();
  useGlobalKeyboard();

  return (
    <div className="app-shell" data-density="comfortable">
      <TitleBar />
      <Sidebar />
      <main className="main-area">
        <MainHeader />
        <InventoryView />
        <MapView />
        <EditorView />
        <HealthView />
        <Statusbar resultCount={inventoryRows.length} />
      </main>
      <QuickLook />
      <CommandPalette />
      <Onboarding />
    </div>
  );
}
