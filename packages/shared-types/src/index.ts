// Phase 1.1+ generates types here from Rust via ts-rs / specta.
// Hand-rolled stubs live here only until generation is wired.

export type ToolId =
  | "claude-code"
  | "codex"
  | "cursor"
  | "antigravity";

export type ComponentType =
  | "settings"
  | "memory"
  | "rule"
  | "skill"
  | "command"
  | "agent"
  | "mcp"
  | "hook"
  | "plugin"
  | "marketplace"
  | "session"
  | "task"
  | "outputStyle"
  | "statusline"
  | "permission";

export type Scope = "user" | "project" | "enterprise" | "plugin";

export interface ToolSummary {
  id: ToolId;
  displayName: string;
  detected: boolean;
  componentCount: number;
}
