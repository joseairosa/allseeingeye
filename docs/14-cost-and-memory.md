# 14 - Cost & Memory

Status: PENDING

This spec covers three things that are tightly coupled and should ship together:

1. **Project memory walker** - find every `CLAUDE.md`, `AGENTS.md`,
   `GEMINI.md` on disk, not just the user-level ones.
2. **Size / cost diagnostics** - surface what each indexed file costs in
   bytes / tokens, flag bloat.
3. **Token usage analytics** - parse local session transcripts to roll
   up real spend by project / model / day, recommend reductions.

Phases land in order: A unblocks B, A and C-backend can run in
parallel, C-frontend lands after C-backend.

---

## 14A - Project memory walker (backend)

### Problem

Today the memory roots in `registry/tools.rs` only cover the **user-level**
files:

| Tool         | Path                  |
|--------------|-----------------------|
| Claude Code  | `~/.claude/CLAUDE.md` |
| Codex        | `AGENTS.md` (declared `Scope::Project` but no walker exists) |
| Antigravity  | `~/.gemini/GEMINI.md` |

The user has **42** project-level `CLAUDE.md` files under
`~/Development` alone. None are indexed, none are scanned for secrets,
and none can be flagged as oversized.

### Solution

Add a project-tree walker that runs as part of `Pipeline::full_scan`
alongside the existing glob walkers. The walker:

1. Reads a configurable list of **project search roots** from app
   settings. Default = `[$HOME/Development, $HOME]`.
2. Walks each root with hard limits (depth, denylist) and finds files
   whose basename matches the memory file set per tool.
3. For each match, validates the parent directory is a "project" (has at
   least one project marker file).
4. Emits one `ScanCandidate` per match with the right `tool`, `type =
   Memory`, `scope = Project`, and a derived display `name` (e.g.
   `projectfinish/CLAUDE.md`).

### Walker contract

| Aspect | Rule |
|--------|------|
| Max depth | 4 levels from each root. Deeper = ignored. |
| Symlinks | Followed once; cycle-detected via canonical-path set. |
| Denylist (always skip) | `node_modules`, `.git`, `.next`, `dist`, `build`, `target`, `.venv`, `venv`, `__pycache__`, `.cache`, `.Trash`, `Library`, `vendor`, `Pods`, `.terraform`, `out` |
| Project marker (any of) | `.git/`, `package.json`, `Cargo.toml`, `pyproject.toml`, `Gemfile`, `go.mod`, `pubspec.yaml`, `composer.json`, `mix.exs`, `Project.toml` |
| File set per tool | Claude Code: `CLAUDE.md`, `CLAUDE.local.md`. Codex: `AGENTS.md`. Antigravity: `GEMINI.md`. |
| Hidden dirs | Skipped except for `.claude/` and `.cursor/` (those carry tool config). |

### IDs

Component IDs for project memory must be **path-stable**. The existing
`build_id_for(component_type, scope, path)` already hashes the path so
two `CLAUDE.md` files in different projects get distinct IDs without any
schema change.

### Settings

Add a `projectMemoryRoots: string[]` to `app_settings` (existing table).
Default is `["~/Development", "~"]`. UI exposure is deferred to 14B
(Health view) - the setting reads/writes work in 14A so the walker can
already pick up overrides via direct DB edit.

### Tests

- Unit: walker respects depth, denylist, project markers, symlink loops.
- Unit: tool routing - `CLAUDE.md` under a Codex-only project still
  routes to Claude Code (the walker is per-filename, not per-tool).
- Integration (`tests/real_home_scan_proof.rs` extended): assert at
  least 5 `Memory` rows under `~/Development` are indexed on the
  developer's machine.

### Out of scope for 14A

- Watching project trees with `notify`. Project trees are huge and
  watcher overhead is unbounded. Re-scan happens on full-scan only;
  manual refresh is one click. Live watching is a 14D candidate.

---

## 14B - Size / cost diagnostics (backend + frontend)

### Backend

Two cheap additions, no schema migration:

1. `ComponentDetail` IPC payload gains:
   - `estimated_tokens: u64` - `size / 4`. Documented as a 4-char
     heuristic; users see a tooltip with the caveat. We do **not** ship
     `tiktoken` or any vendor tokenizer - too heavy for what's
     ultimately a rough rendered number.
   - `is_oversized: bool` - true when `type = Memory` and `size > 8192`
     (rough 2k-token line, the point at which a memory file is a real
     contributor to every-turn cost).

2. New IPC query `health::bloated_memory()` returns memory components
   above the threshold sorted by size desc, with computed
   `tokens_per_turn` and `est_monthly_cost_usd` (uses the price table
   from 14C; if 14C ships first, the cost is real, otherwise the field
   is `null` and the UI hides the column).

### Frontend

1. **Inventory rows** - memory components show a trailing
   `12.4kB · ~3.1k tok` chip. Use the design-system text-tertiary token
   so it doesn't compete with the filename.

2. **Health view** - new section "Bloated memory" between the existing
   "Drift" and "Cold" sections. Lists oversized memory files sorted by
   size, each with a "open in editor" button.

3. **QuickLook footer** - for memory files only, render
   `~3.1k tokens · 1.5% of a 200k context window`.

4. **Settings** - add a "Project memory roots" field (textarea, one
   path per line). Reads / writes `app_settings.projectMemoryRoots`.

### Tests

- Snapshot: inventory row renders the size chip for memory only.
- Health view: bloat list renders sorted by size desc.
- E2E (Playwright): user with 5+ project CLAUDE.md files sees them in
  Health, click navigates to Editor.

---

## 14C - Token usage analytics (backend + frontend)

### Data sources

| Tool        | Path | Format | Per-turn fields |
|-------------|------|--------|-----------------|
| Claude Code | `~/.claude/projects/<encoded-cwd>/<session-uuid>.jsonl` | JSONL | `message.usage.{input_tokens, output_tokens, cache_read_input_tokens, cache_creation_input_tokens}`, `message.model`, `cwd` (in first line / from dirname) |
| Codex       | `~/.codex/sessions/YYYY/MM/DD/rollout-<uuid>.jsonl` | JSONL | `payload.{model, usage}` on `event_type: "token_count"`; `cwd` from `session_meta` first line |
| Cursor      | not exposed locally | - | skipped |
| Antigravity | not exposed locally | - | skipped |

### Schema

```sql
CREATE TABLE token_usage (
  tool          TEXT NOT NULL,                 -- 'claude-code' | 'codex'
  project_path  TEXT NOT NULL,                 -- decoded cwd, '/'-prefixed
  model         TEXT NOT NULL,                 -- raw vendor model id
  day           TEXT NOT NULL,                 -- YYYY-MM-DD (UTC)
  sessions      INTEGER NOT NULL,              -- distinct session count
  turns         INTEGER NOT NULL,              -- assistant turns folded
  input         INTEGER NOT NULL,
  output        INTEGER NOT NULL,
  cache_read    INTEGER NOT NULL,
  cache_create  INTEGER NOT NULL,
  est_cost_usd  REAL NOT NULL,                 -- computed at row write
  refreshed_at  INTEGER NOT NULL,
  PRIMARY KEY (tool, project_path, model, day)
);

CREATE INDEX idx_token_usage_day      ON token_usage(day);
CREATE INDEX idx_token_usage_project  ON token_usage(project_path);

CREATE TABLE usage_session_watermark (
  tool       TEXT NOT NULL,
  session_id TEXT NOT NULL,
  bytes_read INTEGER NOT NULL,                  -- offset into the JSONL we've consumed
  PRIMARY KEY (tool, session_id)
);
```

### Refresh strategy

- **No watcher.** Transcripts are append-only; we re-scan when the Cost
  view first mounts AND when the user clicks "refresh" AND on a 30-min
  background timer while the view is visible.
- Per-session watermark: each session's `bytes_read` is the offset we
  last consumed. New scans seek to that offset and only parse the tail.
  This keeps repeat scans O(new bytes) regardless of total history.
- The aggregate writes are upserts keyed by
  `(tool, project_path, model, day)`. Re-running a scan is idempotent.

### Pricing

`usage/pricing.rs` ships a static price table. Comment at the top
states a clear "verify before quoting" caveat - prices change.

```rust
struct ModelPrice {
  pattern:      &'static str,         // matched as a prefix on model id
  input_per_m:  f64,                  // $ per 1M tokens
  output_per_m: f64,
  cache_read_per_m:   f64,
  cache_create_per_m: f64,
}
```

Unknown models fall through to a "default Sonnet-tier" entry; their
rows tag `model_known = false` so the UI can footnote uncertainty.

### Recommendations engine

`usage::recommend()` returns up to 5 ordered recommendations. Each
recommendation has:

```rust
struct CostRec {
  kind:               CostRecKind,    // BloatedMemory | LowCacheHitRate | OldModelOnHotProject
  title:              String,         // user-facing
  rationale:          String,
  estimated_savings_usd_30d: f64,
  related_components: Vec<ComponentId>, // for "open in editor" links
}
```

Heuristics (v1):

| Heuristic | Trigger | Estimated savings |
|-----------|---------|-------------------|
| `BloatedMemory` | A `Memory` component > 8kB lives in a project that spent > $5 in last 30d. | `oversize_bytes / 4 / 1e6 * input_per_m * turns_30d` |
| `LowCacheHitRate` | A project with > $20 / 30d has `cache_read / (cache_read + input) < 0.4`. | Difference vs. 0.7 baseline. |
| `OldModelOnHotProject` | Top-3 project by spend used Opus for > 50% of turns when Sonnet would suffice. | Opus rate minus Sonnet rate. |

Heuristics are ordered by `estimated_savings_usd_30d` desc.

### IPC surface

```ts
type CostQuery = "summary" | "byProject" | "byDay" | "recommendations";
type CostResponse =
  | { kind: "summary"; tokens30d: TokenTotals; costUsd30d: number; topProject: string }
  | { kind: "byProject"; rows: { project: string; costUsd: number; tokens: TokenTotals }[] }
  | { kind: "byDay";     rows: { day: string; tokens: TokenTotals; costUsd: number }[] }
  | { kind: "recommendations"; recs: CostRec[] };

invoke("usage_query", { kind: CostQuery, refresh?: boolean })
invoke("usage_refresh", {})  // returns the next refreshed_at
```

### Frontend - "Cost" view

New sidebar entry between **Health** and **Editor**. Layout:

```
┌─────────────────────────────────────────────────────────────────────┐
│ Cost                                                       refresh  │
├─────────────────────────────────────────────────────────────────────┤
│  $42.18 · 5.4M tokens · last 30d                                    │
│  top project: projectfinish ($18.40)                                │
├──────────────────────────────────────┬──────────────────────────────┤
│  by project (bar)                    │  by day (sparkline)          │
│  projectfinish ████████████ $18.40   │  ▁▃▅█▆▇▃▁▂▅█▇▄...            │
│  allseeingeye  ███████      $11.20   │                              │
│  artemis-app   ████         $ 6.50   │  recommendations             │
│  ...                                 │  • Trim CLAUDE.md in pf      │
│                                      │    Saves ~$4.20 / 30d        │
│                                      │  • Switch art-app to Sonnet  │
│                                      │    Saves ~$2.10 / 30d        │
└──────────────────────────────────────┴──────────────────────────────┘
```

All numbers tagged with the data source and the "approximate" caveat.
The recommendations panel is the headline payoff - that's why this
phase exists.

### Tests

- Backend: parse a fixture JSONL with known turns; aggregate matches
  hand-computed totals.
- Backend: re-scan after appending lines to a session; only the new
  lines are consumed (watermark advanced).
- Backend: pricing table covers Opus/Sonnet/Haiku/GPT; unknown model
  falls through to default with `model_known = false`.
- Frontend: Cost view renders KPIs from a mocked IPC payload.
- Frontend: recommendation card "open file" navigates to Editor.

### Out of scope for 14C

- Per-turn drill-down ("why was this turn so expensive?"). Useful but
  noisy; deferred.
- Cursor / Antigravity usage. No documented local source.
- API-level rate-limit / quota tracking. The local transcripts are
  enough for cost; rate-limit data is server-side only.

---

## Risks

1. **Walker explosion** - If the user's project roots are
   `[$HOME, /]`, the walker could traverse millions of files. Mitigated
   by the depth cap, the denylist, and the project-marker requirement.
2. **Tokenizer drift** - `size / 4` is a heuristic. Tokens per char vary
   for non-English content. We document this in the tooltip and quote
   ranges, not single numbers, in recommendations.
3. **Pricing rot** - the table will go stale. We mitigate by stamping
   the table version in the row and showing it in the UI footer.
4. **Cost-rec false positives** - `LowCacheHitRate` could fire on
   projects with genuinely small contexts. Threshold tuned to require
   $20+ / 30d so we only nag on projects where the savings are real.

## Specialist dispatch

| Phase | Agent | Depends on |
|-------|-------|------------|
| 14A   | `aseye-rust-backend` | none |
| 14B-backend | piggybacks on 14A's PR | 14A |
| 14B-frontend | `aseye-frontend-features` | 14A merged |
| 14C-backend | `aseye-rust-backend` (parallel session) | none |
| 14C-frontend | `aseye-frontend-features` | 14C-backend merged |
