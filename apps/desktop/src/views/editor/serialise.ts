/**
 * Format-specific AST projection + serialisation.
 *
 * The editor maintains the form as a JS object. To round-trip the
 * raw text the form needs:
 *   * `project(raw) -> ast` - read the relevant slice (frontmatter
 *     for Markdown, structured value for JSON / TOML / YAML).
 *   * `serialise(ast, originalRaw) -> raw` - re-emit the slice
 *     verbatim while preserving the surrounding text (Markdown body,
 *     comments, etc.) where we can.
 *
 * Phase 3.3 ships this for the two cases the editor first surfaces:
 *   * markdownFrontmatter / mdc (skill, agent, rule, ...)
 *   * json (settings, MCP, hooks)
 *
 * TOML / YAML pure-data formats fall back to a JSON-shaped form ast
 * with a non-round-tripping serialiser - the form still works, but
 * the raw view is the source of truth for those formats and a form
 * edit re-emits them as JSON. Documented in the source so the next
 * phase can swap in real TOML / YAML emitters when those land.
 */
import type { Format } from "@aseye/shared-types";
import type { AstProjector, AstSerialiser, FormAst } from "./EditState";

/**
 * Pick the right (project, serialise) pair for a component format.
 * Defaults to a JSON pair when the format is unrecognised; the form
 * pane gates on `schema` being null so an unknown format renders a
 * read-only stub anyway.
 */
export function projectorFor(format: Format): {
  project: AstProjector;
  serialise: AstSerialiser;
} {
  switch (format) {
    case "markdown":
    case "markdownfrontmatter":
    case "mdc":
      return {
        project: projectMarkdownFrontmatter,
        serialise: serialiseMarkdownFrontmatter,
      };
    case "json":
      return { project: projectJson, serialise: serialiseJson };
    default:
      // JSON-shaped form ast for everything else. Edits land back
      // as JSON which is technically a content swap for TOML/YAML
      // files, but this branch is gated by the form pane (no schema
      // → read-only stub) so it's never actually hit.
      return { project: projectJson, serialise: serialiseJson };
  }
}

// ─── Markdown + YAML frontmatter ────────────────────────────────────

const FRONTMATTER_DELIMITER = /^---\s*$/;

interface FrontmatterSplit {
  hasFrontmatter: boolean;
  yaml: string;
  body: string;
  /** EOL used by the original document (`\n` or `\r\n`). */
  eol: string;
}

/**
 * Split a Markdown document into (frontmatter, body). Mirrors the
 * Rust parser's permissive `---` handling - leading delimiter on
 * line 1, closing `---` on its own line. CRLF and LF both work.
 */
export function splitFrontmatter(raw: string): FrontmatterSplit {
  const eol = raw.includes("\r\n") ? "\r\n" : "\n";
  const lines = raw.split(eol);
  if (lines.length === 0 || !FRONTMATTER_DELIMITER.test(lines[0] ?? "")) {
    return { hasFrontmatter: false, yaml: "", body: raw, eol };
  }
  const closingIdx = lines.findIndex(
    (line, idx) => idx > 0 && FRONTMATTER_DELIMITER.test(line),
  );
  if (closingIdx <= 0) {
    return { hasFrontmatter: false, yaml: "", body: raw, eol };
  }
  const yaml = lines.slice(1, closingIdx).join(eol);
  const body = lines.slice(closingIdx + 1).join(eol);
  return { hasFrontmatter: true, yaml, body, eol };
}

/**
 * Parse a YAML object into a flat AST. We deliberately don't pull
 * `js-yaml` for this MVP - the bundled schemas only declare
 * primitive scalars, string arrays, and booleans, all of which
 * round-trip cleanly through a tiny line-oriented parser. Anything
 * the parser doesn't recognise is preserved as a string for the
 * raw pane to handle.
 */
function parseYamlScalarMap(yaml: string): FormAst {
  const out: FormAst = {};
  const lines = yaml.split(/\r?\n/);
  let i = 0;
  while (i < lines.length) {
    const line = lines[i] ?? "";
    i += 1;
    const trimmed = line.trim();
    if (trimmed === "" || trimmed.startsWith("#")) continue;
    const colon = line.indexOf(":");
    if (colon < 0) continue;
    const key = line.slice(0, colon).trim();
    const valueText = line.slice(colon + 1).trimStart();
    if (valueText === "" || valueText === "|" || valueText === ">") {
      // Block scalar / nested object: collect indented lines until
      // we hit a non-indented line.
      const indented: string[] = [];
      while (i < lines.length) {
        const next = lines[i] ?? "";
        if (next.length > 0 && (next.startsWith(" ") || next.startsWith("\t"))) {
          indented.push(next);
          i += 1;
        } else if (next.trim() === "") {
          indented.push(next);
          i += 1;
        } else {
          break;
        }
      }
      // Treat as a list when every non-empty indented line starts
      // with `-`; otherwise fall back to a raw string.
      const items = indented
        .map((l) => l.trim())
        .filter((l) => l.length > 0);
      if (items.every((l) => l.startsWith("- "))) {
        out[key] = items.map((l) => parseYamlScalar(l.slice(2).trim()));
      } else {
        out[key] = indented.join("\n");
      }
      continue;
    }
    out[key] = parseYamlScalar(valueText);
  }
  return out;
}

/** Map a single scalar token (`true`, `42`, `"foo"`, plain) to a JS value. */
function parseYamlScalar(token: string): unknown {
  const trimmed = token.trim();
  if (trimmed === "true") return true;
  if (trimmed === "false") return false;
  if (trimmed === "null" || trimmed === "~") return null;
  if (/^-?\d+$/.test(trimmed)) return Number.parseInt(trimmed, 10);
  if (/^-?\d+\.\d+$/.test(trimmed)) return Number.parseFloat(trimmed);
  if (
    (trimmed.startsWith('"') && trimmed.endsWith('"')) ||
    (trimmed.startsWith("'") && trimmed.endsWith("'"))
  ) {
    return trimmed.slice(1, -1);
  }
  if (trimmed.startsWith("[") && trimmed.endsWith("]")) {
    const inner = trimmed.slice(1, -1).trim();
    if (inner === "") return [];
    return inner.split(",").map((s) => parseYamlScalar(s.trim()));
  }
  return trimmed;
}

/** Serialise a flat AST back to YAML using the same primitive set. */
function emitYamlScalarMap(ast: FormAst): string {
  const lines: string[] = [];
  for (const [key, value] of Object.entries(ast)) {
    lines.push(`${key}: ${emitYamlScalar(value)}`);
  }
  return lines.join("\n");
}

function emitYamlScalar(value: unknown): string {
  if (value === null || value === undefined) return "null";
  if (typeof value === "boolean") return value ? "true" : "false";
  if (typeof value === "number") return String(value);
  if (typeof value === "string") {
    // Quote strings that contain `:`, `#`, leading whitespace,
    // or that would otherwise be mis-parsed as another scalar type.
    if (
      value === "" ||
      /[:#\n]/.test(value) ||
      /^\s/.test(value) ||
      /^(true|false|null|~|-?\d+(\.\d+)?)$/.test(value)
    ) {
      return JSON.stringify(value);
    }
    return value;
  }
  if (Array.isArray(value)) {
    if (value.length === 0) return "[]";
    return "[" + value.map(emitYamlScalar).join(", ") + "]";
  }
  // Objects fall back to JSON serialisation - the form pane never
  // produces nested objects on its own.
  return JSON.stringify(value);
}

const projectMarkdownFrontmatter: AstProjector = (raw) => {
  const split = splitFrontmatter(raw);
  if (!split.hasFrontmatter) {
    return { ok: true, ast: {} };
  }
  try {
    return { ok: true, ast: parseYamlScalarMap(split.yaml) };
  } catch (err) {
    return { ok: false, error: err instanceof Error ? err.message : String(err) };
  }
};

const serialiseMarkdownFrontmatter: AstSerialiser = (ast, original) => {
  const split = splitFrontmatter(original);
  const yaml = emitYamlScalarMap(ast);
  // If the original lacked frontmatter we add it; the body is
  // preserved verbatim.
  if (!split.hasFrontmatter) {
    return `---${split.eol}${yaml}${split.eol}---${split.eol}${original}`;
  }
  return `---${split.eol}${yaml}${split.eol}---${split.eol}${split.body}`;
};

// ─── JSON ────────────────────────────────────────────────────────────

const projectJson: AstProjector = (raw) => {
  const trimmed = raw.trim();
  if (trimmed === "") return { ok: true, ast: {} };
  try {
    const value: unknown = JSON.parse(raw);
    if (value === null || typeof value !== "object" || Array.isArray(value)) {
      // We expect an object at the root for settings / MCP shapes.
      return { ok: true, ast: {} };
    }
    return { ok: true, ast: value as FormAst };
  } catch (err) {
    return { ok: false, error: err instanceof Error ? err.message : String(err) };
  }
};

const serialiseJson: AstSerialiser = (ast, original) => {
  // Preserve the original indentation when we can detect it; default
  // to two-space indent so the formatter stays consistent with the
  // rest of the codebase.
  const indent = detectJsonIndent(original) ?? 2;
  return JSON.stringify(ast, null, indent) + "\n";
};

function detectJsonIndent(raw: string): number | null {
  // Look for the first indented line and count its leading spaces.
  const match = raw.match(/\n( +)\S/);
  if (match && match[1]) {
    return match[1].length;
  }
  return null;
}
