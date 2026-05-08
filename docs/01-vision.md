# 01 - Vision

## One-liner

> All Seeing Eye is a beautiful native desktop app that gives you a single, live, searchable view of every agentic component on your machine - and lets you create, edit, share, and toggle them without leaving the app.

## Problem statement

The agentic dev stack has exploded. A typical developer in 2026 has:

- One or more CLI agents installed (Claude Code, Codex, Goose, Aider).
- One or more agentic IDEs (Cursor, Antigravity, Windsurf, Zed).
- One or more VS Code-based agentic extensions (Cline, Continue, Roo Code, Kilo Code, Copilot).
- Dozens of MCP servers configured across those tools, often duplicated.
- Hundreds of skills, agents, commands, hooks, and rules - scattered across `~/.claude/`, `~/.codex/`, `~/.cursor/`, `~/.gemini/antigravity/`, project-local `.claude/`, `.cursor/rules/`, `.agents/`, and so on.
- Multiple memory files (`CLAUDE.md`, `AGENTS.md`, `GEMINI.md`, `.cursorrules`, `copilot-instructions.md`) that drift and contradict.
- Plugin/marketplace caches that grow without ever being inspected.

The pain:

1. **Discovery** - "What skills do I even have? Which one fires for X?"
2. **Drift** - "Did I update the rule in Claude Code or Cursor? Or both? Or neither?"
3. **Quality** - "Which of my 40 MCP servers actually work today? Which are noisy? Which are unused?"
4. **Sharing** - "I want to send this skill to a teammate" - currently means tarring a folder and explaining the install path.
5. **Health** - "What changed in my setup last week and why is the agent suddenly slower?"

## Vision

A single Tauri-based macOS / Windows / Linux app, named **All Seeing Eye**, that:

- **Indexes everything**: every agentic primitive across every supported tool, in real time, by watching the file system.
- **Visualizes it**: a graph + grid + list view that shows components, their tool of origin, their relationships, their freshness, their usage.
- **Edits in place**: a first-class editor for skills, agents, commands, rules, hooks, MCP servers - with schema-aware validation per tool.
- **Compares across tools**: a single canonical "rule" or "memory" file that surfaces drift between Claude Code's CLAUDE.md and Cursor's .cursorrules.
- **Promotes and exports**: turn a Claude Code skill into a Codex skill, an Antigravity skill, or a Goose recipe with one click. Bundle a set into a plugin manifest.
- **Health and analytics**: which MCP servers errored last week, which skills you actually used, which rules never matched.
- **Looks gorgeous**: dark-first, glassy, motion-rich, dense-but-airy. The kind of tool you want to leave open.

## Target users

Three concentric rings.

### Ring 1 - Power-user solo dev (José)

Runs 5+ agentic tools. Has 50+ skills, 20+ MCP servers, project + global rules in three different formats. Wants control + visibility. Will pay for it. **Primary persona.**

### Ring 2 - Tech leads on small teams

Need to standardize agentic config across 3-10 engineers. Today they hand around bash snippets and Notion docs. Want a way to "freeze" a known-good loadout and distribute.

### Ring 3 - Tooling/DevEx orgs

Internal platform team that publishes a curated marketplace of agents and skills for their company. Wants admin features, audit logs, central allow/deny lists.

MVP serves Ring 1. v1 adds Ring 2. v2 considers Ring 3.

## Value props (in priority order)

1. **One pane of glass** for every agentic primitive. No more "where is that skill defined again?"
2. **Live drift detection** between equivalent components across tools.
3. **Native editor** with per-tool validation - faster than Code/Cursor for these specific files because of schema awareness.
4. **Health and usage** that no individual tool exposes (cross-tool MCP health, unused skills, hot vs cold agents).
5. **Sharing** - export selectively, import safely, turn ad-hoc setups into reproducible bundles.
6. **Beautiful** - because devs spend hours per day in their tooling and aesthetics matter.

## Non-goals (explicitly)

- **Not a chat client.** All Seeing Eye does not run agents. It manages their configuration. (We may show transcripts read-only, but never invoke models.)
- **Not a replacement for any tool.** Claude Code stays Claude Code; we just index and edit its files.
- **Not a cloud product at MVP.** Local-first. No accounts required to use it. Sync is v2 territory.
- **Not Windows-only or Mac-only.** Cross-platform from day one (Tauri makes this cheap).
- **No telemetry without explicit opt-in.** This is a tool that reads your secrets and rules - trust is the product.

## Success criteria

MVP is a success if:

- Surveyed power-users can answer "show me every skill I have, across every tool" in under 5 seconds, in the app.
- 90% of components from 5+ tools are correctly parsed and displayed without manual configuration.
- A user can edit a skill in All Seeing Eye and the change is reflected in the host tool's next session, with no manual reload.
- App startup is under 2s on a 5-year-old MacBook Pro with 200+ components indexed.
- Default install footprint is under 30 MB.

## North star metric

**Components under management** - count of distinct agentic primitives indexed by All Seeing Eye, across the user's machine. Tracks both adoption (more tools indexed) and engagement (more skills/agents created). One number that captures whether the product is working.

## Naming

`All Seeing Eye` - already chosen. The eye both **sees** (inventories, observes) and **understands** (parses, validates, relates). Logo direction: a stylised geometric eye with concentric inner rings, evoking surveillance, awareness, and clarity. See `07-visual-design.md`.
