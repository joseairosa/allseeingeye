#!/usr/bin/env node
// scripts/perf-summary.mjs
//
// Frontend bundle size summary + budget enforcement.
//
// Reads `apps/desktop/dist/assets/*.js`, computes raw + gzipped sizes,
// compares to `perf-budgets.json` at the repo root, prints a Markdown
// summary suitable for a PR comment, and exits non-zero when any budget
// is exceeded (after the configured tolerance).
//
// Locally:        pnpm perf:summary
// CI (perf job):  invoked after `pnpm --filter @aseye/desktop build`
//
// Output contract:
//   stdout: human-readable Markdown table (also valid for sticky PR
//           comments).
//   $GITHUB_STEP_SUMMARY (when set): the same Markdown.
//   $GITHUB_OUTPUT (when set): `total_gzip_bytes`, `largest_gzip_bytes`,
//           `largest_chunk_name`, `over_budget` (true|false).
//   exit code: 0 = pass, 1 = budget exceeded, 2 = configuration error
//   (missing dist/, missing budgets file, malformed input).
//
// Why this script exists:
//   PRD H1 - H3 (cold start, idle CPU/memory, binary size) cannot be
//   measured deterministically in headless CI. The closest CI-checkable
//   proxy for "the WebView ships fast" is the gzipped JS bundle size.
//   See `docs/perf-budgets.md` for the full reasoning.

import { readFileSync, statSync, readdirSync, appendFileSync } from "node:fs";
import { gzipSync } from "node:zlib";
import { fileURLToPath } from "node:url";
import { dirname, join, resolve } from "node:path";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
const REPO_ROOT = resolve(__dirname, "..");
const DIST_DIR = join(REPO_ROOT, "apps", "desktop", "dist", "assets");
const BUDGETS_PATH = join(REPO_ROOT, "perf-budgets.json");

/** Format a byte count as a human-readable string (KB, MB). */
function fmtBytes(n) {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / (1024 * 1024)).toFixed(2)} MB`;
}

/** Format a delta (signed bytes) as a human-readable signed string. */
function fmtDelta(n) {
  const sign = n >= 0 ? "+" : "-";
  return `${sign}${fmtBytes(Math.abs(n))}`;
}

/**
 * Load the budgets file. Exits 2 with a clear message on any failure;
 * the budget config is the contract this script enforces and silently
 * skipping it would defeat the gate.
 */
function loadBudgets() {
  let raw;
  try {
    raw = readFileSync(BUDGETS_PATH, "utf8");
  } catch (err) {
    console.error(
      `[perf-summary] Could not read ${BUDGETS_PATH}: ${err.message}`,
    );
    process.exit(2);
  }
  let parsed;
  try {
    parsed = JSON.parse(raw);
  } catch (err) {
    console.error(
      `[perf-summary] ${BUDGETS_PATH} is not valid JSON: ${err.message}`,
    );
    process.exit(2);
  }
  const fe = parsed && parsed.frontend;
  if (
    !fe ||
    typeof fe.totalGzipBytes !== "number" ||
    typeof fe.largestChunkGzipBytes !== "number" ||
    typeof fe.tolerancePercent !== "number"
  ) {
    console.error(
      `[perf-summary] ${BUDGETS_PATH} missing required \`frontend\` fields ` +
        `(totalGzipBytes, largestChunkGzipBytes, tolerancePercent).`,
    );
    process.exit(2);
  }
  return fe;
}

/**
 * Enumerate `*.js` files in `apps/desktop/dist/assets/`. We do not walk
 * subdirectories because Vite emits a flat `assets/` layout; if that
 * changes the script will surface a 0-chunk error rather than silently
 * miss files in subfolders.
 */
function enumerateJsChunks() {
  let entries;
  try {
    entries = readdirSync(DIST_DIR, { withFileTypes: true });
  } catch (err) {
    console.error(
      `[perf-summary] Could not read ${DIST_DIR}: ${err.message}\n` +
        `Did you run \`pnpm --filter @aseye/desktop build\` first?`,
    );
    process.exit(2);
  }
  const chunks = [];
  for (const entry of entries) {
    if (!entry.isFile()) continue;
    if (!entry.name.endsWith(".js")) continue;
    const path = join(DIST_DIR, entry.name);
    const buf = readFileSync(path);
    const raw = buf.byteLength;
    // gzip level 9 matches `gzip -9` so local + CI byte counts agree.
    // Level 9 is what HTTP servers typically pre-compress static assets
    // at; using anything lower would understate the regression budget.
    const gz = gzipSync(buf, { level: 9 }).byteLength;
    chunks.push({ name: entry.name, raw, gz });
  }
  if (chunks.length === 0) {
    console.error(
      `[perf-summary] No .js files found in ${DIST_DIR}. ` +
        `The dist directory exists but is empty - did the build fail silently?`,
    );
    process.exit(2);
  }
  // Sort by gzipped size descending so the table reads as "biggest first".
  chunks.sort((a, b) => b.gz - a.gz);
  return chunks;
}

/**
 * Apply tolerance to a budget. Returns { effective, withinTolerance }
 * where `effective` is the tolerated ceiling and `withinTolerance` is
 * `true` when `actual <= effective`.
 */
function checkBudget(actual, budget, tolerancePercent) {
  const effective = Math.floor(budget * (1 + tolerancePercent / 100));
  return { effective, withinTolerance: actual <= effective };
}

function main() {
  const fe = loadBudgets();
  const chunks = enumerateJsChunks();
  const totalGz = chunks.reduce((acc, c) => acc + c.gz, 0);
  const totalRaw = chunks.reduce((acc, c) => acc + c.raw, 0);
  const largest = chunks[0];

  const tot = checkBudget(totalGz, fe.totalGzipBytes, fe.tolerancePercent);
  const lar = checkBudget(
    largest.gz,
    fe.largestChunkGzipBytes,
    fe.tolerancePercent,
  );
  const overBudget = !tot.withinTolerance || !lar.withinTolerance;

  // Build the Markdown summary. The same string is printed to stdout
  // and to $GITHUB_STEP_SUMMARY so the artifact lives in the run UI.
  const lines = [];
  lines.push(`# Frontend bundle size`);
  lines.push("");
  lines.push(
    `**Total gzipped JS:** ${fmtBytes(totalGz)} ` +
      `(budget ${fmtBytes(fe.totalGzipBytes)}, ` +
      `tolerated ${fmtBytes(tot.effective)}, ` +
      `delta ${fmtDelta(totalGz - fe.totalGzipBytes)}) ` +
      `${tot.withinTolerance ? "PASS" : "FAIL"}`,
  );
  lines.push(
    `**Largest chunk:** \`${largest.name}\` ${fmtBytes(largest.gz)} gz ` +
      `(budget ${fmtBytes(fe.largestChunkGzipBytes)}, ` +
      `tolerated ${fmtBytes(lar.effective)}, ` +
      `delta ${fmtDelta(largest.gz - fe.largestChunkGzipBytes)}) ` +
      `${lar.withinTolerance ? "PASS" : "FAIL"}`,
  );
  lines.push(`**Total raw JS:** ${fmtBytes(totalRaw)} (informational)`);
  lines.push(`**Tolerance:** ${fe.tolerancePercent}%`);
  lines.push("");
  lines.push("| Chunk | Raw | Gzipped |");
  lines.push("|-------|-----|---------|");
  for (const c of chunks) {
    lines.push(`| \`${c.name}\` | ${fmtBytes(c.raw)} | ${fmtBytes(c.gz)} |`);
  }
  lines.push("");
  lines.push(
    `_Source of truth: \`perf-budgets.json\`. ` +
      `Update procedure: \`docs/perf-budgets.md\`._`,
  );
  const summary = lines.join("\n");

  // Always print to stdout - local invocation relies on this.
  console.log(summary);

  // GitHub Actions: publish to the run summary panel.
  const stepSummary = process.env.GITHUB_STEP_SUMMARY;
  if (stepSummary) {
    appendFileSync(stepSummary, `${summary}\n`);
  }

  // GitHub Actions: emit values for the script-comment job.
  const ghOutput = process.env.GITHUB_OUTPUT;
  if (ghOutput) {
    const out = [
      `total_gzip_bytes=${totalGz}`,
      `largest_gzip_bytes=${largest.gz}`,
      `largest_chunk_name=${largest.name}`,
      `over_budget=${overBudget ? "true" : "false"}`,
    ].join("\n");
    appendFileSync(ghOutput, `${out}\n`);
  }

  if (overBudget) {
    console.error(
      `[perf-summary] Budget exceeded. ` +
        `See ${BUDGETS_PATH} and docs/perf-budgets.md for the update procedure.`,
    );
    process.exit(1);
  }
}

main();
