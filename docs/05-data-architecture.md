# 05 - Data Architecture

How All Seeing Eye stores, watches, and serves data. Disk is truth; the in-process index is a cache.

## Layers

```
+--------------------------------------------------------+
|  UI (React)                                            |
|  - Components, queries, optimistic updates             |
+----------------------------+---------------------------+
                             |  IPC (Tauri commands + events)
+----------------------------v---------------------------+
|  Core (Rust)                                           |
|  - Tool registry                                       |
|  - File watcher (notify crate)                         |
|  - Parser dispatch                                     |
|  - Index (SQLite + FTS5)                               |
|  - Validator (per-tool schemas)                        |
|  - Atomic writer                                       |
|  - MCP probe (transport client)                        |
+----------------------------+---------------------------+
                             |
+----------------------------v---------------------------+
|  Disk                                                  |
|  - User and project tool roots                         |
|  - All Seeing Eye config and sidecar                   |
|  - Local SQLite index                                  |
+--------------------------------------------------------+
```

The frontend never touches the file system directly. All disk access flows through the Rust core via Tauri commands, which is also the security boundary.

## Tool registry

A static-but-extensible registry of supported tools. Each entry declares:

```ts
{
  id: 'claude-code',
  displayName: 'Claude Code',
  detection: {
    binary: ['claude', 'claude-code'],            // PATH names
    paths: ['~/.claude', '~/.claude.json'],
    versionCommand: 'claude --version'
  },
  componentRoots: [
    { type: 'settings',     path: '~/.claude/settings.json',          format: 'json' },
    { type: 'settings',     path: '~/.claude/settings.local.json',    format: 'json' },
    { type: 'memory',       path: '~/.claude/CLAUDE.md',              format: 'md',   flavour: 'CLAUDE.md' },
    { type: 'rule',         pattern: '~/.claude/rules/*.md',           format: 'md-frontmatter' },
    { type: 'skill',        pattern: '~/.claude/skills/*/SKILL.md',    format: 'md-frontmatter', folder: true },
    { type: 'command',      pattern: '~/.claude/commands/*.md',        format: 'md' },
    { type: 'agent',        pattern: '~/.claude/agents/*.md',          format: 'md-frontmatter' },
    { type: 'mcp',          path: '~/.claude.json',                    format: 'json',         keyPath: 'mcpServers' },
    { type: 'plugin',       pattern: '~/.claude/plugins/cache/*/*/*/.claude-plugin/plugin.json', format: 'json' },
    { type: 'session',      pattern: '~/.claude/sessions/*.json',       format: 'json' },
    ...
  ],
  watch: ['~/.claude', '~/.claude.json'],
  // hooks/output_styles/permissions are settings-embedded; we expose them as virtual components projected from settings
  virtualComponents: [
    { type: 'hook',          source: 'settings.hooks',           shape: 'array' },
    { type: 'permission',    source: 'settings.permissions',     shape: 'object' },
    { type: 'statusline',    source: 'settings.statusline',      shape: 'object' },
    { type: 'outputStyle',   source: 'output-styles/*.md',       shape: 'file-glob' }
  ]
}
```

This declarative registry lives in Rust as a `Vec<ToolDescriptor>` plus generated TypeScript bindings via `ts-rs`. Adding a new tool is a code change, not a runtime config.

## Index schema (SQLite + FTS5)

A single embedded SQLite database at `~/Library/Application Support/AllSeeingEye/index.sqlite` (macOS) / XDG-equivalent on Linux / `%APPDATA%/AllSeeingEye/` on Windows.

```sql
CREATE TABLE component (
  id              TEXT PRIMARY KEY,         -- aseye://...
  type            TEXT NOT NULL,
  tool            TEXT NOT NULL,
  scope           TEXT NOT NULL,
  origin          TEXT NOT NULL,
  plugin_id       TEXT,
  name            TEXT NOT NULL,
  display_name    TEXT,
  description     TEXT,
  path            TEXT NOT NULL,
  format          TEXT NOT NULL,
  size            INTEGER,
  mtime           INTEGER,                  -- ms since epoch
  ctime           INTEGER,
  enabled         INTEGER NOT NULL DEFAULT 1,
  health          TEXT,
  last_used_at    INTEGER,
  use_count       INTEGER NOT NULL DEFAULT 0,
  parsed_json     TEXT,                     -- normalised typed view as JSON
  parse_errors    TEXT,                     -- JSON array
  hash            TEXT NOT NULL,            -- content sha256, for change detection
  updated_at      INTEGER NOT NULL
);

CREATE INDEX idx_component_tool_type ON component(tool, type);
CREATE INDEX idx_component_mtime     ON component(mtime DESC);

CREATE TABLE component_file (
  component_id    TEXT NOT NULL REFERENCES component(id) ON DELETE CASCADE,
  path            TEXT NOT NULL,
  role            TEXT,                     -- 'main' | 'script' | 'reference' | 'asset' | 'sidecar'
  PRIMARY KEY (component_id, path)
);

CREATE TABLE relation (
  source_id       TEXT NOT NULL,
  kind            TEXT NOT NULL,            -- 'bundles' | 'imports' | 'equivalentTo' | ...
  target_id       TEXT NOT NULL,
  inferred        INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (source_id, kind, target_id)
);

CREATE TABLE tag (
  component_id    TEXT NOT NULL,
  tag             TEXT NOT NULL,
  PRIMARY KEY (component_id, tag)
);

CREATE TABLE pin (
  component_id    TEXT PRIMARY KEY,
  pinned_at       INTEGER NOT NULL
);

CREATE TABLE note (
  component_id    TEXT PRIMARY KEY,
  body            TEXT NOT NULL,
  updated_at      INTEGER NOT NULL
);

-- Full-text search on name + description + raw body
CREATE VIRTUAL TABLE component_fts USING fts5(
  id UNINDEXED,
  name,
  description,
  body,
  tokenize = 'unicode61 remove_diacritics 2'
);

-- Health probe history (MCP servers etc.)
CREATE TABLE health_probe (
  component_id    TEXT NOT NULL,
  probed_at       INTEGER NOT NULL,
  status          TEXT NOT NULL,            -- 'up' | 'down' | 'degraded'
  latency_ms      INTEGER,
  details_json    TEXT,
  PRIMARY KEY (component_id, probed_at)
);

-- Usage events derived from session mining
CREATE TABLE usage_event (
  component_id    TEXT NOT NULL,
  occurred_at     INTEGER NOT NULL,
  session_id      TEXT,
  kind            TEXT NOT NULL,            -- 'invoke' | 'load' | 'error'
  details_json    TEXT
);
CREATE INDEX idx_usage_component_ts ON usage_event(component_id, occurred_at DESC);

-- Schema versioning
CREATE TABLE schema_version (version INTEGER NOT NULL);
```

## File watching

Backed by the `notify` Rust crate. One watcher per registered root (user-level paths) plus one per opened project. Events:

- **Create** -> parse + insert.
- **Modify** -> re-parse + update; FTS row replaced; relations recomputed lazily.
- **Delete** -> mark component as deleted (soft delete for 5 minutes to handle editor swap-saves), then remove.
- **Rename** -> treated as delete + create unless source and dest both reside in our roots, in which case we update the row in place.

Coalescing: 200 ms debounce per path. Editors that write via "atomic save" (write temp -> rename) emit a Delete + Create burst; debouncer collapses to a single "Modify".

## Atomic writes

Every write goes through:

```rust
fn atomic_write(path: &Path, content: &[u8]) -> Result<()> {
    let tmp = path.with_extension(format!("aseye-tmp-{}", uuid()));
    write_all(&tmp, content)?;
    fsync(&tmp)?;
    rename(&tmp, path)?;             // atomic on POSIX
    fsync(parent_dir(path))?;
    Ok(())
}
```

We hold an `flock` on the file during write to play nicely with simultaneous reads from the host tool. If the host tool holds the lock (rare; most don't), we retry with backoff up to 1s, then surface a "tool is busy" error.

## Parser dispatch

A single `parse(component_root_descriptor, file_path)` entry point in core. Internally:

1. Read raw bytes (capped at 5 MB; larger files are flagged and skipped to protect memory).
2. Branch on `format`:
   - `json` / `toml` / `yaml` -> structured parse, then normalise to `parsed_json`.
   - `md` / `md-frontmatter` -> split frontmatter and body; YAML-parse frontmatter.
   - `mdc` -> same as `md-frontmatter`.
   - `jsonl` -> stream, accumulate metadata only.
   - `sqlite` -> open read-only, run a known query for the index.
3. Run type-specific normaliser (e.g., for an MCP entry, dedupe across embedding tools).
4. Validate against bundled schemas; collect non-fatal warnings.
5. Compute SHA-256 of raw content -> `hash`.
6. Update SQLite row.

## Validator

JSON Schema bundles per tool, sourced from official schemas where available (Claude Code's `https://json.schemastore.org/claude-code-settings.json`) and authored locally where not. Schemas live as static assets shipped with the app.

Validation runs on every parse and on every save. Save is blocked on validation failure unless the user opts in to "save anyway".

## MCP probing

For each `mcp` component, we periodically (default every 5 minutes; tunable; off by default in `panic` mode) probe the server using a transport-appropriate handshake:

- `stdio` - spawn the configured command in a subprocess, send `initialize`, read `initialize` response, send `tools/list`, then `shutdown`. Capture stdout/stderr lines for diagnostics. Hard timeout 5s.
- `sse` - GET the URL with appropriate headers, expect `200 OK` and `text/event-stream`. Send `initialize` over the channel.
- `http` - POST `initialize` to the URL.

Result is written to `health_probe` and the component's `health` field. We never pass user-supplied env vars into a probe except those declared in the MCP entry; we never proxy traffic from the user-space session into our probe.

Probing is **opt-in per tool** in MVP - off by default. Power users turn it on for their MCP-heavy setups.

## Sidecar metadata

User-applied metadata (tags, pins, notes) lives in our own SQLite, not the tool's files. This avoids polluting the tool's directories. Identity is by `aseye://` URI, which is stable across sessions because it's derived from `(tool, scope, type, name)`, not file inode.

If a component is deleted on disk and later recreated with the same name, our sidecar metadata reattaches. (This is a deliberate UX choice: pinning survives a Git checkout that briefly removes the file.)

## Cross-process coordination

Only one All Seeing Eye instance runs at a time per machine. Enforced via a lock file at `~/Library/Application Support/AllSeeingEye/instance.lock`. Second instance opens an IPC channel to the existing instance to bring its window forward.

## Indexing performance targets

| Operation | Target | Hard ceiling |
|-----------|--------|--------------|
| Full scan of ~200 components | < 800 ms | 2 s |
| Incremental update on file change | < 50 ms | 200 ms |
| FTS query | < 30 ms | 100 ms |
| Save + re-parse | < 100 ms | 300 ms |
| MCP probe (single server) | < 1 s | 5 s |

Strategies:
- Parsers run on a Tokio thread pool sized to physical cores.
- FTS upserts are batched per file event tick.
- Large session/history files are streamed with a hard byte cap; full content is never loaded into memory.

## Cache and eviction

- Disk index is durable; never evicted unless the user runs "Reset index".
- In-memory cache holds the last 200 parsed components (LRU) for editor responsiveness.
- Probed MCP responses cached for 5 minutes (tunable per server).

## Migration

Schema versioning via `schema_version` table. On launch, core checks the current version; if older than embedded schema, applies migrations sequentially. Each migration is a Rust function plus optional SQL. Forward-only; never roll back.

## Backup and restore

Index is rebuildable from the source tools. We do not back it up.
Sidecar (tags, pins, notes) is precious. Backed up once a day to `~/Library/Application Support/AllSeeingEye/backups/sidecar-YYYY-MM-DD.sqlite`, last 7 retained. Manual export available.

## Concurrency model

```
                   +-----------------+
                   |   FileWatcher   |
                   +--------+--------+
                            |
                  events    v
                   +-----------------+
                   |   Coalescer     |  (debounce 200ms)
                   +--------+--------+
                            |
                            v
                   +-----------------+
                   |   ParseWorker   |  (tokio task per file)
                   +--------+--------+
                            |
                            v
                   +-----------------+
                   |   IndexWriter   |  (single, owns the SQLite write conn)
                   +--------+--------+
                            |
                            v
                   +-----------------+
                   |   EventBus      |  (broadcast to UI subscribers)
                   +-----------------+
```

A single index-writer task owns the SQLite write connection; readers use a separate read-only connection pool. This avoids contention and surfaces `BUSY` errors only for the writer, which retries with exponential backoff capped at 1s.

## Failure modes and recovery

| Failure | Mitigation |
|---------|------------|
| Parse error on a file | Surface as a UI badge on the component; don't crash. Component shown with limited data. |
| Watcher OS limit hit (Linux inotify) | Detect, surface a warning, suggest raising `fs.inotify.max_user_watches`. |
| SQLite corruption | Detect via `PRAGMA integrity_check` on launch; if corrupt, archive the file and rebuild. Sidecar metadata is at risk; warn the user. |
| Disk full mid-save | Atomic write fails before rename; original file untouched. Surface error. |
| Tool format change | Bundled validator schemas updated via app update. Old schema falls back to lenient mode (parse but don't validate). |

## Privacy boundary

The index never leaves the local machine unless the user explicitly exports a bundle. Telemetry, when added, is metric-only (counts of parse errors, parse durations, never component content). See `02-prd.md` H6 / I3.
