// Prevents additional console window on Windows in release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// `#[tokio::main]` installs a Tokio runtime on the main thread before
// `run()` is called. The Tauri `setup` callback fires synchronously on
// the same thread, so the live-index pipeline's `tokio::spawn` inside
// `Coalescer::start` can reach the runtime via thread-local context.
// Without this, the watcher panics with "no reactor running".
#[tokio::main]
async fn main() {
    aseye_desktop_lib::run();
}
