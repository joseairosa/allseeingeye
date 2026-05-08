import type { CSSProperties } from "react";
import type { Meta, StoryObj } from "@storybook/react-vite";
import {
  CloseIcon,
  CommandSearchIcon,
  DensityIcon,
  FiltersIcon,
  NavEditorIcon,
  NavHealthIcon,
  NavInventoryIcon,
  NavMapIcon,
  PinIcon,
  PlusIcon,
  RefreshIcon,
  SaveIcon,
  SearchIcon,
  TagIcon,
  ThemeIcon,
  TypeIcon,
  type TypeIconId,
} from "@/components/icons";
import { Shell } from "./_shell";

const TYPE_IDS: TypeIconId[] = [
  "icon-skill",
  "icon-agent",
  "icon-command",
  "icon-mcp",
  "icon-rule",
  "icon-memory",
  "icon-hook",
];

const ACTION_ICONS = [
  { name: "CloseIcon", node: <CloseIcon className="icon-24" /> },
  { name: "SearchIcon", node: <SearchIcon className="icon-24" /> },
  { name: "CommandSearchIcon", node: <CommandSearchIcon className="icon-24" /> },
  { name: "RefreshIcon", node: <RefreshIcon className="icon-24" /> },
  { name: "FiltersIcon", node: <FiltersIcon className="icon-24" /> },
  { name: "DensityIcon", node: <DensityIcon className="icon-24" /> },
  { name: "ThemeIcon", node: <ThemeIcon className="icon-24" /> },
  { name: "NavInventoryIcon", node: <NavInventoryIcon className="icon-24" /> },
  { name: "NavMapIcon", node: <NavMapIcon className="icon-24" /> },
  { name: "NavEditorIcon", node: <NavEditorIcon className="icon-24" /> },
  { name: "NavHealthIcon", node: <NavHealthIcon className="icon-24" /> },
  { name: "SaveIcon", node: <SaveIcon className="icon-24" /> },
  { name: "PinIcon", node: <PinIcon className="icon-24" /> },
  { name: "TagIcon", node: <TagIcon className="icon-24" /> },
  { name: "PlusIcon", node: <PlusIcon className="icon-24" /> },
] as const;

const meta: Meta = {
  title: "Tokens/Icons",
};

export default meta;

const containerStyle: CSSProperties = {
  padding: "32px",
  display: "grid",
  gap: "32px",
  color: "var(--text-primary)",
  background: "var(--bg-base)",
  minHeight: "100vh",
};

const gridStyle: CSSProperties = {
  display: "grid",
  gridTemplateColumns: "repeat(auto-fill, minmax(120px, 1fr))",
  gap: "12px",
};

const cardStyle: CSSProperties = {
  display: "grid",
  placeItems: "center",
  gap: "8px",
  padding: "16px 8px",
  border: "1px solid var(--border-subtle)",
  borderRadius: "10px",
  background: "var(--bg-elev-2)",
  fontSize: "12px",
  color: "var(--text-secondary)",
  fontFamily: "var(--font-mono)",
  textAlign: "center",
  wordBreak: "break-word",
};

const swatchStyle: CSSProperties = {
  width: "40px",
  height: "40px",
  display: "grid",
  placeItems: "center",
  color: "var(--text-primary)",
};

export const TypeIcons: StoryObj = {
  render: () => (
    <Shell>
      <div style={containerStyle}>
        <h2 style={{ margin: 0 }}>Type icons (sprite)</h2>
        <p style={{ margin: 0, color: "var(--text-secondary)" }}>
          24px primary; <code>type-mini</code> renders at 16px in dense rows.
        </p>
        <div style={gridStyle}>
          {TYPE_IDS.map((id) => (
            <div key={id} style={cardStyle}>
              <span style={swatchStyle}>
                <TypeIcon id={id} />
              </span>
              <span>{id}</span>
            </div>
          ))}
        </div>
      </div>
    </Shell>
  ),
};

export const ActionIcons: StoryObj = {
  render: () => (
    <Shell>
      <div style={containerStyle}>
        <h2 style={{ margin: 0 }}>Action icons (inline SVG)</h2>
        <p style={{ margin: 0, color: "var(--text-secondary)" }}>
          Stroke 1.5px, follow <code>currentColor</code>.
        </p>
        <div style={gridStyle}>
          {ACTION_ICONS.map((it) => (
            <div key={it.name} style={cardStyle}>
              <span style={{ ...swatchStyle, stroke: "currentColor", fill: "none" }}>
                {it.node}
              </span>
              <span>{it.name}</span>
            </div>
          ))}
        </div>
      </div>
    </Shell>
  ),
};
