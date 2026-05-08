//! Tauri IPC commands and events.
//!
//! Phase 1.6 wires the full read-only and mutating command surface plus
//! the event channels. Phase 1.1 contributes only the `list_tools`
//! command, which surfaces the static registry + live detection.

use crate::registry::{self, DetectedTool};

/// Probe every registered tool and return what we found.
///
/// This is a synchronous read-only command. It performs filesystem
/// stat-ing and may shell out briefly to capture each detected tool's
/// version (hard-capped at 2s per tool inside `detect`). The result is
/// suitable to drive the onboarding screen and the Tools sidebar.
#[tauri::command]
#[must_use]
pub fn list_tools() -> Vec<DetectedTool> {
    registry::detect_all()
}
