/**
 * Recommendations panel. Up to 5 cards, each backed by a `CostRec`
 * from the recommendations engine. Cards expose every component the
 * backend tagged as related so the user can jump straight to the
 * Editor and act on the suggestion.
 *
 * Interaction contract: clicking a related-component chip switches the
 * view to "editor" via `setView` and selects the component via
 * `selectComponent`. Both action names are sourced from the existing
 * Zustand store; nothing on the cost view invents new actions.
 */
import type { CostRec, CostRecKind } from "@aseye/shared-types";
import { useUi } from "@/store/ui";
import { formatUsd, shortenProjectPath } from "./format";

interface CostRecommendationsProps {
  recs: ReadonlyArray<CostRec>;
  isLoading: boolean;
  /** Cap honoured by the spec: at most 5 cards in the panel. */
  limit?: number;
}

const DEFAULT_LIMIT = 5;

const KIND_LABEL: Record<CostRecKind, string> = {
  bloatedMemory: "Bloated memory",
  lowCacheHitRate: "Low cache hit rate",
  oldModelOnHotProject: "Older model on hot project",
};

const KIND_CLASS: Record<CostRecKind, string> = {
  bloatedMemory: "rec-kind-memory",
  lowCacheHitRate: "rec-kind-cache",
  oldModelOnHotProject: "rec-kind-model",
};

export function CostRecommendations({
  recs,
  isLoading,
  limit = DEFAULT_LIMIT,
}: CostRecommendationsProps): React.ReactElement {
  if (isLoading && recs.length === 0) {
    return (
      <section className="cost-pane cost-recs-pane" aria-labelledby="cost-recs-heading">
        <h3 id="cost-recs-heading">Recommendations</h3>
        <ol className="cost-rec-list" aria-busy="true">
          {[0, 1].map((k) => (
            <li className="cost-rec-card" key={k}>
              <span className="skeleton-block" style={{ width: "70%" }} />
              <span
                className="skeleton-block"
                style={{ width: "90%", marginTop: 8 }}
              />
            </li>
          ))}
        </ol>
      </section>
    );
  }

  if (recs.length === 0) {
    return (
      <section className="cost-pane cost-recs-pane" aria-labelledby="cost-recs-heading">
        <h3 id="cost-recs-heading">Recommendations</h3>
        <p className="settings-todo">
          No savings opportunities surfaced. Indexed projects either have
          modest spend or already follow the cost-aware patterns.
        </p>
      </section>
    );
  }

  const visible = recs.slice(0, limit);

  return (
    <section className="cost-pane cost-recs-pane" aria-labelledby="cost-recs-heading">
      <h3 id="cost-recs-heading">Recommendations</h3>
      <ol className="cost-rec-list">
        {visible.map((rec, idx) => (
          <CostRecCard key={`${rec.kind}-${idx}-${rec.title}`} rec={rec} />
        ))}
      </ol>
    </section>
  );
}

function CostRecCard({ rec }: { rec: CostRec }): React.ReactElement {
  const setView = useUi((s) => s.setView);
  const selectComponent = useUi((s) => s.selectComponent);

  function openInEditor(componentId: string): void {
    selectComponent(componentId);
    setView("editor");
  }

  return (
    <li className="cost-rec-card">
      <div className="cost-rec-card-head">
        <span className={`cost-rec-kind ${KIND_CLASS[rec.kind]}`}>
          {KIND_LABEL[rec.kind]}
        </span>
        <span className="cost-rec-savings">
          ~{formatUsd(rec.estimatedSavingsUsd30d)} / 30d
        </span>
      </div>
      <strong className="cost-rec-title">{rec.title}</strong>
      <p className="cost-rec-rationale">{rec.rationale}</p>
      <div className="cost-rec-meta">
        <span title={rec.projectPath}>{shortenProjectPath(rec.projectPath)}</span>
      </div>
      {rec.relatedComponents.length > 0 ? (
        <div
          className="cost-rec-actions"
          role="group"
          aria-label="open related components"
        >
          {rec.relatedComponents.map((id) => (
            <button
              key={id}
              type="button"
              className="cost-rec-action"
              onClick={() => openInEditor(id)}
              title={id}
            >
              <span aria-hidden="true">↗</span>
              <span>Open in editor</span>
            </button>
          ))}
        </div>
      ) : null}
    </li>
  );
}
