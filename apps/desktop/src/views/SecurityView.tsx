/**
 * Security view (Phase 7.3).
 *
 * Sections:
 *   - Header: filter pills (All / Critical / High / Medium / Low / Suppressed)
 *     with live counts driven by `useSecuritySummary`.
 *   - Body:   `useSecurityFindings` rows grouped by severity DESC, each
 *     row carrying a redacted preview, source label, and inline actions.
 *   - Bulk:   per-row checkbox + "Suppress N" header action.
 *   - Empty state: shield-check + "no findings" message.
 *
 * IPC contract: every read goes through TanStack Query hooks; mutations
 * (suppress) invalidate the security cache so the Sidebar count and
 * Inventory shield badge converge automatically.
 */
import { memo, useMemo, useState } from "react";
import { useUi } from "@/store/ui";
import {
  useSecurityFindings,
  useSecuritySummary,
  useSuppressFinding,
  useUnsuppressFinding,
} from "@/ipc/hooks";
import { RedactedPreview } from "@/components/RedactedPreview";
import { ShieldCheckIcon, ShieldIcon } from "@/components/icons";
import type {
  FindingSummary,
  SecurityFilter,
  Severity,
} from "@aseye/shared-types";

/** Severity bucket the header pills cycle through. `null` -> all severities. */
type SeverityBucket = Severity | null | "suppressed";

interface SeverityPillMeta {
  id: SeverityBucket;
  label: string;
}

const PILLS: readonly SeverityPillMeta[] = [
  { id: null, label: "all" },
  { id: "critical", label: "critical" },
  { id: "high", label: "high" },
  { id: "medium", label: "medium" },
  { id: "low", label: "low" },
  { id: "suppressed", label: "suppressed" },
] as const;

const SEVERITY_ORDER: readonly Severity[] = [
  "critical",
  "high",
  "medium",
  "low",
] as const;

const SEVERITY_LABEL: Record<Severity, string> = {
  critical: "Critical",
  high: "High",
  medium: "Medium",
  low: "Low",
};

const CATEGORY_LABEL: Record<string, string> = {
  secret: "Secret",
  "mcp-permission": "MCP permission",
};

/**
 * Translate a `SeverityBucket` selection into a `SecurityFilter` payload.
 * `"suppressed"` maps to `suppressed: true` (and unrestricted severity);
 * a concrete severity maps to `severity: <s>` with `suppressed: false`
 * so the active list stays surfaced.
 */
function bucketToFilter(bucket: SeverityBucket): SecurityFilter {
  if (bucket === null) {
    return {
      componentId: null,
      severity: null,
      category: null,
      suppressed: false,
      limit: null,
      offset: null,
    };
  }
  if (bucket === "suppressed") {
    return {
      componentId: null,
      severity: null,
      category: null,
      suppressed: true,
      limit: null,
      offset: null,
    };
  }
  return {
    componentId: null,
    severity: bucket,
    category: null,
    suppressed: false,
    limit: null,
    offset: null,
  };
}

interface FindingRowProps {
  finding: FindingSummary;
  selected: boolean;
  onToggleSelect: (id: string) => void;
  onSuppress: (componentId: string, pattern: string) => void;
  onUnsuppress: (componentId: string, pattern: string) => void;
  onJumpToComponent: (componentId: string) => void;
}

const FindingRow = memo(function FindingRow({
  finding,
  selected,
  onToggleSelect,
  onSuppress,
  onUnsuppress,
  onJumpToComponent,
}: FindingRowProps) {
  const categoryLabel =
    CATEGORY_LABEL[finding.category] ?? finding.category;

  return (
    <div className="security-row" role="row" aria-selected={selected}>
      <span className="security-row-select">
        <input
          type="checkbox"
          checked={selected}
          onChange={() => onToggleSelect(finding.id)}
          aria-label={`select finding ${finding.pattern}`}
        />
      </span>
      <span className={`shield-badge ${finding.severity}`} aria-hidden="true">
        <ShieldIcon />
      </span>
      <div className="security-row-main">
        <div className="security-row-title">
          <strong>{finding.componentName}</strong>
          <span className="mono">{finding.pattern}</span>
        </div>
        <small className="mono security-row-path">{finding.componentPath}</small>
        <div className="security-row-meta">
          <span className={`health-pill ${severityPillClass(finding.severity)}`}>
            {SEVERITY_LABEL[finding.severity]}
          </span>
          <span className="security-row-category">{categoryLabel}</span>
          <span className="mono security-row-source">{finding.sourceLabel}</span>
        </div>
      </div>
      <RedactedPreview
        value={finding.redactedPreview}
        label={`secret preview for ${finding.pattern}`}
      />
      <div className="security-row-actions">
        <button
          type="button"
          className="text-button quiet"
          onClick={() => onJumpToComponent(finding.componentId)}
        >
          view component
        </button>
        {finding.suppressed ? (
          <button
            type="button"
            className="text-button quiet"
            onClick={() => onUnsuppress(finding.componentId, finding.pattern)}
          >
            unsuppress
          </button>
        ) : (
          <button
            type="button"
            className="text-button quiet"
            onClick={() => onSuppress(finding.componentId, finding.pattern)}
          >
            suppress
          </button>
        )}
      </div>
    </div>
  );
});

/** Map severity to the colour used by the existing `health-pill` classes. */
function severityPillClass(severity: Severity): string {
  switch (severity) {
    case "critical":
    case "high":
      return "error";
    case "medium":
      return "warn";
    case "low":
    default:
      return "cold";
  }
}

interface FindingGroupProps {
  severity: Severity;
  findings: FindingSummary[];
  selectedIds: ReadonlySet<string>;
  onToggleSelect: (id: string) => void;
  onSuppress: (componentId: string, pattern: string) => void;
  onUnsuppress: (componentId: string, pattern: string) => void;
  onJumpToComponent: (componentId: string) => void;
}

function FindingGroup({
  severity,
  findings,
  selectedIds,
  onToggleSelect,
  onSuppress,
  onUnsuppress,
  onJumpToComponent,
}: FindingGroupProps) {
  if (findings.length === 0) return null;
  return (
    <section className="security-group" aria-labelledby={`security-${severity}`}>
      <h3 className="security-section-header" id={`security-${severity}`}>
        <span className={`shield-badge ${severity}`} aria-hidden="true">
          <ShieldIcon />
        </span>
        <span>{SEVERITY_LABEL[severity]}</span>
        <span className="security-section-count">{findings.length}</span>
      </h3>
      {findings.map((finding) => (
        <FindingRow
          key={finding.id}
          finding={finding}
          selected={selectedIds.has(finding.id)}
          onToggleSelect={onToggleSelect}
          onSuppress={onSuppress}
          onUnsuppress={onUnsuppress}
          onJumpToComponent={onJumpToComponent}
        />
      ))}
    </section>
  );
}

function EmptyState() {
  return (
    <div className="security-empty" aria-live="polite">
      <span className="security-empty-icon" aria-hidden="true">
        <ShieldCheckIcon />
      </span>
      <strong>No findings</strong>
      <small>The audit engine has nothing to report on this filter.</small>
    </div>
  );
}

export function SecurityView() {
  const view = useUi((s) => s.view);
  const selectComponent = useUi((s) => s.selectComponent);
  const setView = useUi((s) => s.setView);
  const isActive = view === "security";
  const [bucket, setBucket] = useState<SeverityBucket>(null);
  const [selectedIds, setSelectedIds] = useState<ReadonlySet<string>>(
    () => new Set(),
  );

  const filter = useMemo(() => bucketToFilter(bucket), [bucket]);
  const { data, isPending, isError } = useSecurityFindings(filter);
  const { data: summary } = useSecuritySummary();
  const suppressMut = useSuppressFinding();
  const unsuppressMut = useUnsuppressFinding();

  // Memoise the dataset so a stable reference flows into `grouped`.
  // Without this, the `data ?? []` fallback creates a fresh array on
  // every render and the memo below tears down each pass.
  const findings = useMemo(() => data ?? [], [data]);
  const grouped = useMemo(() => {
    const acc: Record<Severity, FindingSummary[]> = {
      critical: [],
      high: [],
      medium: [],
      low: [],
    };
    for (const f of findings) acc[f.severity].push(f);
    return acc;
  }, [findings]);

  function toggleSelect(id: string): void {
    setSelectedIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }

  function clearSelection(): void {
    setSelectedIds(new Set());
  }

  function handleSuppress(componentId: string, pattern: string): void {
    suppressMut.mutate({ componentId, pattern });
  }

  function handleUnsuppress(componentId: string, pattern: string): void {
    unsuppressMut.mutate({ componentId, pattern });
  }

  function handleBulkSuppress(): void {
    const targets = findings.filter((f) => selectedIds.has(f.id));
    for (const t of targets) {
      suppressMut.mutate({ componentId: t.componentId, pattern: t.pattern });
    }
    clearSelection();
  }

  // Bulk-unsuppress branch for when the user is filtering to suppressed
  // findings and wants to return many at once. Mirrors `handleBulkSuppress`
  // shape so the keyboard / accessibility behaviour stays identical.
  function handleBulkUnsuppress(): void {
    const targets = findings.filter((f) => selectedIds.has(f.id));
    for (const t of targets) {
      unsuppressMut.mutate({ componentId: t.componentId, pattern: t.pattern });
    }
    clearSelection();
  }

  function handleJumpToComponent(componentId: string): void {
    selectComponent(componentId);
    setView("inventory");
  }

  return (
    <section
      className={`view${isActive ? " active" : ""}`}
      data-view-panel="security"
      aria-labelledby="security-heading"
      hidden={!isActive}
    >
      <div className="view-toolbar">
        <h2 id="security-heading">Security</h2>
        {selectedIds.size > 0 ? (
          bucket === "suppressed" ? (
            <button
              type="button"
              className="primary-button"
              onClick={handleBulkUnsuppress}
              disabled={unsuppressMut.isPending}
            >
              unsuppress {selectedIds.size} selected
            </button>
          ) : (
            <button
              type="button"
              className="primary-button"
              onClick={handleBulkSuppress}
              disabled={suppressMut.isPending}
            >
              suppress {selectedIds.size} selected
            </button>
          )
        ) : null}
      </div>

      <div className="filter-strip" aria-label="severity buckets">
        {PILLS.map((p) => {
          const count = pillCount(p.id, summary);
          const active = bucket === p.id;
          return (
            <button
              key={p.label}
              type="button"
              aria-pressed={active}
              className={`chip security-pill${active ? " selected" : ""}`}
              onClick={() => setBucket(p.id)}
            >
              <span>{p.label}</span>
              <span className="security-pill-count">{count}</span>
            </button>
          );
        })}
      </div>

      {isPending ? (
        <div className="security-empty" aria-live="polite">
          <small>loading findings...</small>
        </div>
      ) : null}

      {isError ? (
        <div className="security-empty" role="alert">
          <strong>could not load findings</strong>
          <small>check the index process and retry</small>
        </div>
      ) : null}

      {!isPending && !isError && findings.length === 0 ? <EmptyState /> : null}

      {!isPending && !isError && findings.length > 0
        ? SEVERITY_ORDER.map((sev) => (
            <FindingGroup
              key={sev}
              severity={sev}
              findings={grouped[sev]}
              selectedIds={selectedIds}
              onToggleSelect={toggleSelect}
              onSuppress={handleSuppress}
              onUnsuppress={handleUnsuppress}
              onJumpToComponent={handleJumpToComponent}
            />
          ))
        : null}
    </section>
  );
}

/**
 * Pluck the count for a given pill from the security summary. The
 * summary keeps a separate `suppressed` total because the severity
 * counts already filter suppressed rows out.
 */
function pillCount(
  bucket: SeverityBucket,
  summary: ReturnType<typeof useSecuritySummary>["data"],
): number {
  if (!summary) return 0;
  if (bucket === null) return summary.total;
  if (bucket === "suppressed") return summary.suppressed;
  switch (bucket) {
    case "critical":
      return summary.bySeverity.critical;
    case "high":
      return summary.bySeverity.high;
    case "medium":
      return summary.bySeverity.medium;
    case "low":
      return summary.bySeverity.low;
    default:
      return 0;
  }
}
