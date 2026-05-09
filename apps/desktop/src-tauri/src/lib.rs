//! All Seeing Eye desktop library entry point.
//!
//! Phase 0.1 - bare scaffold. Real modules (registry/watcher/parser/index/ipc/mcp)
//! arrive in Phase 1.x. They are declared here so the layout matches docs/08.

#![allow(clippy::missing_errors_doc)]

mod fs;
mod index;
mod ipc;
mod mcp;
mod parser;
mod pipeline;
mod registry;
mod security;
mod usage;
mod validator;
mod watcher;

// Phase 1.2: re-export the index public surface at the crate root so
// future Tauri state setup (Phase 1.6) can write
// `aseye_desktop_lib::IndexHandle` without reaching into the private
// module path. We expose the handle, the error type, the pooled-read
// connection alias, the module-local `Result`, and the per-platform
// default path resolver - everything an IPC consumer needs.
pub use index::{default_db_path, IndexError, IndexHandle, ReadConnection, Result as IndexResult};

// Phase 1.5: re-export the atomic-write + safety guard surface at crate
// root so the IPC handlers in Phase 1.6 (and any other in-crate consumer)
// can call `aseye_desktop_lib::safe_atomic_write(...)` without reaching
// into the private module path. Mirrors how `IndexHandle` is exposed.
pub use fs::{
    assert_safe_target, assert_safe_target_with_override, assert_within_root, atomic_write,
    safe_atomic_write, safe_atomic_write_with_options, write_sidecar_backup, FsError,
};

// Phase 1.3: re-export the file watcher public surface at crate root.
// IPC handlers in Phase 1.6 will call `Watcher::start(...)` and
// `subscribe()` without reaching into the private module path. Mirrors
// the pattern used for `IndexHandle` and `safe_atomic_write` above.
pub use watcher::{WatchEvent, Watcher, WatcherError};

// Phase 1.4: re-export the parser dispatch surface at crate root so the
// IPC handlers (Phase 1.6) can call `aseye_desktop_lib::parse_file(...)`
// without reaching into the private module path. Mirrors how
// `IndexHandle`, `safe_atomic_write`, and `Watcher` are exposed.
pub use parser::{
    parse_bytes, parse_file, ParseError, ParseWarning, ParseWarningKind, ParsedComponent,
    MAX_PARSE_SIZE,
};

// Phase 5.2: re-export the registry classification surface at crate
// root so the criterion benches under `benches/` can hold the perf
// gate honest without reaching into the otherwise-private `registry`
// module. Pure additive re-export; no behaviour change. The frontend
// IPC layer continues to consume these via the existing `pipeline::*`
// path, not these aliases.
pub use registry::types::Format;
pub use registry::{
    classify_path as registry_classify_path, registry as registry_descriptors, DetectedTool,
    ToolDescriptor,
};

// Phase 1.6: re-export the live-index pipeline + IPC surface at crate
// root.
pub use pipeline::{Pipeline, PipelineError, PipelineEvent, ScanContext, ScanReport};

// Phase 7.1: re-export the security audit surface at crate root so
// callers (the upsert layer today, the IPC handlers in Phase 7.3) can
// reach the scanner and finding shape without crawling the private
// module path. Mirrors the pattern used for `IndexHandle`, `Watcher`,
// and `Pipeline`.
pub use security::{
    persist_findings, redact, scan_parsed, scan_text, Category as SecurityCategory, Finding,
    SecurityError, Severity,
};

// Phase 3.2: re-export the validator surface at crate root so the IPC
// command (`validate_component`) and any future in-crate consumer can
// reach `validate`, `validate_by_id`, and the outcome types without
// crawling the private module path. Mirrors the security re-export
// above.
pub use validator::{
    schema_for_tuple, validate as validate_component_raw, validate_by_id, ParseErrorKind,
    ValidationError, ValidationOutcome, ValidationWarning, ValidationWarningKind, ValidatorError,
};

// Phase 6.2: re-export the auto-update IPC + settings surface at crate
// root so the daily-check task and the Tauri command handlers can
// reach `UpdateSettings`, `UpdateChannel`, etc. without crawling the
// private module path. The frontend UI consumes the matching ts-rs
// bindings under `bindings/updates/`.
pub use ipc::updates::{
    default_settings_path, spawn_daily_check, UpdateAvailable, UpdateChannel, UpdateError,
    UpdateSettings,
};

// Phase 14C: re-export the token-usage refresh entry point. The IPC
// commands consume this through the crate-private `crate::usage::*`
// path; re-exporting here lets integration tests under `tests/`
// drive the aggregator end-to-end without standing up a Tauri app.
pub use usage::{refresh_from_home as usage_refresh_from_home, RefreshOutcome};

use std::sync::Arc;

use tauri::Manager;

#[tauri::command]
fn ping() -> &'static str {
    "pong"
}

/// # Panics
/// Panics only if Tauri runtime fails to initialise, which is unrecoverable.
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,aseye=debug")),
        )
        .init();

    tracing::info!("All Seeing Eye starting");

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            #[cfg(debug_assertions)]
            if let Some(window) = app.get_webview_window("main") {
                window.open_devtools();
            }

            // Open the index DB and start the live-index pipeline. Both
            // are non-blocking constructors; the pipeline spawns its
            // own background dispatch task on `start_with_home`.
            let index =
                IndexHandle::open(default_db_path()).expect("failed to open index database");
            let index = Arc::new(index);
            let pipeline = Pipeline::start_with_home(Arc::clone(&index), None)
                .expect("failed to start pipeline");
            let scan_ctx = pipeline.scan_context();
            let events_rx = pipeline.subscribe_events();

            // Bridge pipeline events to Tauri events. The bridge runs
            // until the pipeline's broadcaster is dropped.
            ipc::spawn_event_bridge(app.handle().clone(), events_rx);

            // Tauri owns these; cloning the Arc keeps the pipeline
            // alive even though the `Pipeline` itself is not exposed
            // through state (it is not `Sync` because of the watcher).
            // We leak the pipeline into a long-lived heap allocation so
            // the dispatch task and the watcher continue running. The
            // intentional leak is fine because there is exactly one
            // pipeline per process and it is meant to live for the
            // lifetime of the app.
            //
            // We could `Box::leak(Box::new(pipeline))` but using the
            // managed-state pattern lets us hand the pipeline back via
            // `app.state::<...>()` later if needed.
            let _ = Box::leak(Box::new(pipeline));

            // Phase 6.2 - load the user's update preferences (channel +
            // auto-check). Falls back to in-memory defaults if the
            // config dir cannot be resolved, so the app keeps running
            // even on stripped-down hosts where `dirs::config_dir`
            // returns None.
            let settings = if let Some(p) = default_settings_path() {
                UpdateSettings::load_or_default(p)
            } else {
                tracing::warn!(
                    "no user config dir; update preferences will not persist this session"
                );
                UpdateSettings::load_or_default(std::path::PathBuf::from("update-channel.json"))
            };
            app.manage(settings);

            // Spawn the daily background check. Sleeps 30s after launch
            // before the first poll so the filesystem scan finishes
            // first. Errors are logged via `tracing::warn!`; never
            // shown to the user as toasts.
            spawn_daily_check(app.handle().clone());

            app.manage(index);
            app.manage(scan_ctx);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            ping,
            ipc::commands::list_tools,
            ipc::commands::list_components,
            ipc::commands::get_component,
            ipc::commands::read_component_raw,
            ipc::commands::search,
            ipc::commands::start_full_scan,
            ipc::commands::get_health_summary,
            // Phase 7.3 - Security view IPC.
            ipc::commands::list_security_findings,
            ipc::commands::suppress_finding,
            ipc::commands::unsuppress_finding,
            ipc::commands::get_findings_count_per_component,
            ipc::commands::get_security_summary,
            // Phase 3.2 - per-tool schema validation by component id.
            ipc::commands::validate_component,
            // Phase 3.3 - Editor save flow + bundled schema lookup.
            ipc::commands::save_component,
            ipc::commands::get_component_with_raw,
            ipc::commands::get_validation_schema,
            ipc::updates::check_for_update,
            ipc::updates::install_update_and_relaunch,
            ipc::updates::get_update_channel,
            ipc::updates::set_update_channel,
            ipc::updates::get_auto_check_setting,
            ipc::updates::set_auto_check_setting,
            // Phase 14C - token usage analytics.
            ipc::commands::usage_query,
            ipc::commands::usage_refresh,
            // Phase 14B - app settings (project memory roots).
            ipc::commands::get_project_memory_roots,
            ipc::commands::set_project_memory_roots,
            // Audit follow-ups - Settings + Onboarding wiring.
            ipc::commands::check_path_readable,
            ipc::commands::rebuild_index,
            ipc::commands::reset_index,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
