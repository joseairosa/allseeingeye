# All Seeing Eye

Desktop control plane for agentic tooling. See `docs/` for the full spec; `design/` for the static prototype.

## Status

Phase 0.1 scaffold landed. Build phases tracked in TaskList; roadmap in `docs/10-roadmap.md`.

## Stack

- Tauri 2 (Rust 1.81+)
- React 19 + Vite 6 + TypeScript strict
- SQLite + FTS5 via rusqlite (Phase 1.2)
- pnpm + Cargo workspaces

## Layout

```
apps/desktop/        Tauri app (frontend in src/, Rust backend in src-tauri/)
packages/shared-types/  TS types, eventually generated from Rust
packages/ui/         Storybook (Phase 0.4)
docs/                Specification (00-11)
design/              Static design prototype (HTML/CSS/JS)
```

## Develop

```bash
pnpm install
pnpm tauri:dev
```

Runtime requirements: Node >=20, pnpm >=10, Rust >=1.81.0, Xcode CLI tools (macOS).
