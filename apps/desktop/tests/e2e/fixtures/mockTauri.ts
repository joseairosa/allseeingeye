/**
 * Mock Tauri runtime for Playwright.
 *
 * Tauri's `invoke()` (in `@tauri-apps/api/core`) reaches into
 * `window.__TAURI_INTERNALS__.invoke(cmd, args)`. In a plain Chromium
 * browser that property does not exist and every IPC call rejects.
 * We patch the property at page-init time with deterministic responses
 * that mirror the real backend's wire shape (camelCase, BigInt-friendly
 * strings, etc.) so the React shell renders the same way it would in
 * the packaged desktop app.
 *
 * Coverage is intentionally narrow: only the commands the E2E specs
 * actually trigger. Unhandled commands resolve to a typed error so a
 * regression that adds a new IPC call surfaces fast instead of hanging.
 */

/**
 * Wire shape we hand back from `list_components` / `get_component`.
 * `bigint` fields are emitted as JSON numbers because the real ts-rs
 * binding declares them `bigint`, but the React layer only reads them
 * for display — it never does arithmetic, so a `number` round-trips
 * through the same code paths without runtime assertion failures.
 */
const FIXTURE_COMPONENTS = [
  {
    id: "aseye://claude-code/user/skill/spec",
    name: "spec",
    displayName: "Spec",
    description: "TDD spec runner skill",
    kind: "skill",
    tool: "claude-code",
    scope: "user",
    format: "markdownFrontmatter",
    path: "/Users/test/.claude/skills/spec/SKILL.md",
    size: 1024,
    mtime: 1_733_000_000,
    hash: "deadbeef",
    hasParseErrors: false,
    lastUsedAt: 1_733_500_000,
    useCount: 7,
  },
  {
    id: "aseye://claude-code/user/skill/lint",
    name: "lint",
    displayName: "Lint",
    description: "Lint and typecheck",
    kind: "skill",
    tool: "claude-code",
    scope: "user",
    format: "markdownFrontmatter",
    path: "/Users/test/.claude/skills/lint/SKILL.md",
    size: 768,
    mtime: 1_732_000_000,
    hash: "cafebabe",
    hasParseErrors: false,
    lastUsedAt: 1_732_500_000,
    useCount: 3,
  },
  {
    id: "aseye://codex/user/agent/specwriter",
    name: "specwriter",
    displayName: "Spec Writer",
    description: "Drafts specs from PRs",
    kind: "agent",
    tool: "codex",
    scope: "user",
    format: "markdown",
    path: "/Users/test/.codex/agents/specwriter.md",
    size: 512,
    mtime: 1_731_000_000,
    hash: "1234abcd",
    hasParseErrors: false,
    lastUsedAt: null,
    useCount: 0,
  },
];

const FIXTURE_HEALTH = {
  totalComponents: 3,
  totalParseErrors: 0,
  byToolKind: [
    { tool: "claude-code", kind: "skill", count: 2 },
    { tool: "codex", kind: "agent", count: 1 },
  ],
};

const FIXTURE_FINDINGS_COUNTS: unknown[] = [];
const FIXTURE_SECURITY_SUMMARY = {
  totalFindings: 0,
  totalSuppressed: 0,
  bySeverity: { critical: 0, high: 0, medium: 0, low: 0 },
  byCategory: { secret: 0, mcpPermission: 0, mcpTrust: 0, other: 0 },
};
/**
 * `DetectedTool` wire shape (per
 * `src-tauri/bindings/registry/DetectedTool.ts`). The Diagnostics
 * panel reads `existingRootPaths.length` directly, so omitting the
 * field crashes the UI on first render. We hand back a complete
 * record per tool.
 */
const FIXTURE_TOOLS = [
  {
    id: "claude-code",
    displayName: "Claude Code",
    detected: true,
    binary: "/usr/local/bin/claude",
    version: "1.0.0",
    existingRootPaths: ["/Users/test/.claude"],
  },
  {
    id: "codex",
    displayName: "Codex",
    detected: true,
    binary: "/usr/local/bin/codex",
    version: "0.1.0",
    existingRootPaths: ["/Users/test/.codex"],
  },
  {
    id: "cursor",
    displayName: "Cursor",
    detected: false,
    binary: null,
    version: null,
    existingRootPaths: [],
  },
  {
    id: "antigravity",
    displayName: "Antigravity",
    detected: false,
    binary: null,
    version: null,
    existingRootPaths: [],
  },
];

/**
 * Inline init script. Runs in the page's main world before any user
 * code. Patches `window.__TAURI_INTERNALS__` with a synchronous
 * dispatcher; `__TAURI_EVENT_PLUGIN_INTERNALS__` gets a no-op stub so
 * pipeline-event subscriptions resolve cleanly.
 *
 * The script is serialised verbatim through Playwright's
 * `addInitScript`, so it must be self-contained ES (no imports) and
 * avoid TypeScript-only syntax. We embed the fixture data as JSON
 * literals that the script reconstructs at page startup.
 */
export function buildInitScript(): string {
  const payload = {
    components: FIXTURE_COMPONENTS,
    health: FIXTURE_HEALTH,
    findings: FIXTURE_FINDINGS_COUNTS,
    summary: FIXTURE_SECURITY_SUMMARY,
    tools: FIXTURE_TOOLS,
  };
  // Emit as JSON literal so the page-side script gets a plain data
  // structure with no closure-captured references.
  const json = JSON.stringify(payload);
  return `(() => {
    const FIXTURES = ${json};
    // Suppress the first-launch onboarding modal. App.tsx auto-opens
    // it whenever isTauriRuntime() returns true AND
    // loadOnboardingCompleted() is false; patching __TAURI_INTERNALS__
    // satisfies the first half, so we satisfy the second half here.
    try { window.localStorage.setItem("aseye.onboarding.completed", "true"); } catch (_) {}
    function lower(s) { return typeof s === "string" ? s.toLowerCase() : ""; }
    function matchSearch(text) {
      const q = lower(text || "");
      if (!q) return FIXTURES.components;
      return FIXTURES.components.filter((c) => {
        return lower(c.name).includes(q)
          || lower(c.displayName || "").includes(q)
          || lower(c.description || "").includes(q)
          || lower(c.id).includes(q);
      });
    }
    function listComponents(filter) {
      // Honour the basic toolId/kind/scope filters the real backend
      // applies. The query (text) field is plumbed through search,
      // not list_components, in the live backend; we do the same.
      let rows = FIXTURES.components.slice();
      if (filter && filter.toolId) rows = rows.filter((c) => c.tool === filter.toolId);
      if (filter && filter.kind) rows = rows.filter((c) => c.kind === filter.kind);
      if (filter && filter.scope) rows = rows.filter((c) => c.scope === filter.scope);
      return rows;
    }
    function search(query) {
      const rows = matchSearch(query && query.text);
      return rows.slice(0, (query && query.limit) || 50).map((c) => ({
        id: c.id,
        name: c.name,
        displayName: c.displayName,
        kind: c.kind,
        tool: c.tool,
        snippet: null,
      }));
    }
    function getComponent(id) {
      const c = FIXTURES.components.find((x) => x.id === id);
      if (!c) return null;
      return Object.assign({}, c, {
        parsedJson: null,
        parseErrors: null,
        origin: "userCreated",
        pluginId: null,
      });
    }
    let eventIdCounter = 0;
    function dispatch(cmd, args) {
      switch (cmd) {
        case "list_tools":
          return FIXTURES.tools;
        case "list_components":
          return listComponents((args && args.filter) || {});
        case "get_component":
          return getComponent(args && args.id);
        case "read_component_raw":
          return "# fixture body\\n";
        case "search":
          return search((args && args.query) || { text: "" });
        case "start_full_scan":
          return { scanned: FIXTURES.components.length, errors: 0 };
        case "get_health_summary":
          return FIXTURES.health;
        case "list_security_findings":
          return [];
        case "suppress_finding":
        case "unsuppress_finding":
          return null;
        case "get_findings_count_per_component":
          return FIXTURES.findings;
        case "get_security_summary":
          return FIXTURES.summary;
        // The Tauri event plugin uses these internal IPC channels for
        // listen() / unlisten(). The React layer subscribes to
        // pipeline-event in usePipelineEventInvalidator at mount; we
        // hand back a synthetic id and never emit any events.
        case "plugin:event|listen":
          return ++eventIdCounter;
        case "plugin:event|unlisten":
          return null;
        default:
          // Surface unhandled commands loudly so a missing fixture
          // shows up in the test output rather than as a hang.
          return Promise.reject(new Error("mockTauri: unhandled command " + cmd));
      }
    }
    let cbCounter = 0;
    const cbRegistry = new Map();
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      configurable: true,
      writable: true,
      value: {
        invoke: (cmd, args) => {
          try {
            const out = dispatch(cmd, args);
            return Promise.resolve(out);
          } catch (e) {
            return Promise.reject(e);
          }
        },
        transformCallback: (cb, once) => {
          const id = ++cbCounter;
          cbRegistry.set(id, { cb, once });
          return id;
        },
        unregisterCallback: (id) => {
          cbRegistry.delete(id);
        },
        convertFileSrc: (p) => p,
      },
    });
    Object.defineProperty(window, "__TAURI_EVENT_PLUGIN_INTERNALS__", {
      configurable: true,
      writable: true,
      value: {
        unregisterListener: () => {},
      },
    });
  })();`;
}
