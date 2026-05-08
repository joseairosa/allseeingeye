# 04 - Data Sources

Per-tool reference: where each tool stores each component, in what format, how we parse it, and gotchas. This is a reference document - dense but precise. Numbers in column 1 cross-reference the unified taxonomy in `03-component-model.md`.

Conventions:
- `~` = `$HOME` on Unix, `%USERPROFILE%` on Windows.
- Paths shown for macOS / Linux; per-platform variants noted where they differ.
- "Project" means the directory the user opens; "user" is global to the OS user.

## 4.1 Claude Code (Anthropic)

| # | Component | Path | Format | Notes |
|---|-----------|------|--------|-------|
| 2 | Settings (user) | `~/.claude/settings.json` | JSON | Schema at `https://json.schemastore.org/claude-code-settings.json`. |
| 2 | Settings (local) | `~/.claude/settings.local.json` | JSON | Local overrides, gitignored by convention. |
| 2 | Settings (project) | `<repo>/.claude/settings.json` | JSON | Per-project shared settings. |
| 2 | Settings (managed) | platform-specific managed dir | JSON | Enterprise MDM-deployed. |
| 2 | User-level state | `~/.claude.json` | JSON | Per-user state including projects map and user-scoped MCP. |
| 3 | Memory (user) | `~/.claude/CLAUDE.md` | Markdown | Cross-project always-on. |
| 3 | Memory (project) | `<repo>/CLAUDE.md` or `<repo>/.claude/CLAUDE.md` | Markdown | Project-level. |
| 3 | Memory alt | `<repo>/AGENTS.md` | Markdown | OpenAI-conventions equivalent. |
| 4 | Rules (user) | `~/.claude/rules/*.md` | Markdown + frontmatter | Path-specific rules via `paths:` frontmatter. |
| 4 | Rules (project) | `<repo>/.claude/rules/*.md` | Markdown + frontmatter | Same shape as user. |
| 5 | Skills (user) | `~/.claude/skills/<name>/SKILL.md` | Markdown + YAML frontmatter | Optional `scripts/`, `references/`, `assets/`. |
| 5 | Skills (project) | `<repo>/.claude/skills/<name>/SKILL.md` | Same | |
| 5 | Skills (plugin) | `<plugin>/skills/<name>/SKILL.md` | Same | Namespaced as `plugin:skill`. |
| 6 | Commands (user) | `~/.claude/commands/*.md` | Markdown | Same shape as skills. Slash-invoked. |
| 6 | Commands (project) | `<repo>/.claude/commands/*.md` | Markdown | |
| 7 | Agents (user) | `~/.claude/agents/<name>.md` | Markdown + YAML frontmatter | Fields: name, description, tools, model, isolation, hooks. |
| 7 | Agents (project) | `<repo>/.claude/agents/<name>.md` | Same | |
| 8 | MCP (user) | inside `~/.claude.json` (top-level `mcpServers`) | JSON | |
| 8 | MCP (project) | `<repo>/.mcp.json` | JSON | Version-controlled, requires user approval at first run. |
| 8 | MCP (settings-embedded) | inside `settings.json` `mcpServers` | JSON | |
| 8 | MCP (managed) | `managed-mcp.json` | JSON | Enterprise. |
| 9 | Hooks | inside `settings.json` `hooks` | JSON | Events: PreToolUse, PostToolUse, SessionStart, SessionEnd, UserPromptSubmit, UserPromptExpansion, Stop, StopFailure, PreCompact, InstructionsLoaded, plus team events TeammateIdle, TaskCompleted. |
| 9 | Hooks (plugin) | `<plugin>/hooks/hooks.json` | JSON | |
| 10 | Plugins | `~/.claude/plugins/cache/<marketplace>/<plugin>/<version>/` | folder + `.claude-plugin/plugin.json` | |
| 10 | Plugin install index | `~/.claude/plugins/installed_plugins.json` | JSON | |
| 10 | Plugin enable map | inside `settings.json` `enabledPlugins` | JSON | |
| 11 | Marketplaces | `~/.claude/plugins/marketplaces/<id>/` | folder | Plus `extraKnownMarketplaces` in settings. |
| 11 | Marketplace registry | `~/.claude/plugins/known_marketplaces.json` | JSON | |
| 12 | Sessions | `~/.claude/sessions/<id>.json` | JSON | |
| 12 | Projects (per-project session map) | `~/.claude/projects/` | folder | |
| 13 | Tasks | `~/.claude/tasks/<id>/` | folder | Per-session task lists. |
| 13 | TODOs | `~/.claude/todos/` | folder | |
| 14 | Output styles | `~/.claude/output-styles/*.md` | Markdown | |
| 15 | Statusline | inside `settings.json` `statusline` | JSON | |
| 16 | Permissions | inside `settings.json` `permissions` | JSON | allow/deny lists + defaultMode. |

Watch strategy: chokidar/notify on `~/.claude/`, `~/.claude.json`, and known project roots. Debounce 200ms.

## 4.2 Codex (OpenAI)

| # | Component | Path | Format | Notes |
|---|-----------|------|--------|-------|
| 2 | Config | `~/.codex/config.toml` | TOML | Top-level + tables for `[mcp_servers.X]`, `[profiles.X]`, `[projects."..."]`. |
| 2 | Auth | `~/.codex/auth.json` | JSON | Read-only, redact tokens. |
| 2 | Models cache | `~/.codex/models_cache.json` | JSON | |
| 2 | Version | `~/.codex/version.json` | JSON | |
| 3 | Memory | `<repo>/AGENTS.md` | Markdown | Codex's memory convention. |
| 5 | Skills | `~/.codex/skills/<name>/SKILL.md` | Markdown | System skills under `~/.codex/skills/.system/`. |
| 8 | MCP | `[mcp_servers.X]` tables in `config.toml` | TOML | `command`, `args`, `env`. |
| 8 | MCP (per-memory) | `~/.codex/memories/<plugin>/.mcp.json` | JSON | Memories double as plugin equivalents. |
| 10 | Plugin staging | `~/.codex/.tmp/plugins/` | folder | Internal plugin sync. |
| 12 | Sessions | `~/.codex/sessions/YYYY/MM/` | JSONL? | Date-partitioned. |
| 12 | History | `~/.codex/history.jsonl` | JSONL | Append-only chat history. |
| 14 | Personality | `model_reasoning_effort`, `personality`, plus `.personality_migration` marker | TOML | Values: none / friendly / pragmatic. |
| 16 | Approval | `approval_policy` in `config.toml` | TOML | on-request / never. |
| 16 | Sandbox | `sandbox_mode` in `config.toml` | TOML | read-only / workspace-write / danger-full-access. |
| 16 | Project trust | `[projects."<path>"] trust_level = "trusted"` | TOML | |
| -- | Profiles | `[profiles.<name>]` tables | TOML | Switch with `codex --profile`. |
| -- | Logs | `~/.codex/log/codex-tui.log`, `~/.codex/logs_2.sqlite` | text + sqlite | |
| -- | Generated images | `~/.codex/generated_images/<sessionId>/*.png` | binary | Skip indexing; preview on demand. |

Watch strategy: notify on `~/.codex/config.toml`, plus `skills/`, `memories/`, `sessions/`. Tail `history.jsonl` for usage stats.

## 4.3 Cursor

| # | Component | Path | Format | Notes |
|---|-----------|------|--------|-------|
| 2 | Settings (app) | `~/Library/Application Support/Cursor/` | various | macOS path. |
| 2 | CLI args / startup | `~/.cursor/argv.json` | JSON | |
| 4 | Rules (project) | `<repo>/.cursor/rules/*.mdc` | MDC (Markdown + frontmatter) | Frontmatter: description, alwaysApply, globs. |
| 4 | Rules (user) | `~/.cursor/rules/*.mdc` | MDC | |
| 4 | Rules (team) | dashboard-managed | n/a | Shown read-only when synced locally. |
| 3 | Memory | `<repo>/AGENTS.md` or `.cursorrules` | Markdown | `.cursorrules` is the legacy form. |
| 6 | Commands | `<repo>/.cursor/commands/*.md` | Markdown | |
| 8 | MCP (user) | `~/.cursor/mcp.json` | JSON | |
| 8 | MCP (project) | `<repo>/.cursor/mcp.json` | JSON | |
| 9 | Hooks | `~/.cursor/hooks.json` (and project) | JSON | Events include `beforeSubmitPrompt`. |
| 10 | Extensions | `~/.cursor/extensions/` | VS Code marketplace | Read-only summary; we don't manage VS Code extensions. |

Watch strategy: notify on `~/.cursor/`, plus open project's `.cursor/`.

## 4.4 Antigravity (Google)

| # | Component | Path | Format | Notes |
|---|-----------|------|--------|-------|
| 3 | Memory (global) | `~/.gemini/GEMINI.md` | Markdown | Shared with Gemini CLI - conflicts can happen. |
| 4 | Rules (workspace) | `<repo>/.agents/rules/<name>.md` | Markdown | |
| 6 | Workflows (global) | `~/.gemini/antigravity/global_workflows/<name>.md` | Markdown | Slash-invoked. |
| 6 | Workflows (workspace) | `<repo>/.agents/workflows/<name>.md` | Markdown | |
| 5 | Skills (global) | `~/.gemini/antigravity/skills/<name>/SKILL.md` | Markdown + YAML frontmatter | Optional `scripts/`, `references/`, `assets/`. |
| 5 | Skills (workspace) | `<repo>/.agent/skills/<name>/SKILL.md` | Same | Note singular `.agent/` for skills, plural `.agents/` for rules/workflows. |
| 8 | MCP | `~/.gemini/antigravity/mcp_config.json` | JSON | |
| 12 | Conversations | `~/.gemini/antigravity/conversations/` | folder | Threaded chats. |
| 3 | Brain | `~/.gemini/antigravity/brain/` | folder | Persistent memory store. |
| 7 | Agents | spawnable via Agent Manager | n/a | No on-disk format yet; we surface running ones via the app's IPC if exposed, otherwise read-only registry. |

Watch strategy: notify on `~/.gemini/`, plus open project's `.agents/` and `.agent/`.

Gotcha: `~/.gemini/GEMINI.md` is shared with the Gemini CLI; mutating it from All Seeing Eye affects both tools. Surface this in the UI.

## 4.5 Cline (VS Code extension)

| # | Component | Path | Format | Notes |
|---|-----------|------|--------|-------|
| 2 | Extension state | `~/Library/Application Support/Code/User/globalStorage/saoudrizwan.claude-dev/` | various | Includes settings, history, MCP servers. macOS path; Linux `~/.config/Code/User/...`, Windows `%APPDATA%/Code/User/...`. |
| 4 | Custom rules / instructions | `<repo>/.clinerules` or extension state | text | |
| 6 | Workflows | extension state JSON | JSON | Plan / Act mode workflows. |
| 9 | Hooks | extension state JSON | JSON | |
| 13 | Tasks (Kanban) | extension state | JSON | |
| 8 | MCP servers | extension state JSON (cline_mcp_settings.json) | JSON | |
| 12 | History | extension state JSONL | JSONL | |
| -- | .clineignore | `<repo>/.clineignore` | text | gitignore-shaped. |

Watch strategy: notify on extension state dir + project root.

## 4.6 Continue.dev

| # | Component | Path | Format | Notes |
|---|-----------|------|--------|-------|
| 2 | Config | `~/.continue/config.yaml` (current) or `config.json` (legacy) | YAML/JSON | The product pivoted; recent versions use hub.continue.dev assemblies. |
| 4 | Rules | `~/.continue/rules/*.md` or via hub assembly | Markdown | |
| 6 | Slash commands | `~/.continue/prompts/*.prompt.md` or assembly | Markdown | |
| 7 | Modes | `~/.continue/modes/*.json` or assembly | JSON | |
| 8 | MCP servers | inside `config.yaml` | YAML | |
| -- | Hub assemblies | `~/.continue/assemblies/` | folder | Imported from hub.continue.dev. |

Watch strategy: notify on `~/.continue/`.

## 4.7 Windsurf (Codeium / Cognition)

| # | Component | Path | Format | Notes |
|---|-----------|------|--------|-------|
| 3 | Global rules | `~/.codeium/windsurf/global_rules.md` | Markdown | Always-on. |
| 4 | Workspace rules | `<repo>/.windsurf/rules/<name>.md` | Markdown + frontmatter | Frontmatter: trigger (always_on / model_decision / glob / manual), globs. |
| 6 | Workflows | `<repo>/.windsurf/workflows/*.md` | Markdown | |
| 5 | Skills | `~/.codeium/windsurf/skills/` | Markdown | Recently added concept. |
| 8 | MCP | Cascade MCP config | JSON | |
| 3 | Memories | per-project memory store | varies | Cascade-managed. |

Path note: macOS often `~/Library/Application Support/Windsurf/`; settings often live in both locations. We watch both.

## 4.8 Aider

| # | Component | Path | Format | Notes |
|---|-----------|------|--------|-------|
| 2 | Config | `~/.aider.conf.yml` or `<repo>/.aider.conf.yml` | YAML | Equivalent to env vars `AIDER_*` and CLI flags. |
| 3 | Conventions | path referenced via `read:` setting (commonly `<repo>/CONVENTIONS.md`) | Markdown | |
| 12 | History | `<repo>/.aider.chat.history.md` and `.aider.input.history` | Markdown | |
| -- | API keys | `.env` with `AIDER_*` | text | Read-only redacted view. |
| -- | In-chat slash commands | runtime-only | n/a | We document them; no on-disk format. |

## 4.9 Goose (Block / AAIF)

| # | Component | Path | Format | Notes |
|---|-----------|------|--------|-------|
| 2 | Config | `~/.config/goose/config.yaml` | YAML | Confirm post-AAIF migration. |
| 6 | Recipes | `~/.config/goose/recipes/*.yaml` | YAML | Reusable workflows: prompts, parameters, MCP servers, subrecipes. |
| 8 | Extensions (MCP-based) | inside config.yaml or `~/.config/goose/extensions/` | YAML | |
| 12 | Sessions | `~/.config/goose/sessions/` | varies | |
| -- | Subrecipes | reference graph in recipes | YAML | |

Goose recipes are the killer concept here: they bundle prompt + parameters + extensions + subrecipes into one shareable file. Useful as a model for our **Bundle** export.

## 4.10 GitHub Copilot

| # | Component | Path | Format | Notes |
|---|-----------|------|--------|-------|
| 3 | Repo instructions | `<repo>/.github/copilot-instructions.md` | Markdown | |
| 4 | Path instructions | `<repo>/.github/instructions/<name>.md` | Markdown | Glob-scoped. |
| 6 | Custom prompts | `<repo>/.github/prompts/<name>.prompt.md` | Markdown | |
| 7 | Chat modes | `<repo>/.github/chatmodes/<name>.chatmode.md` | Markdown | |
| 2 | User settings (VS Code) | inside VS Code settings | JSON | |

## 4.11 Roo Code (sunsetting May 15 2026)

| # | Component | Path | Format | Notes |
|---|-----------|------|--------|-------|
| 2 | Extension state | VS Code globalStorage | various | We may keep read-only support post-sunset for users to migrate off. |
| 7 | Custom modes | extension state JSON | JSON | |
| 4 | Custom instructions | extension state | text | |
| 8 | MCP | extension state JSON | JSON | |

Roo Code is in maintenance/EOL. Lower priority for index v1.

## 4.12 Kilo Code

| # | Component | Path | Format | Notes |
|---|-----------|------|--------|-------|
| 2 | Config (CLI) | `~/.config/kilocode/config.json` | JSON | |
| 4 | Custom rules | per spec | text | Confirm at integration time. |
| -- | KiloClaw / Gateway | not modelled | n/a | Cloud features; out of scope. |

## 4.13 Zed

| # | Component | Path | Format | Notes |
|---|-----------|------|--------|-------|
| 2 | Settings | `~/.config/zed/settings.json` | JSONC | `assistant`, `language_models`, `context_servers` keys. |
| 4 | Rules | `<repo>/.rules` or AGENTS.md | Markdown | |
| 7 | External agents | declared in settings via Agent Client Protocol | JSON | |
| 8 | Context servers | inside settings.json `context_servers` | JSONC | Zed's name for MCP servers. |
| 12 | Threads | `~/.local/share/zed/threads/` | sqlite | |

## 4.14 Augment

| # | Component | Path | Format | Notes |
|---|-----------|------|--------|-------|
| -- | IDE-resident; Context Engine MCP exposed | n/a | n/a | Limited public file format. We may parse VS Code extension storage similarly to Cline. |

Defer to v2 unless a clear file-system surface materialises.

## 4.15 JetBrains Junie

| # | Component | Path | Format | Notes |
|---|-----------|------|--------|-------|
| -- | Inside JetBrains IDE | platform settings | XML | JetBrains stores in `.idea/` and platform config dirs. |

Defer to v2.

## 4.16 Sourcegraph Amp

Limited public docs at time of research. Track for v2.

---

## Cross-tool primitives

### Model Context Protocol (MCP)

Open standard; the same JSON config shape (with minor variations) appears in: Claude Code, Cursor, Cline, Goose, Continue, Antigravity, Zed (as "context servers"), Windsurf (Cascade MCP). Single canonical schema in our internal model:

```json
{
  "name": "github",
  "transport": "stdio",
  "command": "npx",
  "args": ["-y", "@modelcontextprotocol/server-github"],
  "env": { "GITHUB_TOKEN": "***" }
}
```

Or:

```json
{
  "name": "stripe",
  "transport": "http",
  "url": "https://mcp.stripe.com",
  "headers": { "Authorization": "Bearer ***" }
}
```

We dedupe across tools when `command + args` (stdio) or `url` (http/sse) match exactly. Surface as one server with multiple "registrations".

### Memory file family

`CLAUDE.md`, `AGENTS.md`, `GEMINI.md`, `.cursorrules`, `.clinerules`, `copilot-instructions.md`, `CONVENTIONS.md`. We treat them all as `Memory` instances. The `flavour` field preserves original filename so writes go back to the right place.

### Frontmatter conventions

YAML frontmatter at top of file, between two `---` lines, is the de-facto standard across Claude Code, Cursor, Antigravity, Windsurf, Copilot. TOML frontmatter is rare. JSON-only configs use sibling files.

## Parsing strategy

| Format | Parser |
|--------|--------|
| JSON | `serde_json` (Rust) on the backend. |
| TOML | `toml` crate. |
| YAML | `serde_yaml`. |
| Markdown + frontmatter | `gray-matter`-equivalent: `serde_yaml` for frontmatter, raw body. |
| MDC (Cursor) | Same as Markdown + frontmatter. |
| JSONL | line-by-line stream, lazy. |
| SQLite | `rusqlite` for read-only inspection. |

## Conflict and merging

When two tools claim the same path (e.g., `AGENTS.md` works for Cursor, Codex, and Claude Code simultaneously), the file appears under each tool's tree but points to the same on-disk node. Editing once updates all views.

## What we don't read

- Encrypted password stores or OS keychains.
- Project-private `.env` files (we surface their location; never the values).
- Binary cache files except metadata.

## Update cadence

Most tools update their format and add new component types every few months. We treat `04-data-sources.md` as living and track each tool's CHANGELOG. New types arrive as a parser plus a UI affordance, never as a blocking issue.
