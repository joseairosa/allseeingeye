/**
 * ComponentRow story.
 *
 * The live row is an inner component inside `InventoryView` that reads
 * from the Zustand store and the live IPC. Extracting it would require
 * touching InventoryView, so we re-render the same DOM/CSS the
 * inventory grid produces from typed `ComponentSummary` fixtures.
 *
 * Phase 2.1 swap: previously the story imported `inventoryRows` from
 * `@/lib/fixtures` (which is being deprecated). The shapes are now
 * `ComponentSummary` directly so the story matches the live IPC
 * payload.
 */
import type { Meta, StoryObj } from "@storybook/react-vite";
import type { ReactNode } from "react";
import { TypeIcon, type TypeIconId } from "@/components/icons";
import type { ComponentSummary, ToolId } from "@aseye/shared-types";
import { Shell } from "./_shell";

const TYPE_TO_ICON: Record<string, TypeIconId> = {
  skill: "icon-skill",
  agent: "icon-agent",
  command: "icon-command",
  mcp: "icon-mcp",
  rule: "icon-rule",
  memory: "icon-memory",
  hook: "icon-hook",
};

const TOOL_DISPLAY_NAME: Record<ToolId, string> = {
  "claude-code": "Claude Code",
  codex: "Codex",
  cursor: "Cursor",
  antigravity: "Antigravity",
};

interface RowFixture {
  summary: ComponentSummary;
  flag: "" | "selected" | "issue" | "drift" | "disabled";
  used: string;
  health: { className: string; label: string };
}

function makeSummary(overrides: Partial<ComponentSummary>): ComponentSummary {
  return {
    id: overrides.id ?? "stub",
    name: overrides.name ?? "stub",
    displayName: overrides.displayName ?? null,
    description: overrides.description ?? null,
    kind: overrides.kind ?? "skill",
    tool: overrides.tool ?? "claude-code",
    scope: overrides.scope ?? "user",
    format: overrides.format ?? "markdownfrontmatter",
    path: overrides.path ?? "~/.claude/skills/stub/SKILL.md",
    size: overrides.size ?? 0n,
    mtime: overrides.mtime ?? 0n,
    hash: overrides.hash ?? "0",
    hasParseErrors: overrides.hasParseErrors ?? false,
    lastUsedAt: overrides.lastUsedAt ?? null,
    useCount: overrides.useCount ?? 0,
  };
}

const FIXTURES: Record<string, RowFixture> = {
  selectedSkill: {
    summary: makeSummary({
      id: "aseye://claude-code/user/skill/spec",
      name: "spec",
      description: "/spec - Unified Spec-Driven Development workflow",
      kind: "skill",
    }),
    flag: "selected",
    used: "2d ago",
    health: { className: "up", label: "up 12ms" },
  },
  mcpDegraded: {
    summary: makeSummary({
      id: "aseye://claude-code/user/mcp/github",
      name: "github",
      description: "GitHub MCP server registered in three host tools",
      kind: "mcp",
      hasParseErrors: false,
    }),
    flag: "issue",
    used: "142ms",
    health: { className: "warn", label: "degraded" },
  },
  memoryDrift: {
    summary: makeSummary({
      id: "aseye://claude-code/project/memory/CLAUDE.md",
      name: "CLAUDE.md",
      description: "Project memory shadowing user-level instructions",
      kind: "memory",
      scope: "project",
    }),
    flag: "drift",
    used: "today",
    health: { className: "warn", label: "drift" },
  },
  coldDisabled: {
    summary: makeSummary({
      id: "aseye://antigravity/user/skill/promo-video",
      name: "promo-video",
      description: "Disabled skill for generating product video plans",
      kind: "skill",
      tool: "antigravity",
    }),
    flag: "disabled",
    used: "92d ago",
    health: { className: "cold", label: "disabled" },
  },
};

interface RowArgs {
  fixtureId: keyof typeof FIXTURES;
}

function StaticRow({ fixtureId }: RowArgs): ReactNode {
  const fx = FIXTURES[fixtureId];
  if (!fx) return null;
  const { summary, flag, used, health } = fx;
  const iconId = TYPE_TO_ICON[summary.kind] ?? "icon-skill";
  return (
    <div className="inventory-grid" role="table" aria-label="components">
      <div className="grid-head" role="row">
        <span role="columnheader">type</span>
        <span role="columnheader">name</span>
        <span role="columnheader">tool</span>
        <span role="columnheader">scope</span>
        <span role="columnheader">state</span>
        <span role="columnheader">used</span>
      </div>
      <button
        type="button"
        className={`component-row ${flag}`.trim()}
        role="row"
      >
        <span role="cell" className="type-cell">
          <TypeIcon id={iconId} />
          <strong>{summary.kind}</strong>
        </span>
        <span role="cell" className="name-cell">
          <span>{summary.displayName ?? summary.name}</span>
          <small>{summary.description ?? summary.path}</small>
        </span>
        <span role="cell">{TOOL_DISPLAY_NAME[summary.tool]}</span>
        <span role="cell">{summary.scope}</span>
        <span role="cell">
          <span className={`health-pill ${health.className}`}>{health.label}</span>
        </span>
        <span role="cell">{used}</span>
      </button>
    </div>
  );
}

const meta: Meta<RowArgs> = {
  title: "Inventory/ComponentRow",
  args: { fixtureId: "selectedSkill" },
  argTypes: {
    fixtureId: {
      options: Object.keys(FIXTURES),
      control: { type: "select" },
    },
  },
  render: (args) => (
    <Shell>
      <main className="main-area">
        <StaticRow fixtureId={args.fixtureId} />
      </main>
    </Shell>
  ),
};

export default meta;

type Story = StoryObj<RowArgs>;

export const SelectedSkill: Story = { args: { fixtureId: "selectedSkill" } };
export const McpDegraded: Story = { args: { fixtureId: "mcpDegraded" } };
export const MemoryDrift: Story = { args: { fixtureId: "memoryDrift" } };
export const ColdDisabled: Story = { args: { fixtureId: "coldDisabled" } };
