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
mod registry;
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
        .setup(|app| {
            #[cfg(debug_assertions)]
            if let Some(window) = app.get_webview_window("main") {
                window.open_devtools();
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![ping, ipc::list_tools])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
