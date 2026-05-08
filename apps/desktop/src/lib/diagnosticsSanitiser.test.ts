import { describe, expect, it } from "vitest";
import {
  REDACTED,
  sanitiseForClipboard,
  type DiagnosticsReport,
} from "./diagnosticsSanitiser";

function baseReport(): DiagnosticsReport {
  return {
    appVersion: "0.0.1",
    platform: "macos",
    userAgent: "Mozilla/5.0 (Macintosh)",
    generatedAt: "2026-05-08T00:00:00.000Z",
    panic: { active: false, lastToggledAt: null },
    index: {
      totalComponents: 12,
      totalParseErrors: 0,
      byToolKind: [{ tool: "claude-code", kind: "skill", count: 3 }],
    },
    tools: [
      {
        id: "claude-code",
        displayName: "Claude Code",
        detected: true,
        binary: "/usr/local/bin/claude",
        version: "1.2.3",
        watchRoots: ["~/.claude"],
      },
    ],
    recentEvents: [],
    recentParseErrors: [],
  };
}

describe("sanitiseForClipboard", () => {
  it("redacts an Anthropic sk-ant- token wherever it appears", () => {
    const report = baseReport();
    const tool = report.tools[0];
    if (!tool) throw new Error("fixture invariant");
    tool.version = "sk-ant-AAAA1111BBBB2222CCCC3333DDDD4444EEEE5555FFFFGGGGHHHHIIIIJJJJ";

    const sanitised = sanitiseForClipboard(report);
    const sanitisedTool = sanitised.tools[0];
    expect(sanitisedTool).toBeDefined();
    expect(sanitisedTool?.version).toBe(REDACTED);
    // Non-secret strings preserved.
    expect(sanitisedTool?.displayName).toBe("Claude Code");
    expect(sanitisedTool?.binary).toBe("/usr/local/bin/claude");
  });

  it("redacts an OpenAI sk- token in nested event payloads", () => {
    const report = baseReport();
    report.recentEvents.push({
      timestamp: 1,
      event: {
        event: "parseError",
        id: "aseye://x",
        path: "sk-abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGH",
      },
    });

    const sanitised = sanitiseForClipboard(report);
    const evt = sanitised.recentEvents[0]?.event;
    expect(evt).toBeDefined();
    if (evt && evt.event === "parseError") {
      expect(evt.path).toBe(REDACTED);
      expect(evt.id).toBe("aseye://x");
    } else {
      throw new Error("expected parseError event after sanitisation");
    }
  });

  it("redacts a GitHub PAT (ghp_) token", () => {
    const report = baseReport();
    report.recentParseErrors.push({
      timestamp: 1,
      id: "aseye://y",
      path: "ghp_AAAA1111BBBB2222CCCC3333DDDD4444EEEE",
    });

    const sanitised = sanitiseForClipboard(report);
    const err = sanitised.recentParseErrors[0];
    expect(err).toBeDefined();
    expect(err?.path).toBe(REDACTED);
    expect(err?.id).toBe("aseye://y");
  });

  it("redacts a generic password=... assignment", () => {
    const report = baseReport();
    report.userAgent = "password=hunter2hunter2hunter2";
    const sanitised = sanitiseForClipboard(report);
    expect(sanitised.userAgent).toBe(REDACTED);
  });

  it("redacts a generic api_key=... pattern in arrays of strings", () => {
    const report = baseReport();
    const tool = report.tools[0];
    if (!tool) throw new Error("fixture invariant");
    tool.watchRoots = ["~/.claude", "api_key=ABCDEFGHIJKLMNOPQRST1234"];

    const sanitised = sanitiseForClipboard(report);
    const sanitisedTool = sanitised.tools[0];
    expect(sanitisedTool).toBeDefined();
    expect(sanitisedTool?.watchRoots).toEqual(["~/.claude", REDACTED]);
  });

  it("leaves non-secret values untouched", () => {
    const report = baseReport();
    const sanitised = sanitiseForClipboard(report);
    expect(sanitised).toEqual(report);
    // Defensive: ensure we returned a fresh object, not the same reference.
    expect(sanitised).not.toBe(report);
  });
});
