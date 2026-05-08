import { useMemo } from "react";
import { useUi } from "@/store/ui";
import { inventoryRows, type ComponentRow } from "@/lib/fixtures";
import { FiltersIcon, SearchIcon, TypeIcon } from "@/components/icons";

const TYPE_TO_ICON: Record<string, string> = {
  skill: "icon-skill",
  agent: "icon-agent",
  command: "icon-command",
  mcp: "icon-mcp",
  rule: "icon-rule",
  memory: "icon-memory",
  hook: "icon-hook",
};

const FILTER_CHIPS = [
  { id: "tool:claude-code", label: "Claude Code", selected: true },
  { id: "type:skill", label: "Skill", selected: true },
  { id: "scope:user", label: "User", selected: false },
  { id: "last:7d", label: "Recently used", selected: false },
  { id: "has:relations", label: "Has relations", selected: false },
] as const;

function rowMatches(row: ComponentRow, query: string): boolean {
  const normalized = query.toLowerCase().replace(/-/g, " ").trim();
  if (!normalized) return true;
  const haystack = [
    row.name,
    row.kind,
    row.tool,
    row.scope,
    row.desc,
    row.path,
  ]
    .join(" ")
    .toLowerCase()
    .replace(/-/g, " ");
  const terms = normalized
    .split(/\s+/)
    .map((term) => term.replace(/^(type|tool|scope):/, ""))
    .filter(Boolean);
  return terms.every((t) => haystack.includes(t));
}

function Row({ row }: { row: ComponentRow }) {
  const selectedId = useUi((s) => s.selectedComponentId);
  const selectComponent = useUi((s) => s.selectComponent);
  const setView = useUi((s) => s.setView);

  const flags = [
    selectedId === row.id ? "selected" : "",
    row.rowFlag && row.rowFlag !== "selected" ? row.rowFlag : "",
  ]
    .filter(Boolean)
    .join(" ");

  return (
    <button
      type="button"
      className={`component-row ${flags}`.trim()}
      role="row"
      onClick={() => selectComponent(row.id)}
      onDoubleClick={() => setView("editor")}
    >
      <span role="cell" className="type-cell">
        <TypeIcon id={(TYPE_TO_ICON[row.kind] ?? "icon-skill") as Parameters<typeof TypeIcon>[0]["id"]} />
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
  );
}

export function InventoryView() {
  const search = useUi((s) => s.search);
  const setSearch = useUi((s) => s.setSearch);
  const view = useUi((s) => s.view);

  const visible = useMemo(
    () => inventoryRows.filter((r) => rowMatches(r, search)),
    [search],
  );

  const isActive = view === "inventory";

  return (
    <section
      className={`view${isActive ? " active" : ""}`}
      data-view-panel="inventory"
      aria-labelledby="inventory-heading"
      hidden={!isActive}
    >
      <h2 className="sr-only" id="inventory-heading">Component inventory</h2>

      <div className="inventory-toolbar">
        <label className="search-field">
          <SearchIcon />
          <input
            type="search"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            aria-label="search components"
            placeholder="search..."
          />
        </label>
        <button type="button" className="text-button">
          <FiltersIcon />
          filters
        </button>
      </div>

      <div className="filter-strip" aria-label="active filters">
        {FILTER_CHIPS.map((chip) => (
          <button
            key={chip.id}
            type="button"
            className={`chip${chip.selected ? " selected" : ""}`}
          >
            {chip.label}
          </button>
        ))}
        <button type="button" className="chip ghost">+ tag</button>
      </div>

      <div className="inventory-grid" role="table" aria-label="components">
        <div className="grid-head" role="row">
          <span role="columnheader">type</span>
          <span role="columnheader">name</span>
          <span role="columnheader">tool</span>
          <span role="columnheader">scope</span>
          <span role="columnheader">state</span>
          <span role="columnheader">used</span>
        </div>
        {visible.map((row) => (
          <Row key={row.id} row={row} />
        ))}
      </div>
    </section>
  );
}
