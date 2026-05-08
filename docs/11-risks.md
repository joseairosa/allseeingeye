# 11 - Risks

Things that can go wrong, the impact if they do, and the plan to handle it. Risks are scored Likelihood x Impact, each on a 1-5 scale. Score = L x I. Anything > 12 is monitored monthly; > 16 monthly with a written mitigation plan.

## Technical risks

### TR-1 - Tool format churn

| L | I | Score |
|---|---|-------|
| 4 | 4 | 16 |

**What.** Claude Code, Codex, Antigravity, Cursor, and Cline ship updates every few weeks. New component types, renamed fields, new file paths.

**Mitigation.**
- Subscribe to each tool's changelog / GitHub releases via RSS.
- Bundled per-tool schemas live in app, are updated via app release; not gated on full reparse work.
- Lenient parsers: unknown frontmatter fields are preserved verbatim and round-trip on save.
- Field renames handled with a mapping layer in the parser; deprecated names supported for one major version cycle.
- Feature flag per tool descriptor lets us ship a half-updated parser as "limited support" without breaking the main flow.

### TR-2 - Atomic write fails on a real-world filesystem edge case

| L | I | Score |
|---|---|-------|
| 2 | 5 | 10 |

**What.** Cross-device renames (rare on home dirs but possible with bind mounts), case-insensitive filesystems on macOS confusing temp file resolution, network filesystems (NFS, SMB) where rename is not atomic.

**Mitigation.**
- Pre-flight check on first write to a path: detect filesystem type and capability.
- On unsupported filesystems, fall back to a write-and-flush-then-replace strategy with `fsync` in between, and surface a one-time UI banner that writes to this directory are non-atomic.
- Snapshot the original bytes before write to a sidecar `.aseye-backup` file; restore on detected corruption.
- Mandatory soak test: 10k random writes across mounted volumes (APFS, ext4, ntfs, exfat, smb) in CI.

### TR-3 - Watcher saturation on Linux inotify

| L | I | Score |
|---|---|-------|
| 3 | 3 | 9 |

**What.** `fs.inotify.max_user_watches` defaults to 8192 on some distros; large monorepos exceed this.

**Mitigation.**
- Watch only the specific subdirectories we care about (`.claude/`, `.cursor/`, `.agents/`, `.github/`, etc.), never project root recursively.
- On `ENOSPC` from inotify, surface a UI warning with a suggested `sudo sysctl -w fs.inotify.max_user_watches=524288` and the rationale.
- Fallback: 30s polling for the watch root if watcher install fails.

### TR-4 - SQLite corruption

| L | I | Score |
|---|---|-------|
| 1 | 4 | 4 |

**What.** Sudden power loss, disk full, malformed migration.

**Mitigation.**
- WAL mode, `PRAGMA synchronous = NORMAL`.
- `PRAGMA integrity_check` on launch; on failure, archive and rebuild from disk (sidecar metadata is at risk; user warned).
- Sidecar daily backup (last 7 days) outside of SQLite for the precious user data.

### TR-5 - Monaco bundle bloat

| L | I | Score |
|---|---|-------|
| 3 | 2 | 6 |

**What.** Monaco's full bundle is large (4 MB+ gz). We need YAML / JSON / Markdown only.

**Mitigation.**
- Custom Monaco build via `monaco-editor-webpack-plugin` equivalent that ships only required language workers.
- Lazy load Monaco only when Editor view is opened.

### TR-6 - Tauri 2.x maturity

| L | I | Score |
|---|---|-------|
| 3 | 3 | 9 |

**What.** Tauri 2 is stable but the plugin ecosystem has gaps; some auto-updater signing flows have rough edges on Windows.

**Mitigation.**
- Stick to Tauri's first-party plugins (updater, dialog, fs, notification).
- Self-host any niche functionality in Rust crates we control.
- CI matrix includes Windows from day one to catch regressions early.

### TR-7 - Tool detection collisions

| L | I | Score |
|---|---|-------|
| 2 | 3 | 6 |

**What.** A user has both Antigravity and Gemini CLI; they share `~/.gemini/GEMINI.md`. Mutating it from All Seeing Eye affects both tools, possibly invalidating the user's mental model.

**Mitigation.**
- Surface a "shared with" badge on shared-fate files.
- A confirmation modal on first edit clarifies the impact; user can choose to proceed and remember the choice.

## Security risks

### SR-1 - Reading and exposing secrets

| L | I | Score |
|---|---|-------|
| 4 | 5 | 20 |

**What.** Tool config files contain API keys (OpenAI, Anthropic, GitHub, Stripe). If we leak them via UI clipboard, logs, telemetry, or screenshots, the user is exposed.

**Mitigation.**
- All key-shaped fields (`*_TOKEN`, `*_KEY`, `*_SECRET`, `*password*`, `Authorization`, `Cookie`, etc.) detected via a curated regex set and masked by default in the UI.
- Reveal requires an explicit click and a 5-second auto-mask afterwards.
- Never copy a secret to clipboard automatically.
- Diagnostics export sanitises secrets even if the user opts in.
- Telemetry (opt-in, v1+) excludes any field we recognise as a secret.
- Internal logging never contains parsed values, only metadata.

### SR-2 - Malicious component import

| L | I | Score |
|---|---|-------|
| 3 | 5 | 15 |

**What.** A user imports a bundle that contains a hook running `rm -rf` or a malicious MCP stdio command.

**Mitigation.**
- Import preview shows every command and hook the bundle would install, with a syntactic risk badge ("runs shell", "writes outside project", etc.).
- Imports always require confirmation; never silent.
- We sign our own bundle exports and verify signatures on import where present; unsigned bundles get a stronger warning.
- We do **not** execute commands during import.

### SR-3 - Path traversal via tool config

| L | I | Score |
|---|---|-------|
| 2 | 4 | 8 |

**What.** A maliciously crafted plugin manifest references a path like `../../../etc/passwd`.

**Mitigation.**
- All path resolution goes through a canonicaliser that asserts the result lies within an expected root.
- Symlinks pointing outside roots are not followed.
- Globs are evaluated with a "trusted base" model.

### SR-4 - Auto-update supply chain

| L | I | Score |
|---|---|-------|
| 2 | 5 | 10 |

**What.** Compromised release pipeline pushes a malicious version.

**Mitigation.**
- All releases signed with our developer cert; signature verified by Tauri updater before install.
- Release pipeline in GitHub Actions uses OIDC for AWS/Apple keys; no long-lived secrets in CI.
- Two-person review for any change to the release workflow.

### SR-5 - macOS full disk access

| L | I | Score |
|---|---|-------|
| 2 | 3 | 6 |

**What.** Some tool paths sit under `~/Library/Application Support/`, which on modern macOS requires Full Disk Access permission for non-Apple-platform apps.

**Mitigation.**
- Surface the permission requirement explicitly in onboarding when we detect missing access.
- Provide a deep link to System Settings.
- Gracefully degrade: the app still works for tools whose paths we can read.

## UX risks

### UR-1 - Editor edits clobber on-disk changes from the host tool

| L | I | Score |
|---|---|-------|
| 3 | 4 | 12 |

**What.** The user opens a file in our Editor; the host tool writes the same file mid-session; the user saves and overwrites the host tool's change.

**Mitigation.**
- Editor watches the file's hash since open. On change, surface a banner: "this file changed externally - reload, diff, or save anyway".
- Save anyway is intentionally last in the action order.
- 3-pane diff with merge into raw view.

### UR-2 - User confused about which scope they're editing

| L | I | Score |
|---|---|-------|
| 3 | 3 | 9 |

**What.** "I edited my CLAUDE.md and nothing changed in this project". Often: edited the user-scoped one, not the project-scoped one.

**Mitigation.**
- Scope is shown prominently in Inventory, Quick Look, and Editor header.
- A "shadowed by" badge surfaces when a user-scoped file is overridden by a project-scoped one with the same role.

### UR-3 - Onboarding overwhelm

| L | I | Score |
|---|---|-------|
| 3 | 3 | 9 |

**What.** First launch shows 200+ components and feels noisy.

**Mitigation.**
- Default Inventory sort: "recently used"; if no usage data, "recently modified".
- Coachmark tour points users to filter chips immediately.
- Hide "internal" components (e.g., Claude Code's `recall-rlm` plugin internals) under a "Show system components" toggle.

### UR-4 - Map is a toy unless it tells you something

| L | I | Score |
|---|---|-------|
| 3 | 3 | 9 |

**What.** Force-directed graphs are pretty but rarely useful unless they answer a real question.

**Mitigation.**
- Ship Map only with concrete use cases:
  - "What does this plugin install?"
  - "What MCP servers do my agents depend on?"
  - "Which skills reference scripts that don't exist?"
- Each cluster mode answers a specific question; we name them after the question, not the structure.

### UR-5 - Drift fatigue

| L | I | Score |
|---|---|-------|
| 3 | 3 | 9 |

**What.** A user sees 20 drift pairs and never resolves any.

**Mitigation.**
- Default Drift list is sorted by intent: real divergence first, micro-formatting last.
- Quick "ignore for 30 days" reduces clutter without permanent dismissal.
- Empty drift state ("no drift detected") is presented with quiet relief, not a sales pitch.

## Product risks

### PR-1 - Niche audience

| L | I | Score |
|---|---|-------|
| 3 | 3 | 9 |

**What.** Most devs run one or two agentic tools, not five. Our value prop is sharpest for power users; the broader market may not exist yet.

**Mitigation.**
- Target Ring 1 (power users) for MVP and v1; do not water down value to chase Ring 2/3.
- The component model and editor are useful even for one-tool users (their CLAUDE.md, hooks, MCP servers); ensure they are first-class.

### PR-2 - Reactive to ecosystem changes

| L | I | Score |
|---|---|-------|
| 3 | 4 | 12 |

**What.** A foundational tool changes its model (e.g., Anthropic deprecates plugin format X), and we spend weeks chasing.

**Mitigation.**
- Tool descriptors are declarative; new shapes are incremental work, not re-architectures.
- Fast tracker: 24-48h turnaround on parser updates for major tools.
- A public dashboard of "tool support status" sets user expectation honestly.

### PR-3 - "Just edit the files in VS Code" objection

| L | I | Score |
|---|---|-------|
| 3 | 3 | 9 |

**What.** A user's habit is "I'll just open `~/.claude/skills/spec/SKILL.md` in Cursor". Why install our app?

**Mitigation.**
- Cross-tool surfaces (drift, MCP health, conversion, bundle, search across all tools) are uniquely ours.
- The form-driven editor with per-tool schema validation is faster for these specific files than a generic editor.
- Performance and aesthetics: starting our app to edit one of these files is faster and more satisfying than navigating in a generic editor.

### PR-4 - We become a coupling layer that locks the ecosystem

| L | I | Score |
|---|---|-------|
| 1 | 5 | 5 |

**What.** Tools change their formats, but our normalised model makes us a shadow standard, and we resist innovation.

**Mitigation.**
- Source of truth is always disk. We never own the format.
- Lossless round-tripping is a hard rule (verified by property tests).
- Public roadmap commitments to support new formats within N weeks of release.

## Operational risks

### OR-1 - Single-engineer bus factor

| L | I | Score |
|---|---|-------|
| 4 | 4 | 16 |

**What.** José is the primary builder and user. If life happens, no continuity.

**Mitigation.**
- All decisions in `docs/` (this folder).
- Public-facing roadmap.
- Bring on a second contributor by v1.
- All code in a single repo, public-readable as soon as MVP ships.

### OR-2 - Codesigning hassle

| L | I | Score |
|---|---|-------|
| 4 | 2 | 8 |

**What.** Apple Developer membership, EV cert for Windows, Linux package signing. Each is a multi-day errand with surprise costs.

**Mitigation.**
- Set them all up before MVP code-freeze.
- Document the renewal calendar.

### OR-3 - Release infrastructure outage

| L | I | Score |
|---|---|-------|
| 2 | 2 | 4 |

**What.** GitHub Actions outage delays a release.

**Mitigation.**
- Releases are not time-critical for MVP; we can wait.
- Manual release procedure documented.

## Privacy risks

### PrR-1 - Telemetry leaking content

| L | I | Score |
|---|---|-------|
| 2 | 5 | 10 |

**What.** Once we ship telemetry (post-MVP), a bug includes file content in metrics.

**Mitigation.**
- Telemetry payload schema defined in code; all fields are typed primitives or whitelisted enums.
- Schema validator on the receive side rejects extra fields.
- Internal review before any new metric is added.
- Off by default, opt-in only.

### PrR-2 - Shared screen leaks secrets via the UI

| L | I | Score |
|---|---|-------|
| 3 | 3 | 9 |

**What.** User screenshares while a secret is revealed in Editor.

**Mitigation.**
- Auto-mask after 5 s of reveal.
- Panic mode (Cmd-Shift-.) instantly masks all values + closes Quick Look.

## Risk register summary

```
                  L1   L2   L3   L4   L5
            I1    -    -    -    -    -
            I2    -    OR3  -    -    -
            I3    -    SR3  -   PR3  -    UR3 PR1 UR2 UR4 UR5 PrR2 OR2
            I4    -    SR3  TR3 OR1  -    UR1 PR2 TR1 TR4
            I5    -    SR4  SR2 SR1  -    PrR1 TR2
```

(Schematic; precise placement above.)

## Review cadence

- Weekly during MVP build: each top-row risk reviewed in a 15-minute meeting.
- Monthly post-MVP.
- A risk gets archived once score < 5 for two consecutive reviews.
- A risk gets escalated to a written mitigation doc when score >= 16.
