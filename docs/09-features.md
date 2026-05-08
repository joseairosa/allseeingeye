# 09 - Features

Implementation notes per feature, organised by how the user encounters them. Each feature lists scope, key behaviours, edge cases, and minimal acceptance criteria.

## F1 - Tool detection and registration

**Scope.** First launch and ongoing. Detect which supported tools are installed, surface a confirmation, watch their roots.

**Behaviour.**
- Probe each tool descriptor's `detection.binary` via `which` and `detection.paths` via `fs::metadata`.
- A tool counts as detected when at least one of its `paths` exists, regardless of binary presence (some tools are GUI-only).
- Detection is **non-destructive**: we never write into a tool's directory just to confirm it.
- Result is presented in a side panel; user can disable a detected tool to skip indexing it.
- Re-detection runs on app launch and on a manual "Re-scan tools" command. We do not poll continuously.

**Edge cases.**
- Multiple Claude Code versions installed via different mechanisms (Homebrew + native installer): treat as one tool but show both binaries in diagnostics.
- A tool's directory exists but is empty: still detected, shows "no components yet".
- A tool we don't support is encountered (e.g., user has `~/.foo`): ignored silently.

**Acceptance.**
- 5 tools installed: All Seeing Eye lists exactly those 5, marks others as undetected within 1s.
- Disabling a detected tool removes its components from the index within 1s.

## F2 - Component indexing

**Scope.** Continuous parsing and indexing of all components from enabled tools.

**Behaviour.**
- On launch: full scan in parallel (parser pool sized to physical cores).
- On file event: incremental update.
- Each component gets a stable `aseye://` URI based on `(tool, scope, type, name)`.
- Sidecar metadata (tags, pins, notes) reattaches across delete + recreate cycles.

**Edge cases.**
- File present but unparseable: store with `parseErrors`, surface in UI with a `!` badge.
- File too large (> 5 MB): skip parsing, store metadata only, surface a warning.
- Symlinks pointing outside registered roots: not followed.
- Circular `@import`s in memory files: detect and break the cycle, surface a warning.

**Acceptance.**
- 200-component setup: full scan completes in under 800 ms on a baseline machine (M-class Mac).
- A `vim`-like atomic save on a watched file results in exactly one `component:upserted` event after debounce.

## F3 - Search and filter

**Scope.** Top-of-Inventory search bar, plus filter chips, plus a small expression language.

**Behaviour.**
- FTS5 over `name + description + body`, with `unicode61` tokeniser.
- Search is debounced at 80 ms; results stream in as they're computed.
- Filter chips combine with AND.
- Expression language is a thin sugar:
  - `type:skill tool:claude-code`
  - `tag:work last:<7d`
  - `health:down`
  - `path:**/skills/**`
  - free text becomes an FTS query.
- Results sortable by recency, name, last used, type.

**Edge cases.**
- Misspelled type/tool names in expressions: highlighted, suggestions offered.
- Empty filtered set: explicit empty state with "Clear filters" button.

**Acceptance.**
- A FTS query over 200 components returns within 30 ms p95.

## F4 - Quick Look

**Scope.** Lightweight read-only preview triggered by Space or hover.

**Behaviour.**
- Right-side panel, 420px, slides in over content with backdrop blur.
- Shows: name, description, path, scope, tool, last used, key relations, first 8 lines of body.
- Buttons: Open Editor, Pin, Tag, Open in Finder/Explorer, Copy path.

**Acceptance.**
- Quick Look opens in under 100 ms on selection change.

## F5 - Editor

**Scope.** Full-featured, schema-aware text editor for all parseable component types.

**Behaviour.**
- Two-pane: form + raw, with toggles to show either alone.
- Form pane is generated from a per-type JSON Schema and renders fields appropriate to value type (text, multiline, boolean, enum, list, glob).
- Raw pane is Monaco with the right language mode (yaml / json / toml / markdown / mdc-as-markdown).
- Edits in either pane round-trip through a single AST in memory (see `08-tech-architecture.md`).
- Save triggers validation; failure blocks save unless user confirms "save anyway".
- "Discard" reverts to the on-disk version since the file was opened.

**Diff against disk.**
- A status indicator shows `clean` (file matches disk hash since open) or `external changes` (the file changed externally during the session). Clicking opens a 3-pane diff: ours / disk / merge.

**Edge cases.**
- File deleted on disk while editor is open: editor shifts to "create" mode; saving recreates the file.
- File renamed on disk during editing: editor updates its path silently and continues.
- Schema validation failure on save: surface inline errors, focus the first one.

**Acceptance.**
- Save -> watcher fires upserted event -> Editor's "saved 1s ago" appears within 200 ms.

## F6 - Toggle / enable

**Scope.** Enable or disable a component without deleting it.

**Behaviour.**
- For tools that support enabling individual components (Claude Code's `enabledPlugins`, etc.), toggle updates the right config field.
- For tools that don't have a per-component enable, we simulate by renaming `<name>.md` to `<name>.md.disabled` (a soft convention).
- Toggle state is reflected in Inventory with a dim row + slashed icon.

**Edge cases.**
- Toggling a plugin that has dependent components: propagation rules are tool-specific; we surface a confirm dialog.

**Acceptance.**
- Toggle from Quick Look reflects in the host tool on its next session start.

## F7 - Drift detection

**Scope.** Side-by-side comparison of equivalent components across tools.

**Behaviour.**
- Heuristic equivalence on launch and after every batch of upserts:
  - Same scope (user vs project).
  - Canonical filename for tool (CLAUDE.md vs AGENTS.md vs GEMINI.md vs ...).
  - Shingled content overlap > threshold.
- User can mark a pair as `equivalentTo` (asserted) or `not equivalentTo` (suppressed).
- Drift view lists pairs sorted by divergence percentage.
- Per pair: 3-pane diff (left, right, "merge into"), with actions: adopt-left, adopt-right, merge.
- Merge writes to all candidates atomically.

**Edge cases.**
- One side has imports/includes the other doesn't: noted in the diff, not auto-resolved.
- Three-way drift (a single concept split across 3 tools): UI handles N pairs; merge produces a single canonical body that propagates to all.

**Acceptance.**
- A user with CLAUDE.md and .cursorrules whose content is 60% overlap sees a drift pair on launch.

## F8 - Health probing (MCP)

**Scope.** Optional per-server probing of MCP server availability.

**Behaviour.**
- Off by default. User enables per-server.
- Probe interval default 5 minutes; per-server tunable.
- Probes use transport-appropriate handshake (see `05-data-architecture.md`).
- Results stored in `health_probe`; UI shows latency p50, error rate, last error string.

**Edge cases.**
- A stdio server that requires interactive auth: probe times out; surface "needs auth" suggestion.
- Server that exists in 4 tools' configs (deduped to one): we probe once and reflect health on all references.

**Acceptance.**
- Toggle a server's probe on -> first probe completes within 5s -> latency reflected in Health view.

## F9 - Usage analytics

**Scope.** Cold/hot reporting from session histories where available.

**Behaviour.**
- Mine session histories for tool/skill/agent invocation counts.
- "Last used" date populated where derivable.
- "Cold" report: components not invoked in N days (default 30; tunable).
- All mining is local; no network.

**Sources by tool.**
- Claude Code: `~/.claude/sessions/*.json`.
- Codex: `~/.codex/history.jsonl`, `~/.codex/sessions/`.
- Cline: extension state JSONL.
- Goose: session files.
- Aider: `.aider.chat.history.md` parsed for tool calls.
- Cursor / Antigravity / Copilot: limited; we surface what's accessible.

**Edge cases.**
- Histories are large (100k+ lines): we sample by tail and project counts; full mining is opt-in.

**Acceptance.**
- A skill invoked 3 times in the last week shows "used 3 times in 7d" in Quick Look within 5s of opening it.

## F10 - Bundle export

**Scope.** Select N components and export as a sharable artifact.

**Behaviour.**
- User picks any number of components in Inventory.
- "Export bundle" opens a wizard: choose target format (Claude Code plugin, Goose recipe, Cursor rules pack, generic zip).
- For target-format export, we apply a known transformation:
  - Claude Code plugin: emit `.claude-plugin/plugin.json` and copy components to `skills/`, `agents/`, etc.
  - Goose recipe: emit a YAML recipe with prompts and MCP references.
  - Cursor rules pack: a `.cursor/rules/` folder.
  - Generic: a tar.gz with a manifest and the raw files.
- Generated `README.md` describes the bundle and install steps.

**Edge cases.**
- A selected component has tool-specific frontmatter that doesn't translate (e.g., Claude Code's `isolation: worktree`): we emit a warning in the README and drop the field with a comment.
- Cyclic references: bundle includes all referenced components by default (depth 1); user can opt out per item.

**Acceptance.**
- Export 5 components as a Claude Code plugin -> resulting folder installs cleanly via `claude --plugin-dir`.

## F11 - Bundle import

**Scope.** Drop a bundle (zip / folder / URL) and install into the matching host tool.

**Behaviour.**
- User drags a folder or zip onto the app, or pastes a URL (GitHub repo, raw zip).
- We inspect the manifest (`.claude-plugin/plugin.json`, recipe YAML, etc.) and detect the target tool.
- Show a preview: which components will be added, where they'll land, conflicts.
- User confirms; we copy files into the tool's directories and update its plugin index where applicable.

**Edge cases.**
- Bundle for a tool the user doesn't have installed: offer to "save for later".
- Conflict with existing component (same name): rename / replace / cancel.

**Acceptance.**
- Import a Claude Code plugin -> appears in Inventory under the right plugin scope -> Claude Code recognises it on next session start.

## F12 - Convert / promote

**Scope.** One-click transform of a component from one tool's format into another.

**Behaviour.**
- Right-click a component in Inventory -> "Convert to..." -> list of compatible target tools.
- Conversion uses a per-pair transformer (e.g., Claude Code skill -> Antigravity skill is largely lossless; Cursor rule -> Claude Code rule needs frontmatter mapping).
- Result is a new component shown in a confirm panel before being written.

**Edge cases.**
- Lossy conversions: user is shown a diff and a list of dropped fields; "Proceed" or "Cancel".

**Acceptance.**
- Convert a `~/.claude/skills/spec/SKILL.md` to an Antigravity skill -> resulting `.agent/skills/spec/SKILL.md` parses and works in Antigravity.

## F13 - Multi-window

**Scope.** Open an Editor in a separate window while keeping Map or Inventory in the main window.

**Behaviour.**
- Cmd-Shift-O or "Open in new window" duplicates the current view in a fresh window.
- All windows share the same backend store; saves in one reflect in all.

**Edge cases.**
- Last window closes -> app exits (mac: stays in dock per platform convention).

**Acceptance.**
- Two windows open simultaneously, same component edited from both - operations are rejected on the non-active window with a clear "currently being edited elsewhere" message.

## F14 - Settings

**Scope.** App settings (not host-tool settings).

**Categories.**
- General: theme, density, dyslexia font, animation level.
- Tools: enable/disable per detected tool, override paths.
- Index: rebuild, reset, location.
- Health: MCP probing defaults.
- Privacy: telemetry (off; only appears once we ship telemetry), analytics opt-in.
- Updates: channel (stable, beta), auto-update on/off.
- Diagnostics: copy report.

**Acceptance.**
- All settings persist across restarts.

## F15 - Onboarding

**Scope.** First launch only.

**Behaviour.**
- See `06-ux-design.md` "Onboarding flow".
- Total median duration < 60 seconds.

**Acceptance.**
- First launch on a machine with 5 tools and 200 components: from launch to landing on Inventory < 30 s.

## F16 - Diagnostics

**Scope.** A panel for debugging and copy-out reports.

**Behaviour.**
- Last 100 file events.
- Last 50 parse errors.
- Index stats.
- Watcher status per tool.
- "Copy diagnostics" -> sanitised JSON to clipboard.

**Acceptance.**
- A user reporting a bug can attach a diagnostics report that contains zero file content but enough metadata to reproduce.

## F17 - Reduced motion / accessibility

**Scope.** Respect OS / app settings for animation and add a11y enhancements.

**Behaviour.**
- `prefers-reduced-motion` -> all animations replaced with instant transitions.
- Dyslexia font option in settings (uses OpenDyslexic).
- High-contrast variants of dark/light themes (v1).
- All shortcuts rebindable.

## F18 - Updates

**Scope.** App auto-update and version transparency.

**Behaviour.**
- Daily check.
- Notify the user of an available update; never install without confirmation.
- Release notes shown inline.
- Channel selectable (stable, beta).

## F19 - Panic mode

**Scope.** A single-keystroke (Cmd-Shift-.) mode that detaches all watchers and clears caches.

**Behaviour.**
- Useful when screensharing, demoing, or when the user wants the app to stop running probes.
- Visible badge in the chrome.
- Click to resume.

## F20 - Cmd-K command palette

**Scope.** Universal access to actions and components.

**Behaviour.**
- Top: matching components, ranked by usage + name match.
- Below: matching actions ("Generate spec from skill", "Open Editor for /spec", "Mark drift as not equivalent", ...).
- All keyboard navigable; Enter to fire.

**Acceptance.**
- Time from Cmd-K to action fired for a known component name < 800 ms.

---

Each feature above maps to one or more PRD requirement IDs in `02-prd.md`. Implementation order is governed by the roadmap in `10-roadmap.md`.
