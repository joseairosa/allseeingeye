/**
 * Projects view (Phase 17.A - shell only).
 *
 * Lists every project surfaced by the index. A "project" is the
 * parent directory of any indexed memory file (CLAUDE.md /
 * AGENTS.md / GEMINI.md). The view itself reads the list once via
 * `useProjects` and renders one card per project with a size /
 * token chip. The action buttons (Analyze CLAUDE.md / Audit
 * worktrees / Reorganize docs) ship in 17.B / 17.C / 17.D and are
 * placeholders for now.
 */
import { useMemo, useState, type ReactElement } from "react";
import { useUi } from "@/store/ui";
import {
  useAnalyzeMemory,
  useAuditWorktrees,
  useProjects,
  useReorganizeDocs,
} from "@/ipc/hooks";
import { formatBytes, formatTokensK } from "@/lib/tokens";
import type {
  MemoryAnalysisReport,
  ProjectSummary,
  ReorganizeReport,
  WorktreeReport,
} from "@aseye/shared-types";

export function ProjectsView(): ReactElement {
  const view = useUi((s) => s.view);
  const isActive = view === "projects";
  const projects = useProjects();

  const data = projects.data;
  const total = data?.length ?? 0;
  const oversized = useMemo<number>(
    () => (data ?? []).filter((p) => p.isOversized).length,
    [data],
  );

  return (
    <section
      className={`view${isActive ? " active" : ""}`}
      data-view-panel="projects"
      aria-labelledby="projects-heading"
      hidden={!isActive}
    >
      <div className="view-toolbar">
        <h2 id="projects-heading">Projects</h2>
        <span className="cost-refreshed-meta" aria-live="polite">
          {projects.isPending && !data
            ? "loading…"
            : projects.isError
              ? `could not load projects: ${projects.error.message}`
              : `${total} indexed memory root${total === 1 ? "" : "s"}` +
                (oversized > 0
                  ? ` · ${oversized} oversized`
                  : "")}
        </span>
      </div>

      {projects.isPending && !data ? (
        <div className="projects-list" aria-busy="true">
          {[0, 1, 2].map((k) => (
            <div className="project-card skeleton" key={k}>
              <div className="skeleton-block" style={{ height: 18, width: "40%" }} />
              <div className="skeleton-block" style={{ height: 14, width: "70%", marginTop: 8 }} />
            </div>
          ))}
        </div>
      ) : null}

      {!projects.isPending && total === 0 ? (
        <div className="projects-empty">
          <p>No projects detected yet.</p>
          <p className="settings-todo">
            A project is the parent directory of any indexed memory
            file (CLAUDE.md, AGENTS.md, GEMINI.md). Make sure the
            scanner has run at least once and that{" "}
            <span className="mono">~/Development</span> contains at
            least one project with a memory file at its root.
          </p>
        </div>
      ) : null}

      {data && total > 0 ? (
        <div className="projects-list">
          {data.map((project) => (
            <ProjectCard key={project.projectPath} project={project} />
          ))}
        </div>
      ) : null}
    </section>
  );
}

interface ProjectCardProps {
  project: ProjectSummary;
}

function ProjectCard({ project }: ProjectCardProps): ReactElement {
  // primaryMemoryTokensEst rides as bigint (ts-rs maps Rust's u64 to
  // bigint). formatTokensK takes number; coerce here. The estimate is
  // bounded by file size which is also bounded, so Number() is safe.
  const sizeLabel = `${formatBytes(project.primaryMemorySize)} · ~${formatTokensK(
    Number(project.primaryMemoryTokensEst),
  )} tok`;
  const primaryBasename =
    project.memoryFiles[0]?.basename ?? "(memory file)";
  const otherCount = project.memoryFiles.length - 1;

  const analyze = useAnalyzeMemory();
  const [report, setReport] = useState<MemoryAnalysisReport | null>(null);
  const [analysisError, setAnalysisError] = useState<string | null>(null);

  const audit = useAuditWorktrees();
  const [wtReport, setWtReport] = useState<WorktreeReport | null>(null);
  const [wtError, setWtError] = useState<string | null>(null);

  const reorg = useReorganizeDocs();
  const [reorgPreview, setReorgPreview] =
    useState<ReorganizeReport | null>(null);
  const [reorgApplied, setReorgApplied] =
    useState<ReorganizeReport | null>(null);
  const [reorgError, setReorgError] = useState<string | null>(null);

  async function handleAnalyze(): Promise<void> {
    setAnalysisError(null);
    try {
      const next = await analyze.mutateAsync({
        projectPath: project.projectPath,
        memoryPath: project.primaryMemoryPath,
      });
      setReport(next);
    } catch (err) {
      setAnalysisError(err instanceof Error ? err.message : String(err));
    }
  }

  async function handleAudit(): Promise<void> {
    setWtError(null);
    try {
      const next = await audit.mutateAsync(project.projectPath);
      setWtReport(next);
    } catch (err) {
      setWtError(err instanceof Error ? err.message : String(err));
    }
  }

  async function handleReorgPreview(): Promise<void> {
    setReorgError(null);
    setReorgApplied(null);
    try {
      const next = await reorg.mutateAsync({
        projectPath: project.projectPath,
        dryRun: true,
      });
      setReorgPreview(next);
    } catch (err) {
      setReorgError(err instanceof Error ? err.message : String(err));
    }
  }

  async function handleReorgApply(): Promise<void> {
    setReorgError(null);
    try {
      const next = await reorg.mutateAsync({
        projectPath: project.projectPath,
        dryRun: false,
      });
      setReorgApplied(next);
      setReorgPreview(null);
    } catch (err) {
      setReorgError(err instanceof Error ? err.message : String(err));
    }
  }

  return (
    <article
      className={`project-card${project.isOversized ? " oversized" : ""}`}
      aria-labelledby={`project-${project.projectPath}`}
    >
      <header className="project-card-header">
        <h3
          id={`project-${project.projectPath}`}
          className="project-card-title"
        >
          <span>{project.displayName}</span>
          <span
            className={`size-chip${project.isOversized ? " warn" : ""}`}
            title={
              project.isOversized
                ? `${primaryBasename} is over the 8 KB / ~2k token threshold`
                : "Approximate, based on ~4 chars/token."
            }
          >
            {project.isOversized ? "⚠ " : ""}
            {sizeLabel}
          </span>
        </h3>
        <p className="project-card-meta">
          <span className="mono">{project.projectPath}</span>
        </p>
        <p className="project-card-meta">
          Primary memory:{" "}
          <span className="mono">{primaryBasename}</span>
          {otherCount > 0 ? (
            <span>
              {" "}+ {otherCount} other memory file{otherCount === 1 ? "" : "s"}
            </span>
          ) : null}
          {project.hasGit ? (
            <span> · git</span>
          ) : (
            <span> · not a git repo</span>
          )}
        </p>
      </header>
      <footer className="project-card-actions">
        <button
          type="button"
          className="text-button"
          onClick={() => {
            void handleAnalyze();
          }}
          disabled={analyze.isPending}
          aria-busy={analyze.isPending}
        >
          {analyze.isPending ? "Analyzing…" : `Analyze ${primaryBasename}`}
        </button>
        <button
          type="button"
          className="text-button"
          onClick={() => {
            void handleAudit();
          }}
          disabled={!project.hasGit || audit.isPending}
          aria-busy={audit.isPending}
          title={
            project.hasGit
              ? "Run git worktree list and show disk usage per worktree"
              : "Project has no .git/ directory"
          }
        >
          {audit.isPending ? "Auditing…" : "Audit worktrees"}
        </button>
        <button
          type="button"
          className="text-button"
          onClick={() => {
            void handleReorgPreview();
          }}
          disabled={reorg.isPending}
          aria-busy={reorg.isPending}
          title="Preview moves of loose .md files into docs/ + link rewrites"
        >
          {reorg.isPending && reorgPreview === null
            ? "Previewing…"
            : "Reorganize docs"}
        </button>
      </footer>
      {analysisError ? (
        <div
          className="validation-box"
          role="alert"
          aria-live="polite"
          data-toast-kind="error"
        >
          <span>!</span>
          <p>{analysisError}</p>
          <button
            type="button"
            className="text-button quiet"
            onClick={() => setAnalysisError(null)}
            style={{ marginLeft: "auto" }}
          >
            dismiss
          </button>
        </div>
      ) : null}
      {report ? (
        <AnalysisResult report={report} onClear={() => setReport(null)} />
      ) : null}
      {wtError ? (
        <div
          className="validation-box"
          role="alert"
          aria-live="polite"
          data-toast-kind="error"
        >
          <span>!</span>
          <p>{wtError}</p>
          <button
            type="button"
            className="text-button quiet"
            onClick={() => setWtError(null)}
            style={{ marginLeft: "auto" }}
          >
            dismiss
          </button>
        </div>
      ) : null}
      {wtReport ? (
        <WorktreeResult report={wtReport} onClear={() => setWtReport(null)} />
      ) : null}
      {reorgError ? (
        <div
          className="validation-box"
          role="alert"
          aria-live="polite"
          data-toast-kind="error"
        >
          <span>!</span>
          <p>{reorgError}</p>
          <button
            type="button"
            className="text-button quiet"
            onClick={() => setReorgError(null)}
            style={{ marginLeft: "auto" }}
          >
            dismiss
          </button>
        </div>
      ) : null}
      {reorgApplied ? (
        <ReorganizeResult
          report={reorgApplied}
          mode="applied"
          onClear={() => setReorgApplied(null)}
        />
      ) : null}
      {reorgPreview ? (
        <ReorganizeResult
          report={reorgPreview}
          mode="preview"
          onApply={() => {
            void handleReorgApply();
          }}
          isApplying={reorg.isPending}
          onClear={() => setReorgPreview(null)}
        />
      ) : null}
    </article>
  );
}

interface ReorganizeResultProps {
  report: ReorganizeReport;
  mode: "preview" | "applied";
  onApply?: () => void;
  isApplying?: boolean;
  onClear: () => void;
}

function ReorganizeResult({
  report,
  mode,
  onApply,
  isApplying,
  onClear,
}: ReorganizeResultProps): ReactElement {
  const moveCount = report.moves.length;
  const rewriteCount = report.linkRewrites.length;
  const errorCount = report.errors.length;

  return (
    <section
      className="project-card-analysis"
      aria-labelledby="reorganize-heading"
    >
      <header className="project-card-analysis-header">
        <strong id="reorganize-heading">
          {mode === "preview" ? "Reorganise preview" : "Reorganise applied"}
        </strong>
        <span>
          {moveCount} move{moveCount === 1 ? "" : "s"} ·{" "}
          {rewriteCount} link rewrite{rewriteCount === 1 ? "" : "s"}
          {errorCount > 0
            ? ` · ${errorCount} error${errorCount === 1 ? "" : "s"}`
            : ""}
          {report.walkIncomplete ? " · walk incomplete" : ""}
        </span>
        <button
          type="button"
          className="text-button quiet"
          onClick={onClear}
          aria-label="dismiss reorganise report"
        >
          clear
        </button>
      </header>

      {moveCount === 0 && rewriteCount === 0 && errorCount === 0 ? (
        <p className="settings-todo">
          No loose top-level <code>.md</code> files to move. Allowlist
          (README, CLAUDE, AGENTS, GEMINI, CHANGELOG, LICENSE,
          CONTRIBUTING, CODE_OF_CONDUCT, SECURITY, COMPONENTS) and
          existing <code>docs/</code> files are skipped.
        </p>
      ) : null}

      {moveCount > 0 ? (
        <details className="project-card-recommendations" open>
          <summary>
            <strong>Moves ({moveCount})</strong>
          </summary>
          <ul style={{ marginTop: 6 }}>
            {report.moves.map((m, idx) => (
              <li key={idx} className="project-card-rec rec-move">
                <span className="mono">
                  {fileName(m.from)} → docs/{fileName(m.to)}
                </span>
                <p className="settings-todo">
                  {formatBytes(m.size)}
                </p>
              </li>
            ))}
          </ul>
        </details>
      ) : null}

      {rewriteCount > 0 ? (
        <details className="project-card-recommendations">
          <summary>
            <strong>Link rewrites ({rewriteCount})</strong>
          </summary>
          <ul style={{ marginTop: 6 }}>
            {report.linkRewrites.slice(0, 50).map((r, idx) => (
              <li key={idx} className="project-card-rec rec-rewrite">
                <span className="mono">
                  {fileName(r.file)}:{Number(r.line)}
                </span>
                <p>
                  <span className="settings-todo">{r.before}</span>
                  {" → "}
                  <span>{r.after}</span>
                </p>
              </li>
            ))}
            {rewriteCount > 50 ? (
              <li className="settings-todo">
                +{rewriteCount - 50} more (apply still rewrites all of
                them)
              </li>
            ) : null}
          </ul>
        </details>
      ) : null}

      {errorCount > 0 ? (
        <details className="project-card-recommendations" open>
          <summary>
            <strong>Errors ({errorCount})</strong>
          </summary>
          <ul style={{ marginTop: 6 }}>
            {report.errors.map((e, idx) => (
              <li key={idx} className="project-card-rec rec-staleReference">
                <span className="mono">{e.path}</span>
                <p>
                  <strong>{String(e.kind)}</strong>: {e.message}
                </p>
              </li>
            ))}
          </ul>
        </details>
      ) : null}

      {mode === "preview" && (moveCount > 0 || rewriteCount > 0) ? (
        <div className="settings-row-actions">
          <button
            type="button"
            className="primary-button"
            onClick={onApply}
            disabled={isApplying}
            aria-busy={isApplying}
          >
            {isApplying
              ? "Applying…"
              : `Apply ${moveCount} move${moveCount === 1 ? "" : "s"} and ${rewriteCount} rewrite${rewriteCount === 1 ? "" : "s"}`}
          </button>
          <small className="settings-todo">
            Pre-reorg sidecars (
            <code>{"<file>.aseye-pre-reorg-<unix>.bak"}</code>) are
            written next to each source file before the move.
          </small>
        </div>
      ) : null}
    </section>
  );
}

function fileName(p: string): string {
  const idx = p.lastIndexOf("/");
  return idx === -1 ? p : p.slice(idx + 1);
}

interface WorktreeResultProps {
  report: WorktreeReport;
  onClear: () => void;
}

function WorktreeResult({ report, onClear }: WorktreeResultProps): ReactElement {
  const total = report.worktrees.length;
  const totalDisk = formatBytes(report.totalDiskUsageBytes);
  return (
    <section className="project-card-analysis" aria-labelledby="worktree-heading">
      <header className="project-card-analysis-header">
        <strong id="worktree-heading">Worktrees</strong>
        <span>
          {total} worktree{total === 1 ? "" : "s"} · {totalDisk} total
        </span>
        <button
          type="button"
          className="text-button quiet"
          onClick={onClear}
          aria-label="dismiss worktree report"
        >
          clear
        </button>
      </header>
      {total === 0 ? (
        <p className="settings-todo">No worktrees reported by git.</p>
      ) : (
        <ul className="project-card-recommendations">
          {report.worktrees.map((wt) => {
            const ageDays =
              wt.mtimeUnix > 0
                ? Math.floor(
                    (Date.now() / 1000 - Number(wt.mtimeUnix)) / 86_400,
                  )
                : null;
            return (
              <li key={wt.path} className="project-card-rec rec-worktree">
                <strong>
                  {wt.isMain ? "main · " : ""}
                  {wt.branch ?? "(detached)"}
                </strong>
                <p className="mono">{wt.path}</p>
                <p>
                  {formatBytes(wt.diskUsageBytes)}
                  {wt.incomplete ? " (lower bound)" : ""}
                  {ageDays !== null ? ` · ${ageDays}d old` : ""}
                  {wt.locked ? " · locked" : ""}
                  {wt.lockReason ? ` (${wt.lockReason})` : ""}
                </p>
                {!wt.isMain ? (
                  <p className="settings-todo">
                    To remove: <span className="mono">git worktree remove {wt.path}</span>
                  </p>
                ) : null}
              </li>
            );
          })}
        </ul>
      )}
    </section>
  );
}

interface AnalysisResultProps {
  report: MemoryAnalysisReport;
  onClear: () => void;
}

function AnalysisResult({ report, onClear }: AnalysisResultProps): ReactElement {
  const recCount = report.recommendations.length;
  return (
    <section className="project-card-analysis" aria-labelledby="analysis-heading">
      <header className="project-card-analysis-header">
        <strong id="analysis-heading">Analysis</strong>
        <span>
          {formatBytes(report.sizeBytes)} · ~
          {formatTokensK(Number(report.tokensEst))} tok ·{" "}
          {recCount === 0
            ? "no issues found"
            : `${recCount} suggestion${recCount === 1 ? "" : "s"}`}
        </span>
        <button
          type="button"
          className="text-button quiet"
          onClick={onClear}
          aria-label="dismiss analysis"
        >
          clear
        </button>
      </header>
      {recCount === 0 ? (
        <p className="settings-todo">
          No optimization opportunities flagged. The file is within the
          8 KB / ~2k token threshold and has no internal duplicates,
          duplicates of your global CLAUDE.md, or stale references.
        </p>
      ) : (
        <ul className="project-card-recommendations">
          {report.recommendations.map((rec, idx) => (
            <li key={idx} className={`project-card-rec rec-${rec.kind}`}>
              <strong>{recommendationLabel(rec.kind)}</strong>
              {rec.lineRange ? (
                <span className="settings-todo">
                  {" "}line {Number(rec.lineRange[0])}
                  {Number(rec.lineRange[0]) !== Number(rec.lineRange[1])
                    ? `–${Number(rec.lineRange[1])}`
                    : ""}
                </span>
              ) : null}
              <p>{rec.message}</p>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

function recommendationLabel(kind: string): string {
  switch (kind) {
    case "oversized":
      return "Oversized";
    case "duplicateOfGlobal":
      return "Duplicate of global CLAUDE.md";
    case "internalDuplicate":
      return "Internal duplicate";
    case "unknownFrontmatterField":
      return "Unknown frontmatter field";
    case "staleReference":
      return "Stale reference";
    default:
      return kind;
  }
}
