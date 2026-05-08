//! Tool detection.
//!
//! Pure-ish probes that ask the local filesystem and `PATH` whether a given
//! tool is installed. The only side-effects are filesystem stat-ing and a
//! short-lived subprocess for the version command.
//!
//! No `SQLite`, no IPC, no watchers - those are later phases.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use super::types::{DetectedTool, ToolDescriptor};

/// Subprocess hard timeout for the version command.
const VERSION_TIMEOUT: Duration = Duration::from_secs(2);

/// Detect every registered tool in registry order.
#[must_use]
pub fn detect_all() -> Vec<DetectedTool> {
    super::registry()
        .iter()
        .map(|d| detect_one(d, None))
        .collect()
}

/// Detect a single tool. `home_override` is for tests; production callers
/// should pass `None` to use the real `$HOME`.
#[must_use]
pub fn detect_one(
    descriptor: &ToolDescriptor,
    home_override: Option<&Path>,
) -> DetectedTool {
    let home: Option<PathBuf> = home_override
        .map(Path::to_path_buf)
        .or_else(dirs::home_dir);

    let existing_root_paths = collect_existing_paths(
        descriptor.detection_paths.iter().map(String::as_str),
        home.as_deref(),
    );

    let binary = find_binary(&descriptor.binary_names);

    let detected = !existing_root_paths.is_empty() || binary.is_some();

    let version = binary
        .as_ref()
        .and(descriptor.version_command.as_deref())
        .and_then(run_version_command);

    DetectedTool {
        id: descriptor.id,
        display_name: descriptor.display_name.clone(),
        detected,
        binary,
        version,
        existing_root_paths,
    }
}

/// Expand a leading `~/` against the supplied (or system) home directory.
///
/// Returns the original path unchanged when no home is available; callers
/// then probe the path as-is, which will simply not exist on a system
/// without a discoverable HOME.
#[must_use]
pub fn expand_home(path: &str, home: Option<&Path>) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = home {
            return home.join(rest);
        }
    }
    if path == "~" {
        if let Some(home) = home {
            return home.to_path_buf();
        }
    }
    PathBuf::from(path)
}

fn collect_existing_paths<'a, I>(
    candidates: I,
    home: Option<&Path>,
) -> Vec<String>
where
    I: IntoIterator<Item = &'a str>,
{
    candidates
        .into_iter()
        .filter_map(|raw| {
            let resolved = expand_home(raw, home);
            // We only care that the path exists - file or directory both count.
            // `try_exists` is the correct API: `exists` swallows IO errors.
            match resolved.try_exists() {
                Ok(true) => Some(path_to_string(&resolved)),
                Ok(false) | Err(_) => None,
            }
        })
        .collect()
}

fn find_binary(names: &[String]) -> Option<String> {
    names
        .iter()
        .find_map(|name| which::which(name).ok().map(|p| path_to_string(&p)))
}

/// Run the configured version command, returning trimmed stdout when the
/// process exits successfully within the hard timeout.
fn run_version_command(cmd_line: &str) -> Option<String> {
    let mut tokens = cmd_line.split_whitespace();
    let program = tokens.next()?;
    let args: Vec<&str> = tokens.collect();

    let mut child = Command::new(program)
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;

    let deadline = Instant::now() + VERSION_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => {
                let output = child.wait_with_output().ok()?;
                let stdout =
                    String::from_utf8_lossy(&output.stdout).trim().to_owned();
                return if stdout.is_empty() { None } else { Some(stdout) };
            }
            // Either the process exited non-zero, or `try_wait` failed
            // outright. In both cases there's nothing to report.
            Ok(Some(_)) | Err(_) => return None,
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    }
}

#[cfg(unix)]
fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(not(unix))]
fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::super::tools;
    use super::super::types::ToolId;
    use super::*;

    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn tool_id_serialises_kebab_case() {
        let json = serde_json::to_string(&ToolId::ClaudeCode).unwrap();
        assert_eq!(json, "\"claude-code\"");
        let round: ToolId = serde_json::from_str("\"claude-code\"").unwrap();
        assert_eq!(round, ToolId::ClaudeCode);
    }

    #[test]
    fn detect_one_finds_existing_paths_under_fake_home() {
        let home = tempdir().unwrap();
        let claude_dir = home.path().join(".claude");
        fs::create_dir_all(&claude_dir).unwrap();
        fs::write(claude_dir.join("settings.json"), b"{}").unwrap();

        let skill_dir = claude_dir.join("skills").join("foo");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            b"---\nname: foo\ndescription: a skill\n---\nbody\n",
        )
        .unwrap();

        let descriptor = tools::claude_code();
        let detected = detect_one(&descriptor, Some(home.path()));

        assert_eq!(detected.id, ToolId::ClaudeCode);
        assert!(detected.detected, "tool should be detected via FS");
        assert!(
            detected
                .existing_root_paths
                .iter()
                .any(|p| p.ends_with(".claude")),
            "existing_root_paths should include the .claude dir, got {:?}",
            detected.existing_root_paths,
        );
    }

    #[test]
    fn detect_one_reports_absent_tool() {
        let home = tempdir().unwrap();
        // Empty home: nothing for any tool to find. We also want any
        // would-be binary lookup to fail, so use a tool without a binary
        // name (Antigravity).
        let descriptor = tools::antigravity();
        let detected = detect_one(&descriptor, Some(home.path()));

        assert_eq!(detected.id, ToolId::Antigravity);
        assert!(!detected.detected, "no FS evidence and no binary names");
        assert!(detected.existing_root_paths.is_empty());
        assert!(detected.binary.is_none());
        assert!(detected.version.is_none());
    }

    #[test]
    fn detect_all_returns_one_entry_per_descriptor() {
        let detected = detect_all();
        assert_eq!(detected.len(), super::super::registry().len());
    }

    #[test]
    fn expand_home_replaces_tilde() {
        let home = PathBuf::from("/tmp/fakehome");
        let resolved = expand_home("~/.claude", Some(&home));
        assert_eq!(resolved, PathBuf::from("/tmp/fakehome/.claude"));
    }

    #[test]
    fn expand_home_passes_through_absolute() {
        let home = PathBuf::from("/tmp/fakehome");
        let resolved = expand_home("/etc/config", Some(&home));
        assert_eq!(resolved, PathBuf::from("/etc/config"));
    }
}
