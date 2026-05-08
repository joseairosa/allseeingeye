//! All Seeing Eye desktop library entry point.
//!
//! Phase 0.1 - bare scaffold. Real modules (registry/watcher/parser/index/ipc/mcp)
//! arrive in Phase 1.x. They are declared here so the layout matches docs/08.

#![allow(clippy::missing_errors_doc)]

mod registry;
mod watcher;
mod parser;
mod index;
mod ipc;
mod mcp;

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
        .invoke_handler(tauri::generate_handler![ping])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
