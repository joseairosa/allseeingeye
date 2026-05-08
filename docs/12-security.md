# 12 - Security

How All Seeing Eye keeps the user's secrets safe, reasons about MCP servers connecting to dangerous systems, and surfaces security issues across the indexed agentic stack. This doc is read-then-revisit: come back when adding a new component type or a new tool.

## Threat model

**What we're protecting against**

1. **Credential leakage** - tool config files contain API keys (Anthropic, OpenAI, GitHub, Stripe, AWS, Slack, ...). Risks: clipboard exposure, screen-share, telemetry, log files, error reports, third-party tools shoulder-surfing the index.
2. **Privileged MCP servers** - MCP servers can be configured with unrestricted database access, write-capable cloud APIs, payment APIs, and so on. A skill that calls an MCP server with admin credentials is one prompt away from data destruction.
3. **Malicious imported components** - a downloaded plugin or recipe can ship hooks that run shell commands, MCP servers that spawn arbitrary processes, agents with overly-permissive tool lists.
4. **Path traversal via tool config** - a maliciously crafted plugin manifest references `../../../etc/passwd` or `~/.ssh/id_rsa`.
5. **Indexed-content leakage** - the All Seeing Eye index itself contains parsed copies of the user's CLAUDE.md, .mcp.json, etc. If we leak the index, we leak everything the host tools have.
6. **Update-channel poisoning** - a compromised release replaces the user's app with a malicious build.

**What we're explicitly NOT protecting against**

- **OS-level compromise.** If the user's machine is rooted, no app-level mitigation matters. We rely on macOS / Windows / Linux protections.
- **Compromised host tools.** If Claude Code itself is malicious, we still index it but cannot vet it. We surface what's there; we don't proactively defend against the host.
- **Supply-chain attacks on our deps** at the level of typo-squatting or maintainer-takeover. Mitigated by `cargo-deny`, `cargo-audit`, `pnpm audit`, dependency review, and signed releases - but not eliminated.

## Audit taxonomy

Findings the engine produces, by category and severity. Severity is `low | medium | high | critical`. The Security view in the UI groups by category and sorts by severity.

### A. Secret exposure (HIGH)

Patterns the engine searches in any parsed file body or structured value:

| Pattern | Finding | Severity |
|---------|---------|----------|
| `sk-ant-[a-zA-Z0-9_-]{40,}` | Anthropic API key | critical |
| `sk-[a-zA-Z0-9]{20,}` | OpenAI API key | critical |
| `sk-proj-[a-zA-Z0-9_-]+` | OpenAI project key | critical |
| `ghp_[A-Za-z0-9]{36}` | GitHub PAT (classic) | critical |
| `github_pat_[A-Za-z0-9_]{82}` | GitHub PAT (fine-grained) | critical |
| `gho_[A-Za-z0-9]{36}` | GitHub OAuth token | critical |
| `xoxb-` / `xoxp-` / `xoxa-` | Slack token | critical |
| `AKIA[0-9A-Z]{16}` | AWS access key id | critical |
| `(?i)aws_secret_access_key\s*=\s*[A-Za-z0-9/+=]{40}` | AWS secret access key | critical |
| `eyJ[A-Za-z0-9_-]+\.eyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+` | JWT bearer | high |
| `-----BEGIN (RSA |EC |DSA |OPENSSH |)PRIVATE KEY-----` | Private key block | critical |
| `(?i)password\s*[:=]\s*["']?[^\s"']{8,}` | Bare password value | high |
| `(?i)(api[_-]?key|secret|token)\s*[:=]\s*["']?[A-Za-z0-9_-]{16,}` | Generic secret-shaped value | medium |
| `(?i)Authorization:\s*Bearer\s+[A-Za-z0-9._-]{20,}` | Authorization header value | high |

These regexes are **detection signals only**. Each finding is paired with a `redacted_preview` (first 8 chars + `…` + last 4 chars) so the user can identify the secret without us copying its value into the index, telemetry, logs, or clipboard.

### B. MCP server permission posture (HIGH/CRITICAL)

For every indexed MCP server, infer a posture: `read-only | write | unknown`. Detection rules:

| MCP shape | Inference |
|-----------|-----------|
| Postgres MCP `--read-only` flag set | read-only |
| Postgres MCP without `--read-only` | write (HIGH) |
| Postgres connection string with role known to be read-only (e.g., `aseye_ro`, `*_readonly`, `readonly_user`) | read-only |
| Postgres connection string with admin/superuser role | write (CRITICAL) |
| `@modelcontextprotocol/server-filesystem` with no `--paths-only-readable` flag | write (HIGH) |
| GitHub MCP `--read-only` | read-only |
| GitHub MCP without `--read-only`, with token scope `repo` | write (HIGH) |
| Stripe MCP with live-mode key (`sk_live_*`) | write (CRITICAL) |
| Stripe MCP with test-mode key (`sk_test_*`) | write (LOW) |
| Generic stdio MCP whose env vars include any value matched by Audit category A | always emit a coupled finding |

Findings are paired: a "write-capable Postgres MCP" finding includes the connection string's database name and host (no credentials), so the user can audit the actual production blast radius.

### C. Hook command risk (MEDIUM/HIGH)

Hooks defined in `~/.claude/settings.json` and `~/.cursor/hooks.json` execute shell commands on tool events. Static rules:

| Hook command pattern | Severity |
|----------------------|----------|
| `rm -rf` (anywhere in command) | high |
| `curl | sh` / `wget | sh` | critical |
| Any command writing to `/etc/`, `/usr/`, `~/.ssh/` | high |
| Hooks that pass tool input directly into a shell without escaping | high |
| Hooks calling unknown remote endpoints | medium |

Output: the hook's full command and which event triggers it. The user can suppress per-finding.

### D. Plugin / bundle origin (MEDIUM)

For each indexed plugin:

- If installed from a marketplace we recognise as official (Anthropic's `claude-plugins-official`, our own publish channel): no finding.
- If installed from a third-party GitHub repo: medium finding with the repo URL + last-update timestamp + SHA.
- If installed from an unknown origin (local path, archive): medium finding with location.

The user can mark a plugin as "trusted" to suppress; the suppression survives across sessions.

### E. Path traversal in component config (HIGH)

Any component whose configuration references a path outside the user's home dir, or via `..` segments, gets a finding:

- An MCP server whose `command` is `bash -c '...'` with embedded path manipulation.
- A skill whose body or scripts directory references `~/../...`.
- A hook that writes to `/tmp/<predictable>`.

We don't block these, but they appear in the Security view.

### F. Sensitive directory exposure (MEDIUM)

Any indexed file whose path is inside a sensitive directory:

- `~/.ssh/`
- `~/.aws/`
- `~/.kube/`
- `~/.docker/config.json`
- macOS Keychains
- Windows DPAPI stores

These should never appear under a tool root, but we surface them if they do.

### G. License and provenance (LOW)

- Plugin without a LICENSE file: low finding.
- Plugin with a copyleft license (GPL, AGPL) installed in a closed-source project: low finding (informational).
- Components shipped without a `description` field or `version` field: low finding.

## Suppress flow

Every finding has a "suppress" action. Suppression:

- Is per-component, per-finding-type, optionally with a comment.
- Is stored in the SQLite sidecar (`security_finding_suppression` table).
- Has a TTL: default 30 days. Re-evaluated when the underlying component changes.
- Surfaces in the Security view as a "Suppressed" tab so it doesn't disappear forever.

## Detection cadence

- **On parse**: every component upsert triggers a synchronous secret-detection pass over the parsed body and structured value. Cheap regex sweep; sub-millisecond per file.
- **On scan / load**: full re-evaluation when the user runs a manual scan or opens the Security view. Includes MCP posture inference and hook risk classification, which are heavier than the regex sweep.
- **Background**: optional periodic re-scan (default off). The user can enable a daily background scan in Settings.

## Privacy model and finding data

Findings are stored in a local SQLite table `security_finding`:

```sql
CREATE TABLE security_finding (
  id              TEXT PRIMARY KEY,         -- aseye-finding-<sha256-prefix>
  component_id    TEXT NOT NULL REFERENCES component(id) ON DELETE CASCADE,
  category        TEXT NOT NULL,            -- 'secret' | 'mcp-permission' | 'hook' | ...
  pattern         TEXT NOT NULL,            -- which detection rule fired
  severity        TEXT NOT NULL,            -- 'low' | 'medium' | 'high' | 'critical'
  file_path       TEXT NOT NULL,
  line            INTEGER,
  redacted_preview TEXT NOT NULL,           -- first 8 + ellipsis + last 4
  evidence_json   TEXT NOT NULL,            -- structured detail (db host, hook event, etc.)
  detected_at     INTEGER NOT NULL,
  suppressed      INTEGER NOT NULL DEFAULT 0,
  suppress_reason TEXT,
  suppress_until  INTEGER
);

CREATE TABLE security_finding_suppression (
  component_id    TEXT NOT NULL,
  pattern         TEXT NOT NULL,
  suppressed_at   INTEGER NOT NULL,
  reason          TEXT,
  PRIMARY KEY (component_id, pattern)
);
```

The actual secret value is never stored. Only the redacted preview, the location, and metadata. If a user purges the index (`ctx purge`-style), all findings go with it.

## CI / release pipeline mitigations

Cross-cutting controls in the build pipeline:

- **Gitleaks** scans every PR diff and the full history weekly. Findings post as PR comments and block merge for HIGH severity.
- **cargo-deny** enforces our license allowlist and forbids transitive deps from a small denylist.
- **cargo-audit** reads the GH advisory DB and fails the build on RUSTSEC vulnerabilities.
- **pnpm audit** does the same for npm.
- **Code signing**: macOS notarised + signed; Windows EV cert. Tauri Updater verifies Ed25519 signatures on every update.
- **Update channel separation**: `stable` and `beta`. The signing key is the same; channel selection is a manifest field. A compromised stable channel does not give an attacker the beta channel without the same key.
- **Release workflow review**: any change to `.github/workflows/release.yml` requires a co-signed review.

## In-app surfaces

How findings reach the user:

- **Inventory rows**: a small shield badge appears next to the component name when at least one finding exists.
- **Quick Look**: the right-side panel adds a "Security" section listing findings with severity, redacted preview, and suppress action.
- **Sidebar Health group**: "Security issues" entry with a count + status ring (red for any critical, amber for any high, grey otherwise).
- **Dedicated Security view**: full list with grouping, filtering, and bulk suppress.
- **Toast on first detection**: a subtle bottom-right toast when a critical finding is detected for the first time. Auto-dismisses unless multiple critical findings are stacking.

## Composing with existing privacy guarantees

This document extends, not contradicts, the privacy posture in:

- `02-prd.md` H6 / I3 - "no telemetry without explicit opt-in", "telemetry, when opt-in, excludes content".
- `11-risks.md` SR-1 (reading and exposing secrets), SR-2 (malicious component import), SR-3 (path traversal), PrR-1 (telemetry leaking content), PrR-2 (shared screen leaks via UI).

Concretely:
- The redacted preview is a one-way transformation; the secret value itself never enters the on-disk security finding row, the FTS index, or any telemetry payload.
- Suppression entries are local-only. They never sync.
- The Security view auto-masks even the redacted preview in panic mode (Cmd-Shift-.).

## Out of scope (this version)

- **Active blocking** - we do not refuse to index a component because it contains a secret. We index, surface, and let the user act.
- **Auto-rotation** - we do not rotate keys or call third-party APIs to revoke them.
- **Network egress monitoring** - we do not observe what MCP servers actually do at runtime; we infer from configuration.
- **Sandboxing of running tools** - that's the host tool's job, not ours.

## Future considerations

- Integration with macOS Keychain / Windows Credential Manager so users can store secrets there and have All Seeing Eye prompt MCP servers with the right env at run time. v2.
- Differential privacy for opt-in telemetry on aggregate finding counts (never individual findings).
- Community-maintained ruleset hub (analogous to gitleaks rules) so power users can add patterns. v2 onwards.

## Change log

| Date | Change |
|------|--------|
| 2026-05-08 | Initial draft. |
