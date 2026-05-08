/**
 * Secret detection helpers.
 *
 * High-level shape detection mirroring `docs/12-security.md` audit category A.
 * These regexes are detection signals only - never used to redact, log, or
 * transmit the underlying value. The frontend uses `detectSecretKind` to pick
 * an appropriate `SecretField` rendering for plain-text inputs.
 *
 * The authoritative scanner is the Rust audit engine. The frontend never
 * needs to be exhaustive; it only needs to choose between three visual
 * affordances: token, password, auth-header.
 */

export type SecretFieldKind = "token" | "password" | "auth-header";

interface Pattern {
  kind: SecretFieldKind;
  re: RegExp;
}

/**
 * Ordered patterns - first match wins. Tokens win over passwords win over
 * generic auth headers because a key surrounded by `password=` is still
 * better rendered as a token.
 */
const PATTERNS: readonly Pattern[] = [
  // Tokens: explicit branded prefixes first.
  { kind: "token", re: /sk-ant-[A-Za-z0-9_-]{40,}/ },
  { kind: "token", re: /sk-proj-[A-Za-z0-9_-]+/ },
  { kind: "token", re: /sk-[A-Za-z0-9]{20,}/ },
  { kind: "token", re: /ghp_[A-Za-z0-9]{36}/ },
  { kind: "token", re: /github_pat_[A-Za-z0-9_]{82}/ },
  { kind: "token", re: /gho_[A-Za-z0-9]{36}/ },
  { kind: "token", re: /xox[baprs]-[A-Za-z0-9-]{10,}/ },
  { kind: "token", re: /AKIA[0-9A-Z]{16}/ },
  // JWTs: three base64url segments separated by dots.
  { kind: "token", re: /eyJ[A-Za-z0-9_-]+\.eyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+/ },
  // PEM private key blocks.
  {
    kind: "token",
    re: /-----BEGIN (?:RSA |EC |DSA |OPENSSH )?PRIVATE KEY-----/,
  },
  // Authorization: Bearer ...
  { kind: "auth-header", re: /Authorization:\s*Bearer\s+[A-Za-z0-9._-]{20,}/i },
  // Generic password=... assignment.
  { kind: "password", re: /password\s*[:=]\s*["']?[^\s"']{8,}/i },
  // Generic api[_-]?key/secret/token=... assignment.
  {
    kind: "token",
    re: /(?:api[_-]?key|secret|token)\s*[:=]\s*["']?[A-Za-z0-9_-]{16,}/i,
  },
];

/**
 * Detect the "shape" of a secret in the supplied text. Returns null when no
 * pattern matches. Used by callers that have a free-form string and want to
 * pick a `SecretField` `kind` automatically.
 *
 * The function never returns the matched value or its position - those are
 * the concern of the Rust audit engine, not the UI.
 */
export function detectSecretKind(text: string): SecretFieldKind | null {
  if (!text) return null;
  for (const { kind, re } of PATTERNS) {
    if (re.test(text)) return kind;
  }
  return null;
}

/**
 * Build a masked rendering of `value` keeping the last `tail` characters
 * visible. Used by `SecretField` to render the masked variant.
 */
export function maskSecret(value: string, tail = 4): string {
  if (!value) return "";
  if (value.length <= tail) return "•".repeat(value.length);
  // Cap the bullet run at 12 so very long values don't blow the layout.
  const bullets = "•".repeat(Math.min(12, value.length - tail));
  return `${bullets}${value.slice(-tail)}`;
}
