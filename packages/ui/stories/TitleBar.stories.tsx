import type { Meta, StoryObj } from "@storybook/react-vite";
import { TitleBar } from "@/components/TitleBar";
import { Shell } from "./_shell";

const meta: Meta<typeof TitleBar> = {
  title: "Chrome/TitleBar",
  component: TitleBar,
  decorators: [
    (Story) => (
      <Shell>
        <Story />
      </Shell>
    ),
  ],
};

export default meta;

type Story = StoryObj<typeof TitleBar>;

export const Default: Story = {};

export const Light: Story = {
  globals: { theme: "light" },
};
