/**
 * ComponentRow story.
 *
 * The live row is currently an inner component inside InventoryView that
 * reads from the zustand store. Extracting it would require touching
 * InventoryView, which is out of scope for Phase 0.4 (`apps/desktop/src/views/`
 * is locked). Instead, we re-render the same DOM/CSS the inventory grid
 * produces from a fixture row, so the story exercises every visual flag
 * (selected, drift, issue, disabled) and every health pill colour without
 * pulling state in.
 */
import type { Meta, StoryObj } from "@storybook/react-vite";
import type { ReactNode } from "react";
import { TypeIcon, type TypeIconId } from "@/components/icons";
import { inventoryRows, type ComponentRow } from "@/lib/fixtures";
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

interface RowArgs {
  row: ComponentRow;
}

function StaticRow({ row }: RowArgs): ReactNode {
  const flag = row.rowFlag ?? "";
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
          <TypeIcon id={TYPE_TO_ICON[row.kind] ?? "icon-skill"} />
          <strong>{row.kind}</strong>
        </span>
        <span role="cell" className="name-cell">
          <span>{row.name}</span>
          <small>{row.smallLabel}</small>
        </span>
        <span role="cell">{row.tool}</span>
        <span role="cell">{row.scope}</span>
        <span role="cell">
          <span className={`health-pill ${row.health}`}>{row.healthLabel}</span>
        </span>
        <span role="cell">{row.used}</span>
      </button>
    </div>
  );
}

const meta: Meta<RowArgs> = {
  title: "Inventory/ComponentRow",
  args: { row: inventoryRows[0]! },
  argTypes: {
    row: {
      options: inventoryRows.map((r) => r.id),
      mapping: Object.fromEntries(inventoryRows.map((r) => [r.id, r])),
      control: { type: "select", labels: Object.fromEntries(inventoryRows.map((r) => [r.id, `${r.kind} - ${r.name}`])) },
    },
  },
  render: (args) => (
    <Shell>
      <main className="main-area">
        <StaticRow row={args.row} />
      </main>
    </Shell>
  ),
};

export default meta;

type Story = StoryObj<RowArgs>;

export const SelectedSkill: Story = { args: { row: inventoryRows[0]! } };
export const McpDegraded: Story = {
  args: { row: inventoryRows.find((r) => r.id === "github-mcp")! },
};
export const MemoryDrift: Story = {
  args: { row: inventoryRows.find((r) => r.id === "claude-md")! },
};
export const ColdDisabled: Story = {
  args: { row: inventoryRows.find((r) => r.id === "promo-video")! },
};
