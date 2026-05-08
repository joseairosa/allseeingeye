# Desktop tests

Three test layers cover the desktop app. Each runs independently.

## 1. Vitest (frontend unit + integration)

Run from the repo root or `apps/desktop/`:

```bash
pnpm --filter @aseye/desktop test
```

What it covers:

- IPC wrapper logic (`src/ipc/index.test.ts`)
- Store reducers (`src/store/*.test.ts`)
- Pure helpers in `src/lib/`

What it does **not** cover: anything that requires a DOM in motion, IPC
round-trips against a real backend, or visual regression.

## 2. Playwright (E2E)

Run from `apps/desktop/`:

```bash
pnpm exec playwright install --with-deps chromium
pnpm test:e2e
```

What it covers (`tests/e2e/`):

- `inventory-search.spec.ts` — typing into the search field filters rows.
- `palette-open.spec.ts` — Cmd-K opens the palette and Enter fires actions.
- `quicklook-open.spec.ts` — clicking a row opens the Quick Look panel.
- `theme-toggle.spec.ts` — the theme button toggles `body.light`.
- `panic-mode.spec.ts` — Cmd-Shift-. toggles `body.panic`.

How it works:

- Tests run against `pnpm --filter @aseye/desktop dev` (Vite, not Tauri).
- `tests/e2e/fixtures/mockTauri.ts` patches `window.__TAURI_INTERNALS__`
  via `addInitScript` so `invoke()` resolves against an in-memory data
  set. This keeps E2E hermetic and CI-runnable without a real Tauri
  build.
- Tracing on first retry, screenshot on failure. Reports land in
  `playwright-report/` (gitignored).

What it does **not** cover:

- WebKit-specific (WKWebView) regressions — those need a packaged Tauri
  smoke test, which is out of MVP scope.
- Real OS keychain or auto-update flows — those have separate, opt-in
  cargo-side suites.

## 3. Cargo tests (Rust)

Run from `apps/desktop/src-tauri/` via the project's Rust shim:

```bash
../../../scripts/with-rust.sh cargo test          # default suite
../../../scripts/with-rust.sh cargo test --release -- --ignored   # soak suite
../../../scripts/with-rust.sh cargo bench         # micro-benchmarks
```

What the **default** suite covers:

- Unit tests for parsers, file-system safety, registry, validator,
  security scanner, IPC commands.
- Integration tests for atomic writes (parent fsync, escape detection,
  forbidden segments).
- Round-trip property tests (`proptest`, 256 cases each):
  - Markdown frontmatter dict + body — structural equality.
  - JSON parse → re-serialise → parse — value equality.
  - TOML parse → JSON projection → re-parse — structural equality.
- Property tests for the SQLite index (`proptest`, 64 cases each):
  - Re-upsert of identical content reports `Unchanged`.
  - Sequential upserts to the same path report the latest hash and body.

### `#[ignore]` soak tests

Long-running stress tests are tagged `#[ignore]` so the default suite
stays fast. Run them explicitly via `cargo test --release -- --ignored`
or via the targeted name:

| Test | Purpose |
|------|---------|
| `fs::atomic::tests::soak_atomic_writes` | 1 000 sequential writes → final content matches. |
| `fs::atomic::tests::soak_atomic_writes_concurrent` | 4 OS threads × 1 000 writes to distinct paths; no temp-file debris. |
| `fs::atomic::tests::soak_safe_atomic_write_under_changing_roots` | Rotates trusted roots while writes are in flight; never crashes, never writes outside roots. |
| `index::soak::soak_ten_thousand_row_insert` | 10 000-row insert; FTS5 query latency p50 < 50 ms across 100 random queries. |

### `cargo bench`

`benches/parser_bench.rs` and `benches/classify_bench.rs` track the
per-input parser dispatch and the path classifier. They are NOT part of
`cargo test` — `harness = false` in `Cargo.toml` keeps them isolated.

## CI matrix

- **Every PR**: vitest + Playwright (Linux only) + cargo test (default).
- **Push to `main` and weekly cron**: ignored cargo soak tests on Linux.
- **Tagged release**: full Tauri build matrix (deferred to release.yml).
