import type { Meta, StoryObj } from "@storybook/react-vite";
import { QuickLook } from "@/components/QuickLook";
import { useUi } from "@/store/ui";
import { inventoryRows } from "@/lib/fixtures";
import { Shell } from "./_shell";

interface Args {
  open: boolean;
  componentId: string;
}

const COMPONENT_IDS = inventoryRows.map((r) => r.id);

const meta: Meta<Args> = {
  title: "Panels/QuickLook",
  args: { open: true, componentId: "spec" },
  argTypes: {
    open: { control: "boolean" },
    componentId: { options: COMPONENT_IDS, control: { type: "select" } },
  },
  render: (args) => {
    useUi.setState({
      quickLookOpen: args.open,
      selectedComponentId: args.componentId,
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
