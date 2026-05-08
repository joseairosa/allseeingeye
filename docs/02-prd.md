# 02 - Product Requirements

Requirements are stated in MoSCoW form: **Must**, **Should**, **Could**, **Won't (this version)**. MVP scope is the union of "Must" rows.

## A. Discovery and indexing

| ID | Pri | Requirement |
|----|-----|-------------|
| A1 | Must | Auto-detect installed tools at first launch by probing canonical paths (see `04-data-sources.md`). |
| A2 | Must | Live file-watching: changes to any indexed component reflect in the UI within 2 seconds. |
| A3 | Must | Manual "scan" trigger to re-index everything from scratch. |
| A4 | Must | Support enable/disable of individual tools (no point watching `~/.codex/` if user doesn't have Codex). |
| A5 | Should | Support scanning project directories that the user opens (project-scoped components like `.claude/`, `.cursor/rules/`). |
| A6 | Should | Detect newly-installed tools without restart. |
| A7 | Could | Suggest installable tools the user hasn't yet adopted, based on what's missing. |
| A8 | Won't | Cloud-side indexing. Local only at MVP. |

## B. Component types supported

See `03-component-model.md` for the full taxonomy. MVP must cover at minimum:

| ID | Pri | Component | Notes |
|----|-----|-----------|-------|
| B1 | Must | Settings / config | Per-tool root config files |
| B2 | Must | Memory / instructions | CLAUDE.md, AGENTS.md, GEMINI.md, .cursorrules, copilot-instructions.md |
| B3 | Must | Rules | Path-scoped instruction files with frontmatter |
| B4 | Must | Skills | SKILL.md folders + scripts/references/assets |
| B5 | Must | Commands / workflows | Slash-invoked saved prompts |
| B6 | Must | Agents / subagents | Specialised AI workers with own system prompt |
| B7 | Must | MCP servers | stdio, SSE, HTTP transports |
| B8 | Must | Hooks | Event-triggered scripts/prompts |
| B9 | Must | Plugins | Bundles + their marketplaces |
| B10 | Should | Sessions / history | Read-only viewer with search |
| B11 | Should | Tasks / TODOs | Cross-tool unified view |
| B12 | Should | Output styles / personalities | Per-tool tone profiles |
| B13 | Could | Statuslines | Per-tool status display config |
| B14 | Could | Permissions / sandbox profiles | Allow/deny lists, sandbox modes |
| B15 | Could | Auth / API keys | Read-only redacted view (never decrypt) |

## C. Editor

| ID | Pri | Requirement |
|----|-----|-------------|
| C1 | Must | Native Monaco-based editor for any text-format component. |
| C2 | Must | YAML / TOML / JSON / Markdown frontmatter is parsed into a form view; users can edit either form or raw. |
| C3 | Must | Per-tool schema validation with inline errors. |
| C4 | Must | Save writes back to the original file with atomic write (temp + rename). |
| C5 | Must | Undo / redo across the session, plus a session-level "discard" that reverts since the file was opened. |
| C6 | Should | Diff view against the on-disk version, useful when the file changed externally. |
| C7 | Should | Multi-cursor, find-replace, syntax highlighting. |
| C8 | Could | AI-assisted refactor (e.g., "rewrite this Cursor rule as a Claude Code rule"). v2. |
| C9 | Won't | Run/preview the component live. Out of scope - we don't invoke models. |

## D. Cross-tool features

| ID | Pri | Requirement |
|----|-----|-------------|
| D1 | Must | Unified search across all components: filter by tool, type, name, content, freshness. |
| D2 | Must | Drift detection: a "Memory drift" view that pairs equivalent files across tools (CLAUDE.md vs .cursorrules vs GEMINI.md) and highlights divergence. |
| D3 | Must | Bulk enable / disable of components per tool (where the underlying tool supports it). |
| D4 | Should | Promote / convert: turn a Claude Code skill into a Codex skill or an Antigravity skill (best-effort transform). |
| D5 | Should | Bundle: select N components and export as a Claude Code plugin / Goose recipe / Cursor rules pack. |
| D6 | Could | Import: drop a plugin / recipe URL or zip and install into the matching host tool. |
| D7 | Could | Merge: a wizard that resolves drift between two memory files into a single canonical version that propagates. |

## E. Health and analytics

| ID | Pri | Requirement |
|----|-----|-------------|
| E1 | Must | MCP server health: ping each configured server (transport-appropriate), surface up / degraded / down. |
| E2 | Should | Usage stats: parse session histories where available, count invocations per skill / command / agent / MCP, last-used timestamps. |
| E3 | Should | "Cold" report: components not used in N days, suggested for removal. |
| E4 | Could | Token consumption rollup, where the host tool emits it. |
| E5 | Won't | Real-time agent loop monitoring. That's the host tool's job. |

## F. UX

| ID | Pri | Requirement |
|----|-----|-------------|
| F1 | Must | Three primary views: **Inventory** (grid/list), **Map** (force-directed graph), **Editor** (focused single-component). |
| F2 | Must | Global keyboard-driven command palette (Cmd-K). |
| F3 | Must | Dark-first, with a light mode. Both polished. |
| F4 | Must | All actions reachable from keyboard. |
| F5 | Should | Custom workspace layouts (multi-pane). |
| F6 | Should | Quick-look preview on hover / focus, no click required. |
| F7 | Could | Pinning, tagging, custom collections. |

## G. Sharing

| ID | Pri | Requirement |
|----|-----|-------------|
| G1 | Should | Export single component or bundle to a sharable archive. |
| G2 | Should | Generate Markdown documentation for a bundle on export. |
| G3 | Could | Publish to a community registry (out of scope for MVP). |
| G4 | Could | Sign / verify exported bundles. |

## H. Non-functional

| ID | Pri | Requirement |
|----|-----|-------------|
| H1 | Must | Cold start under 2s on a baseline machine (M1 Air, 8GB) with 200 components. |
| H2 | Must | Idle CPU under 1%; idle memory under 200 MB. |
| H3 | Must | App binary under 30 MB on macOS. |
| H4 | Must | Works offline. No network required for core features. |
| H5 | Must | Never reads files outside the configured roots. |
| H6 | Must | Never transmits content of memory / rules / sessions / API keys without explicit opt-in. |
| H7 | Must | All disk writes are atomic and survive a crash mid-write without corrupting the user's tool config. |
| H8 | Should | Auto-update with signed releases; user can disable. |
| H9 | Should | Accessibility: WCAG 2.1 AA where applicable; keyboard, screen reader, reduced-motion respected. |
| H10 | Could | i18n scaffolding ready, English-only at MVP. |

## I. Security and privacy

| ID | Pri | Requirement |
|----|-----|-------------|
| I1 | Must | API keys / tokens stored in tool configs are masked in the UI by default; reveal requires a deliberate click. |
| I2 | Must | App never copies secrets to clipboard, log files, or telemetry. |
| I3 | Must | Telemetry, if any, is opt-in, anonymised, and excludes file content. |
| I4 | Must | Code-signing on macOS and Windows. |
| I5 | Should | A "panic" mode that detaches all watchers and clears in-memory caches (useful for screensharing). |

## Out of scope (this version)

- Cloud sync of components across machines.
- Multi-user / team accounts.
- A built-in chat interface.
- Running models or shelling out to host tools to invoke skills.
- Mobile companion app.

These return as future considerations in `10-roadmap.md`.
