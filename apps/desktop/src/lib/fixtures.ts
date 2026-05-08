/**
 * Static fixtures from the design prototype.
 *
 * @deprecated Phase 2.1 wires Inventory, Sidebar, and Quick Look to the live
 * IPC layer (`@/ipc/index`, `@/ipc/hooks`). The remaining export
 * (`detectedToolsFixture`) is consumed only by `SettingsView` which still
 * needs `set_tool_indexed` IPC support. Once the Settings tools toggle is
 * wired (TODO phase-2.x), this whole file can be deleted.
 */
import type { ToolId } from "@aseye/shared-types";

interface DotClass {
  dotClass: "claude" | "codex" | "cursor" | "anti";
}

/**
 * Settings-view fixture for the detected tools list. Replaced by
 * `useTools()` once `SettingsView` consumes the live IPC.
 *
 * @deprecated - replaced by IPC in Phase 2.1. Kept until SettingsView wires
 *   `useTools()` + `set_tool_indexed` (Phase 2.x).
 */
export interface DetectedToolFixture extends DotClass {
  id: ToolId;
  displayName: string;
  rootPath: string;
  indexed: boolean;
}

/**
 * @deprecated - replaced by IPC in Phase 2.1. Kept until SettingsView wires
 *   `useTools()` + `set_tool_indexed` (Phase 2.x).
 */
export const detectedToolsFixture: DetectedToolFixture[] = [
  {
    id: "claude-code",
    displayName: "Claude Code",
    rootPath: "~/.claude",
    indexed: true,
    dotClass: "claude",
  },
  {
    id: "codex",
    displayName: "Codex",
    rootPath: "~/.codex",
    indexed: true,
    dotClass: "codex",
  },
  {
    id: "cursor",
    displayName: "Cursor",
    rootPath: "~/.cursor",
    indexed: true,
    dotClass: "cursor",
  },
  {
    id: "antigravity",
    displayName: "Antigravity",
    rootPath: "~/.gemini/antigravity",
    indexed: false,
    dotClass: "anti",
  },
];
