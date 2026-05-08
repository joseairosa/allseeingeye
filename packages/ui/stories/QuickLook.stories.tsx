/**
 * QuickLook story.
 *
 * The live `QuickLook` component reads from the Zustand UI store and the
 * IPC `useComponent(id)` hook. To render in Storybook we (1) prime the
 * shared QueryClient with a `ComponentDetail` for the selected id and
 * (2) flip the UI store into the `quickLookOpen` state.
 */
import type { Meta, StoryObj } from "@storybook/react-vite";
import { QuickLook } from "@/components/QuickLook";
import { useUi } from "@/store/ui";
import type { ComponentDetail, ComponentSummary } from "@aseye/shared-types";
import { Shell } from "./_shell";
import { queryClient } from "../.storybook/preview";

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

function makeDetail(summary: ComponentSummary): ComponentDetail {
  return {
    ...summary,
    parsedJson: null,
    parseErrors: null,
    origin: "userCreated",
    pluginId: null,
  };
}

const FIXTURES: Record<string, ComponentDetail> = {
  spec: makeDetail(
    makeSummary({
      id: "aseye://claude-code/user/skill/spec",
      name: "spec",
      description: "/spec - Unified Spec-Driven Development workflow",
      kind: "skill",
      path: "~/.claude/skills/spec/SKILL.md",
    }),
  ),
  "github-mcp": makeDetail(
    makeSummary({
      id: "aseye://claude-code/user/mcp/github",
      name: "github",
      description: "GitHub MCP server registered in three host tools",
      kind: "mcp",
    }),
  ),
  "promo-video": makeDetail(
    makeSummary({
      id: "aseye://antigravity/user/skill/promo-video",
      name: "promo-video",
      description: "Disabled skill for generating product video plans",
      kind: "skill",
      tool: "antigravity",
    }),
  ),
};

interface Args {
  open: boolean;
  componentId: keyof typeof FIXTURES;
}

const meta: Meta<Args> = {
  title: "Panels/QuickLook",
  args: { open: true, componentId: "spec" },
  argTypes: {
    open: { control: "boolean" },
    componentId: {
      options: Object.keys(FIXTURES),
      control: { type: "select" },
    },
  },
  render: (args) => {
    const detail = FIXTURES[args.componentId];
    if (detail) {
      // Prime the cache so `useComponent(id)` resolves synchronously
      // without trying to invoke the (absent) Tauri host.
      queryClient.setQueryData(["component", detail.id], detail);
    }
    useUi.setState({
      quickLookOpen: args.open,
      selectedComponentId: detail?.id ?? null,
    });
    return (
      <Shell>
        <QuickLook />
      </Shell>
    );
  },
};

export default meta;

type Story = StoryObj<Args>;

export const Open: Story = { args: { open: true, componentId: "spec" } };

export const Closed: Story = {
  args: { open: false, componentId: "spec" },
};

export const McpDegraded: Story = {
  args: { open: true, componentId: "github-mcp" },
};

export const ColdSkill: Story = {
  args: { open: true, componentId: "promo-video" },
};
