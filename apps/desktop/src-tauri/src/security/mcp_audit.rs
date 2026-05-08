//! MCP server permission audit (Phase 7.2).
//!
//! Mirrors `docs/12-security.md` Section B - given a parsed component
//! that describes one or more MCP server registrations, infer a
//! permission posture for each entry and emit a [`Finding`] when the
//! posture is privileged enough to surface in the Security view.
//!
//! Inference is **purely static**: we read the configuration and
//! reason about flags, env-var names, connection strings, and package
//! names. We never connect to the database, never resolve the
//! filesystem, never call an HTTP endpoint. v1 is "what does the
//! config say?" - runtime probing is intentionally out of scope per
//! the doc's "Out of scope" list.
//!
//! # Recognised shapes
//!
//! `docs/04-data-sources.md` documents the canonical MCP server JSON:
//!
//! * stdio: `{ "command": "...", "args": [...], "env": {...} }`
//! * http:  `{ "transport": "http", "url": "...", "headers": {...} }`
//! * sse:   `{ "transport": "sse",  "url": "..." }`
//!
//! Across host tools, MCP servers live either as a top-level
//! `mcpServers` object (Claude Code's `~/.claude.json`,
//! `~/.cursor/mcp.json`), or as a TOML `[mcp_servers.<name>]` table
//! (Codex). Both forms parse into a `serde_json::Value` map of name
//! -> server config thanks to the parser dispatch normalising TOML to
//! JSON. We accept either layout.
//!
//! # What we emit
//!
//! For every privileged shape we produce a `Finding` with:
//!
//! * `category = Category::McpPermission`,
//! * a stable `pattern` name (`postgres-mcp-write`, `github-mcp-write`,
//!   ...) that the suppression table can join on,
//! * a severity matching the docs/12 table,
//! * `source_label` = JSON-pointer-style path into the MCP map (e.g.
//!   `/mcpServers/postgres`),
//! * `evidence` = a small JSON object with the reason (host, database,
//!   token-env-var-name, repo scope, ...). NEVER the credential value.
//!
//! Unknown / unrecognised MCP servers emit no finding - we'd rather
//! miss than misreport. The audit is additive over time as the rule
//! table grows.

use serde_json::{json, Map, Value};

use super::finding::{Category, Finding, Severity};

/// Audit a parsed component for MCP-permission findings.
///
/// The function is defensive: arbitrary JSON / TOML / YAML input that
/// doesn't look like an MCP server configuration returns an empty
/// vector. Callers (the upsert pipeline) are still expected to filter
/// by `ComponentType::Mcp` before calling - this guard exists so unit
/// tests and one-off audits can pass any parsed structure without
/// upstream routing.
#[must_use]
pub fn audit_mcp_component(parsed: &crate::parser::ParsedComponent) -> Vec<Finding> {
    let mut findings = Vec::new();
    let Some(structured) = parsed.structured.as_ref() else {
        return findings;
    };

    // Two layout shapes we support out of the box:
    // 1) Top-level `mcpServers` object whose values are server configs.
    // 2) Top-level `mcp_servers` (Codex / TOML) - same shape after the
    //    parser normalises TOML keys.
    // 3) The structured value IS a single server config (one MCP per
    //    file - rare but valid, e.g. when an audit harness passes a
    //    single entry directly).
    let now = unix_now_millis();

    if let Some(servers) = pick_servers_map(structured) {
        for (name, config) in servers {
            audit_one_server(name, config, &mut findings, now);
        }
        return findings;
    }

    // Fallback: treat the whole structured value as a single server
    // config when it has the canonical fields. This branch keeps unit
    // tests ergonomic (one server per fixture) without needing to
    // wrap each fixture in `{"mcpServers": {...}}`.
    if looks_like_server(structured) {
        audit_one_server("server", structured, &mut findings, now);
    }
    findings
}

/// Find the servers map inside a parsed structure, if present.
///
/// Returns the entries as `(name, config)` pairs in the natural
/// iteration order of the underlying `BTreeMap` (keys sorted), so
/// findings come out deterministic across runs.
fn pick_servers_map(value: &Value) -> Option<Vec<(&str, &Value)>> {
    let obj = value.as_object()?;
    for key in ["mcpServers", "mcp_servers"] {
        if let Some(Value::Object(map)) = obj.get(key) {
            // Hand-build the `(name, config)` pairs so we own the
            // iteration order (alphabetical via the underlying
            // `BTreeMap`/preserved-insertion `Map`).
            let pairs: Vec<(&str, &Value)> = map.iter().map(|(k, v)| (k.as_str(), v)).collect();
            return Some(pairs);
        }
    }
    None
}

/// Heuristic check: does this object have one of the canonical MCP
/// server fields (`command`, `url`, or an explicit `transport`)?
fn looks_like_server(value: &Value) -> bool {
    let Some(obj) = value.as_object() else {
        return false;
    };
    obj.contains_key("command") || obj.contains_key("url") || obj.contains_key("transport")
}

/// Audit a single MCP server entry.
///
/// Routing here is deliberately specific: each known MCP package has
/// its own rule. Generic stdio MCP entries with no recognised package
/// are ignored - we'd rather miss than guess. The doc's
/// "generic stdio MCP whose env vars include any value matched by
/// Audit category A" rule is intentionally out of scope at this
/// layer; secrets are caught by the existing `scanner::scan_parsed`
/// pass and the upsert pipeline fans both out.
fn audit_one_server(name: &str, config: &Value, sink: &mut Vec<Finding>, now: i64) {
    let pointer = format!("/mcpServers/{}", escape_pointer(name));
    let Some(obj) = config.as_object() else {
        return;
    };

    // Try each ruleset in turn; the first to match owns the entry.
    if try_postgres(name, &pointer, obj, sink, now) {
        return;
    }
    if try_github(name, &pointer, obj, sink, now) {
        return;
    }
    if try_filesystem(name, &pointer, obj, sink, now) {
        return;
    }
    // The final `try_*` returns its own bool but we don't need the
    // value: success means a finding was pushed and we're done; the
    // "fall through to unknown MCP" branch is a no-op anyway.
    let _ = try_stripe(name, &pointer, obj, sink, now);
}

// -----------------------------------------------------------------------------
// Postgres MCP
// -----------------------------------------------------------------------------

/// Recognise either `@modelcontextprotocol/server-postgres` (the
/// official package) or `mcp-server-postgres` (the standalone build).
const POSTGRES_PACKAGES: &[&str] = &[
    "@modelcontextprotocol/server-postgres",
    "mcp-server-postgres",
];

fn try_postgres(
    name: &str,
    pointer: &str,
    obj: &Map<String, Value>,
    sink: &mut Vec<Finding>,
    now: i64,
) -> bool {
    let args = obj.get("args").and_then(Value::as_array);
    if !mentions_any_package(args, POSTGRES_PACKAGES) {
        return false;
    }

    let read_only_flag = arg_present(args, "--read-only");

    // Parse the connection string out of the args: convention is the
    // last positional argument that starts with `postgres://` or
    // `postgresql://`.
    let conn_str = args
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .find(|s| s.starts_with("postgres://") || s.starts_with("postgresql://"));

    let parsed = conn_str.and_then(parse_postgres_conn);

    // Order of precedence: if the user explicitly bound an admin role,
    // the command-line flag does not save them.
    if let Some(p) = parsed.as_ref() {
        if is_admin_role(&p.user) {
            sink.push(make_finding(
                "postgres-mcp-admin",
                Severity::Critical,
                pointer,
                json!({
                    "package": package_used(args, POSTGRES_PACKAGES),
                    "name": name,
                    "host": p.host,
                    "database": p.database,
                    "role": p.user,
                }),
                "postgres-admin-role",
                now,
            ));
            return true;
        }
    }

    if read_only_flag {
        sink.push(make_finding(
            "postgres-mcp-read-only",
            Severity::Low,
            pointer,
            json!({
                "package": package_used(args, POSTGRES_PACKAGES),
                "name": name,
                "host": parsed.as_ref().map(|p| p.host.clone()),
                "database": parsed.as_ref().map(|p| p.database.clone()),
                "reason": "explicit --read-only flag",
            }),
            "postgres-read-only-flag",
            now,
        ));
        return true;
    }

    if let Some(p) = parsed.as_ref() {
        if is_readonly_role(&p.user) {
            sink.push(make_finding(
                "postgres-mcp-read-only",
                Severity::Low,
                pointer,
                json!({
                    "package": package_used(args, POSTGRES_PACKAGES),
                    "name": name,
                    "host": p.host,
                    "database": p.database,
                    "role": p.user,
                    "reason": "role name pattern indicates read-only",
                }),
                "postgres-readonly-role",
                now,
            ));
            return true;
        }
    }

    // Fall through: writable Postgres MCP without `--read-only` and
    // without an obviously-RO role.
    sink.push(make_finding(
        "postgres-mcp-write",
        Severity::High,
        pointer,
        json!({
            "package": package_used(args, POSTGRES_PACKAGES),
            "name": name,
            "host": parsed.as_ref().map(|p| p.host.clone()),
            "database": parsed.as_ref().map(|p| p.database.clone()),
            "role": parsed.as_ref().map(|p| p.user.clone()),
        }),
        "postgres-no-read-only",
        now,
    ));
    true
}

/// Parsed view of a Postgres connection string. We only extract the
/// fields we surface as evidence - never the password or query
/// parameters.
struct PgConn {
    user: String,
    host: String,
    database: String,
}

/// Lightweight Postgres URL parser.
///
/// Avoids pulling in the `url` crate (already in tree but the parser
/// here is intentionally narrower than RFC 3986). Handles:
/// * `postgres://` and `postgresql://` schemes,
/// * optional `user[:password]@`,
/// * `host[:port]`,
/// * `/database`,
/// * trailing `?key=value` query string (ignored).
///
/// Returns `None` for any input we can't confidently destructure -
/// the caller treats that as "no role inference" and falls through to
/// the default writable-MCP rule.
fn parse_postgres_conn(s: &str) -> Option<PgConn> {
    let stripped = s
        .strip_prefix("postgres://")
        .or_else(|| s.strip_prefix("postgresql://"))?;
    // Trim the query string (everything after `?`).
    let head = stripped.split('?').next().unwrap_or(stripped);

    // Userinfo is everything before the last `@`. The host portion is
    // anchored on the *last* `@` so passwords containing `@` (rare but
    // legal when percent-encoded) don't confuse us.
    let (userinfo, hostpath) = match head.rsplit_once('@') {
        Some((u, h)) => (Some(u), h),
        None => (None, head),
    };

    let user = userinfo
        .and_then(|u| u.split(':').next())
        .unwrap_or("")
        .to_owned();

    // Split host[:port]/database. If there's no `/`, the database is
    // empty - the user did not specify one.
    let (host_with_port, db) = match hostpath.split_once('/') {
        Some((h, d)) => (h, d),
        None => (hostpath, ""),
    };
    let host = host_with_port
        .split(':')
        .next()
        .unwrap_or(host_with_port)
        .to_owned();
    let database = db.to_owned();

    Some(PgConn {
        user,
        host,
        database,
    })
}

/// Is the role name shaped like an admin/superuser account? The match
/// is conservative - we only flag widely-recognised admin role names
/// and explicit `_admin` substrings. Lowercase comparison.
fn is_admin_role(user: &str) -> bool {
    let lc = user.to_ascii_lowercase();
    matches!(lc.as_str(), "postgres" | "superuser" | "rds_superuser") || lc.contains("_admin")
}

/// Is the role name shaped like a read-only account? Matches the
/// (case-insensitive) regex `(_ro|_readonly|readonly_)` from the doc.
fn is_readonly_role(user: &str) -> bool {
    let lc = user.to_ascii_lowercase();
    lc.ends_with("_ro")
        || lc.ends_with("_readonly")
        || lc.starts_with("readonly_")
        || lc.contains("_readonly_")
        || lc.contains("_ro_")
}

// -----------------------------------------------------------------------------
// GitHub MCP
// -----------------------------------------------------------------------------

const GITHUB_PACKAGES: &[&str] = &["@modelcontextprotocol/server-github", "gh-mcp-server"];

fn try_github(
    name: &str,
    pointer: &str,
    obj: &Map<String, Value>,
    sink: &mut Vec<Finding>,
    now: i64,
) -> bool {
    let args = obj.get("args").and_then(Value::as_array);
    if !mentions_any_package(args, GITHUB_PACKAGES) {
        return false;
    }

    if arg_present(args, "--read-only") {
        sink.push(make_finding(
            "github-mcp-read-only",
            Severity::Low,
            pointer,
            json!({
                "package": package_used(args, GITHUB_PACKAGES),
                "name": name,
                "reason": "explicit --read-only flag",
            }),
            "github-read-only-flag",
            now,
        ));
        return true;
    }

    // Inspect env vars for the token. The doc lists `GITHUB_TOKEN` /
    // `GH_TOKEN` as the canonical names; either matches. Names ending
    // in `_READONLY` / `_RO` are treated as informational read-only.
    let env = obj.get("env").and_then(Value::as_object);
    let token_var = env
        .into_iter()
        .flat_map(|m| m.keys().map(String::as_str))
        .find(|k| matches!(*k, "GITHUB_TOKEN" | "GH_TOKEN") || k.contains("TOKEN"));

    if let Some(var) = token_var {
        if var.ends_with("_READONLY") || var.ends_with("_RO") {
            sink.push(make_finding(
                "github-mcp-read-only",
                Severity::Low,
                pointer,
                json!({
                    "package": package_used(args, GITHUB_PACKAGES),
                    "name": name,
                    "tokenEnvVar": var,
                    "reason": "token env-var name suffix indicates read-only",
                }),
                "github-readonly-token-name",
                now,
            ));
            return true;
        }
    }

    sink.push(make_finding(
        "github-mcp-write",
        Severity::High,
        pointer,
        json!({
            "package": package_used(args, GITHUB_PACKAGES),
            "name": name,
            "tokenEnvVar": token_var,
        }),
        "github-no-read-only",
        now,
    ));
    true
}

// -----------------------------------------------------------------------------
// Filesystem MCP
// -----------------------------------------------------------------------------

const FILESYSTEM_PACKAGES: &[&str] = &["@modelcontextprotocol/server-filesystem"];

fn try_filesystem(
    name: &str,
    pointer: &str,
    obj: &Map<String, Value>,
    sink: &mut Vec<Finding>,
    now: i64,
) -> bool {
    let args = obj.get("args").and_then(Value::as_array);
    if !mentions_any_package(args, FILESYSTEM_PACKAGES) {
        return false;
    }

    // Capture every positional that doesn't look like a flag - those
    // are the granted paths. We never resolve them; surfacing as
    // evidence is the value here.
    let paths: Vec<&str> = args
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter(|s| {
            !s.starts_with('-')
                && !s.starts_with('@')
                && !s.starts_with("npx")
                && !FILESYSTEM_PACKAGES.contains(s)
        })
        .collect();

    let read_only = arg_present(args, "--paths-only-readable");

    let severity = if read_only {
        Severity::Low
    } else {
        Severity::High
    };
    let pattern = if read_only {
        "filesystem-mcp-read-only"
    } else {
        "filesystem-mcp-write"
    };
    let id_seed = if read_only {
        "filesystem-paths-only-readable"
    } else {
        "filesystem-no-paths-only-readable"
    };

    sink.push(make_finding(
        pattern,
        severity,
        pointer,
        json!({
            "package": "@modelcontextprotocol/server-filesystem",
            "name": name,
            "paths": paths,
            "readOnly": read_only,
        }),
        id_seed,
        now,
    ));
    true
}

// -----------------------------------------------------------------------------
// Stripe MCP
// -----------------------------------------------------------------------------

fn try_stripe(
    name: &str,
    pointer: &str,
    obj: &Map<String, Value>,
    sink: &mut Vec<Finding>,
    now: i64,
) -> bool {
    // Stripe has multiple packagings (official + third-party). We
    // detect by the presence of an `sk_live_*` / `sk_test_*` value
    // anywhere in env or args - that's the load-bearing signal. A
    // package-name match would be overly narrow.
    let env_vals: Vec<&str> = obj
        .get("env")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|m| m.values().filter_map(Value::as_str))
        .collect();
    let arg_vals: Vec<&str> = obj
        .get("args")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    let header_vals: Vec<&str> = obj
        .get("headers")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|m| m.values().filter_map(Value::as_str))
        .collect();

    let any_live = env_vals
        .iter()
        .chain(arg_vals.iter())
        .chain(header_vals.iter())
        .any(|v| v.contains("sk_live_"));
    let any_test = env_vals
        .iter()
        .chain(arg_vals.iter())
        .chain(header_vals.iter())
        .any(|v| v.contains("sk_test_"));

    if any_live {
        sink.push(make_finding(
            "stripe-mcp-live",
            Severity::Critical,
            pointer,
            json!({
                "name": name,
                "mode": "live",
            }),
            "stripe-live-key",
            now,
        ));
        return true;
    }
    if any_test {
        sink.push(make_finding(
            "stripe-mcp-test",
            Severity::Low,
            pointer,
            json!({
                "name": name,
                "mode": "test",
            }),
            "stripe-test-key",
            now,
        ));
        return true;
    }
    false
}

// -----------------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------------

/// `args` contains any of the listed package names as a value
/// (typical shape for `npx -y <pkg>`). We accept either a literal
/// match (`"@modelcontextprotocol/server-github"`) or any arg that
/// has the package name as a substring (covers
/// `@modelcontextprotocol/server-github@1.2.3`).
fn mentions_any_package(args: Option<&Vec<Value>>, packages: &[&str]) -> bool {
    let Some(args) = args else {
        return false;
    };
    args.iter()
        .filter_map(Value::as_str)
        .any(|s| packages.iter().any(|p| s.contains(p)))
}

fn package_used<'a>(args: Option<&Vec<Value>>, packages: &'a [&'a str]) -> Option<&'a str> {
    let args = args?;
    for s in args.iter().filter_map(Value::as_str) {
        for p in packages {
            if s.contains(p) {
                return Some(*p);
            }
        }
    }
    None
}

/// True when `args` contains the exact flag string.
fn arg_present(args: Option<&Vec<Value>>, flag: &str) -> bool {
    args.into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .any(|s| s == flag)
}

fn escape_pointer(segment: &str) -> String {
    if !segment.contains('~') && !segment.contains('/') {
        return segment.to_owned();
    }
    let mut out = String::with_capacity(segment.len());
    for ch in segment.chars() {
        match ch {
            '~' => out.push_str("~0"),
            '/' => out.push_str("~1"),
            other => out.push(other),
        }
    }
    out
}

fn make_finding(
    pattern: &str,
    severity: Severity,
    source_label: &str,
    evidence: Value,
    id_seed: &str,
    detected_at: i64,
) -> Finding {
    Finding {
        id: build_finding_id(source_label, id_seed),
        component_id: None,
        category: Category::McpPermission,
        pattern: pattern.to_owned(),
        severity,
        source_label: source_label.to_owned(),
        line: None,
        // MCP findings carry no secret value - the redacted preview
        // is purely informational and reuses the package/name pair so
        // the UI has something compact to render.
        redacted_preview: pattern.to_owned(),
        detected_at,
        evidence: Some(evidence),
    }
}

/// Stable id derived from `(source_label, seed)`. The seed encodes
/// "which rule fired", so a server flipped from RO to RW gets a new
/// id (and the old finding is naturally retired by the parent
/// component's re-upsert).
fn build_finding_id(source_label: &str, seed: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(source_label.as_bytes());
    h.update(b"\x00");
    h.update(b"mcp-permission");
    h.update(b"\x00");
    h.update(seed.as_bytes());
    let digest = h.finalize();
    let mut hex = String::with_capacity(16);
    for byte in &digest[..8] {
        use std::fmt::Write as _;
        let _ = write!(hex, "{byte:02x}");
    }
    format!("aseye-finding-{hex}")
}

fn unix_now_millis() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_millis()).ok())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::{parse_bytes, ParsedComponent};
    use crate::registry::types::Format;

    fn parsed_json(s: &str) -> ParsedComponent {
        parse_bytes(s.as_bytes(), Format::Json).expect("parse json")
    }

    fn first<'a>(findings: &'a [Finding], pattern: &str) -> &'a Finding {
        findings
            .iter()
            .find(|f| f.pattern == pattern)
            .unwrap_or_else(|| panic!("missing pattern {pattern}: {findings:?}"))
    }

    #[test]
    fn audits_postgres_mcp_with_read_only_flag() {
        // `--read-only` set: the writable rule must NOT fire. We expect
        // a low-severity informational finding instead.
        let parsed = parsed_json(
            r#"{
              "mcpServers": {
                "pg": {
                  "command": "npx",
                  "args": [
                    "-y", "@modelcontextprotocol/server-postgres",
                    "postgres://app@db.example.com/prod",
                    "--read-only"
                  ]
                }
              }
            }"#,
        );
        let findings = audit_mcp_component(&parsed);
        assert!(
            !findings.iter().any(|f| f.pattern == "postgres-mcp-write"),
            "expected no write finding, got {findings:?}"
        );
        let f = first(&findings, "postgres-mcp-read-only");
        assert_eq!(f.severity, Severity::Low);
        assert_eq!(f.category, Category::McpPermission);
        assert_eq!(f.source_label, "/mcpServers/pg");
    }

    #[test]
    fn audits_postgres_mcp_without_read_only_flag() {
        let parsed = parsed_json(
            r#"{
              "mcpServers": {
                "pg": {
                  "command": "npx",
                  "args": [
                    "-y", "@modelcontextprotocol/server-postgres",
                    "postgres://app@db.example.com:5432/prod"
                  ]
                }
              }
            }"#,
        );
        let findings = audit_mcp_component(&parsed);
        let f = first(&findings, "postgres-mcp-write");
        assert_eq!(f.severity, Severity::High);
        let ev = f.evidence.as_ref().expect("evidence present");
        assert_eq!(
            ev.get("package").and_then(Value::as_str),
            Some("@modelcontextprotocol/server-postgres")
        );
        assert_eq!(
            ev.get("host").and_then(Value::as_str),
            Some("db.example.com")
        );
        assert_eq!(ev.get("database").and_then(Value::as_str), Some("prod"));
    }

    #[test]
    fn audits_postgres_mcp_with_admin_role() {
        let parsed = parsed_json(
            r#"{
              "mcpServers": {
                "pg": {
                  "command": "npx",
                  "args": [
                    "-y", "@modelcontextprotocol/server-postgres",
                    "postgres://postgres@db.internal/prod"
                  ]
                }
              }
            }"#,
        );
        let findings = audit_mcp_component(&parsed);
        let f = first(&findings, "postgres-mcp-admin");
        assert_eq!(f.severity, Severity::Critical);
        let ev = f.evidence.as_ref().unwrap();
        assert_eq!(ev.get("role").and_then(Value::as_str), Some("postgres"));
    }

    #[test]
    fn audits_postgres_mcp_with_readonly_role_username() {
        let parsed = parsed_json(
            r#"{
              "mcpServers": {
                "pg": {
                  "command": "npx",
                  "args": [
                    "-y", "@modelcontextprotocol/server-postgres",
                    "postgres://reporting_ro@db.internal/analytics"
                  ]
                }
              }
            }"#,
        );
        let findings = audit_mcp_component(&parsed);
        // Should NOT report writable - role suffix indicates read-only.
        assert!(
            !findings.iter().any(|f| f.pattern == "postgres-mcp-write"),
            "unexpected write finding: {findings:?}"
        );
        let f = first(&findings, "postgres-mcp-read-only");
        assert_eq!(f.severity, Severity::Low);
        let ev = f.evidence.as_ref().unwrap();
        assert_eq!(ev.get("role").and_then(Value::as_str), Some("reporting_ro"));
    }

    #[test]
    fn audits_github_mcp_without_read_only() {
        let parsed = parsed_json(
            r#"{
              "mcpServers": {
                "github": {
                  "command": "npx",
                  "args": ["-y", "@modelcontextprotocol/server-github"],
                  "env": { "GITHUB_TOKEN": "ghp_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx" }
                }
              }
            }"#,
        );
        let findings = audit_mcp_component(&parsed);
        let f = first(&findings, "github-mcp-write");
        assert_eq!(f.severity, Severity::High);
        let ev = f.evidence.as_ref().unwrap();
        assert_eq!(
            ev.get("tokenEnvVar").and_then(Value::as_str),
            Some("GITHUB_TOKEN")
        );
    }

    #[test]
    fn audits_github_mcp_with_read_only() {
        let parsed = parsed_json(
            r#"{
              "mcpServers": {
                "github": {
                  "command": "npx",
                  "args": [
                    "-y", "@modelcontextprotocol/server-github", "--read-only"
                  ]
                }
              }
            }"#,
        );
        let findings = audit_mcp_component(&parsed);
        assert!(
            !findings.iter().any(|f| f.pattern == "github-mcp-write"),
            "expected no write finding, got {findings:?}"
        );
        let f = first(&findings, "github-mcp-read-only");
        assert_eq!(f.severity, Severity::Low);
    }

    #[test]
    fn audits_github_mcp_readonly_via_token_var_suffix() {
        // `GITHUB_TOKEN_READONLY` is treated as informational read-only
        // since the env-var name suffix indicates the operator's
        // intent.
        let parsed = parsed_json(
            r#"{
              "mcpServers": {
                "github": {
                  "command": "npx",
                  "args": ["-y", "gh-mcp-server"],
                  "env": { "GITHUB_TOKEN_READONLY": "ghp_xxxxxxxxxxxx" }
                }
              }
            }"#,
        );
        let findings = audit_mcp_component(&parsed);
        let f = first(&findings, "github-mcp-read-only");
        assert_eq!(f.severity, Severity::Low);
    }

    #[test]
    fn audits_filesystem_mcp_without_paths_only_readable() {
        let parsed = parsed_json(
            r#"{
              "mcpServers": {
                "fs": {
                  "command": "npx",
                  "args": [
                    "-y", "@modelcontextprotocol/server-filesystem",
                    "/Users/alice/Documents", "/Users/alice/Code"
                  ]
                }
              }
            }"#,
        );
        let findings = audit_mcp_component(&parsed);
        let f = first(&findings, "filesystem-mcp-write");
        assert_eq!(f.severity, Severity::High);
        let ev = f.evidence.as_ref().unwrap();
        let paths = ev
            .get("paths")
            .and_then(Value::as_array)
            .expect("paths array");
        assert_eq!(paths.len(), 2);
        assert!(paths
            .iter()
            .any(|v| v.as_str() == Some("/Users/alice/Documents")));
    }

    #[test]
    fn audits_filesystem_mcp_with_paths_only_readable() {
        let parsed = parsed_json(
            r#"{
              "mcpServers": {
                "fs": {
                  "command": "npx",
                  "args": [
                    "-y", "@modelcontextprotocol/server-filesystem",
                    "--paths-only-readable",
                    "/Users/alice/Documents"
                  ]
                }
              }
            }"#,
        );
        let findings = audit_mcp_component(&parsed);
        assert!(
            !findings.iter().any(|f| f.pattern == "filesystem-mcp-write"),
            "expected no write finding, got {findings:?}"
        );
        let f = first(&findings, "filesystem-mcp-read-only");
        assert_eq!(f.severity, Severity::Low);
    }

    #[test]
    fn audits_stripe_live_key() {
        let parsed = parsed_json(
            r#"{
              "mcpServers": {
                "stripe": {
                  "command": "npx",
                  "args": ["-y", "stripe-mcp"],
                  "env": { "STRIPE_API_KEY": "sk_live_FIXTURE_NOT_A_REAL_KEY" }
                }
              }
            }"#,
        );
        let findings = audit_mcp_component(&parsed);
        let f = first(&findings, "stripe-mcp-live");
        assert_eq!(f.severity, Severity::Critical);
    }

    #[test]
    fn audits_stripe_test_key() {
        let parsed = parsed_json(
            r#"{
              "mcpServers": {
                "stripe": {
                  "command": "npx",
                  "args": ["-y", "stripe-mcp"],
                  "env": { "STRIPE_API_KEY": "sk_test_FIXTURE_NOT_A_REAL_KEY" }
                }
              }
            }"#,
        );
        let findings = audit_mcp_component(&parsed);
        let f = first(&findings, "stripe-mcp-test");
        assert_eq!(f.severity, Severity::Low);
    }

    #[test]
    fn audits_unknown_mcp_returns_no_finding() {
        // A custom internal MCP server with no recognised package name:
        // we'd rather miss than misreport.
        let parsed = parsed_json(
            r#"{
              "mcpServers": {
                "internal": {
                  "command": "/usr/local/bin/internal-mcp",
                  "args": ["--config", "/etc/internal/mcp.yaml"]
                }
              }
            }"#,
        );
        let findings = audit_mcp_component(&parsed);
        assert!(
            findings.is_empty(),
            "expected no findings for unknown MCP, got {findings:?}"
        );
    }

    #[test]
    fn audits_http_transport_extracts_url_evidence() {
        // HTTP transport with a Stripe live key in the Authorization
        // header. We don't have a Stripe-specific http detector, so
        // the rule that fires is the env-or-args-or-headers `sk_live_`
        // sweep.
        let parsed = parsed_json(
            r#"{
              "mcpServers": {
                "stripe": {
                  "transport": "http",
                  "url": "https://mcp.stripe.com",
                  "headers": {
                    "Authorization": "Bearer sk_live_FIXTURE_NOT_A_REAL_KEY"
                  }
                }
              }
            }"#,
        );
        let findings = audit_mcp_component(&parsed);
        let f = first(&findings, "stripe-mcp-live");
        assert_eq!(f.severity, Severity::Critical);
        // The source_label captures the JSON pointer to the entry,
        // which the UI can resolve back to the raw config to surface
        // the URL alongside.
        assert_eq!(f.source_label, "/mcpServers/stripe");
    }

    #[test]
    fn returns_empty_for_non_mcp_input() {
        // A markdown skill body / a bare object without any of the
        // canonical MCP fields must come back empty.
        let parsed = parse_bytes(
            br"---
name: example
description: a skill
---
body
",
            Format::MarkdownFrontmatter,
        )
        .expect("parse");
        let findings = audit_mcp_component(&parsed);
        assert!(findings.is_empty(), "expected empty, got {findings:?}");
    }

    #[test]
    fn evidence_json_round_trips_through_persist() {
        // Phase 7.2: writing an MCP-permission finding through
        // `persist_findings` and reading it back via
        // `load_finding_evidence` preserves the JSON object byte-for-
        // byte (semantically; key ordering is `serde_json::Value`'s
        // canonical order).
        use crate::index::IndexHandle;
        use crate::security::{load_finding_evidence, persist_findings};

        let handle = IndexHandle::open_in_memory().expect("open");

        // Seed a component row so the FK on `security_finding` is
        // satisfied. The exact shape of the row doesn't matter for
        // this test.
        handle
            .write(|c| {
                c.execute(
                    "INSERT INTO component (
                        id, type, tool, scope, origin, name, path, format, hash, updated_at
                     ) VALUES (?1, 'mcp', 'claude-code', 'user', 'userCreated',
                              'pg', '/tmp/x.json', 'json', 'h', 0)",
                    rusqlite::params!["aseye://test/mcp"],
                )?;
                Ok(())
            })
            .unwrap();

        let parsed = parsed_json(
            r#"{
              "mcpServers": {
                "pg": {
                  "command": "npx",
                  "args": [
                    "-y", "@modelcontextprotocol/server-postgres",
                    "postgres://app@db.example.com/prod"
                  ]
                }
              }
            }"#,
        );
        let findings = audit_mcp_component(&parsed);
        let written = first(&findings, "postgres-mcp-write").clone();

        handle
            .write(|c| {
                persist_findings(
                    c,
                    "aseye://test/mcp",
                    "/tmp/x.json",
                    std::slice::from_ref(&written),
                )
                .expect("persist");
                Ok(())
            })
            .unwrap();

        let loaded = handle
            .read(|c| Ok(load_finding_evidence(c, &written.id).expect("load")))
            .unwrap();
        let evidence = loaded.expect("evidence present");
        assert_eq!(evidence, written.evidence.unwrap());
    }

    #[test]
    fn mcp_audit_idempotent() {
        // Two consecutive audit + persist cycles produce the same row
        // count - the finding id is stable across runs and the upsert's
        // ON CONFLICT DO NOTHING clause swallows the duplicate.
        use crate::index::IndexHandle;
        use crate::security::persist_findings;

        let handle = IndexHandle::open_in_memory().expect("open");
        handle
            .write(|c| {
                c.execute(
                    "INSERT INTO component (
                        id, type, tool, scope, origin, name, path, format, hash, updated_at
                     ) VALUES (?1, 'mcp', 'claude-code', 'user', 'userCreated',
                              'pg', '/tmp/x.json', 'json', 'h', 0)",
                    rusqlite::params!["aseye://test/mcp"],
                )?;
                Ok(())
            })
            .unwrap();

        let parsed = parsed_json(
            r#"{
              "mcpServers": {
                "pg": {
                  "command": "npx",
                  "args": [
                    "-y", "@modelcontextprotocol/server-postgres",
                    "postgres://app@db.example.com/prod"
                  ]
                }
              }
            }"#,
        );
        for _ in 0..3 {
            let findings = audit_mcp_component(&parsed);
            handle
                .write(|c| {
                    persist_findings(c, "aseye://test/mcp", "/tmp/x.json", &findings)
                        .expect("persist");
                    Ok(())
                })
                .unwrap();
        }

        let count: i64 = handle
            .read(|c| {
                Ok(c.query_row(
                    "SELECT COUNT(*) FROM security_finding WHERE component_id = ?1",
                    rusqlite::params!["aseye://test/mcp"],
                    |r| r.get(0),
                )?)
            })
            .unwrap();
        assert_eq!(count, 1, "audit + persist must be idempotent");
    }
}
