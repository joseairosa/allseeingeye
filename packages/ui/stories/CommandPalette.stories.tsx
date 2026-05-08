import type { Meta, StoryObj } from "@storybook/react-vite";
import { CommandPalette } from "@/components/CommandPalette";
import { useUi } from "@/store/ui";
import { Shell } from "./_shell";

interface Args {
  open: boolean;
}

const meta: Meta<Args> = {
  title: "Panels/CommandPalette",
  args: { open: true },
  argTypes: { open: { control: "boolean" } },
  render: (args) => {
    useUi.setState({ paletteOpen: args.open });
    return (
      <Shell>
        <CommandPalette />
      </Shell>
    );
  },
};

export default meta;

type Story = StoryObj<Args>;

export const Open: Story = { args: { open: true } };
export const Closed: Story = { args: { open: false } };
export const OpenLight: Story = {
  args: { open: true },
  globals: { theme: "light" },
};
