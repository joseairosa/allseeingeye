//! Embedded SQL schema constants.
//!
//! Each `pub const` here is a single `CREATE TABLE`, `CREATE INDEX`, or
//! `CREATE VIRTUAL TABLE` statement transcribed verbatim from
//! `docs/05-data-architecture.md` (the "Index schema (`SQLite` + FTS5)"
//! section). They are concatenated by `migrate::INITIAL_SCHEMA` so the
//! migration runner can apply the whole bootstrap in a single
//! `execute_batch` call.
//!
//! Keeping each statement as its own constant makes them grep-able and
//! lets future migrations rebuild a single table without copy-pasting.

/// Component table - the spine of the index.
pub const CREATE_COMPONENT: &str = "
CREATE TABLE component (
  id              TEXT PRIMARY KEY,
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
  mtime           INTEGER,
  ctime           INTEGER,
  enabled         INTEGER NOT NULL DEFAULT 1,
  health          TEXT,
  last_used_at    INTEGER,
  use_count       INTEGER NOT NULL DEFAULT 0,
  parsed_json     TEXT,
  parse_errors    TEXT,
  hash            TEXT NOT NULL,
  updated_at      INTEGER NOT NULL
);
";

/// Composite index for the common `WHERE tool = ? AND type = ?` filter
/// driving the Inventory view.
pub const CREATE_IDX_COMPONENT_TOOL_TYPE: &str =
    "CREATE INDEX idx_component_tool_type ON component(tool, type);";

/// Sorted-by-recency index for the "recently changed" sidebar facet.
pub const CREATE_IDX_COMPONENT_MTIME: &str =
    "CREATE INDEX idx_component_mtime ON component(mtime DESC);";

/// Files that participate in a component (skill scripts, plugin assets,
/// sidecar references). One row per file, deleted on cascade with the
/// owning component.
pub const CREATE_COMPONENT_FILE: &str = "
CREATE TABLE component_file (
  component_id    TEXT NOT NULL REFERENCES component(id) ON DELETE CASCADE,
  path            TEXT NOT NULL,
  role            TEXT,
  PRIMARY KEY (component_id, path)
);
";

/// Cross-component relations (`bundles`, `imports`, `equivalentTo`, ...).
pub const CREATE_RELATION: &str = "
CREATE TABLE relation (
  source_id       TEXT NOT NULL,
  kind            TEXT NOT NULL,
  target_id       TEXT NOT NULL,
  inferred        INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (source_id, kind, target_id)
);
";

/// User-applied tags. Sidecar metadata - never written back to the host
/// tool's directory.
pub const CREATE_TAG: &str = "
CREATE TABLE tag (
  component_id    TEXT NOT NULL,
  tag             TEXT NOT NULL,
  PRIMARY KEY (component_id, tag)
);
";

/// User pins. Sidecar metadata.
pub const CREATE_PIN: &str = "
CREATE TABLE pin (
  component_id    TEXT PRIMARY KEY,
  pinned_at       INTEGER NOT NULL
);
";

/// User-authored notes per component. Sidecar metadata.
pub const CREATE_NOTE: &str = "
CREATE TABLE note (
  component_id    TEXT PRIMARY KEY,
  body            TEXT NOT NULL,
  updated_at      INTEGER NOT NULL
);
";

/// FTS5 virtual table backing the search view. `id` is unindexed because
/// we only need it as the join key back to `component`. Tokeniser is
/// `unicode61` with diacritic folding so accented identifiers match
/// their ASCII equivalents.
pub const CREATE_COMPONENT_FTS: &str = "
CREATE VIRTUAL TABLE component_fts USING fts5(
  id UNINDEXED,
  name,
  description,
  body,
  tokenize = 'unicode61 remove_diacritics 2'
);
";

/// MCP probe history. Composite PK so identical timestamps from a single
/// probe burst don't collide.
pub const CREATE_HEALTH_PROBE: &str = "
CREATE TABLE health_probe (
  component_id    TEXT NOT NULL,
  probed_at       INTEGER NOT NULL,
  status          TEXT NOT NULL,
  latency_ms      INTEGER,
  details_json    TEXT,
  PRIMARY KEY (component_id, probed_at)
);
";

/// Usage events derived from session mining. Append-only; no PK so
/// duplicate events from rerunning the miner don't fail to insert.
pub const CREATE_USAGE_EVENT: &str = "
CREATE TABLE usage_event (
  component_id    TEXT NOT NULL,
  occurred_at     INTEGER NOT NULL,
  session_id      TEXT,
  kind            TEXT NOT NULL,
  details_json    TEXT
);
";

/// Index covering "events for this component, newest first" - the
/// access pattern of the Health view's per-component history.
pub const CREATE_IDX_USAGE_COMPONENT_TS: &str =
    "CREATE INDEX idx_usage_component_ts ON usage_event(component_id, occurred_at DESC);";

/// Schema version table. Single-row contract - migrations always upsert
/// the row at PK = 1, so reads can `SELECT version FROM schema_version
/// WHERE rowid = 1` without scanning.
pub const CREATE_SCHEMA_VERSION: &str = "
CREATE TABLE schema_version (
  version INTEGER NOT NULL
);
";

// Phase 7.1: Security audit tables. Mirrored from
// `docs/12-security.md` ("Privacy model and finding data"). The
// `evidence_json` column from the spec is intentionally deferred to
// Phase 7.2 (where MCP-permission findings need structured evidence) -
// secret findings are fully described by `pattern`, `redacted_preview`,
// `source_label`, and `line` so the column would currently be a free
// NULL on every row.

/// Security findings produced by every audit pass. One row per stable
/// finding id (the scanner deterministically derives the id from the
/// source label, category, pattern, and matched byte range, so a
/// re-scan that yields the same match is a no-op via the upsert's
/// `ON CONFLICT(id) DO NOTHING`). `ON DELETE CASCADE` keeps the
/// findings tied to their owning component - if the component is
/// removed from the index, its findings vanish with it.
pub const CREATE_SECURITY_FINDING: &str = "
CREATE TABLE security_finding (
  id              TEXT PRIMARY KEY,
  component_id    TEXT NOT NULL REFERENCES component(id) ON DELETE CASCADE,
  category        TEXT NOT NULL,
  pattern         TEXT NOT NULL,
  severity        TEXT NOT NULL,
  file_path       TEXT NOT NULL,
  line            INTEGER,
  source_label    TEXT NOT NULL,
  redacted_preview TEXT NOT NULL,
  detected_at     INTEGER NOT NULL,
  suppressed      INTEGER NOT NULL DEFAULT 0,
  suppress_reason TEXT,
  suppress_until  INTEGER
);
";

/// Index covering "findings for this component" - the access pattern
/// of the per-component Quick Look panel.
pub const CREATE_IDX_FINDING_COMPONENT: &str =
    "CREATE INDEX idx_finding_component ON security_finding(component_id);";

/// Index covering "newest critical/high findings first" - the access
/// pattern of the Security view's default sort.
pub const CREATE_IDX_FINDING_SEVERITY_DETECTED: &str =
    "CREATE INDEX idx_finding_severity_detected ON security_finding(severity, detected_at DESC);";

/// User-applied per-(component, pattern) suppressions. The suppress
/// flow lives in the security UI; this table is the persistent store
/// the audit engine consults when deciding whether to re-emit a
/// finding on subsequent upserts.
pub const CREATE_SECURITY_FINDING_SUPPRESSION: &str = "
CREATE TABLE security_finding_suppression (
  component_id    TEXT NOT NULL,
  pattern         TEXT NOT NULL,
  suppressed_at   INTEGER NOT NULL,
  reason          TEXT,
  PRIMARY KEY (component_id, pattern)
);
";

// Phase 14A: app-wide settings. Key/value table so the surface stays
// flat and forward-compatible. Values are JSON strings - a setting
// that is a list of paths is stored as `[\"~/Development\", \"~\"]`,
// a setting that is a bool is `true`, etc. Keeping the storage
// uniform avoids a per-setting migration each time we add a new
// field.
//
// Mirrored from `docs/14-cost-and-memory.md` section 14A "Settings".
pub const CREATE_APP_SETTINGS: &str = "
CREATE TABLE app_settings (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
";

// Phase 14C: Token usage analytics. Stores per-day rollups of assistant
// turns folded out of Claude Code and Codex JSONL session transcripts.
// Aggregation is keyed by (tool, project_path, model, day) so re-running
// the parser is idempotent. The watermark table lets re-scans only
// consume the bytes appended since the last run, keeping repeat passes
// O(new bytes) regardless of total history. See docs/14-cost-and-memory.md
// section 14C for the full rationale.

/// Per-day token usage rollup. One row per
/// `(tool, project_path, model, day)` tuple.
pub const CREATE_TOKEN_USAGE: &str = "
CREATE TABLE token_usage (
  tool          TEXT NOT NULL,
  project_path  TEXT NOT NULL,
  model         TEXT NOT NULL,
  day           TEXT NOT NULL,
  sessions      INTEGER NOT NULL,
  turns         INTEGER NOT NULL,
  input         INTEGER NOT NULL,
  output        INTEGER NOT NULL,
  cache_read    INTEGER NOT NULL,
  cache_create  INTEGER NOT NULL,
  est_cost_usd  REAL NOT NULL,
  refreshed_at  INTEGER NOT NULL,
  PRIMARY KEY (tool, project_path, model, day)
);
";

/// Index covering "rows for the last N days across all projects" - the
/// access pattern of the Cost view's by-day sparkline.
pub const CREATE_IDX_TOKEN_USAGE_DAY: &str =
    "CREATE INDEX idx_token_usage_day ON token_usage(day);";

/// Index covering "rows for one project across all models / days" - the
/// access pattern of the Cost view's by-project bar chart and the
/// recommendations engine.
pub const CREATE_IDX_TOKEN_USAGE_PROJECT: &str =
    "CREATE INDEX idx_token_usage_project ON token_usage(project_path);";

/// Per-session watermark recording how many bytes of a given session's
/// JSONL we have already folded into `token_usage`. Re-scans seek to
/// `bytes_read` and only parse the appended tail.
pub const CREATE_USAGE_SESSION_WATERMARK: &str = "
CREATE TABLE usage_session_watermark (
  tool       TEXT NOT NULL,
  session_id TEXT NOT NULL,
  bytes_read INTEGER NOT NULL,
  PRIMARY KEY (tool, session_id)
);
";
