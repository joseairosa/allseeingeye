/**
 * Static fixtures mirroring the design prototype until the backend is wired
 * (Phase 2.1). Each row matches the data-* attributes from `design/index.html`.
 */
import type { ComponentType, Scope } from "@aseye/shared-types";

export type HealthState = "up" | "warn" | "error" | "cold" | "unprobed";

export interface ComponentRow {
  id: string;
  name: string;
  kind: ComponentType;
  tool: string;
  scope: Scope | "shared";
  path: string;
  desc: string;
  body: string;
  relations: string;
  smallLabel: string;
  health: HealthState;
  healthLabel: string;
  used: string;
  rowFlag?: "selected" | "issue" | "drift" | "disabled";
}

export const inventoryRows: ComponentRow[] = [
  {
    id: "spec",
    name: "spec",
    kind: "skill",
    tool: "Claude Code",
    scope: "user",
    path: "~/.claude/skills/spec/SKILL.md",
    desc: "/spec - Unified Spec-Driven Development workflow",
    body: "Defines the spec workflow, dispatch steps, reviewer handoffs, and verification gates. References spec-reviewer and spec-verify.",
    relations: "spawns agent: spec-reviewer; references skill: spec-verify",
    smallLabel: "/spec - Unified Spec-Driven Dev",
    health: "up",
    healthLabel: "up 12ms",
    used: "2d ago",
    rowFlag: "selected",
  },
  {
    id: "frontend-design",
    name: "frontend-design",
    kind: "skill",
    tool: "Claude Code",
    scope: "user",
    path: "~/.codex/memories/claude-pilot/.claude/skills/frontend-design/SKILL.md",
    desc: "Production-grade frontend design guidance",
    body: "Avoids generic UI defaults. Defines aesthetic direction, typography, color and layout checks.",
    relations: "references docs/07-visual-design.md",
    smallLabel: "production UI standards",
    health: "up",
    healthLabel: "healthy",
    used: "today",
  },
  {
    id: "sql-dba",
    name: "sql-dba",
    kind: "agent",
    tool: "Codex",
    scope: "user",
    path: "~/.codex/agents/sql-dba.md",
    desc: "Database review agent with read-only SQL access",
    body: "A focused agent for schema inspection, query review, migration risk checks, and index recommendations.",
    relations: "uses mcp: postgresql-erp",
    smallLabel: "schema and query specialist",
    health: "unprobed",
    healthLabel: "unprobed",
    used: "3d ago",
  },
  {
    id: "review-pr",
    name: "/review-pr",
    kind: "command",
    tool: "Claude Code",
    scope: "project",
    path: ".claude/commands/review-pr.md",
    desc: "Code review workflow for pull requests",
    body: "Runs a review pass, checks tests, and reports findings first with file and line references.",
    relations: "activates agent: reviewer; references rule: code-quality",
    smallLabel: "findings-first review",
    health: "up",
    healthLabel: "enabled",
    used: "1d ago",
  },
  {
    id: "github-mcp",
    name: "github",
    kind: "mcp",
    tool: "3 tools",
    scope: "user",
    path: "~/.claude.json / ~/.cursor/mcp.json / ~/.codex/config.toml",
    desc: "GitHub MCP server registered in three host tools",
    body: "stdio transport through npx. Claude Code and Cursor point to the same command; Codex has a different env key name.",
    relations: "registered by Claude Code, Cursor, Codex",
    smallLabel: "used by 3 tools",
    health: "warn",
    healthLabel: "degraded",
    used: "142ms",
    rowFlag: "issue",
  },
  {
    id: "standards-typescript",
    name: "standards-typescript",
    kind: "rule",
    tool: "Cursor",
    scope: "project",
    path: ".cursor/rules/standards-typescript.mdc",
    desc: "Project TypeScript conventions",
    body: "Applies to TypeScript files and preserves strict mode conventions, component boundaries, and test expectations.",
    relations: "equivalentTo rule: claude-code/testing",
    smallLabel: "path-scoped coding rules",
    health: "up",
    healthLabel: "enabled",
    used: "5d ago",
  },
  {
    id: "claude-md",
    name: "CLAUDE.md",
    kind: "memory",
    tool: "Claude Code",
    scope: "project",
    path: "./CLAUDE.md",
    desc: "Project memory shadowing user-level instructions",
    body: "Contains project architecture, local commands, and behavioral expectations. Candidate equivalent with Cursor AGENTS.md.",
    relations: "equivalentTo memory: Cursor AGENTS.md",
    smallLabel: "project memory",
    health: "warn",
    healthLabel: "drift",
    used: "today",
    rowFlag: "drift",
  },
  {
    id: "promo-video",
    name: "promo-video",
    kind: "skill",
    tool: "Antigravity",
    scope: "user",
    path: "~/.gemini/antigravity/skills/promo-video/SKILL.md",
    desc: "Disabled skill for generating product video plans",
    body: "This skill has not been used in 90 days and is disabled by user choice.",
    relations: "none",
    smallLabel: "cold component",
    health: "cold",
    healthLabel: "disabled",
    used: "92d ago",
    rowFlag: "disabled",
  },
];

export interface ToolSummary {
  id: "claude-code" | "codex" | "cursor" | "antigravity";
  displayName: string;
  count: number;
  dotClass: "claude" | "codex" | "cursor" | "anti";
}

export const tools: ToolSummary[] = [
  { id: "claude-code", displayName: "Claude Code", count: 142, dotClass: "claude" },
  { id: "codex", displayName: "Codex", count: 48, dotClass: "codex" },
  { id: "cursor", displayName: "Cursor", count: 21, dotClass: "cursor" },
  { id: "antigravity", displayName: "Antigravity", count: 14, dotClass: "anti" },
];

/**
 * Settings-view fixture for the detected tools list. Falls back here while
 * the `list_tools` IPC command is not yet exposed (Phase 1.6 / 2.1).
 *
 * TODO(phase-1.6): replace with `invoke<DetectedTool[]>('list_tools')`.
 */
export interface DetectedToolFixture {
  id: ToolSummary["id"];
  displayName: string;
  rootPath: string;
  indexed: boolean;
  dotClass: ToolSummary["dotClass"];
}

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

export interface TypeSummary {
  id: ComponentType;
  displayName: string;
  count: number;
  iconId: string;
  hasIssue?: boolean;
}

export const componentTypes: TypeSummary[] = [
  { id: "skill", displayName: "Skills", count: 61, iconId: "icon-skill" },
  { id: "agent", displayName: "Agents", count: 34, iconId: "icon-agent" },
  { id: "command", displayName: "Commands", count: 47, iconId: "icon-command" },
  { id: "mcp", displayName: "MCP servers", count: 12, iconId: "icon-mcp", hasIssue: true },
  { id: "rule", displayName: "Rules", count: 58, iconId: "icon-rule" },
  { id: "memory", displayName: "Memory", count: 8, iconId: "icon-memory" },
];

export interface HealthSummary {
  id: string;
  label: string;
  count: string;
  ring: HealthState;
}

export const healthSummaries: HealthSummary[] = [
  { id: "drift", label: "Drift", count: "3 pairs", ring: "warn" },
  { id: "mcp", label: "MCP issues", count: "2", ring: "error" },
  { id: "cold", label: "Cold", count: "18", ring: "cold" },
];
