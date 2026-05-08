# 08 - Technical Architecture

How we build it. Decisions are stated with their reasoning and explicit alternatives considered.

## Top-level decision: Tauri

**Pick: Tauri 2.x.**

| Criterion | Tauri 2.x | Electron 32 | Wails | Native (SwiftUI / WPF / GTK) |
|-----------|-----------|-------------|-------|-------------------------------|
| Binary size | 5-15 MB | 80-150 MB | 7-20 MB | small |
| Memory footprint | low (system webview) | high (Chromium + Node) | low | lowest |
| Cross-platform | Yes - mac/win/linux | Yes | Yes | No |
| Frontend stack | any web | any web | any web | platform-specific |
| Backend language | Rust | Node | Go | platform-specific |
| Auto-update story | ships with v2 | mature | OK | DIY |
| File watcher quality | excellent (`notify`) | OK (chokidar) | OK | excellent |
| Mature SQLite story | excellent (`rusqlite`) | OK (`better-sqlite3`) | OK | excellent |
| Plugin ecosystem | small but growing | huge | small | n/a |
| Codesigning, notarisation | supported | mature | supported | best |

Reasoning:
- Binary and memory footprint are non-functional MVP requirements (under 30 MB, under 200 MB idle). Electron breaks both.
- We're a **read-many-write-careful** app over the user's home directory; Rust's atomic-write story, file watching, and zero-cost SQLite are decisive.
- Cross-platform from day one rules out native.
- Tauri 2's mobile capability is irrelevant for us at MVP but doesn't hurt.

Alternatives we considered and rejected:
- **Electron**: faster initial DX given Node ecosystem, but the 80 MB+ binary kills our footprint goal and the GC pauses on large file events would need engineering anyway.
- **Wails**: Go has weaker SQLite + watcher ergonomics, smaller community, fewer learning resources.
- **Native** (one app per platform): too expensive for a small team.

Risks of Tauri:
- WebView2 quirks on Windows; tested early.
- Smaller plugin ecosystem - we'll write some things ourselves.
- macOS WKWebView has known quirks with very long lists; we virtualise list rendering from day one.

## Frontend stack

**React 19 + Vite + TypeScript strict + Tailwind v4 + Radix Primitives + cmdk + Zustand + TanStack Query + Monaco**.

| Choice | Reasoning | Alternative |
|--------|-----------|-------------|
| React 19 | Server components irrelevant for desktop, but the concurrent rendering and `useTransition` give us responsive search and view switches. Mature ecosystem. | Solid (lighter, but smaller ecosystem); Svelte 5 (smaller; fewer libs we need). |
| Vite | Fast dev, works perfectly with Tauri. | None worth considering. |
| TypeScript strict | Non-negotiable for an editor over user-critical files. | n/a |
| Tailwind v4 | Token-driven design system maps directly onto our colour and spacing tokens. | CSS modules (more boilerplate). |
| Radix Primitives | Accessibility heavy lifting. | Reach UI (less maintained). |
| cmdk | Industry-standard command palette. | DIY (don't reinvent). |
| Zustand | Tiny, fast, no provider hell. | Jotai (atom thrash for our shape); Redux (too much). |
| TanStack Query | We treat IPC commands as async resources; gives us caching, retries, suspense. | SWR. |
| Monaco | Best-in-class text editor; we need YAML/JSON/Markdown smarts. | CodeMirror 6 - lighter, viable alternative; Monaco wins on built-in YAML language server. |
| Sigma.js | Mature graph rendering for the Map view. | `react-force-graph-2d` (also good). |

Bundle target: under 1 MB gzip for the SPA chunk.

## Backend stack (Rust)

| Concern | Crate | Notes |
|---------|-------|-------|
| Tauri | `tauri` v2 | Window mgmt, IPC, signing, updater. |
| File watching | `notify` v6 | Cross-platform; we wrap with our own debounce. |
| SQLite | `rusqlite` | Single dependency; bundled libsqlite3 to avoid system version drift. |
| Async runtime | `tokio` | Multi-threaded; for parser fan-out. |
| Serialisation | `serde`, `serde_json`, `serde_yaml`, `toml` | Standard. |
| YAML frontmatter | DIY via `serde_yaml` + split | gray-matter-style. |
| Hashing | `sha2` | sha256 for content hashing. |
| MCP probing | `tokio-process`, `reqwest`, `eventsource-client` | Per transport. |
| JSON Schema | `jsonschema` | Validation. |
| TS bindings | `ts-rs` or `specta` | Generate TS types from Rust types for IPC. |

## IPC contract

A single Tauri command per high-level operation, plus event channels for live updates.

Commands (read-only):
```ts
listTools(): Tool[]
listComponents(filter: ComponentFilter): Component[]
getComponent(id: string): Component
search(query: SearchQuery): SearchResult[]
getRelations(id: string): Relation[]
getDriftPairs(): DriftPair[]
getHealthSummary(): HealthSummary
```

Commands (mutating):
```ts
saveComponent(id: string, content: string | StructuredEdit): SaveResult
toggleComponent(id: string, enabled: boolean): void
addTag(id: string, tag: string): void
removeTag(id: string, tag: string): void
pinComponent(id: string): void
exportBundle(ids: string[], opts: ExportOptions): ExportArtifact
importBundle(path: string, opts: ImportOptions): ImportResult
probeMcp(id: string): McpHealthResult
```

Events (server -> client, via `Window::emit`):
```ts
'component:upserted'    -> { id, hash, mtime }
'component:deleted'     -> { id }
'component:parseError'  -> { id, error }
'tool:detected'         -> { toolId }
'health:probe'          -> { id, status, latencyMs }
'index:rebuilt'         -> { count }
'drift:updated'         -> { pairs: number }
```

The TanStack Query cache invalidates on event. Optimistic updates for save/toggle/tag operations; rollback on backend error.

## Threading model

```
Main thread (Tauri runtime)
  +-- UI thread (WebView, React)
  +-- Tokio multi-threaded runtime
        +-- File watcher coalescer (1 task)
        +-- Index writer (1 task, owns SQLite write conn)
        +-- Parser pool (N = num_cpus)
        +-- MCP probe pool (4 tasks)
        +-- Relation recomputer (1 task, debounced)
```

Hot paths never block on disk I/O on the main runtime thread. SQLite reads use a connection pool.

## Schema-aware editor

The Editor's left pane (form view) is generated from a JSON Schema per component type. The right pane (raw) is Monaco. Edits in either pane round-trip through a single source-of-truth `EditState` in Zustand:

```
   form change ──┐              ┌── raw change
                 v              v
            +------ EditState ------+
            |   parsed AST          |
            |   raw text            |
            |   dirty flags         |
            +-----------+-----------+
                        |
                        v
                  serializer (per format)
                        |
                        v
                   on save: atomic write
```

Both panes show the same data; whichever the user touched last is "primary" for save. Conflict between form and raw edits within the same session is impossible because the form derives from the parsed AST and edits to raw re-parse on idle.

## File-system safety

- Writes only to paths under registered tool roots or open project roots.
- Refuses to follow symlinks that escape registered roots.
- Refuses to write inside `.git/`, `node_modules/`, `target/`, `dist/`, `.venv/`, `__pycache__/`.
- Always atomic write. Never partial.
- File `O_EXCL`-creates are used for new components to avoid clobbering.

## Background work

- File watcher runs from app start.
- MCP health probe is opt-in per server, with a global default (off in MVP).
- Relation recomputation runs on a 1-second debounced trigger after batches of file events.
- "Cold" usage stats are computed lazily when the Health view is opened (or on a cron the user enables).

## Auto-update

Tauri Updater plugin. Signed releases. Mac/Win/Linux. Checks daily, prompts the user for release notes confirmation; never installs without consent. The user can pin a version or disable.

## Telemetry (off by default)

When (and only when) the user opts in:
- Anonymous install id.
- Counts of components by type and tool (no names, no content).
- Parse error counts by tool/format.
- App startup duration.

Implementation: `reqwest` POST to a metrics endpoint. We only ship this in v1; MVP has no telemetry pipeline.

## Build and release

| Concern | Approach |
|---------|----------|
| CI | GitHub Actions matrix: macOS arm64, macOS x64, Linux x64, Windows x64. |
| Releases | Tag-driven; CI builds, signs, notarises, pushes to GitHub Releases + update channel. |
| Code signing | Apple Developer ID + notarisation for macOS; EV cert for Windows. |
| Linux | AppImage + .deb + .rpm. |
| Binary diff updates | Tauri's built-in delta updater (small downloads). |

## Testing strategy

| Layer | Approach |
|-------|----------|
| Rust unit | `cargo test` per crate; parsers are pure functions. |
| Rust integration | Spin up a tmp dir with synthetic tool layouts; assert index state and file writes. |
| TS unit | Vitest. |
| Component (storybook) | Chromatic visual regression. |
| E2E | Playwright on the built Tauri binary, on each platform in CI. Smoke + critical-path. |
| Schema tests | For each per-tool schema, fixtures of valid + invalid examples. |
| Property | `proptest` for the parser/serializer round-trip - parsing then serialising must produce byte-identical output. |

CI gates: typecheck, lint, unit, integration, build, sign on main. PRs only need typecheck + lint + unit; full matrix on tagged release.

## Observability (in-app only)

`Cmd-,` -> "Diagnostics" panel:
- Last 100 file events.
- Last 50 parse errors with file paths.
- Index stats (rows, FTS size, disk size).
- Watcher status per tool.
- "Copy diagnostics" produces a sanitised report (no file content).

## Plugin / extension story (the app's own)

Future. We don't ship an extension API in MVP. Tools we recognise are coded in. v2 might expose a plugin point for the community to add new tool descriptors without a release - but only if the community demand exists.

## Dependency hygiene

- `cargo-deny` and `cargo-audit` in CI.
- Renovate-like PR bot for npm; security advisories block merge.
- We re-evaluate every dependency at v1 against "do we need this".

## Known limits at MVP

- Watching very large project monorepos (>50k files) can saturate inotify on Linux. Mitigation: scope project watch to the relevant subdirs (`.claude/`, `.cursor/`, `.agents/`, `.github/`).
- Tauri's file picker on Linux is OS-dependent and inconsistent; we use it sparingly.
- Codesigning on Windows with EV certs requires a dongle or hardware token; budget for it.

## Repository layout

```
all-seeing-eye/
+-- apps/
|   +-- desktop/                 (Tauri app)
|       +-- src-tauri/           (Rust)
|       |   +-- src/
|       |   |   +-- main.rs
|       |   |   +-- registry/    (tool descriptors)
|       |   |   +-- watcher/
|       |   |   +-- parser/
|       |   |   +-- index/
|       |   |   +-- ipc/
|       |   |   +-- mcp/
|       |   +-- schemas/         (per-tool JSON schemas)
|       |   +-- Cargo.toml
|       +-- src/                 (React)
|           +-- views/
|           +-- components/
|           +-- ipc/
|           +-- store/
|           +-- styles/
+-- packages/
|   +-- shared-types/            (TS bindings generated from Rust)
|   +-- ui/                      (Storybook + design tokens)
+-- docs/                        (this folder)
+-- .github/
+-- README.md
```

Single repo (monorepo). pnpm + Cargo workspaces.
