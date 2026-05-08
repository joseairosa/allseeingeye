/**
 * CommandPalette story.
 *
 * Phase 2.4: the palette consumes `useSearch()` (FTS) and `useComponents()`
 * (recent list) hooks. We seed the QueryClient with believable fixtures so
 * the open palette shows real-shape data without a live Tauri host.
 */
import type { Meta, StoryObj } from "@storybook/react-vite";
import { CommandPalette } from "@/components/CommandPalette";
import { useUi } from "@/store/ui";
import type {
  ComponentSummary,
  SearchQuery,
  SearchResult,
} from "@aseye/shared-types";
import { Shell } from "./_shell";
import { queryClient } from "../.storybook/preview";

interface Args {
  open: boolean;
  preset: "empty" | "spec";
}

const RECENT_COMPONENTS: ComponentSummary[] = [
  {
    id: "aseye://claude-code/skill/spec",
    name: "spec",
    displayName: null,
    description: "Unified spec-driven development",
    kind: "skill",
    tool: "claude-code",
    scope: "user",
    format: "markdown",
    path: "~/.claude/skills/spec/SKILL.md",
    size: 8192n,
    mtime: 1_730_000_000n,
    hash: "abc",
    hasParseErrors: false,
    lastUsedAt: 1_730_500_000n,
    useCount: 42,
  },
  {
    id: "aseye://claude-code/command/review-pr",
    name: "review-pr",
    displayName: "/review-pr",
    description: "CTO-level code review of an open PR",
    kind: "command",
    tool: "claude-code",
    scope: "user",
    format: "markdown",
    path: "~/.claude/commands/review-pr.md",
    size: 4096n,
    mtime: 1_729_900_000n,
    hash: "def",
    hasParseErrors: false,
    lastUsedAt: 1_730_400_000n,
    useCount: 14,
  },
  {
    id: "aseye://codex/agent/rescue",
    name: "rescue",
    displayName: null,
    description: "Delegate to Codex rescue subagent",
    kind: "agent",
    tool: "codex",
    scope: "user",
    format: "markdown",
    path: "~/.codex/agents/rescue.md",
    size: 2048n,
    mtime: 1_729_800_000n,
    hash: "ghi",
    hasParseErrors: false,
    lastUsedAt: 1_730_300_000n,
    useCount: 7,
  },
];

const SPEC_RESULTS: SearchResult[] = [
  {
    snippet: "Unified <mark>spec</mark>-driven development",
    id: "aseye://claude-code/skill/spec",
    name: "spec",
    displayName: null,
    description: "Unified spec-driven development",
    kind: "skill",
    tool: "claude-code",
    scope: "user",
    format: "markdown",
    path: "~/.claude/skills/spec/SKILL.md",
    size: 8192n,
    mtime: 1_730_000_000n,
    hash: "abc",
    hasParseErrors: false,
    lastUsedAt: 1_730_500_000n,
    useCount: 42,
  },
  {
    snippet: "<mark>spec</mark>-verify pass",
    id: "aseye://claude-code/skill/spec-verify",
    name: "spec-verify",
    displayName: null,
    description: "Spec verification phase",
    kind: "skill",
    tool: "claude-code",
    scope: "user",
    format: "markdown",
    path: "~/.claude/skills/spec-verify/SKILL.md",
    size: 5120n,
    mtime: 1_729_950_000n,
    hash: "jkl",
    hasParseErrors: false,
    lastUsedAt: 1_730_200_000n,
    useCount: 9,
  },
  {
    snippet: "/<mark>spec</mark> command",
    id: "aseye://claude-code/command/spec",
    name: "spec",
    displayName: "/spec",
    description: "Unified spec dispatcher",
    kind: "command",
    tool: "claude-code",
    scope: "user",
    format: "markdown",
    path: "~/.claude/commands/spec.md",
    size: 1024n,
    mtime: 1_729_990_000n,
    hash: "mno",
    hasParseErrors: false,
    lastUsedAt: null,
    useCount: 0,
  },
];

/**
 * The empty filter `useComponents()` sends. Mirrors the call site in
 * `CommandPalette` so the seeded cache key matches.
 */
const RECENT_FILTER = {
  toolId: null,
  kind: null,
  scope: null,
  query: null,
  tag: null,
  limit: 20,
  offset: null,
};

const SEARCH_QUERY: SearchQuery = {
  text: "spec",
  limit: 8,
  toolId: null,
  kind: null,
  scope: null,
};

function seed(preset: Args["preset"]): void {
  queryClient.setQueryData(["components", RECENT_FILTER], RECENT_COMPONENTS);
  if (preset === "spec") {
    queryClient.setQueryData(["search", SEARCH_QUERY], SPEC_RESULTS);
  }
}

const meta: Meta<Args> = {
  title: "Panels/CommandPalette",
  args: { open: true, preset: "empty" },
  argTypes: {
    open: { control: "boolean" },
    preset: { control: { type: "radio" }, options: ["empty", "spec"] },
  },
  render: (args) => {
    seed(args.preset);
    useUi.setState({ paletteOpen: args.open });
    const defaultQuery = args.preset === "spec" ? "spec" : "";
    return (
      <Shell>
        <CommandPalette defaultQuery={defaultQuery} />
      </Shell>
    );
  },
};

export default meta;

type Story = StoryObj<Args>;

export const Closed: Story = { args: { open: false, preset: "empty" } };

export const OpenEmpty: Story = { args: { open: true, preset: "empty" } };

export const OpenWithQuery: Story = { args: { open: true, preset: "spec" } };

export const OpenLight: Story = {
  args: { open: true, preset: "spec" },
  globals: { theme: "light" },
};
