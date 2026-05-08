/**
 * Sidebar story.
 *
 * Sidebar reads `useTools()` and `useHealthSummary()` since Phase 2.1, so
 * the story seeds the QueryClient with believable fixtures rather than
 * letting the hooks call the (absent) Tauri host.
 */
import type { Meta, StoryObj } from "@storybook/react-vite";
import { Sidebar } from "@/components/Sidebar";
import type { DetectedTool, HealthSummary } from "@aseye/shared-types";
import { Shell } from "./_shell";
import { queryClient } from "../.storybook/preview";

const TOOLS_FIXTURE: DetectedTool[] = [
  {
    id: "claude-code",
    displayName: "Claude Code",
    detected: true,
    binary: "/usr/local/bin/claude",
    version: "1.0.0",
    existingRootPaths: ["~/.claude"],
  },
  {
    id: "codex",
    displayName: "Codex",
    detected: true,
    binary: "/usr/local/bin/codex",
    version: "0.14.0",
    existingRootPaths: ["~/.codex"],
  },
  {
    id: "cursor",
    displayName: "Cursor",
    detected: true,
    binary: null,
    version: null,
    existingRootPaths: ["~/.cursor"],
  },
  {
    id: "antigravity",
    displayName: "Antigravity",
    detected: false,
    binary: null,
    version: null,
    existingRootPaths: [],
  },
];

const HEALTH_FIXTURE: HealthSummary = {
  totalComponents: 237,
  totalParseErrors: 2,
  byToolKind: [
    { tool: "claude-code", kind: "skill", count: 61 },
    { tool: "claude-code", kind: "agent", count: 34 },
    { tool: "claude-code", kind: "command", count: 47 },
    { tool: "claude-code", kind: "mcp", count: 12 },
    { tool: "claude-code", kind: "rule", count: 58 },
    { tool: "claude-code", kind: "memory", count: 8 },
    { tool: "claude-code", kind: "hook", count: 9 },
    { tool: "codex", kind: "agent", count: 21 },
    { tool: "codex", kind: "settings", count: 27 },
    { tool: "cursor", kind: "rule", count: 21 },
  ],
};

function seed(): void {
  queryClient.setQueryData(["tools"], TOOLS_FIXTURE);
  queryClient.setQueryData(["health"], HEALTH_FIXTURE);
}

const meta: Meta<typeof Sidebar> = {
  title: "Chrome/Sidebar",
  component: Sidebar,
  decorators: [
    (Story) => {
      seed();
      return (
        <Shell>
          <Story />
        </Shell>
      );
    },
  ],
};

export default meta;

type Story = StoryObj<typeof Sidebar>;

export const Default: Story = {};

export const Light: Story = {
  globals: { theme: "light" },
};

export const Compact: Story = {
  globals: { density: "compact" },
};
