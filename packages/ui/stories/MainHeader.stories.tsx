import type { Meta, StoryObj } from "@storybook/react-vite";
import { MainHeader } from "@/components/MainHeader";
import { useUi, type ViewId } from "@/store/ui";
import { Shell } from "./_shell";

interface Args {
  view: ViewId;
}

const meta: Meta<Args> = {
  title: "Chrome/MainHeader",
  args: { view: "inventory" },
  argTypes: {
    view: {
      options: ["inventory", "map", "editor", "health"] satisfies ViewId[],
      control: { type: "inline-radio" },
    },
  },
  render: (args) => {
    // Sync the global UI store before each render so the header's title
    // tracks the toolbar arg. Stories share the store within a session;
    // this keeps the story declarative.
    useUi.setState({ view: args.view });
    return (
      <Shell>
        <main className="main-area">
          <MainHeader />
        </main>
      </Shell>
    );
  },
};

export default meta;

type Story = StoryObj<Args>;

export const Inventory: Story = { args: { view: "inventory" } };
export const Map: Story = { args: { view: "map" } };
export const Editor: Story = { args: { view: "editor" } };
export const Health: Story = { args: { view: "health" } };
