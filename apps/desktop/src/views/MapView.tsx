/**
 * Map view (Phase 4.3 stub).
 *
 * MVP ships an illustrative static graph - the real Sigma.js / WebGL
 * implementation lands in v1 (`docs/10-roadmap.md` Phase v1 "Map view").
 * Cluster labels are hydrated from the live `useTools()` result so the
 * stub at least reflects the user's actual tool inventory.
 *
 * The static node positions and edges are intentional: they demonstrate
 * the visual language (clusters, edges with severity tints, type-icon
 * glyphs) so the design is locked while the backing data is built out.
 */
import { useUi } from "@/store/ui";
import { useTools } from "@/ipc/hooks";
import { TypeIcon } from "@/components/icons";

interface NodeProps {
  x: number;
  y: number;
  iconId: Parameters<typeof TypeIcon>[0]["id"];
  name: string;
  small: string;
  selected?: boolean;
  issue?: boolean;
}

function GraphNode({ x, y, iconId, name, small, selected, issue }: NodeProps) {
  const cls = ["graph-node", selected ? "selected" : "", issue ? "issue" : ""]
    .filter(Boolean)
    .join(" ");
  return (
    <button
      type="button"
      className={cls}
      style={{ "--x": `${x}px`, "--y": `${y}px` } as React.CSSProperties}
    >
      <TypeIcon id={iconId} />
      <span>{name}</span>
      <small>{small}</small>
    </button>
  );
}

export function MapView() {
  const view = useUi((s) => s.view);
  const isActive = view === "map";
  const tools = useTools();
  const detectedToolNames =
    tools.data
      ?.filter((t) => t.detected)
      .map((t) => t.displayName) ?? [];

  return (
    <section
      className={`view${isActive ? " active" : ""}`}
      data-view-panel="map"
      aria-labelledby="map-heading"
      hidden={!isActive}
    >
      <div className="view-toolbar">
        <h2 id="map-heading">Relationship map</h2>
        <div className="segmented" role="group" aria-label="cluster mode">
          <button type="button" className="active" disabled>tool</button>
          <button type="button" disabled>type</button>
          <button type="button" disabled>project</button>
        </div>
      </div>

      <div
        className="map-stub-banner"
        role="note"
        aria-live="polite"
      >
        <strong>Illustrative stub.</strong> Real graph rendering lands in v1.
        {detectedToolNames.length > 0 ? (
          <span>
            {" "}Currently indexing: {detectedToolNames.join(", ")}.
          </span>
        ) : null}
      </div>

      <div className="map-canvas" aria-label="component relationship graph">
        <svg
          className="map-lines"
          aria-hidden="true"
          viewBox="0 0 920 540"
          preserveAspectRatio="none"
        >
          <path className="edge accent" d="M196 148 C260 100, 340 140, 408 178" />
          <path className="edge" d="M196 148 C244 258, 284 316, 392 348" />
          <path className="edge dashed" d="M408 178 C540 108, 626 126, 748 196" />
          <path className="edge accent-2" d="M392 348 C506 424, 628 412, 734 344" />
          <path className="edge" d="M748 196 C770 256, 762 302, 734 344" />
        </svg>
        <div className="graph-cluster cluster-claude">Claude Code</div>
        <div className="graph-cluster cluster-codex">Codex</div>

        <GraphNode x={124} y={96} iconId="icon-skill" name="spec" small="Claude" selected />
        <GraphNode x={338} y={144} iconId="icon-agent" name="reviewer" small="Claude" />
        <GraphNode x={296} y={316} iconId="icon-hook" name="PostToolUse" small="Claude" />
        <GraphNode x={686} y={160} iconId="icon-mcp" name="github" small="3 tools" issue />
        <GraphNode x={672} y={308} iconId="icon-memory" name="CLAUDE.md" small="project" />

        <div className="map-legend">
          <span><TypeIcon id="icon-skill" className="type-mini" /> skill</span>
          <span><TypeIcon id="icon-agent" className="type-mini" /> agent</span>
          <span><TypeIcon id="icon-mcp" className="type-mini" /> mcp</span>
          <span><TypeIcon id="icon-memory" className="type-mini" /> memory</span>
        </div>
      </div>
    </section>
  );
}
