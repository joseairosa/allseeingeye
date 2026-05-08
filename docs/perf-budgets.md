# Performance budgets

This document explains every entry in `perf-budgets.json` at the repo
root, why the number is what it is, and how to update it.

## What we measure (and what we don't)

We measure things a headless Linux CI runner can measure deterministically
on every PR:

- **Frontend bundle size** (gzipped sum of `dist/assets/*.js`, plus the
  largest single chunk).
- **Rust micro-benchmarks** (parser dispatch + path classification),
  reported as criterion mean times.

We do NOT measure (post-MVP, see `docs/10-roadmap.md`):

- Cold-start time. Requires a real WebView and a baseline machine
  (PRD H1: < 2 s on M1 Air, 8 GB, 200 components).
- Idle CPU and runtime memory. Requires a long-running app (PRD H2:
  < 1% idle CPU, < 200 MB idle memory).
- App binary size. Will land alongside the Phase 6 release pipeline
  measuring the actual signed bundle (PRD H3: < 30 MB on macOS).

These three are the formal MVP acceptance gates per
`docs/10-roadmap.md` "Acceptance for MVP done". The budgets in this
file are the CI-checkable proxies that catch regressions BEFORE the
formal acceptance run.

## Cross-references

- `docs/02-prd.md` H1 - H3 (cold start, idle CPU/memory, binary size).
- `docs/05-data-architecture.md` "Indexing performance targets"
  (full-scan < 800 ms, incremental update < 50 ms, FTS query
  < 30 ms, save + re-parse < 100 ms).
- `docs/10-roadmap.md` "Definition of done (per release)" line:
  "Performance budgets met (cold start, idle CPU, memory)".

## Budget reference

### `frontend.totalGzipBytes` (default `250000`)

What: the gzipped sum of every `*.js` chunk under `dist/assets/`
after `pnpm --filter @aseye/desktop build`.

Why this number: the current build produces ~167 KB gzipped JS total.
A 50% headroom (~250 KB) catches "we accidentally added a 100 KB
dependency" without false-firing on routine feature work.

Why a budget on the *gzipped* number: HTTP transfer of the WebView
bundle is gzipped, so the user-felt cost is the gzipped byte count.
Raw byte counts would punish addition of code that compresses well
(e.g. JSON tables) and reward minified-but-low-entropy noise.

### `frontend.largestChunkGzipBytes` (default `200000`)

What: the largest single `*.js` chunk's gzipped size.

Why this number: the user pays the largest-chunk cost on the critical
path because Vite splits per-route by default. Holding the largest
chunk under 200 KB gzipped means even a slow connection can render
the first paint inside H1's 2 s budget. Current largest chunk is ~161
KB gzipped (the main entry); the 200 KB ceiling absorbs a one-off
Monaco-editor preload regression without false-firing.

### `frontend.tolerancePercent` (default `10`)

What: the percentage by which a budget can be exceeded before the CI
job fails.

Why this number: 10% catches "real" regressions (a new dep, a
forgotten code-splitting boundary) without false-firing on routine
churn. The script reports the current measurement on every run, so a
deliberate move past the budget is a single PR that updates the
baseline plus documents WHY in the commit message.

### `rust.parserMeanMicros` (default `15`)

What: the *worst-case* criterion mean time (microseconds) across the
parser fixture set (JSON / TOML / YAML / Markdown+frontmatter). The
gate uses the worst-case so a regression in one branch (e.g. YAML)
cannot be averaged out by the others.

Why this number: current measurements on a developer macbook show
JSON ~2 us, TOML ~3 us, Markdown ~7 us, YAML ~8.6 us (the YAML
branch dominates because `serde_yaml` constructs an internal
`Mapping` representation before reshaping to `serde_json::Value`).
15 us is "current worst + ~75% headroom", which absorbs runner-to-
runner variance on shared-tenancy GitHub Actions hosts without
masking a real regression (a doubling would still trip the gate).

How this fits the indexing budget: full re-scan of 200 components
must complete in < 800 ms (`docs/05-data-architecture.md`). 15 us *
200 = 3 ms total parser time, well inside the per-component 4 ms
budget that leaves room for filesystem IO and SQLite upsert.

### `rust.classifyMeanMicros` (default `1000`)

What: the criterion mean time (microseconds) of the worst case in
`registry::classify::classify_path`. The worst case today is
`outside_registry` (a path that matches no descriptor), because the
function walks every glob and bails out at the bottom.

Why this number is so much larger than the parser budget: the
current implementation calls `Glob::new(pattern).compile_matcher()`
*on every call*, paying the globset construction cost ~once per
descriptor per classification. Measured at 600-700 us on a developer
macbook for the worst case. Caching the compiled matchers is a
roadmap item; for now the budget reflects current reality + 20%
headroom (~1000 us = 1 ms).

What this means for the watcher: after coalescing (200 ms debounce
window), a typical burst is dozens of events, not thousands. 1 ms *
50 events = 50 ms of classification time per tick, which is inside
the < 200 ms incremental-update ceiling (`docs/05-data-architecture.md`).
A future glob-cache PR will lower this dramatically; when it lands,
the budget should drop with it (see "How to lower a budget" below).

### `rust.tolerancePercent` (default `20`)

What: the percentage by which a Rust mean-time budget can grow
before the CI job fails.

Why this number larger than the frontend's 10%: criterion measurements
on a shared GitHub Actions runner are noisier than disk-size byte
counts. A 20% threshold is the empirical "real regression vs. runner
flake" line we calibrate against; if a real regression appears below
that line we'll see it as a sustained drift across consecutive runs
and tighten this. Frontend bytes are deterministic and admit a tighter
budget.

## How to update a budget

1. Make the change that increases the number.
2. Justify it: a new feature, a dependency upgrade, a deliberate
   trade-off. The justification goes in the PR description.
3. Edit `perf-budgets.json`. Update only the number you intend to
   change.
4. Commit with `chore(perf): raise <name> from <old> to <new> because
   <reason>`. The commit message is the durable record - the PR
   description disappears once the PR closes.
5. The perf workflow re-runs against the new budget on the PR. It
   should pass (else the change exceeds even the new budget and you
   are still over).

## How to lower a budget (the rare good case)

When a refactor reduces a budget by > 10%, lower the budget in the
same PR. This protects the win from being silently undone later.

## Why this file exists at the repo root

`perf-budgets.json` is consumed by:

- `scripts/perf-summary.mjs` (local + CI bundle-size job).
- `.github/workflows/perf.yml` Rust bench job.
- `.github/workflows/release.yml` (warn-only sanity check before
  publishing a release tag).

Keeping it at the root, not under `apps/desktop/`, signals it is a
project-wide gate, not a per-app concern.
