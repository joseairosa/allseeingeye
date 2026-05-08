import type { Meta, StoryObj } from "@storybook/react-vite";
import { Sidebar } from "@/components/Sidebar";
import { Shell } from "./_shell";

const meta: Meta<typeof Sidebar> = {
  title: "Chrome/Sidebar",
  component: Sidebar,
  decorators: [
    (Story) => (
      <Shell>
        <Story />
      </Shell>
    ),
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
