import { useUi } from "@/store/ui";
import { inventoryRows } from "@/lib/fixtures";
import { CloseIcon, NavEditorIcon, PinIcon, TagIcon } from "./icons";

export function QuickLook() {
  const open = useUi((s) => s.quickLookOpen);
  const id = useUi((s) => s.selectedComponentId);
  const setView = useUi((s) => s.setView);
  const toggle = useUi((s) => s.toggleQuickLook);

  const row = inventoryRows.find((r) => r.id === id) ?? inventoryRows[0]!;

  return (
    <aside
      className={`quicklook${open ? " open" : ""}`}
      aria-label="quick look panel"
      aria-hidden={!open}
    >
      <div className="quicklook-header">
        <div>
          <div className="eyebrow">{`${row.kind}: ${row.name}`}</div>
          <h2>{row.name}</h2>
        </div>
        <button
          type="button"
          className="icon-button"
          onClick={() => toggle(false)}
          aria-label="close quick look"
          title="Close"
        >
          <CloseIcon />
        </button>
      </div>
      <p className="quick-desc">{row.desc}</p>
      <dl className="meta-grid">
        <div><dt>Tool</dt><dd>{row.tool}</dd></div>
        <div><dt>Scope</dt><dd>{row.scope}</dd></div>
        <div><dt>Path</dt><dd className="mono">{row.path}</dd></div>
        <div><dt>Used</dt><dd>{row.used}</dd></div>
      </dl>
      <section className="quick-section">
        <h3>Preview</h3>
        <p>{row.body}</p>
      </section>
      <section className="quick-section">
        <h3>Relations</h3>
        <p>{row.relations}</p>
      </section>
      <div className="quick-actions">
        <button
          type="button"
          className="primary-button"
          onClick={() => setView("editor")}
        >
          <NavEditorIcon />
          open editor
        </button>
        <button type="button" className="icon-button" aria-label="pin component" title="Pin">
          <PinIcon />
        </button>
        <button type="button" className="icon-button" aria-label="tag component" title="Tag">
          <TagIcon />
        </button>
      </div>
    </aside>
  );
}
