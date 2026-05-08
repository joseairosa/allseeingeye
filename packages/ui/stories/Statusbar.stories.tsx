import type { Meta, StoryObj } from "@storybook/react-vite";
import { Statusbar } from "@/components/Statusbar";

const meta: Meta<typeof Statusbar> = {
  title: "Chrome/Statusbar",
  component: Statusbar,
  args: { resultCount: 237 },
  argTypes: {
    resultCount: { control: { type: "number", min: 0, max: 50000, step: 1 } },
  },
};

export default meta;

type Story = StoryObj<typeof Statusbar>;

export const Default: Story = {};

export const Empty: Story = {
  args: { resultCount: 0 },
};

export const Large: Story = {
  args: { resultCount: 12483 },
};
