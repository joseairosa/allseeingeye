import { useUi } from "@/store/ui";
import { useComponent } from "@/ipc/hooks";
import { formatRelativeTime } from "@/lib/relativeTime";
import type { ComponentDetail, ToolId } from "@aseye/shared-types";
import { CloseIcon, NavEditorIcon, PinIcon, TagIcon } from "./icons";

const TOOL_DISPLAY_NAME: Record<ToolId, string> = {
  "claude-code": "Claude Code",
  codex: "Codex",
  cursor: "Cursor",
  antigravity: "Antigravity",
};

function displayLabel(detail: ComponentDetail): string {
  return detail.displayName?.trim() || detail.name;
}

interface HeaderProps {
  eyebrow: string;
  title: string;
  onClose: () => void;
}

function Header({ eyebrow, title, onClose }: HeaderProps) {
  return (
    <div className="quicklook-header">
      <div>
        <div className="eyebrow">{eyebrow}</div>
        <h2>{title}</h2>
      </div>
      <button
        type="button"
        className="icon-button"
        onClick={onClose}
        aria-label="close quick look"
        title="Close"
      >
        <CloseIcon />
      </button>
    </div>
  );
}

interface BodyProps {
  detail: ComponentDetail;
}

function Body({ detail }: BodyProps) {
  return (
    <>
      {detail.description ? (
        <p className="quick-desc">{detail.description}</p>
      ) : null}
      <dl className="meta-grid">
        <div><dt>Tool</dt><dd>{TOOL_DISPLAY_NAME[detail.tool]}</dd></div>
        <div><dt>Scope</dt><dd>{detail.scope}</dd></div>
        <div><dt>Path</dt><dd className="mono">{detail.path}</dd></div>
        <div><dt>Used</dt><dd>{formatRelativeTime(detail.lastUsedAt)}</dd></div>
      </dl>
      {detail.parseErrors ? (
        <section className="quick-section">
          <h3>Parse error</h3>
          <p className="mono">{detail.parseErrors}</p>
        </section>
      ) : null}
    </>
  );
}

function SkeletonBody() {
  return (
    <>
      <p className="quick-desc skeleton-block" aria-hidden="true">&nbsp;</p>
      <div className="meta-grid">
        <div className="skeleton-block" aria-hidden="true">&nbsp;</div>
        <div className="skeleton-block" aria-hidden="true">&nbsp;</div>
      </div>
    </>
  );
}

export function QuickLook() {
  const open = useUi((s) => s.quickLookOpen);
  const id = useUi((s) => s.selectedComponentId);
  const setView = useUi((s) => s.setView);
  const toggle = useUi((s) => s.toggleQuickLook);
  const { data, isPending } = useComponent(id);

  let eyebrow: string;
  let title: string;
  let body: React.ReactNode;
  if (id === null) {
    eyebrow = "quick look";
    title = "Select a component";
    body = null;
  } else if (isPending && !data) {
    eyebrow = "loading";
    title = "…";
    body = <SkeletonBody />;
  } else if (!data) {
    eyebrow = "quick look";
    title = "Component not found";
    body = null;
  } else {
    eyebrow = `${data.kind}: ${data.name}`;
    title = displayLabel(data);
    body = <Body detail={data} />;
  }

  return (
    <aside
      className={`quicklook${open ? " open" : ""}`}
      aria-label="quick look panel"
      aria-hidden={!open}
    >
      <Header eyebrow={eyebrow} title={title} onClose={() => toggle(false)} />
      {body}
      <div className="quick-actions">
        <button
          type="button"
          className="primary-button"
          onClick={() => setView("editor")}
          disabled={!data}
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
