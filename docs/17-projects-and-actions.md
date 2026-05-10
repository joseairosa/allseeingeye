# 17 - Projects view + actions

Status: PENDING

A new top-level view that lists every project on the user's machine
that has at least one indexed memory file (CLAUDE.md / AGENTS.md /
GEMINI.md), and offers a panel of project-scoped actions: analyse the
memory file for size + optimisation opportunities, audit git worktrees
+ disk usage, reorganise loose `.md` files into a `docs/` folder.

Every action that mutates the user's filesystem runs as a **dry-run by
default**. The user clicks "Apply" to actually perform the changes.
Same shape as the backup restore flow (`dry_run: bool` parameter).

The worktree action is **read-only in v1**. No automated removal.

---

## 17.1 - Goals

1. Surface every project the user touches from one place.
2. Give the user a fast read on which projects have bloated context
   files (CLAUDE.md > 8 KB ≈ 2 k tokens of every-turn cost).
3. Give the user a fast read on git worktree disk usage so stale
   worktrees do not silently accumulate.
4. Provide a one-click "tidy up loose docs" action that moves stray
   `.md` files into a `docs/` folder and rewrites internal links.

Non-goals for v1:

- Cross-project actions (apply to multiple projects at once).
- Worktree removal (read-only listing only).
- Tag / pin / star projects.
- Project search / filter beyond the indexed memory list.

---

## 17.2 - Project discovery

The user already has Phase 14A indexing every CLAUDE.md / AGENTS.md /
GEMINI.md under `~/Development` (and `~`). Each indexed memory row
has a `path` pointing at the file. A project IS the parent directory
of an indexed memory file.

```sql
SELECT DISTINCT
    -- parent of memory file = project root
    rtrim(substr(path, 1, length(path) - length(replace(replace(path, '/', ''), substr(path, instr(reverse_path, '/') + 1), ''))), '/')
    -- (in practice we do this in Rust via Path::parent())
    AS project_path,
    -- prefer CLAUDE.md when multiple memory files coexist
    CASE
        WHEN basename(path) = 'CLAUDE.md' THEN 0
        WHEN basename(path) = 'AGENTS.md' THEN 1
        WHEN basename(path) = 'GEMINI.md' THEN 2
        ELSE 3
    END AS preference
FROM component
WHERE type = 'memory' AND scope = 'project'
ORDER BY project_path ASC;
```

Each project row carries:

```rust
pub struct ProjectSummary {
    pub project_path: PathBuf,
    pub display_name: String,           // last 2 path segments e.g. "Development/projectfinish"
    pub memory_files: Vec<MemoryFileSummary>,
    pub primary_memory_path: PathBuf,   // CLAUDE.md > AGENTS.md > GEMINI.md
    pub primary_memory_size: u64,
    pub primary_memory_tokens_est: u64,
    pub is_oversized: bool,             // primary_memory_size > 8 KiB
    pub has_git: bool,                  // .git/ exists at project_path
}

pub struct MemoryFileSummary {
    pub basename: String,               // "CLAUDE.md"
    pub size: u64,
    pub mtime: i64,
}
```

Computed read-only from the index; no IO beyond the existing SQLite
query plus a `path.exists()` check for `.git/`.

---

## 17.3 - Action 1: CLAUDE.md size + optimisation

Run heuristics against the project's primary memory file (CLAUDE.md
or its variant). Pure read; never modifies the file.

```rust
pub struct MemoryAnalysisReport {
    pub project_path: PathBuf,
    pub memory_path: PathBuf,
    pub size_bytes: u64,
    pub tokens_est: u64,
    pub recommendations: Vec<MemoryRecommendation>,
    pub elapsed_ms: u64,
}

pub struct MemoryRecommendation {
    pub kind: MemoryRecommendationKind,
    pub message: String,                // user-facing explanation
    pub estimated_savings_bytes: u64,   // 0 when not applicable
    pub line_range: Option<(u32, u32)>, // 1-indexed; for "click to jump"
}

pub enum MemoryRecommendationKind {
    /// File is over 8 KiB. Encourage splitting.
    Oversized,
    /// A section's body matches a section in `~/.claude/CLAUDE.md`
    /// (the global file). Suggest moving to user-level so it does not
    /// duplicate per-project.
    DuplicateOfGlobal,
    /// Two H2 sections inside this file have near-identical bodies
    /// (Levenshtein-normalised similarity > 0.85). Suggest deduping.
    InternalDuplicate,
    /// Frontmatter has fields the schema does not consume (we already
    /// know the schema for `(claude-code, memory)`). Suggest removing.
    UnknownFrontmatterField,
    /// Section under H2 "..." references files / paths that no longer
    /// exist on disk. Stale instructions.
    StaleReference,
}
```

### Heuristic implementations

| Heuristic | How |
|-----------|-----|
| Oversized | `size_bytes > 8192` |
| DuplicateOfGlobal | Split this file by H2 (`## `). Split `~/.claude/CLAUDE.md` similarly. For each section in this file, check if any global section has the same body modulo whitespace + an 80% Levenshtein-similarity threshold. |
| InternalDuplicate | Same H2 split; pairwise compare every section against every other section in this file. |
| UnknownFrontmatterField | Use the existing validator's schema for `(claude-code, memory)`. Walk the parsed frontmatter; flag fields not in `properties`. |
| StaleReference | Regex-extract `[label](path)` and bare `path/to/file.md` references from the body. For each: `project_path.join(reference).exists()`. False = stale. |

Heuristics that don't fire produce no entries in `recommendations`.

### IPC

```rust
#[tauri::command]
pub async fn analyze_memory(
    state: State<'_, Arc<IndexHandle>>,
    project_path: String,
) -> Result<MemoryAnalysisReport, String>;
```

Read-only. Never writes; the "apply" surface for this action is
manual editing, which the existing Editor view handles. The report
includes line ranges so the UI can offer "open in editor at line X".

---

## 17.4 - Action 2: Worktree audit (read-only)

Run `git worktree list --porcelain` per project, parse, return rows.

```rust
pub struct WorktreeReport {
    pub project_path: PathBuf,
    pub worktrees: Vec<WorktreeEntry>,
    pub total_disk_usage_bytes: u64,
    pub elapsed_ms: u64,
}

pub struct WorktreeEntry {
    pub path: PathBuf,
    pub branch: Option<String>,
    pub head: String,                   // commit sha
    pub locked: bool,
    pub mtime_unix: i64,                // mtime of the worktree directory
    pub disk_usage_bytes: u64,          // recursive du
    pub is_main: bool,                  // the project root itself
}

pub enum WorktreeError {
    NotAGitRepo,
    GitCommandFailed(String),
}
```

Disk usage uses a bounded recursive walk: cap at 100 k entries / 60 s
per worktree so a runaway `node_modules` directory does not freeze the
report. Caps surface as an `incomplete: bool` flag on the entry.

### IPC

```rust
#[tauri::command]
pub async fn audit_worktrees(
    state: State<'_, Arc<IndexHandle>>,
    project_path: String,
) -> Result<WorktreeReport, String>;
```

Read-only; never invokes `git worktree remove`.

The UI shows a per-row hint: "To remove this worktree, run
`git worktree remove <path>` in your terminal." A future v2 may add
a button (#14, audit issue #14 of Phase 17 as it were).

---

## 17.5 - Action 3: Reorganise docs

Move every loose top-level `*.md` file into `<project>/docs/`. Excludes
files in a small allowlist that conventionally live at the root.

```rust
pub struct ReorganizeReport {
    pub project_path: PathBuf,
    pub dry_run: bool,
    pub moves: Vec<ReorganizeMove>,
    pub link_rewrites: Vec<LinkRewrite>,
    pub errors: Vec<ReorganizeError>,
    pub elapsed_ms: u64,
}

pub struct ReorganizeMove {
    pub from: PathBuf,                  // project_path / "FOO.md"
    pub to: PathBuf,                    // project_path / "docs" / "FOO.md"
    pub size: u64,
}

pub struct LinkRewrite {
    pub file: PathBuf,                  // file that contains the link
    pub line: u32,                      // 1-indexed
    pub before: String,                 // "[label](./FOO.md)"
    pub after: String,                  // "[label](./docs/FOO.md)"
}

pub struct ReorganizeError {
    pub path: PathBuf,
    pub kind: ReorganizeErrorKind,
    pub message: String,
}

pub enum ReorganizeErrorKind {
    AllowlistConflict,                  // file in allowlist; refusing to move
    DestExists,                         // docs/<file> already exists
    Read,
    Write,
    Rename,
}
```

### Allowlist (files that stay at the root)

```
README.md
CLAUDE.md / CLAUDE.local.md
AGENTS.md
GEMINI.md
CHANGELOG.md
LICENSE.md / LICENSE-APACHE.md / LICENSE-MIT.md
CONTRIBUTING.md
CODE_OF_CONDUCT.md
SECURITY.md
COMPONENTS.md
```

### Link rewriting

Walk every `*.md` file under the project (recursive, bounded by the
same 100 k entries / 60 s as the worktree audit). For each file,
scan for inline links matching:

- `](./FOO.md)`
- `](FOO.md)`
- `](../FOO.md)` only when the source file is itself inside `docs/`
- bare paths like `see FOO.md for details` (regex `\bFOO\.md\b`)

Rewrite to point at the new location. Side-by-side reference list goes
into `link_rewrites` so the dry-run preview can show the user every
change before they apply.

### Atomic semantics

`apply()` (i.e. `dry_run = false`) does:

1. Create `<project>/docs/` if absent.
2. For each file in the move list:
   - Write a `.aseye-pre-reorg-<unix>.bak` sidecar of the source
     bytes (defense in depth).
   - `safe_atomic_write_with_options` the file at the destination.
   - `fs::remove_file` the source ONLY after the destination write
     succeeds.
3. For each link rewrite: load file, replace the literal `before`
   with `after`, write atomically.

All sidecars live next to the source file so a manual recovery is
visible.

### IPC

```rust
#[tauri::command]
pub async fn reorganize_docs(
    state: State<'_, Arc<IndexHandle>>,
    project_path: String,
    dry_run: bool,
) -> Result<ReorganizeReport, String>;
```

The dry run version reports what would happen without writing
anything. The `apply` version performs the moves + link rewrites.

---

## 17.6 - UI

New "Projects" sidebar entry between "Cost" and "Security". The view:

```
┌──────────────────────────────────────────────────────────────┐
│ Projects                            42 indexed memory roots  │
├──────────────────────────────────────────────────────────────┤
│ ▶ Development/projectfinish              ⚠️ 18.4 KB ~ 4.6k tok│
│   ▶ Analyze CLAUDE.md   ▶ Audit worktrees   ▶ Reorganize docs│
│                                                               │
│ ▶ Development/allseeingeye               ✓  6.2 KB ~ 1.6k tok│
│   ▶ Analyze CLAUDE.md   ▶ Audit worktrees   ▶ Reorganize docs│
│                                                               │
│ ▶ Development/artemislens-app            ⚠️ 12.1 KB ~ 3.0k tok│
│   ▶ Analyze CLAUDE.md   ▶ Audit worktrees   ▶ Reorganize docs│
└──────────────────────────────────────────────────────────────┘
```

Click "Analyze CLAUDE.md" → result panel lists recommendations with
"Open in editor" buttons that select the component + jump to the
line range. No dry-run/apply for this one (read-only).

Click "Audit worktrees" → result panel lists worktrees with size,
branch, age. No actions. Cells are copyable.

Click "Reorganize docs" → result panel shows a 2-tab preview:
**Moves** (file → file) and **Link rewrites** (file:line, before /
after). Bottom buttons: "Cancel" + "Apply N moves and M rewrites".
Apply runs the same IPC with `dry_run = false`, surfaces a toast
with the outcome, refreshes the project's panel.

---

## 17.7 - Tests

### Unit (Rust)

For each action:

- Empty case (no projects, no recommendations, no worktrees, no docs).
- Happy path with synthetic project tree.
- Error per `*ErrorKind` variant.
- Idempotent: dry-run twice produces identical reports.

For reorganize specifically:

- Allowlist files refused (`AllowlistConflict`).
- Destination conflict refused (`DestExists`).
- Link rewrite handles all four reference shapes correctly.
- Sidecar exists after `apply`.

### Integration (against developer's real home)

`tests/projects_real_home_proof.rs`, gated on at least one indexed
project memory file existing:

- `list_projects` returns ≥ 5 projects on the developer's machine.
- `analyze_memory` against the developer's `allseeingeye` returns
  size + tokens consistent with the on-disk file.
- `audit_worktrees` against any project with a `.git/` directory
  returns at least the main worktree.
- `reorganize_docs` with `dry_run = true` against a tempdir-copied
  project returns a non-error report (no actual mutations).

### Frontend (vitest)

- `ProjectsView` renders the project list.
- "Analyze CLAUDE.md" populates the recommendations panel from a
  mocked IPC payload.
- "Reorganize docs" preview disables the Apply button until the dry
  run lands.

---

## 17.8 - Out of scope (filed for v2)

- Cross-project bulk actions.
- Worktree removal.
- Project tagging / starring / hiding.
- Editing CLAUDE.md inline within the Projects view (use Editor).
- Auto-detect "this project should have a CLAUDE.md but doesn't"
  (would require the project-marker walker from §17.2).

---

## 17.9 - Risks

1. **Heuristic false positives** - "DuplicateOfGlobal" and
   "InternalDuplicate" use Levenshtein similarity which could fire on
   genuinely-different sections that share a long opening sentence.
   Mitigation: each recommendation is informational, not enforced;
   the user reviews before acting.
2. **Reorganize link breakage** - if a `.md` file uses a non-standard
   link shape we miss, the rewrite leaves it pointing at the old
   location. Mitigation: the dry-run preview shows every rewrite, so
   the user sees what's covered. We document the supported shapes.
3. **Worktree disk usage cap** - bounded walk means a giant worktree
   reports `incomplete: true` rather than blocking the UI. The user
   sees that flag and can investigate manually.
4. **Allowlist drift** - the list of files-that-stay-at-root is
   hand-curated. New conventions (e.g. `MIGRATION.md`) won't be
   caught until we update the list. Surface in the dry-run preview
   so the user can spot wrong moves.

---

## 17.10 - Implementation order

Four self-contained commits:

1. **17.A**: Project discovery + ProjectsView shell + sidebar entry.
   Lists projects with size/token chips. No actions yet.
2. **17.B**: Action 1 - CLAUDE.md analysis. Backend module +
   `analyze_memory` IPC + result panel.
3. **17.C**: Action 2 - Worktree audit. Backend module +
   `audit_worktrees` IPC + result panel.
4. **17.D**: Action 3 - Reorganize docs. Backend module +
   `reorganize_docs` IPC (dry-run + apply) + 2-tab result panel.

Each commit ships a usable feature. The user can stop me after any
of A through D.
