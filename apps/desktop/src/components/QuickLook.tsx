import { useUi } from "@/store/ui";
import {
  useComponent,
  useFindingsForComponent,
  useSuppressFinding,
} from "@/ipc/hooks";
import { formatRelativeTime } from "@/lib/relativeTime";
import {
  contextWindowPct,
  estimateTokens,
  formatTokensK,
  MAX_CONTEXT_TOKENS,
} from "@/lib/tokens";
import type {
  ComponentDetail,
  FindingSummary,
  ToolId,
} from "@aseye/shared-types";
import { RedactedPreview } from "./RedactedPreview";
import { CloseIcon, NavEditorIcon, ShieldIcon } from "./icons";

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
      <SecuritySection componentId={detail.id} />
      <CostFooter detail={detail} />
    </>
  );
}

/**
 * Phase 14B - cost footer. Memory components only. Renders a single
 * line below the metadata showing the rough token count and the share
 * of a 200k context window the file consumes if always loaded.
 *
 * The component returns `null` for non-memory kinds rather than an
 * empty string - keeps DOM noise out of Quick Look for the majority
 * of components (skills/agents/commands have no per-turn preamble
 * cost worth quoting).
 */
function CostFooter({ detail }: { detail: ComponentDetail }): React.ReactElement | null {
  if (detail.kind !== "memory") return null;
  const tokens = estimateTokens(detail.size);
  const pct = contextWindowPct(tokens);
  const tokenLabel = formatTokensK(tokens);
  const pctLabel = pct < 0.1 ? "<0.1" : pct.toFixed(1);
  const contextLabel = `${(MAX_CONTEXT_TOKENS / 1000).toFixed(0)}k`;
  return (
    <p
      className="quick-cost-footer"
      title="Approximate, based on ~4 chars/token. Real cost varies by tokenizer and content."
    >
      ~{tokenLabel} tokens · {pctLabel}% of a {contextLabel} context window
    </p>
  );
}

interface SecuritySectionProps {
  componentId: string;
}

/**
 * Phase 7.3 - Quick Look's per-component findings list. Hidden when the
 * component has zero findings so the panel doesn't grow a permanent
 * empty section. Each finding offers an inline suppress action; the
 * mutation invalidates the security caches via `useSuppressFinding`.
 */
function SecuritySection({ componentId }: SecuritySectionProps) {
  const { data } = useFindingsForComponent(componentId);
  const suppressMut = useSuppressFinding();
  const findings = data ?? [];
  if (findings.length === 0) return null;
  return (
    <section className="quick-section quick-security" aria-labelledby="quick-security-heading">
      <h3 id="quick-security-heading">Security</h3>
      {findings.map((f) => (
        <SecurityRow
          key={f.id}
          finding={f}
          onSuppress={() =>
            suppressMut.mutate({
              componentId: f.componentId,
              pattern: f.pattern,
            })
          }
        />
      ))}
    </section>
  );
}

interface SecurityRowProps {
  finding: FindingSummary;
  onSuppress: () => void;
}

function SecurityRow({ finding, onSuppress }: SecurityRowProps) {
  return (
    <div className="quick-security-row">
      <div className="quick-security-head">
        <span className={`shield-badge ${finding.severity}`} aria-hidden="true">
          <ShieldIcon />
        </span>
        <span className="mono quick-security-pattern">{finding.pattern}</span>
        <span className="quick-security-severity">{finding.severity}</span>
      </div>
      <div className="quick-security-meta">
        <RedactedPreview
          value={finding.redactedPreview}
          label={`secret preview for ${finding.pattern}`}
        />
        <span className="mono quick-security-source">{finding.sourceLabel}</span>
      </div>
      {!finding.suppressed ? (
        <button
          type="button"
          className="text-button quiet"
          onClick={onSuppress}
        >
          suppress
        </button>
      ) : null}
    </div>
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
        {/*
          Audit issue #4: pin and tag icon buttons used to live here but
          neither pinning nor tagging exists in the data model today.
          Rather than ship dead-click controls they have been removed.
          When the features land they regain their slots next to the
          primary "open editor" action.
        */}
      </div>
    </aside>
  );
}
