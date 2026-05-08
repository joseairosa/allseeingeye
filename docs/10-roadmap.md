# 10 - Roadmap

Phasing. The absolute scope of MVP is the union of features tagged "MVP" below. v1 and v2 add scope; we never re-litigate MVP.

## Principles

1. **Ship something useful in one focused increment.** MVP indexes and edits Claude Code, Codex, Cursor, Antigravity. Anything else is opportunistic.
2. **No half-finished features.** Each release ships features that work end-to-end, not 80%.
3. **Cut scope, not quality.** If a feature is not ready, push to the next release. Don't ship it broken.
4. **Real users every release.** José plus a small handful of trusted devs use each version before public release.

## MVP - "the inventory and the editor"

**Goal.** A user with Claude Code, Codex, Cursor, and Antigravity installed can open All Seeing Eye, see every component on their machine within 5 seconds, and edit any of them with confidence that the host tool will pick up the change.

**Scope (in).**
- Tool detection for: Claude Code, Codex, Cursor, Antigravity.
- Component types: Settings, Memory, Rule, Skill, Command, Agent, MCP, Hook, Plugin, Marketplace.
- Inventory view (grid + filters + FTS search).
- Quick Look panel.
- Editor (form + raw, save, discard, validate).
- Atomic writes; live re-index on file events.
- Cmd-K command palette (basic).
- Dark theme, light theme, comfortable density, compact density.
- Diagnostics panel.
- Onboarding flow.
- macOS arm64 + x64 builds; Linux x64; Windows x64.
- Code-signed and notarised macOS builds.
- Auto-update (Tauri updater).

**Scope (out).**
- Map view (deferred to v1).
- Drift detection (v1).
- Health probing (v1).
- Bundle export / import / convert (v1).
- Cline, Goose, Continue, Aider, Copilot, Zed, Roo, Kilo, Augment (v1+).
- Telemetry (v1+).
- Multi-window (v1).
- Plugin API for community-added tool descriptors (v2).
- Cloud sync (v2+).

**Acceptance for "MVP done".**

| Check | Pass criteria |
|-------|---------------|
| Cold start | Under 2 s on M-class Mac with 200 components. |
| Indexing accuracy | 95% of canonical components from the four tools parse without errors on a representative dataset. |
| Edit safety | 1,000 random saves produce zero corrupted files in soak test. |
| Memory | Under 200 MB idle; under 400 MB during full re-index. |
| Binary size | Under 30 MB on macOS. |
| Crash-free sessions | > 99.5% in dogfood week. |
| Privacy | No network calls except auto-update channel; verified by network sandbox test. |

**Estimated build effort.** 8-12 weeks for one full-time engineer; faster with parallel UI + backend pairing.

## v1 - "graph, drift, health, and four more tools"

**Goal.** Make the cross-tool insight features the user couldn't get anywhere else: see the graph of relationships, find drift, probe MCP health.

**Scope.**
- Map view with force layout, clustering, filtering, edge inspection.
- Drift detection between equivalent memory and rule files; merge wizard.
- Health probing for MCP servers (opt-in per server).
- Usage analytics from session/history mining (Cold report).
- Tool support: Cline, Goose, Continue, Aider.
- Bundle export (Claude Code plugin, Goose recipe, Cursor rules pack, generic).
- Bundle import.
- Convert / promote across tools.
- Multi-window.
- High-contrast themes.
- Dyslexia font option.
- Telemetry pipeline (opt-in, anonymous).

**Acceptance for v1 done.**
- Drift correctly identifies 90% of intentionally equivalent memory files in a fixture set.
- Convert a Claude Code skill to an Antigravity skill, install in Antigravity, run it - the same behaviour observed.
- Map renders 1,000-node graph at 60 fps.
- MCP health probe runs concurrently across 20 servers without degrading UI.

**Estimated effort.** 8-12 weeks past MVP.

## v2 - "ecosystem and team"

**Goal.** Become useful at the team scale. Community-extensible. Selectively cloud-aware.

**Scope.**
- Tool support: GitHub Copilot, Zed, Roo Code (read-only support for migration), Kilo Code, JetBrains Junie, Sourcegraph Amp, Augment.
- Plugin API for community-contributed tool descriptors and conversion transformers.
- Team workspace concept: a shared bundle definition that team members can sync to their machines via a shared Git repo.
- Cloud sync for sidecar metadata (tags, pins, notes) only - never component bodies.
- Sign / verify exported bundles.
- A community marketplace browser (read-only initially).

**Out of scope, still.**
- Running models or invoking host tools.
- Hosted multi-user collaboration.
- A built-in chat / agent runtime.

## v3+ "open ended"

Subject to where the agentic ecosystem goes. Possible directions:

- Time-travel: snapshot the entire agentic loadout, restore on any machine.
- Audit log of agent activity across tools.
- Cross-tool tool-call replay for debugging.
- Visual builder for skills and agents.
- LLM-assisted refactor and translation.

## Release cadence

- **Daily** during active development: internal dogfood builds.
- **Weekly** beta during MVP push.
- **Monthly** stable releases post-MVP.

## Risks affecting roadmap

See `11-risks.md`. The big ones that could shift dates:
- Tool format churn (Claude Code, Antigravity in particular ship rapidly).
- Tauri 2.x bug surface on Windows.
- Codesigning approval delays.
- A tool we plan to support changes its data model in a way that requires major reparse work.

## Definition of done (per release)

- All acceptance criteria for the release pass.
- Storybook covers all custom components.
- E2E suite passes on macOS, Linux, Windows.
- Performance budgets met (cold start, idle CPU, memory).
- Privacy audit clean: zero network calls except update channel and (post-v1) opt-in telemetry.
- Documentation in `docs/` is current; CHANGELOG updated.
- A 50-component fixture project test-passes for every supported tool.
