/**
 * Diagnostics report sanitiser (Phase 4.2).
 *
 * Walks a `DiagnosticsReport` and replaces any string value whose
 * content trips `detectSecretKind` with the literal `"<redacted>"`. The
 * shape of the report is preserved exactly so support tooling can rely
 * on it; only the leaf string values change.
 *
 * Tool root paths and command output stay; the sanitiser only neutralises
 * tokens, passwords, and auth headers. Event payloads are object-typed,
 * so we recurse into them rather than treating them as opaque strings.
 *
 * IMPORTANT: this runs in the WebView before the JSON is handed to the
 * clipboard. The Rust audit engine remains the authoritative scanner;
 * we are belt-and-braces against accidentally pasting a freshly typed
 * secret from the user's screen.
 */
import { detectSecretKind } from "./secrets";
import type { PipelineEvent } from "@aseye/shared-types";

/** Sentinel used in place of any string value matching a secret pattern. */
export const REDACTED = "<redacted>";

/**
 * Per-tool entry as it appears in the diagnostics report. Mirrors the
 * subset of `DetectedTool` we want to surface.
 */
export interface DiagnosticsToolEntry {
  id: string;
  displayName: string;
  detected: boolean;
  binary: string | null;
  version: string | null;
  watchRoots: string[];
}

/** Per-(tool, kind) row from the health summary. */
export interface DiagnosticsToolKindCount {
  tool: string;
  kind: string;
  count: number;
}

/** Lightweight projection of `StampedPipelineEvent` for the report. */
export interface DiagnosticsRingEntry {
  timestamp: number;
  event: PipelineEvent;
}

/** Parse error projection for the dedicated section. */
export interface DiagnosticsParseError {
  timestamp: number;
  id: string;
  path: string;
}

/** Top-level shape of a diagnostics report. */
export interface DiagnosticsReport {
  appVersion: string;
  platform: string;
  userAgent: string;
  generatedAt: string;
  panic: {
    active: boolean;
    lastToggledAt: number | null;
  };
  index: {
    totalComponents: number;
    totalParseErrors: number;
    byToolKind: DiagnosticsToolKindCount[];
  };
  tools: DiagnosticsToolEntry[];
  recentEvents: DiagnosticsRingEntry[];
  recentParseErrors: DiagnosticsParseError[];
}

/**
 * Recursive JSON-ish value used by the sanitiser. We intentionally avoid
 * `any`: every leaf has a known shape and we narrow per-type.
 */
type JsonValue =
  | string
  | number
  | boolean
  | null
  | JsonValue[]
  | { [key: string]: JsonValue };

/**
 * Walk the value, returning a structurally identical clone with any
 * string trip-wired by `detectSecretKind` replaced by `REDACTED`.
 */
function sanitiseValue(value: JsonValue): JsonValue {
  if (typeof value === "string") {
    return detectSecretKind(value) ? REDACTED : value;
  }
  if (Array.isArray(value)) {
    return value.map(sanitiseValue);
  }
  if (value !== null && typeof value === "object") {
    const out: { [key: string]: JsonValue } = {};
    for (const [k, v] of Object.entries(value)) {
      out[k] = sanitiseValue(v);
    }
    return out;
  }
  return value;
}

/**
 * Top-level sanitiser. Round-trips through the shared walker so every
 * string anywhere in the structure is checked, regardless of nesting.
 */
export function sanitiseForClipboard(
  report: DiagnosticsReport,
): DiagnosticsReport {
  // The cast is safe: `DiagnosticsReport` is a JSON-compatible structure.
  // We deliberately go through `unknown` to keep the surface area honest.
  const walked = sanitiseValue(report as unknown as JsonValue);
  return walked as unknown as DiagnosticsReport;
}
