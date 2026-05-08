import { useUi } from "@/store/ui";
import { SaveIcon } from "@/components/icons";

const RAW_PREVIEW = `---
name: spec
description: /spec - Unified Spec-Driven Development workflow
tools:
  - read
  - write
  - rg
---

# Spec workflow

Use this skill when the user is starting or changing a feature.
Read the relevant docs, create a narrow plan, then dispatch
review and verification before code reaches the main tree.`;

export function EditorView() {
  const view = useUi((s) => s.view);
  const isActive = view === "editor";

  return (
    <section
      className={`view${isActive ? " active" : ""}`}
      data-view-panel="editor"
      aria-labelledby="editor-heading"
      hidden={!isActive}
    >
      <div className="editor-topline">
        <div>
          <h2 id="editor-heading">skill: spec</h2>
          <p className="mono">/Users/joseairosa/.claude/skills/spec/SKILL.md</p>
        </div>
        <div className="editor-actions">
          <button type="button" className="text-button quiet">discard</button>
          <button type="button" className="primary-button">
            <SaveIcon />
            save
          </button>
        </div>
      </div>

      <div className="editor-layout">
        <form className="form-pane" aria-label="schema form" onSubmit={(e) => e.preventDefault()}>
          <div className="pane-title">
            <span>form view</span>
            <span className="health-pill up">valid</span>
          </div>
          <label className="field">
            <span>Name</span>
            <input defaultValue="spec" />
          </label>
          <label className="field">
            <span>Description</span>
            <textarea
              rows={4}
              defaultValue="/spec - Unified Spec-Driven Development workflow with review and verification gates."
            />
          </label>
          <fieldset className="field">
            <legend>Invocation</legend>
            <div className="segmented wide">
              <button type="button" className="active">model</button>
              <button type="button">manual</button>
              <button type="button">disabled</button>
            </div>
          </fieldset>
          <div className="field">
            <span>Files</span>
            <div className="file-list">
              <span>SKILL.md</span>
              <span>steps/dispatch.md</span>
              <span>agents/spec-reviewer.md</span>
              <span>references/schema.md</span>
            </div>
          </div>
          <div className="validation-box">
            <span className="status-ring up" />
            <div>
              <strong>validation ok</strong>
              <p>schema and frontmatter round-trip cleanly</p>
            </div>
          </div>
        </form>

        <div className="raw-pane" aria-label="raw editor preview">
          <div className="pane-title">
            <span>raw view</span>
            <span className="mono">line 12</span>
          </div>
          <pre><code>{RAW_PREVIEW}</code></pre>
        </div>
      </div>
    </section>
  );
}
