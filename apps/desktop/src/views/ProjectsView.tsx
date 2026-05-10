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
import { useMemo, type ReactElement } from "react";
import { useUi } from "@/store/ui";
import { useProjects } from "@/ipc/hooks";
import { formatBytes, formatTokensK } from "@/lib/tokens";
import type { ProjectSummary } from "@aseye/shared-types";

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
          className="text-button quiet"
          disabled
          title="Action lands in 17.B"
        >
          Analyze CLAUDE.md
        </button>
        <button
          type="button"
          className="text-button quiet"
          disabled={!project.hasGit}
          title={
            project.hasGit
              ? "Action lands in 17.C"
              : "Project has no .git/ directory"
          }
        >
          Audit worktrees
        </button>
        <button
          type="button"
          className="text-button quiet"
          disabled
          title="Action lands in 17.D"
        >
          Reorganize docs
        </button>
      </footer>
    </article>
  );
}
