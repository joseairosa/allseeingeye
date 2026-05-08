# 03 - Component Model

This is the conceptual heart of All Seeing Eye. It defines the **unified taxonomy** that we use internally, regardless of which tool a component originates from. The taxonomy is normalised so that a Claude Code subagent and an Antigravity agent appear as instances of the same kind.

## Design principles

1. **Tool-agnostic in the abstract, tool-faithful in the concrete.** A `Skill` from Claude Code and a `Skill` from Antigravity are both `Skill` in our model, but we preserve the original frontmatter and on-disk format byte-for-byte. We never lose information by normalising.
2. **Composable.** A `Plugin` is a bundle of other components. A `Marketplace` is a collection of installable `Plugins`. A `Bundle` (our own concept) is a user-curated export.
3. **Identifiable.** Every component has a stable URI of the form `aseye://<tool>/<scope>/<type>/<name>` where scope is `user|project|enterprise|plugin`.
4. **Source of truth is the file system.** Our internal index is a cache. If the cache and disk disagree, disk wins. Always.

## Top-level types

The 16 first-class component types we model.

| # | Type | Purpose |
|---|------|---------|
| 1 | **Tool** | A registered host tool installation (Claude Code, Codex, ...). Root of the tree. |
| 2 | **Settings** | Top-level configuration for a tool. JSON / TOML / YAML. |
| 3 | **Memory** | Always-on persistent instructions. CLAUDE.md, AGENTS.md, GEMINI.md, .cursorrules, copilot-instructions.md. |
| 4 | **Rule** | Conditional / scoped instructions. Frontmatter-driven (alwaysApply, glob, model_decision, manual). |
| 5 | **Skill** | Model-invoked capability. SKILL.md + optional scripts/, references/, assets/. |
| 6 | **Command** | User-invoked saved prompt. `/foo` slash commands, workflows. |
| 7 | **Agent** | Specialised AI worker with own system prompt, tool list, model. Subagents, modes, roles. |
| 8 | **MCP Server** | External tool integration via Model Context Protocol. stdio / SSE / HTTP. |
| 9 | **Hook** | Event-triggered automation. Command, prompt, agent, http, mcp_tool. |
| 10 | **Plugin** | Versioned bundle: skills + agents + commands + hooks + MCP, with a manifest. |
| 11 | **Marketplace** | Registry / source of installable plugins. |
| 12 | **Session** | A single conversation history, read-only here. |
| 13 | **Task** | A persisted to-do or task-list item, possibly cross-session. |
| 14 | **Output Style** | Tone / personality profile that shapes responses. |
| 15 | **Statusline** | Visual status display config in CLI / IDE. |
| 16 | **Permission Profile** | Allow/deny lists, sandbox modes, approval policies. |

`Auth` (API keys, tokens) is intentionally not a first-class managed type. We surface it read-only and redacted under `Settings`.

## Common shape

Every component, regardless of type, has:

```ts
interface Component<T extends ComponentType> {
  // Identity
  id: string;                    // aseye://claude-code/user/skill/spec
  type: T;
  name: string;                  // human-readable; from frontmatter or filename
  displayName?: string;          // optional override
  description?: string;          // from frontmatter, README, or first paragraph

  // Provenance
  tool: ToolId;                  // claude-code | codex | cursor | antigravity | ...
  scope: Scope;                  // user | project | enterprise | plugin
  origin: Origin;                // builtin | user-created | plugin | marketplace
  pluginId?: string;             // when scope === plugin

  // Filesystem
  path: string;                  // absolute path to the canonical file
  files: string[];               // all related files (scripts/, references/, ...)
  format: Format;                // markdown | json | yaml | toml | jsonl | binary
  size: number;
  mtime: Date;
  ctime: Date;

  // Parsing
  raw: string | Buffer;          // unparsed canonical
  parsed: ParsedT[T];             // typed parsed view (frontmatter + body)
  parseErrors: ParseError[];

  // State
  enabled: boolean;              // tool-reported or inferred
  health?: HealthState;          // for stateful types like MCP servers
  lastUsedAt?: Date;             // from session/history mining
  useCount?: number;

  // User additions
  tags: string[];                // user-applied
  pinned: boolean;
  notes?: string;                // user-added, stored in our sidecar

  // Cross-references
  relations: Relation[];         // see below
}
```

## Relations

Components rarely live alone. We track explicit and inferred relations.

| Relation | From | To | Source |
|----------|------|-----|--------|
| `bundles` | Plugin | Skill / Agent / Command / Hook / MCP | Manifest |
| `publishedBy` | Plugin | Marketplace | Manifest |
| `references` | Skill | Skill (script chain) | Body parse |
| `imports` | Memory | Memory | `@import` / `@./other.md` |
| `triggers` | Hook | Tool | Hook event |
| `activatedBy` | Skill / Command | Glob / Trigger | Frontmatter |
| `equivalentTo` | Memory(claude) | Memory(cursor) | User assertion or fuzzy match |
| `dependsOn` | Plugin | MCP server | Manifest |

Relations power the **Map** view (force-directed graph) and the **drift** detection.

## Type-specific schemas

### 3.1 Tool

```ts
interface Tool {
  id: ToolId;                    // 'claude-code' | 'codex' | 'cursor' | ...
  displayName: string;           // 'Claude Code'
  installation: {
    detected: boolean;
    binary?: string;             // e.g., /usr/local/bin/claude
    version?: string;
    rootPaths: string[];         // ['~/.claude', '/Users/.../Library/Application Support/...']
  };
  componentRoots: ComponentRoot[]; // where to look for each Type
}
```

### 3.2 Settings

```ts
interface Settings {
  ...Component<'settings'>;
  format: 'json' | 'toml' | 'yaml';
  parsed: Record<string, unknown>; // top-level key-value
  schema?: JSONSchema;             // bundled per-tool schema
}
```

### 3.3 Memory

```ts
interface Memory {
  ...Component<'memory'>;
  format: 'markdown';
  parsed: {
    body: string;
    headings: Heading[];          // for the outline view
    imports: string[];             // resolved @-imports
  };
  scope: 'user' | 'project' | 'managed';
  flavour: 'CLAUDE.md' | 'AGENTS.md' | 'GEMINI.md' | '.cursorrules' | 'copilot-instructions.md' | 'CONVENTIONS.md';
}
```

### 3.4 Rule

```ts
interface Rule {
  ...Component<'rule'>;
  format: 'markdown' | 'mdc';
  parsed: {
    frontmatter: {
      description?: string;
      alwaysApply?: boolean;        // Cursor
      globs?: string[];             // Cursor / Claude path-specific rules
      trigger?: 'always_on' | 'model_decision' | 'glob' | 'manual'; // Windsurf
      paths?: string[];             // Claude Code conditional rules
    };
    body: string;
  };
}
```

### 3.5 Skill

```ts
interface Skill {
  ...Component<'skill'>;
  format: 'markdown';
  parsed: {
    frontmatter: {
      name?: string;                // optional in some tools
      description: string;
      'disable-model-invocation'?: boolean; // Claude Code
      tools?: string[];
    };
    body: string;
  };
  assets: {
    scripts: string[];              // optional scripts/ subfolder
    references: string[];           // optional references/
    other: string[];                // assets/, etc.
  };
}
```

### 3.6 Command (a.k.a. Workflow)

```ts
interface Command {
  ...Component<'command'>;
  format: 'markdown';
  parsed: {
    frontmatter: {
      description?: string;
      args?: ArgumentSchema[];
    };
    body: string;
  };
  trigger: 'slash';                 // user-invoked
  invocationName: string;           // /foo
}
```

### 3.7 Agent

```ts
interface Agent {
  ...Component<'agent'>;
  format: 'markdown';
  parsed: {
    frontmatter: {
      name: string;
      description: string;
      tools?: string[];             // allowed tool names
      model?: string;               // 'sonnet', 'opus', 'gemini-2.0-flash', ...
      isolation?: 'worktree' | null;
      hooks?: AgentHook[];          // inline hooks
    };
    body: string;                   // becomes the system prompt
  };
}
```

### 3.8 MCP Server

```ts
interface McpServer {
  ...Component<'mcp'>;
  format: 'json' | 'toml';
  parsed: {
    name: string;
    transport: 'stdio' | 'sse' | 'http';
    command?: string;               // stdio
    args?: string[];                // stdio
    url?: string;                   // sse / http
    env?: Record<string, string>;   // values masked when displayed
    headers?: Record<string, string>;
  };
  exposes?: {                       // populated by health probe
    tools: McpTool[];
    prompts: McpPrompt[];
    resources: McpResource[];
  };
  health: McpHealth;                // up | down | degraded | unprobed
}
```

### 3.9 Hook

```ts
interface Hook {
  ...Component<'hook'>;
  format: 'json';
  parsed: {
    event: HookEvent;
    matcher?: string;
    handler:
      | { type: 'command'; command: string; timeout?: number }
      | { type: 'prompt'; prompt: string }
      | { type: 'agent'; agent: string }
      | { type: 'http'; url: string }
      | { type: 'mcp_tool'; server: string; tool: string };
    async?: boolean;
  };
}

type HookEvent =
  | 'PreToolUse' | 'PostToolUse'
  | 'SessionStart' | 'SessionEnd'
  | 'UserPromptSubmit' | 'UserPromptExpansion'
  | 'Stop' | 'StopFailure'
  | 'PreCompact'
  | 'InstructionsLoaded'
  | 'beforeSubmitPrompt'           // Cursor
  | 'TeammateIdle' | 'TaskCompleted' // Claude Code teams
  | string;                         // tool-specific
```

### 3.10 Plugin

```ts
interface Plugin {
  ...Component<'plugin'>;
  format: 'json';                   // .claude-plugin/plugin.json
  parsed: {
    name: string;
    version: string;
    description?: string;
    author?: string;
    homepage?: string;
    skills: string[];               // ids of contained components
    agents: string[];
    commands: string[];
    hooks: string[];
    mcp: string[];
  };
  source: {
    kind: 'github' | 'local' | 'archive';
    repo?: string;
    ref?: string;                   // commit / tag
    installPath: string;
  };
}
```

### 3.11 Marketplace

```ts
interface Marketplace {
  ...Component<'marketplace'>;
  parsed: {
    id: string;                     // 'claude-plugins-official'
    source: { kind: 'github' | 'http'; url: string };
    plugins: PluginIndexEntry[];    // catalog
  };
  knownTo: ToolId[];                // which host tools recognise it
}
```

### 3.12 Session

```ts
interface Session {
  ...Component<'session'>;
  format: 'jsonl' | 'json' | 'sqlite';
  parsed: {
    startedAt: Date;
    endedAt?: Date;
    title?: string;                 // first user message or LLM-summarised
    turnCount: number;
    toolCalls: ToolCallSummary[];   // counts per tool/skill/agent
    tokensUsed?: number;
  };
  readonly: true;
}
```

### 3.13 Task

```ts
interface Task {
  ...Component<'task'>;
  parsed: {
    title: string;
    description?: string;
    status: 'pending' | 'in_progress' | 'completed' | 'deleted';
    sessionId?: string;             // task scope
    createdAt: Date;
    updatedAt: Date;
    blockedBy?: string[];
  };
}
```

### 3.14 Output Style

```ts
interface OutputStyle {
  ...Component<'outputStyle'>;
  parsed: {
    name: string;
    description?: string;
    body: string;                   // tone / persona instructions
  };
}
```

### 3.15 Statusline

```ts
interface Statusline {
  ...Component<'statusline'>;
  parsed: {
    template: string;
    refreshIntervalMs?: number;
  };
}
```

### 3.16 Permission Profile

```ts
interface PermissionProfile {
  ...Component<'permission'>;
  parsed: {
    defaultMode: 'ask' | 'allow' | 'deny' | 'bypass';
    allow: string[];
    deny: string[];
    sandbox?: 'read-only' | 'workspace-write' | 'danger-full-access';
    approvalPolicy?: 'on-request' | 'never' | 'untrusted';
  };
}
```

## Cross-tool mapping table

The same conceptual component shows up under different names per tool. Normalising is half the value.

| Concept | Claude Code | Codex | Cursor | Antigravity | Cline | Continue | Windsurf | Goose | Aider | Copilot | Zed |
|---------|-------------|-------|--------|-------------|-------|----------|----------|-------|-------|---------|-----|
| Memory | CLAUDE.md / AGENTS.md | AGENTS.md | AGENTS.md / .cursorrules | GEMINI.md / AGENTS.md | clinerules | (rules) | global_rules.md / AGENTS.md | (recipe instructions) | CONVENTIONS.md | copilot-instructions.md | (rules) |
| Rule | .claude/rules/*.md | (in AGENTS.md) | .cursor/rules/*.mdc | .agents/rules/*.md | clinerules | rules | .windsurf/rules/*.md | (recipe blocks) | n/a | .github/instructions/ | (system prompt) |
| Skill | .claude/skills/*/SKILL.md | .codex/skills/*/SKILL.md | (skills) | .agent/skills/*/SKILL.md | skills | (custom prompts) | skills | (recipe steps) | n/a | (custom prompts) | (slash commands) |
| Command | .claude/commands/*.md | (slash) | .cursor/commands/*.md | global_workflows/*.md | workflows | slash commands | workflows | recipes | /commands | .github/prompts/*.prompt.md | slash commands |
| Agent | .claude/agents/*.md | (modes) | (chat modes) | spawnable agents | Plan/Act modes | (modes) | Cascade modes | (subrecipes) | n/a | chat modes / .github/chatmodes/ | external agents |
| MCP | .mcp.json / settings.json | mcp_servers in config.toml | .cursor/mcp.json | mcp_config.json | MCP marketplace | (config.yaml) | Cascade MCP | extensions (MCP) | n/a | (settings) | context servers |
| Hook | settings.json hooks | (n/a) | .cursor/hooks.json | (n/a) | clinehooks | (n/a) | (n/a) | (n/a) | n/a | (n/a) | (n/a) |
| Plugin | .claude-plugin/plugin.json | (memories dir) | (extensions) | (n/a) | (workflows) | hub.continue.dev | plugins | extensions | n/a | extensions | extensions |
| Output Style | output-styles/*.md | personality | (n/a) | (n/a) | (n/a) | (n/a) | (n/a) | (n/a) | n/a | (n/a) | (n/a) |
| Permission | settings.permissions | approval+sandbox | (n/a) | (n/a) | auto-approve | (n/a) | (n/a) | (n/a) | --yes | (n/a) | (n/a) |

Where a cell is `(n/a)`, the tool does not have a directly equivalent concept; we either show "not applicable" or surface a related concept (e.g., Aider's CONVENTIONS.md is closest to Memory).

## Drift detection (what powers the "Memory drift" view)

Two memory or rule files are candidate equivalents when:

1. They are at the **same scope** (both user, both project).
2. They are in the **conventional canonical filename** for their tool.
3. Their **shingled content overlap** exceeds a threshold (default 30%, tuneable).

When candidates are identified, we present them side-by-side with a 3-pane diff and "promote" / "merge" actions. The user can mark a pair as `equivalentTo` to bypass the heuristic in future scans.

## Lifecycle

```
Discovered (on disk)
      |
      v
Parsed (frontmatter + body)
      |
      v
Validated (per-tool schema)
      |
      v
Indexed (searchable)
      |
      v---- Watched (file events) -----+
      |                                 |
      v                                 |
Edited / Toggled / Exported   <--------+
      |
      v
Saved (atomic write back to disk)
```

## What we explicitly don't model

- **Live agent state** (running task, current LLM call). Out of scope.
- **Token consumption per call.** We may aggregate from logs, never observe live.
- **Model performance / quality.** Not our problem.

The taxonomy stays small on purpose. Every additional first-class type doubles spec, parser, validator, editor, and visual surface area. We add types only when an existing one is a poor fit.
