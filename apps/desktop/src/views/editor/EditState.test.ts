/**
 * EditState reducer tests.
 *
 * Ten cases covering each action plus idempotency and history behaviour.
 * The reducer is pure - we don't need a mock IPC layer; we feed it
 * synthetic `ComponentDetailWithRaw` payloads.
 */
import { describe, expect, it } from "vitest";
import type { ComponentDetailWithRaw } from "@aseye/shared-types";
import {
  editReducer,
  HISTORY_LIMIT,
  type AstProjector,
  type AstSerialiser,
  type EditState,
} from "./EditState";

const project: AstProjector = (raw) => {
  // Treat `INVALID` as a parse failure for the failure-mode tests.
  if (raw.startsWith("INVALID")) {
    return { ok: false, error: "synthetic parse error" };
  }
  // Otherwise interpret each line as `key=value` and project that
  // into a flat AST. Just enough to exercise the round-trip.
  const ast: Record<string, unknown> = {};
  for (const line of raw.split("\n")) {
    const eq = line.indexOf("=");
    if (eq > 0) ast[line.slice(0, eq)] = line.slice(eq + 1);
  }
  return { ok: true, ast };
};

const serialise: AstSerialiser = (ast) =>
  Object.entries(ast)
    .map(([k, v]) => `${k}=${String(v)}`)
    .join("\n");

function detail(raw: string, hash = "h0"): ComponentDetailWithRaw {
  return {
    detail: {
      id: "aseye://t/u/skill/spec",
      name: "spec",
      displayName: null,
      description: null,
      kind: "skill",
      tool: "claude-code",
      scope: "user",
      format: "markdownfrontmatter",
      path: "/tmp/spec/SKILL.md",
      // ComponentDetail uses bigint for these fields per the bindings;
      // tests can use 0n since the reducer never inspects them.
      size: 0n,
      mtime: 0n,
      hash,
      hasParseErrors: false,
      lastUsedAt: null,
      useCount: 0,
      parsedJson: null,
      parseErrors: null,
      origin: "userCreated",
      pluginId: null,
    },
    raw,
    hash,
  };
}

function open(raw: string, hash = "h0"): EditState {
  const next = editReducer(null, { type: "open", detail: detail(raw, hash), project });
  if (next === null) throw new Error("expected open() to produce state");
  return next;
}

describe("EditState reducer", () => {
  it("open seeds originalRaw, originalHash, and dirty=false", () => {
    const s = open("name=spec");
    expect(s.id).toBe("aseye://t/u/skill/spec");
    expect(s.originalRaw).toBe("name=spec");
    expect(s.currentRaw).toBe("name=spec");
    expect(s.originalHash).toBe("h0");
    expect(s.dirty).toBe(false);
    expect(s.formAst).toEqual({ name: "spec" });
    expect(s.parseError).toBeNull();
    expect(s.history).toHaveLength(1);
    expect(s.historyIndex).toBe(0);
  });

  it("setRaw updates currentRaw and flips dirty", () => {
    const s = open("name=spec");
    const next = editReducer(s, { type: "setRaw", raw: "name=new", project });
    expect(next?.currentRaw).toBe("name=new");
    expect(next?.dirty).toBe(true);
    expect(next?.formAst).toEqual({ name: "new" });
    // Reverting back to originalRaw unsets dirty.
    const reverted = editReducer(next, {
      type: "setRaw",
      raw: "name=spec",
      project,
    });
    expect(reverted?.dirty).toBe(false);
  });

  it("setRaw keeps the last good AST when re-parse fails", () => {
    const s = open("name=spec");
    const broken = editReducer(s, {
      type: "setRaw",
      raw: "INVALID typing in progress",
      project,
    });
    expect(broken?.currentRaw).toBe("INVALID typing in progress");
    expect(broken?.formAst).toEqual({ name: "spec" });
    expect(broken?.parseError).toContain("synthetic parse error");
  });

  it("setRaw is idempotent when the raw text is unchanged", () => {
    const s = open("name=spec");
    const next = editReducer(s, { type: "setRaw", raw: "name=spec", project });
    expect(next).toBe(s);
  });

  it("setFormField writes into the AST and back to raw", () => {
    const s = open("name=spec\ndescription=x");
    const next = editReducer(s, {
      type: "setFormField",
      pointer: "/description",
      value: "updated",
      serialise,
    });
    expect(next?.formAst).toEqual({ name: "spec", description: "updated" });
    expect(next?.currentRaw).toBe("name=spec\ndescription=updated");
    expect(next?.dirty).toBe(true);
    expect(next?.history).toHaveLength(2);
    expect(next?.historyIndex).toBe(1);
  });

  it("discard reverts currentRaw to originalRaw", () => {
    const s0 = open("name=spec");
    const dirtied = editReducer(s0, {
      type: "setRaw",
      raw: "name=changed",
      project,
    });
    const discarded = editReducer(dirtied, { type: "discard" });
    expect(discarded?.currentRaw).toBe("name=spec");
    expect(discarded?.dirty).toBe(false);
  });

  it("markSaved updates originalHash and clears dirty", () => {
    const s = open("name=spec");
    const dirtied = editReducer(s, {
      type: "setRaw",
      raw: "name=new",
      project,
    });
    const saved = editReducer(dirtied, { type: "markSaved", newHash: "h1" });
    expect(saved?.originalHash).toBe("h1");
    expect(saved?.originalRaw).toBe("name=new");
    expect(saved?.dirty).toBe(false);
    expect(saved?.externalChange).toBeNull();
  });

  it("noteExternalChange + clearExternalChange round-trip", () => {
    const s = open("name=spec");
    const noted = editReducer(s, {
      type: "noteExternalChange",
      payload: { currentHash: "h2", currentContent: "name=ext" },
    });
    expect(noted?.externalChange).toEqual({
      currentHash: "h2",
      currentContent: "name=ext",
    });
    const cleared = editReducer(noted, { type: "clearExternalChange" });
    expect(cleared?.externalChange).toBeNull();
  });

  it("setValidation surfaces a fresh outcome", () => {
    const s = open("name=spec");
    const next = editReducer(s, {
      type: "setValidation",
      outcome: {
        ok: false,
        errors: [
          { path: "/description", message: "missing", schemaKeyword: "required" },
        ],
        warnings: [],
      },
    });
    expect(next?.validation?.errors).toHaveLength(1);
    expect(next?.validation?.errors[0]?.path).toBe("/description");
  });

  it("undoFormChange walks history backwards", () => {
    const s = open("name=spec");
    const a = editReducer(s, {
      type: "setFormField",
      pointer: "/description",
      value: "first",
      serialise,
    });
    const b = editReducer(a, {
      type: "setFormField",
      pointer: "/description",
      value: "second",
      serialise,
    });
    expect(b?.formAst.description).toBe("second");
    const undone = editReducer(b, { type: "undoFormChange", serialise });
    expect(undone?.formAst.description).toBe("first");
    expect(undone?.historyIndex).toBe(1);
    // Walking past the first entry is a no-op (state unchanged).
    const start = editReducer(undone, { type: "undoFormChange", serialise });
    const further = editReducer(start, { type: "undoFormChange", serialise });
    expect(further?.historyIndex).toBe(0);
  });

  it("history is capped at HISTORY_LIMIT", () => {
    const s = open("name=spec");
    let cur: EditState | null = s;
    for (let i = 0; i < HISTORY_LIMIT + 5; i += 1) {
      cur = editReducer(cur, {
        type: "setFormField",
        pointer: "/i",
        value: i,
        serialise,
      });
    }
    expect(cur?.history.length).toBe(HISTORY_LIMIT);
  });
});
