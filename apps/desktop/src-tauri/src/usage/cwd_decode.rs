//! Decode Claude Code's path-encoded project directory names.
//!
//! Claude Code stores per-project session JSONL files under
//! `~/.claude/projects/<encoded-cwd>/<session-uuid>.jsonl`, where
//! `<encoded-cwd>` is the original absolute path with every `/`
//! replaced by `-` (and the leading `/` becoming a leading `-`).
//!
//! Example:
//!   `/Users/joseairosa/Development/allseeingeye`
//!     -> `-Users-joseairosa-Development-allseeingeye`
//!
//! ## Lossiness
//!
//! The encoding is **lossy**: a literal `-` inside a path segment is
//! also written as `-`, so two distinct real paths can collide:
//!
//!   `/a/b-c/d`  -> `-a-b-c-d`
//!   `/a/b/c/d`  -> `-a-b-c-d`
//!
//! There is no way to recover the original path from the directory
//! name alone. The naive decoder produced by [`decode_naive`] simply
//! turns every `-` back into `/` and accepts the false-positive `/`
//! splits when the real path has hyphens.
//!
//! Callers SHOULD prefer the `cwd` field embedded in the first JSONL
//! line (Claude Code records the actual cwd inside the file). This
//! decoder is a fallback for the rare case where no usage-bearing
//! lines exist yet but a session directory has already been created.

use std::path::PathBuf;

/// Decode an encoded cwd directory name into a best-guess absolute path.
///
/// Returns `None` if the name is empty or does not start with the
/// expected `-` prefix (Claude Code only encodes absolute paths, so
/// the decoder refuses to invent a `/` for relative-looking inputs).
///
/// **Heuristic** - hyphens in the original path segments are silently
/// flipped to `/`. The decoder cannot tell the difference; it accepts
/// the loss. Most production cases (`~/Development/<repo>`) round-trip
/// cleanly because the path components there rarely contain hyphens
/// in the leading segments.
///
/// Callers prefer the in-line `cwd` field carried inside the JSONL
/// itself (more reliable). This helper is the documented fallback
/// for sessions that have not yet emitted any cwd-bearing lines.
#[allow(dead_code)]
#[must_use]
pub fn decode_naive(encoded: &str) -> Option<PathBuf> {
    if encoded.is_empty() {
        return None;
    }
    if !encoded.starts_with('-') {
        return None;
    }
    // Replace every `-` with `/`. The leading `-` becomes the root
    // separator. We do NOT attempt to detect hyphens in segments here:
    // the encoding is lossy and the decoder cannot recover them.
    let decoded: String = encoded
        .chars()
        .map(|c| if c == '-' { '/' } else { c })
        .collect();
    Some(PathBuf::from(decoded))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_simple_absolute_path() {
        let p = decode_naive("-Users-joseairosa-Development-allseeingeye").unwrap();
        assert_eq!(
            p,
            PathBuf::from("/Users/joseairosa/Development/allseeingeye")
        );
    }

    #[test]
    fn decodes_root_only() {
        // A single dash is the encoded form of `/` itself.
        let p = decode_naive("-").unwrap();
        assert_eq!(p, PathBuf::from("/"));
    }

    #[test]
    fn decodes_with_hyphenated_segment() {
        // The encoder is lossy: the real path
        // `/Users/joseairosa/Development/codesalvage-com` round-trips
        // back to a path that splits the hyphen as a directory
        // separator. We document this behaviour here so future
        // refactors do not silently change it.
        let p = decode_naive("-Users-joseairosa-Development-codesalvage-com").unwrap();
        assert_eq!(
            p,
            PathBuf::from("/Users/joseairosa/Development/codesalvage/com"),
            "naive decoder splits hyphens; this is the documented loss"
        );
    }

    #[test]
    fn decodes_deep_nested_path() {
        // Real example from the user's home: a worktree under a games dir.
        let p = decode_naive(
            "-Users-joseairosa-Development-games-orbit--claude-worktrees-serene-lumiere-6e64c5",
        )
        .unwrap();
        // We don't pin the exact split because of the lossy encoding;
        // we only assert that the prefix is correct and the result is
        // absolute.
        assert!(p.is_absolute());
        assert!(p.starts_with("/Users/joseairosa/Development/games/orbit"));
    }

    #[test]
    fn rejects_empty_input() {
        assert!(decode_naive("").is_none());
    }

    #[test]
    fn rejects_relative_looking_input() {
        // Relative paths are not produced by Claude Code's encoder, so
        // a missing leading dash is treated as malformed input.
        assert!(decode_naive("Users-joseairosa").is_none());
    }
}
