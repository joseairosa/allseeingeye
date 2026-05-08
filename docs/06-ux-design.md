# 06 - UX Design

User flows, information architecture, key screens. ASCII wireframes are the spec; pixel-pushing comes later (see `07-visual-design.md`).

## Information architecture

Three primary spaces, plus a global command palette.

```
                    +-------------------------------+
                    |      Global Command Palette   |
                    |             (Cmd-K)           |
                    +---------------+---------------+
                                    |
                +-------------------+-------------------+
                |                   |                   |
        +-------v-------+   +-------v-------+   +-------v-------+
        |   Inventory   |   |      Map      |   |    Editor     |
        |   (default)   |   |   (graph)     |   |  (focused)    |
        +-------+-------+   +-------+-------+   +-------+-------+
                |                   |                   |
                +-------------------+-------------------+
                                    |
                          +---------v---------+
                          |   Side panels:    |
                          |   - Quick Look    |
                          |   - Health        |
                          |   - Drift         |
                          |   - History       |
                          +-------------------+
```

## Sidebar (always visible)

The left sidebar is the navigation backbone. Width 240px, collapsible.

```
+-----------------------------------+
| [eye] All Seeing Eye         x    |
+-----------------------------------+
| > Inventory             237       |
|   Map                             |
|   Editor                          |
|                                   |
| TOOLS                             |
|   Claude Code     142  *          |
|   Codex            48              |
|   Cursor           21              |
|   Antigravity      14              |
|   Cline             8              |
|   + Add tool                      |
|                                   |
| TYPES                             |
|   Skills            61            |
|   Agents            34            |
|   Commands          47            |
|   MCP servers       12  !2        |
|   Hooks              9            |
|   Rules             58            |
|   Memory             8            |
|   Plugins            5            |
|   Sessions         320            |
|                                   |
| COLLECTIONS                       |
|   * Pinned           7            |
|   * Project: ape     22           |
|   + New collection                |
|                                   |
| HEALTH                            |
|   Drift            3 pairs        |
|   MCP issues       2              |
|   Cold (90d)       18             |
+-----------------------------------+
| settings | help | v0.1.0          |
+-----------------------------------+
```

`*` denotes pinned. `!2` is a problem badge.

## Inventory view (default landing)

A dense, filterable grid + list hybrid. The default for most tasks.

```
+--------------------------------------------------------------------------------+
| [Sidebar]   Search components.................................     ⌘K  filters|
|             ----------------------------------------------------------------- |
|                                                                                |
|             Tool: All  Type: All  Scope: All  Health: All  Tag: All           |
|             Sort: Recently used  v                                             |
|                                                                                |
|             SKILL    spec                          Claude Code  user   2d ago |
|             agent    sql-dba                       Codex        user   3d ago |
|             COMMAND  /review-pr                    Claude Code  user   1d ago |
|             MCP      github                        used by 3 tools  *up 142ms |
|             RULE     standards-typescript          Claude Code  user   5d ago |
|             MEMORY   CLAUDE.md (project)           ape repo            today  |
|             SKILL    promo-video                   Claude Code  user   12d    |
|             HOOK     PostToolUse Skill|Task        Claude Code  user          |
|             PLUGIN   recall@recall-claude-plugin   Claude Code  user   1.15.17|
|             COMMAND  /spec                         Claude Code  user   1d ago |
|             ...                                                                |
|             ----------------------------------------------------------------- |
|             237 components  -  filtered: 237                       refresh    |
+--------------------------------------------------------------------------------+
```

Behaviour:
- Each row is hover-previewable; pressing `Space` opens a Quick Look panel on the right.
- Double-click or `Enter` opens in Editor.
- Multi-select with Shift / Cmd; bulk actions appear in a contextual top bar.
- Filters are chip-based and combine with AND.
- Search is FTS-backed; matches highlighted in the row.

### Filters

```
+-----------------------------------------------------+
| Tool          Type           Scope        Health    |
| [Claude Code] [Skill]        [User]       [Up]      |
| [Codex]       [Agent]        [Project]    [Down]    |
| [Cursor]      [Command]      [Plugin]     [Degraded]|
| [Antigravity] [MCP]          [Enterprise][Unprobed] |
| ...           [Hook] ...                            |
+-----------------------------------------------------+
| Tag       [+ pinned] [+ ape] [+ work]               |
| Last used [< 1 day] [< 7 days] [< 30 days] [cold]   |
| Has errors [yes] [no]                               |
+-----------------------------------------------------+
```

Filters are always-on chips; clicking toggles them. The query line above accepts a small expression language: `type:skill tool:claude-code last:<7d`.

## Map view (graph)

Force-directed graph. Nodes are components; edges are relations. Clusters by tool by default; switchable to cluster by type or by project.

```
+--------------------------------------------------------------------------------+
| [Sidebar]                                                                      |
|                                                                                |
|        Cluster by: [tool] type project                                         |
|                                                                                |
|              + skill: spec                                                     |
|             /         \                                                        |
|       agent:reviewer   skill:promo-video --- mcp:github                       |
|             |                                    |                             |
|       hook:PreToolUse                       agent:repo-walker                  |
|                                                  |                             |
|                                              memory: CLAUDE.md (project)       |
|                                                                                |
|       [legend]   skill o   agent ⬡   command ▻   mcp ◇                        |
|                  hook △    rule □    memory ▦                                  |
+--------------------------------------------------------------------------------+
```

Behaviour:
- Click a node = select; details panel slides in from the right.
- Hold Space to pan; scroll to zoom.
- A "freeze layout" toggle locks positions for screenshots.
- Click an edge to inspect the relation (source field, line number).

Map is a discovery tool, not a daily driver. Designed to make "what depends on what" obvious.

## Editor view

Focused single-component editor. Two-pane: form on the left, raw on the right. Toggle either pane off.

```
+--------------------------------------------------------------------------------+
| [Sidebar]   skill: spec   |   /Users/joseairosa/.claude/skills/spec/SKILL.md  |
|             user, Claude Code                                save | discard   |
+--------------------------------------------------------------------------------+
|             [form view]                          |       [raw view]            |
|                                                  |       --- (frontmatter)     |
| Name        spec                                 |       name: spec            |
| Desc        /spec - Unified Spec-Driven Dev      |       description: |        |
|             ---------------------------          |         /spec - Unified ... |
|                                                  |       ---                   |
| Trigger     [Slash]   [Mention]                  |                             |
|                                                  |       The user is...        |
| Files       SKILL.md                             |                             |
|             steps/                               |                             |
|             agents/                              |                             |
|             scripts/                             |                             |
|                                                  |                             |
| Validate    OK                                   |       cursor at line 12     |
| Diff        no on-disk changes                   |                             |
+--------------------------------------------------------------------------------+
| validation: ok  |  saved 3s ago  |  ⌘S save  |  ⌘D discard  |  ⌘P palette    |
+--------------------------------------------------------------------------------+
```

Form fields are derived from the per-tool schema. They edit a structured projection; underlying file is rewritten as Markdown + YAML frontmatter on save. Users who prefer raw can collapse the form pane.

## Quick Look (slide-in panel)

Press Space on a row in Inventory or click a node in Map. A right-side 420px panel.

```
                                              +-------------------------------+
                                              | skill: spec               x   |
                                              | /spec - Unified Spec-Driven   |
                                              |                                |
                                              | Tool   Claude Code            |
                                              | Scope  user                   |
                                              | Path   ~/.claude/skills/spec/ |
                                              |                                |
                                              | Used   12 times in last 30d   |
                                              | Health -                       |
                                              |                                |
                                              | Description                    |
                                              | The user is starting...        |
                                              | (preview, 8 lines)             |
                                              |                                |
                                              | Files                          |
                                              | - SKILL.md                     |
                                              | - steps/dispatch.md            |
                                              | - steps/...                    |
                                              |                                |
                                              | Relations                      |
                                              | spawns agent: spec-reviewer   |
                                              | references skill: spec-verify |
                                              |                                |
                                              | [open editor] [pin] [tag] ... |
                                              +-------------------------------+
```

Panel is read-mostly. "Open editor" jumps to the Editor view.

## Drift view

Triggered from the sidebar's Health > Drift item or from the command palette ("Find drift").

```
+--------------------------------------------------------------------------------+
| Drift detected: 3 pairs                                                        |
|                                                                                |
| 1. Memory                                                                      |
|    ~/.claude/CLAUDE.md      <-- 32% diverged -->     ~/.cursor/AGENTS.md      |
|    [side-by-side diff]                                                         |
|    [merge] [adopt left] [adopt right] [mark not equivalent]                    |
|                                                                                |
| 2. Rules                                                                       |
|    ~/.claude/rules/testing.md  <-- 12% -->  ~/.cursor/rules/testing.mdc       |
|    ...                                                                         |
|                                                                                |
| 3. MCP server "github"                                                         |
|    Claude Code config has env GITHUB_TOKEN; Cursor's has GH_TOKEN              |
|    [normalise] [keep both] [mark not equivalent]                               |
+--------------------------------------------------------------------------------+
```

Drift never auto-resolves. The user always picks.

## Health view

```
+--------------------------------------------------------------------------------+
| Health                                                                         |
|                                                                                |
| MCP servers (12)                                                               |
|   * up      | latency p50  | last error              | calls 30d              |
|   github     142 ms          -                          1,432                  |
|   stripe     89 ms          -                            204                   |
|   playwright 1.2 s          -                            312                   |
|   sentry     timeout         "no auth", 2h ago             7                   |
|   ...                                                                          |
|                                                                                |
| Skills (61)                                                                    |
|   used in last 7 days       42                                                 |
|   used in last 30 days      57                                                 |
|   cold (>30 days)            4                                                 |
|                                                                                |
| Plugins (5)                                                                    |
|   updated within 30 days     4                                                 |
|   stale > 90 days             1   recall@recall-claude-plugin (1.15.17 -> 1.16.2)|
+--------------------------------------------------------------------------------+
```

## Command palette (Cmd-K)

Opens at any time over any view.

```
+-----------------------------------------------------+
| > spec                                              |
+-----------------------------------------------------+
| skill   spec                       Claude Code  >   |
| skill   spec-verify                Claude Code  >   |
| skill   spec-bugfix-plan           Claude Code  >   |
| command /spec                      Claude Code  >   |
| Action  Generate spec from skill                    |
| Action  Open Editor for /spec                       |
+-----------------------------------------------------+
```

Top results are components; below are actions. Both selectable with arrow keys + Enter.

## Onboarding flow

First launch.

```
1. [Welcome]      Three sentences: what this is. [Start]
2. [Detect]       Auto-detected: Claude Code, Codex, Cursor, Antigravity.
                  Not found (toggle to scan): Cline, Continue, Goose, Aider.
                  [Continue]
3. [Permission]   "All Seeing Eye reads files in: ~/.claude, ~/.codex, ..."
                  [Grant access] (macOS may prompt full disk access for some paths)
4. [Initial scan] Progress bar. ~5 sec on a 200-component setup.
5. [Tour]         Three coachmarks: Inventory, Map, Cmd-K. Skippable.
6. [You're in]    Lands on Inventory.
```

No account creation. No telemetry consent screen unless we ship telemetry (we don't, MVP).

## Empty states

Each view has a deliberate empty state.

- **Inventory empty after scan**: "We didn't find any agentic tools on this machine. [Pick a tool]" with a list of supported tools and instructions.
- **Inventory filtered to nothing**: "No matches. Clear filters."
- **Map with one node**: "Add another tool to see relationships emerge."
- **Editor with no selection**: a marketing-grade hero pulling from Inventory, "Pick a component to start."
- **Health with nothing wrong**: a calm green check, "All systems good."

## Modals

We minimise modals. Used only for:

- Destructive confirmations (delete a component on disk).
- First-run permission prompts where the OS doesn't gate it itself.
- Bundle export configuration.

Everything else uses inline panels or the right-side slide-in.

## Notifications

Bottom-right toasts, max two stacked. Auto-dismiss after 4 s, or sticky if it represents a problem (parse error, save failure). Click toast to focus the offending component in Inventory.

## Keyboard map (defaults)

| Shortcut | Action |
|----------|--------|
| Cmd-K | Command palette |
| Cmd-1 / 2 / 3 | Switch to Inventory / Map / Editor |
| Cmd-S | Save in Editor |
| Cmd-Z / Cmd-Shift-Z | Undo / redo (Editor) |
| Cmd-F | Focus search in Inventory |
| Space | Quick Look on selected row |
| Enter | Open in Editor |
| Esc | Close panel / clear selection |
| Cmd-, | Settings |
| Cmd-N | New component (context-sensitive, picks a sensible default tool/type) |
| / | Quick filter chip entry |
| Tab / Shift-Tab | Cycle filter chips |
| g i | Go to Inventory |
| g m | Go to Map |
| g h | Go to Health |
| g d | Go to Drift |

Every shortcut is rebindable.

## Accessibility

- All actions reachable from keyboard.
- ARIA labels on every interactive element.
- Color is never the only carrier of meaning (icons + text accompany badges).
- `prefers-reduced-motion` honored: animation reduced to fades; no springy easing.
- Minimum tap target 44px; focus rings always visible.
- Screen reader: each view has a logical landmark structure (banner / nav / main / complementary).

## Errors and recovery

Errors are first-class UI, not modals.

```
+--------------------------------------------------------------+
| ! Parse error in skill "promo-video"                         |
|   YAML frontmatter line 3: missing colon after 'description' |
|   [open editor] [view raw] [ignore for now]                  |
+--------------------------------------------------------------+
```

The component still appears in Inventory with a `!` badge so it's not invisible.

## Multi-window

A second window is allowed and useful: keep Map in one window, Editor in another. State syncs via the IPC bus.

## Empty + dense responsiveness

Views collapse gracefully on a 13" screen. Sidebar collapses to icon-only at < 1280px width. Inventory rows stay; columns hide in priority order.
