//! Auto-update IPC commands and persisted user preferences.
//!
//! Phase 6.2 wires the BACKEND of the auto-update pipeline. The frontend
//! UI (toast banner, Settings panel) is not in scope here; this module
//! exposes the IPC contract it consumes.
//!
//! Public surface:
//!
//! ```text
//! check_for_update()              -> Option<UpdateAvailable>
//! install_update_and_relaunch()   -> ()
//! get_update_channel()            -> UpdateChannel
//! set_update_channel(channel)     -> ()
//! get_auto_check_setting()        -> bool
//! set_auto_check_setting(enabled) -> ()
//! ```
//!
//! Plus a daily background poll spawned from `lib.rs` that emits a
//! Tauri `update-available` event whenever the configured endpoint
//! announces a newer version.
//!
//! ## Persistence
//!
//! User preferences (channel + auto-check toggle) are stored in a
//! single JSON file at:
//!
//! - macOS:   `~/Library/Application Support/AllSeeingEye/update-channel.json`
//! - Linux:   `${XDG_CONFIG_HOME:-~/.config}/AllSeeingEye/update-channel.json`
//! - Windows: `%APPDATA%/AllSeeingEye/update-channel.json`
//!
//! The file is written atomically via `safe_atomic_write` so a crash
//! mid-write can never corrupt the user's preferences.
//!
//! ## Channel-aware endpoint resolution
//!
//! `tauri.conf.json -> plugins.updater.endpoints` carries both URLs
//! (stable + beta). At `check_for_update` time we rebuild the
//! `UpdaterBuilder` with `endpoints(vec![<channel-specific>])` so a
//! single invocation only ever talks to one channel. This matches the
//! v2.x plugin API (`UpdaterBuilder::endpoints(Vec<Url>)`) and avoids
//! the ambiguity of letting the plugin "pick the first" from a list.

#![allow(clippy::needless_pass_by_value)]

#[cfg(test)]
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, Runtime, State};
use tauri_plugin_updater::UpdaterExt;
use ts_rs::TS;
use url::Url;

// ─── Wire types ─────────────────────────────────────────────────────────

/// Update channel preference. The release pipeline publishes
/// `latest-stable.json` and `latest-beta.json` to GitHub Releases; this
/// enum picks which one to consult.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
#[ts(export, export_to = "../bindings/updates/UpdateChannel.ts")]
#[ts(rename_all = "lowercase")]
pub enum UpdateChannel {
    /// Tagged release line. Default for everyone who isn't explicitly opted in.
    #[default]
    Stable,
    /// Pre-release line for early-adopters. Same signing key as stable.
    Beta,
}

/// Payload returned from `check_for_update` and broadcast as the
/// `update-available` Tauri event when the daily poll finds a newer
/// version.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../bindings/updates/UpdateAvailable.ts")]
#[ts(rename_all = "camelCase")]
pub struct UpdateAvailable {
    /// Newer version announced by the manifest.
    pub version: String,
    /// Currently-installed version (from `tauri.conf.json -> version`).
    pub current_version: String,
    /// Optional release notes body.
    pub notes: Option<String>,
    /// Optional RFC3339-ish publish timestamp. Plugin parses
    /// `time::OffsetDateTime`; we render it via `Display` to avoid a
    /// direct dep on `time`.
    pub pub_date: Option<String>,
    /// Channel that produced this update (matches the preference at
    /// the time of the check).
    pub channel: UpdateChannel,
}

/// Errors surfaced to the frontend. Variants are deliberately narrow
/// so the UI can decide between "show an inline message" (e.g.
/// `Network`) and "log + bail silently" (e.g. `MissingEndpoint`).
#[derive(Debug, Serialize, Deserialize, TS, thiserror::Error)]
#[serde(tag = "kind", content = "message", rename_all = "camelCase")]
#[ts(export, export_to = "../bindings/updates/UpdateError.ts")]
#[ts(rename_all = "camelCase")]
pub enum UpdateError {
    /// The plugin's `check()` or `download_and_install()` call failed.
    /// Usually means the endpoint is unreachable, the manifest failed
    /// to parse, or the bundle's signature didn't verify.
    #[error("updater backend error: {0}")]
    Plugin(String),
    /// `tauri.conf.json` had no `plugins.updater.endpoints`, or both
    /// entries failed to parse as URLs. We refuse to silently skip the
    /// check so the user sees the misconfig.
    #[error("no usable updater endpoint configured for channel")]
    MissingEndpoint,
    /// `dirs::config_dir` returned `None` (extremely rare, generally
    /// only on stripped-down container hosts).
    #[error("could not resolve user config directory")]
    NoConfigDir,
    /// Reading or writing `update-channel.json` failed.
    #[error("settings io error: {0}")]
    SettingsIo(String),
}

impl UpdateError {
    /// Tauri's `#[command]` macro requires command-result errors to
    /// implement `Serialize`. Once a command returns `Err(UpdateError)`,
    /// it ships across the IPC boundary using this enum's serde repr.
    /// We surface a string variant for any generic plugin error to
    /// avoid leaking the plugin's private error tree.
    fn from_plugin(e: tauri_plugin_updater::Error) -> Self {
        Self::Plugin(e.to_string())
    }
}

// ─── Settings persistence ───────────────────────────────────────────────

/// On-disk shape of `update-channel.json`. Two booleans, one enum.
/// Stored under `dirs::config_dir()/AllSeeingEye/`.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct UpdateSettingsFile {
    #[serde(default)]
    channel: UpdateChannel,
    #[serde(default = "default_auto_check")]
    auto_check: bool,
}

fn default_auto_check() -> bool {
    true
}

impl Default for UpdateSettingsFile {
    fn default() -> Self {
        Self {
            channel: UpdateChannel::default(),
            auto_check: default_auto_check(),
        }
    }
}

/// Tauri-managed handle wrapping the parsed settings + the on-disk
/// path. Cloneable; shared across the daily-check task and the IPC
/// commands.
#[derive(Debug, Clone)]
pub struct UpdateSettings {
    inner: Arc<Mutex<UpdateSettingsFile>>,
    path: Arc<PathBuf>,
}

impl UpdateSettings {
    /// Load the settings from `path`, creating a default in-memory
    /// representation if the file is missing or malformed. Malformed
    /// files are NOT auto-corrected on disk - we only rewrite when
    /// the user explicitly calls a setter.
    #[must_use]
    pub fn load_or_default(path: PathBuf) -> Self {
        let inner = match std::fs::read(&path) {
            Ok(bytes) => {
                serde_json::from_slice::<UpdateSettingsFile>(&bytes).unwrap_or_else(|err| {
                    tracing::warn!(
                        ?err,
                        ?path,
                        "update-settings: malformed file, using defaults"
                    );
                    UpdateSettingsFile::default()
                })
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => UpdateSettingsFile::default(),
            Err(err) => {
                tracing::warn!(?err, ?path, "update-settings: read failed, using defaults");
                UpdateSettingsFile::default()
            }
        };
        Self {
            inner: Arc::new(Mutex::new(inner)),
            path: Arc::new(path),
        }
    }

    /// Snapshot the current channel.
    #[must_use]
    pub fn channel(&self) -> UpdateChannel {
        self.inner.lock().channel
    }

    /// Snapshot the auto-check toggle.
    #[must_use]
    pub fn auto_check(&self) -> bool {
        self.inner.lock().auto_check
    }

    /// Persist a new channel value. Writes are atomic.
    pub fn set_channel(&self, channel: UpdateChannel) -> Result<(), UpdateError> {
        let snapshot = {
            let mut guard = self.inner.lock();
            guard.channel = channel;
            guard.clone()
        };
        self.persist(&snapshot)
    }

    /// Persist a new auto-check value. Writes are atomic.
    pub fn set_auto_check(&self, enabled: bool) -> Result<(), UpdateError> {
        let snapshot = {
            let mut guard = self.inner.lock();
            guard.auto_check = enabled;
            guard.clone()
        };
        self.persist(&snapshot)
    }

    fn persist(&self, snapshot: &UpdateSettingsFile) -> Result<(), UpdateError> {
        let bytes = serde_json::to_vec_pretty(snapshot)
            .map_err(|e| UpdateError::SettingsIo(e.to_string()))?;
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| UpdateError::SettingsIo(e.to_string()))?;
        }
        // The settings file lives under the user's config dir, NOT under
        // any registered tool root, so we use `atomic_write` directly
        // instead of `safe_atomic_write`. The atomic semantics still
        // hold (temp+fsync+rename+parent fsync) so a crash mid-write
        // can never corrupt the user's preferences.
        crate::fs::atomic_write(self.path.as_ref(), &bytes)
            .map_err(|e| UpdateError::SettingsIo(e.to_string()))?;
        Ok(())
    }

    #[cfg(test)]
    fn path(&self) -> &Path {
        self.path.as_ref()
    }
}

/// Default per-platform path for the user-update preferences file.
///
/// We use `dirs::config_dir` rather than `dirs::data_dir` (which is
/// where the index lives) because the channel preference is plain
/// configuration, not derived state.
#[must_use]
pub fn default_settings_path() -> Option<PathBuf> {
    dirs::config_dir().map(|p| p.join("AllSeeingEye").join("update-channel.json"))
}

// ─── Endpoint resolution ────────────────────────────────────────────────

/// Resolve the update endpoint to use for `channel`, given the
/// configured list. Beta endpoints carry "beta" in the basename
/// (matching `latest-beta.json` from the release workflow); stable
/// endpoints carry "stable" or are simply not beta.
fn endpoint_for_channel(endpoints: &[Url], channel: UpdateChannel) -> Option<Url> {
    let prefer_beta = matches!(channel, UpdateChannel::Beta);
    // Pass 1: exact basename match (`latest-beta.json` / `latest-stable.json`).
    let exact = endpoints.iter().find(|u| {
        let name = u.path_segments().and_then(Iterator::last).unwrap_or("");
        if prefer_beta {
            name.contains("beta")
        } else {
            name.contains("stable")
        }
    });
    if let Some(u) = exact {
        return Some(u.clone());
    }
    // Pass 2: contains "beta" anywhere in the URL (fallback for less
    // strict naming conventions).
    let loose = endpoints.iter().find(|u| {
        let s = u.as_str();
        if prefer_beta {
            s.contains("beta")
        } else {
            !s.contains("beta")
        }
    });
    loose.cloned()
}

/// Read the configured endpoints list from `tauri.conf.json -> plugins.updater`
/// and narrow it to the one the active channel asks for.
///
/// The Tauri Updater plugin keeps its own `Updater::endpoints` field
/// private, so we go straight to `app.config().plugins` (the parsed
/// `tauri.conf.json -> plugins` `HashMap`) to read them. This is also
/// stable across plugin major versions.
/// Sentinel that ships in `tauri.conf.json` until the maintainer runs
/// `pnpm tauri signer generate` and pastes the real public key. We use
/// this to short-circuit the daily check so dev builds don't spam
/// `WARN update-check failed` while no releases exist yet.
const PUBKEY_PLACEHOLDER: &str = "REPLACE_WITH_TAURI_SIGNING_PUBLIC_KEY";

/// `true` when the `tauri.conf.json` updater pubkey is still the
/// placeholder. Callers use this to skip the check entirely - hitting
/// the endpoint without a verifying key would fail at signature time
/// anyway, and the noisy log isn't useful pre-release.
fn updater_is_unconfigured<R: Runtime>(app: &AppHandle<R>) -> bool {
    let config = app.config();
    let Some(updater_cfg) = config.plugins.0.get("updater") else {
        return true;
    };
    updater_cfg
        .get("pubkey")
        .and_then(serde_json::Value::as_str)
        .is_none_or(|s| s.trim().is_empty() || s == PUBKEY_PLACEHOLDER)
}

fn pick_endpoint<R: Runtime>(
    app: &AppHandle<R>,
    channel: UpdateChannel,
) -> Result<Url, UpdateError> {
    let config = app.config();
    let updater_cfg = config
        .plugins
        .0
        .get("updater")
        .ok_or(UpdateError::MissingEndpoint)?;
    let endpoints_raw = updater_cfg
        .get("endpoints")
        .and_then(serde_json::Value::as_array)
        .ok_or(UpdateError::MissingEndpoint)?;
    let mut endpoints: Vec<Url> = Vec::with_capacity(endpoints_raw.len());
    for v in endpoints_raw {
        if let Some(s) = v.as_str() {
            if let Ok(u) = Url::parse(s) {
                endpoints.push(u);
            }
        }
    }
    endpoint_for_channel(&endpoints, channel).ok_or(UpdateError::MissingEndpoint)
}

// ─── Tauri commands ─────────────────────────────────────────────────────

#[tauri::command]
pub async fn check_for_update<R: Runtime>(
    app: AppHandle<R>,
    settings: State<'_, UpdateSettings>,
) -> Result<Option<UpdateAvailable>, UpdateError> {
    if updater_is_unconfigured(&app) {
        // Pre-release builds: surface a clear typed error instead of
        // a noisy plugin failure deep in the manifest fetch.
        return Err(UpdateError::MissingEndpoint);
    }
    let channel = settings.channel();
    check_for_update_inner(&app, channel).await
}

#[tauri::command]
pub async fn install_update_and_relaunch<R: Runtime>(
    app: AppHandle<R>,
    settings: State<'_, UpdateSettings>,
) -> Result<(), UpdateError> {
    let channel = settings.channel();
    let endpoint = pick_endpoint(&app, channel)?;
    let updater = app
        .updater_builder()
        .endpoints(vec![endpoint])
        .map_err(UpdateError::from_plugin)?
        .build()
        .map_err(UpdateError::from_plugin)?;

    let Some(update) = updater.check().await.map_err(UpdateError::from_plugin)? else {
        // No update; nothing to do. Frontend should have already
        // observed this via `check_for_update`.
        return Ok(());
    };

    // The plugin requires both a per-chunk and a finished callback;
    // we ignore the bytes-streamed signal entirely (the frontend doesn't
    // render a progress bar in this phase).
    update
        .download_and_install(|_received, _total| {}, || {})
        .await
        .map_err(UpdateError::from_plugin)?;

    // After install, restart the app. The plugin's macOS / Linux
    // handlers don't auto-restart; Windows MSI does. Calling restart
    // unconditionally is the safest cross-platform behaviour.
    app.restart();
}

#[tauri::command]
pub fn get_update_channel(settings: State<'_, UpdateSettings>) -> UpdateChannel {
    settings.channel()
}

#[tauri::command]
pub fn set_update_channel(
    settings: State<'_, UpdateSettings>,
    channel: UpdateChannel,
) -> Result<(), UpdateError> {
    settings.set_channel(channel)
}

#[tauri::command]
pub fn get_auto_check_setting(settings: State<'_, UpdateSettings>) -> bool {
    settings.auto_check()
}

#[tauri::command]
pub fn set_auto_check_setting(
    settings: State<'_, UpdateSettings>,
    enabled: bool,
) -> Result<(), UpdateError> {
    settings.set_auto_check(enabled)
}

// ─── Inner functions used by both the IPC command and the daily task ────

/// Resolve the channel-specific endpoint and ask the plugin to check.
/// Returns `Ok(None)` if the manifest reports no newer version.
pub async fn check_for_update_inner<R: Runtime>(
    app: &AppHandle<R>,
    channel: UpdateChannel,
) -> Result<Option<UpdateAvailable>, UpdateError> {
    let endpoint = pick_endpoint(app, channel)?;
    let updater = app
        .updater_builder()
        .endpoints(vec![endpoint])
        .map_err(UpdateError::from_plugin)?
        .build()
        .map_err(UpdateError::from_plugin)?;

    let maybe_update = updater.check().await.map_err(UpdateError::from_plugin)?;
    Ok(maybe_update.map(|u| UpdateAvailable {
        version: u.version.clone(),
        current_version: u.current_version.clone(),
        notes: u.body.clone(),
        pub_date: u.date.map(|d| d.to_string()),
        channel,
    }))
}

/// Spawn the daily background check.
///
/// Behaviour:
/// * Sleeps `STARTUP_GRACE` (30s) after launch so we don't compete with
///   the initial filesystem scan for resources.
/// * Polls the configured channel's endpoint every `CHECK_INTERVAL`
///   (24h) while `auto_check` is enabled.
/// * Emits a `update-available` Tauri event with `UpdateAvailable`
///   payload when the manifest reports a newer version.
/// * Errors are logged via `tracing::warn!` and never bubble to the
///   UI; the user opted in to checking, not to noisy failures.
pub fn spawn_daily_check<R: Runtime>(app: AppHandle<R>) {
    tauri::async_runtime::spawn(async move {
        // Resource-considerate: let the filesystem scan finish before
        // we start hitting the network.
        tokio::time::sleep(STARTUP_GRACE).await;

        // If the maintainer hasn't generated a Tauri signing keypair
        // yet (placeholder pubkey in tauri.conf.json), skip the check
        // entirely. Without a verifying key any update would fail at
        // signature time, and the WARN log is just noise pre-release.
        if updater_is_unconfigured(&app) {
            tracing::info!(
                "update-check: skipped (no pubkey configured; run `pnpm tauri signer generate`)"
            );
            return;
        }

        loop {
            // Snapshot the toggle each tick so a user disable takes
            // effect on the next 24h boundary.
            let Some(state) = app.try_state::<UpdateSettings>() else {
                tracing::warn!("update-check: settings state missing, skipping tick");
                tokio::time::sleep(CHECK_INTERVAL).await;
                continue;
            };
            let settings = state.inner().clone();
            if settings.auto_check() {
                let channel = settings.channel();
                match check_for_update_inner(&app, channel).await {
                    Ok(Some(update)) => {
                        if let Err(err) = app.emit("update-available", &update) {
                            tracing::warn!(?err, "update-check: failed to emit event");
                        } else {
                            tracing::info!(
                                version = %update.version,
                                channel = ?channel,
                                "update available"
                            );
                        }
                    }
                    Ok(None) => {
                        tracing::debug!(channel = ?channel, "no update available");
                    }
                    Err(err) => {
                        tracing::warn!(?err, "update-check failed");
                    }
                }
            }
            tokio::time::sleep(CHECK_INTERVAL).await;
        }
    });
}

/// Wait this long after launch before the first update poll. Lets the
/// initial filesystem scan settle first. Tunable via env if we ever
/// need to disable in tests, but the default 30s matches the spec.
const STARTUP_GRACE: std::time::Duration = std::time::Duration::from_secs(30);

/// Interval between successive polls. 24h matches the auto-update
/// section of `docs/08-tech-architecture.md` ("Checks daily").
const CHECK_INTERVAL: std::time::Duration = std::time::Duration::from_hours(24);

// ─── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn auto_check_default_true() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("update-channel.json");
        let settings = UpdateSettings::load_or_default(path);
        assert!(settings.auto_check());
    }

    #[test]
    fn update_channel_default_stable() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("update-channel.json");
        let settings = UpdateSettings::load_or_default(path);
        assert_eq!(settings.channel(), UpdateChannel::Stable);
    }

    #[test]
    fn update_channel_round_trip_via_disk() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("update-channel.json");
        let first = UpdateSettings::load_or_default(path.clone());
        first
            .set_channel(UpdateChannel::Beta)
            .expect("persist beta");

        // Re-instantiate from the same path; the change must survive.
        let second = UpdateSettings::load_or_default(path);
        assert_eq!(second.channel(), UpdateChannel::Beta);
    }

    #[test]
    fn setting_persists_across_processes() {
        // Two consecutive opens simulate two app launches: the second
        // launch must see what the first launch wrote.
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("update-channel.json");

        let first = UpdateSettings::load_or_default(path.clone());
        first.set_channel(UpdateChannel::Beta).expect("persist");
        first.set_auto_check(false).expect("persist");
        drop(first);

        let second = UpdateSettings::load_or_default(path);
        assert_eq!(second.channel(), UpdateChannel::Beta);
        assert!(!second.auto_check());
    }

    #[test]
    fn malformed_file_falls_back_to_defaults() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("update-channel.json");
        std::fs::write(&path, b"this is not json").expect("seed bad bytes");
        let settings = UpdateSettings::load_or_default(path);
        assert_eq!(settings.channel(), UpdateChannel::Stable);
        assert!(settings.auto_check());
    }

    #[test]
    fn update_error_serializes() {
        // Serde representation is `{"kind": "...", "message": "..."}` for
        // tagged variants; verifying each variant round-trips proves the
        // wire shape is stable across IPC.
        let cases = [
            UpdateError::Plugin("boom".into()),
            UpdateError::SettingsIo("io".into()),
            UpdateError::MissingEndpoint,
            UpdateError::NoConfigDir,
        ];
        for case in cases {
            let json = serde_json::to_string(&case).expect("serialize");
            let back: UpdateError = serde_json::from_str(&json).expect("deserialize");
            // We can't derive PartialEq on `thiserror::Error` cleanly
            // without owning every variant's payload type; check the
            // discriminant + message via Debug instead.
            assert_eq!(format!("{case:?}"), format!("{back:?}"));
        }
    }

    #[test]
    fn endpoint_for_channel_picks_stable_when_stable() {
        let endpoints = vec![
            Url::parse("https://example.com/latest-stable.json").unwrap(),
            Url::parse("https://example.com/latest-beta.json").unwrap(),
        ];
        let picked = endpoint_for_channel(&endpoints, UpdateChannel::Stable).unwrap();
        assert!(picked.as_str().contains("stable"));
    }

    #[test]
    fn endpoint_for_channel_picks_beta_when_beta() {
        let endpoints = vec![
            Url::parse("https://example.com/latest-stable.json").unwrap(),
            Url::parse("https://example.com/latest-beta.json").unwrap(),
        ];
        let picked = endpoint_for_channel(&endpoints, UpdateChannel::Beta).unwrap();
        assert!(picked.as_str().contains("beta"));
    }

    #[test]
    fn endpoint_for_channel_returns_none_when_empty() {
        assert!(endpoint_for_channel(&[], UpdateChannel::Stable).is_none());
    }

    #[test]
    fn settings_path_round_trips() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("nested").join("update-channel.json");
        let settings = UpdateSettings::load_or_default(path.clone());
        // `path()` returns the canonical location even when the file
        // doesn't yet exist on disk.
        assert_eq!(settings.path(), path.as_path());
    }

    /// Network-dependent smoke test for the actual updater HTTP call.
    /// Run manually with `cargo test -- --ignored update_check_smoke`.
    /// Skipped by default because it requires a public release server.
    #[test]
    #[ignore = "requires network and a live release endpoint"]
    fn update_check_smoke() {
        // Manual: spin up a tauri::test::mock_app and call
        // `check_for_update_inner`. Left as a placeholder so future
        // contributors know this is the seam.
    }
}
