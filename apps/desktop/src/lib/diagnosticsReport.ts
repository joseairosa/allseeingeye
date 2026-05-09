/**
 * Diagnostics report builder.
 *
 * Pure factory that assembles a `DiagnosticsReport` from the inputs
 * the Settings -> Diagnostics panel and the "Diagnostics export"
 * privacy button both consume. Living in `lib/` so both call sites
 * share the same shape and the same sanitiser hand-off.
 *
 * The sanitiser (`sanitiseForClipboard`) is applied by the caller -
 * not here - so the export path can clearly show "build then
 * sanitise" in two ordered steps.
 */
import type { StampedPipelineEvent } from "./diagnosticsRing";
import type {
  DiagnosticsParseError,
  DiagnosticsReport,
  DiagnosticsRingEntry,
  DiagnosticsToolEntry,
  DiagnosticsToolKindCount,
} from "./diagnosticsSanitiser";

/** Read a coarse platform label from the user-agent. */
export function detectPlatformLabel(): string {
  if (typeof navigator === "undefined") return "unknown";
  const ua = navigator.userAgent;
  // Matches "Macintosh", "Windows NT 10.0", "Linux x86_64" etc.
  const match = /\(([^)]+)\)/.exec(ua);
  return match?.[1] ?? "unknown";
}

/**
 * Translate a `StampedPipelineEvent` into the wire-shape we emit in
 * the diagnostics report. We avoid leaking `Date` objects so JSON
 * serialise produces stable output across runs.
 */
export function toRingEntry(stamped: StampedPipelineEvent): DiagnosticsRingEntry {
  return { timestamp: stamped.timestamp, event: stamped.event };
}

/**
 * Project a parse-error event onto the dedicated parse-errors section
 * shape, or `null` for non-parse-error events.
 */
export function toParseError(
  stamped: StampedPipelineEvent,
): DiagnosticsParseError | null {
  if (stamped.event.event !== "parseError") return null;
  return {
    timestamp: stamped.timestamp,
    id: stamped.event.id,
    path: stamped.event.path,
  };
}

/** Inputs for `buildReport`. */
export interface BuildReportArgs {
  appVersion: string;
  events: StampedPipelineEvent[];
  parseErrors: DiagnosticsParseError[];
  panicActive: boolean;
  panicLastToggledAt: number | null;
  totalComponents: number;
  totalParseErrors: number;
  byToolKind: DiagnosticsToolKindCount[];
  tools: DiagnosticsToolEntry[];
}

/**
 * Pure factory. The caller passes in the resolved query data and
 * panel state; we produce the report shape. No side effects.
 */
export function buildReport(args: BuildReportArgs): DiagnosticsReport {
  return {
    appVersion: args.appVersion,
    platform: detectPlatformLabel(),
    userAgent: typeof navigator === "undefined" ? "unknown" : navigator.userAgent,
    generatedAt: new Date().toISOString(),
    panic: {
      active: args.panicActive,
      lastToggledAt: args.panicLastToggledAt,
    },
    index: {
      totalComponents: args.totalComponents,
      totalParseErrors: args.totalParseErrors,
      byToolKind: args.byToolKind,
    },
    tools: args.tools,
    recentEvents: args.events.map(toRingEntry),
    recentParseErrors: args.parseErrors,
  };
}
