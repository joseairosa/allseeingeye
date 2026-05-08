# All Seeing Eye - Specification Index

A desktop app to discover, visualize, and manage every agentic component scattered across a developer's machine: agents, skills, commands, MCP servers, hooks, rules, memory, plugins, marketplaces, sessions, tasks, output styles, and more.

## Why this exists

A modern dev runs Claude Code, Codex, Cursor, Antigravity, Cline, Goose, Aider, Continue, Windsurf, Zed, Copilot, and others in parallel. Each tool stores agentic primitives in its own directory tree, with its own format. There is no single inventory, no single editor, no cross-tool search, no health view. Components rot silently: stale skills, broken MCP servers, contradictory rules, abandoned plugins.

All Seeing Eye is the inventory + editor + observatory + control plane.

## Spec documents

| # | Doc | Purpose |
|---|-----|---------|
| 00 | This file | Index and reading order |
| 01 | [Vision](./01-vision.md) | Product vision, target users, value prop, non-goals |
| 02 | [PRD](./02-prd.md) | Functional and non-functional requirements |
| 03 | [Component Model](./03-component-model.md) | Unified taxonomy of every agentic primitive |
| 04 | [Data Sources](./04-data-sources.md) | Per-tool file paths, formats, parsing strategy |
| 05 | [Data Architecture](./05-data-architecture.md) | Local index, sync, watch, conflict resolution |
| 06 | [UX Design](./06-ux-design.md) | Information architecture, navigation, key screens |
| 07 | [Visual Design](./07-visual-design.md) | Look and feel, design language, motion |
| 08 | [Tech Architecture](./08-tech-architecture.md) | Tauri vs Electron, frontend stack, IPC, security |
| 09 | [Features](./09-features.md) | Per-feature implementation notes |
| 10 | [Roadmap](./10-roadmap.md) | MVP / v1 / v2 phasing |
| 11 | [Risks](./11-risks.md) | Technical, security, UX risks and mitigations |

## Reading order

1. `01-vision.md` first - if the vision is wrong, nothing else matters.
2. `02-prd.md` to confirm scope.
3. `03-component-model.md` - the conceptual heart of the product.
4. `04-data-sources.md` and `05-data-architecture.md` together - they answer "where does the data live and how do we reflect it accurately."
5. `06-ux-design.md` and `07-visual-design.md` together.
6. `08` through `11` are implementation and planning artefacts; safe to skim until build phase.

## Status

PRE-BUILD. Specs only. No code yet. Decisions captured in spec docs are not final until José signs off.

## Tools surveyed (research basis)

Claude Code, Codex (OpenAI), Cursor, Google Antigravity, Cline, Continue.dev, Windsurf (Codeium / Cognition), Zed, Aider, Goose (Block / AAIF), GitHub Copilot, Roo Code, Kilo Code, Augment, JetBrains Junie, Sourcegraph Amp, Model Context Protocol (MCP). See `04-data-sources.md` for per-tool detail.
