//! Live-index pipeline.
//!
//! Phase 1.6 - composes the registry, watcher, parser, and index modules
//! into a working live-index data flow:
//!
//! ```text
//!   Watcher  ->  classify_path  ->  upsert_component
//!                                     |
//!                                     v
//!                              broadcast PipelineEvent
//! ```
//!
//! The pipeline owns:
//! * a `Watcher` that subscribes to the `watch_paths` of every detected
//!   tool (canonicalised against the union of those paths as trusted
//!   roots),
//! * a Tokio task that reads `WatchEvent`s from the watcher's broadcast
//!   channel, classifies each path against the registry, and performs
//!   the upsert/delete,
//! * a `tokio::sync::broadcast` of `PipelineEvent`s for any number of
//!   downstream consumers (the IPC bridge, debug panels, ...).

pub mod error;
pub mod event;
pub mod scan;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::sync::broadcast;

use crate::index::{delete_component, lookup_component_id_by_path, upsert_component, IndexHandle};
use crate::registry::detect::detect_all_with_home;
use crate::registry::{classify_path, registry as registry_slice};
use crate::watcher::{WatchEvent, Watcher};

pub use error::PipelineError;
pub use event::{PipelineEvent, ScanReport};
pub use scan::full_scan_inner;

/// Capacity of the pipeline's outbound broadcast channel. Subscribers
/// that fall behind by more than this many events will see
/// `RecvError::Lagged(n)` rather than blocking the watcher task.
///
/// 1024 mirrors the watcher's own broadcast capacity so pipeline
/// downstream consumers never bottleneck before the upstream watcher
/// does.
const BROADCAST_CAPACITY: usize = 1024;

/// Stateless context the full-scan command needs. Cloneable and
/// `Send + Sync` so it can be stored as a Tauri `State` independently
/// of the (non-`Sync`) [`Pipeline`].
#[derive(Clone)]
pub struct ScanContext {
    pub index: Arc<IndexHandle>,
    pub detected_tool_ids: Vec<crate::registry::types::ToolId>,
    pub home: Option<PathBuf>,
    pub events_tx: broadcast::Sender<PipelineEvent>,
}

impl ScanContext {
    /// Run a full scan against this context. Mirror of
    /// [`Pipeline::full_scan`] callable without the (non-`Sync`)
    /// `Pipeline` reference.
    pub fn full_scan(&self) -> Result<ScanReport, PipelineError> {
        full_scan_inner(
            &self.index,
            &self.detected_tool_ids,
            self.home.as_deref(),
            &self.events_tx,
        )
    }
}

/// Live-index pipeline owner.
///
/// Construct via [`Pipeline::start`]. Drop the pipeline to tear down the
/// watcher and the dispatch task.
pub struct Pipeline {
    index: Arc<IndexHandle>,
    /// Held to keep the watcher alive for the lifetime of the pipeline;
    /// dropping `Pipeline` drops the watcher which in turn unsubscribes
    /// from the OS notification source.
    _watcher: Watcher,
    /// Outbound event broadcaster. Subscribers receive every classified
    /// `PipelineEvent` (component upsert, delete, parse error, scan
    /// completion).
    events_tx: broadcast::Sender<PipelineEvent>,
    /// Snapshot of detected tools captured at `start()` time, used by
    /// `full_scan` so the scan walks the same tool set the watcher
    /// already subscribed to.
    detected_tool_ids: Vec<crate::registry::types::ToolId>,
    home: Option<PathBuf>,
}

impl std::fmt::Debug for Pipeline {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pipeline")
            .field("detected_tool_ids", &self.detected_tool_ids)
            .field("home", &self.home)
            .finish_non_exhaustive()
    }
}

impl Pipeline {
    /// Start the pipeline against the real system HOME.
    ///
    /// Returns a fully wired `Pipeline` whose dispatch task is already
    /// running. Tests that need to point at a fake HOME use
    /// [`Pipeline::start_with_home`].
    pub fn start(index: Arc<IndexHandle>) -> Result<Self, PipelineError> {
        Self::start_with_home(index, None)
    }

    /// Start the pipeline against a specific HOME directory. Tests pass
    /// a tempdir; production callers pass `None`.
    pub fn start_with_home(
        index: Arc<IndexHandle>,
        home: Option<PathBuf>,
    ) -> Result<Self, PipelineError> {
        // Canonicalise the home so the classifier compares apples to
        // apples - the watcher emits canonical paths (FSEvents on
        // macOS resolves `/var/folders/...` to `/private/var/folders/...`)
        // and we'd otherwise miss matches when the caller passed an
        // un-canonical home.
        let home = home.and_then(|p| std::fs::canonicalize(&p).ok().or(Some(p)));

        let detected = detect_all_with_home(home.as_deref());

        // Resolve the trusted roots from every detected tool's
        // `watch_paths`. We use `existing_root_paths` from detection
        // when available (those are the paths we know exist) and fall
        // back to the descriptor's declared `watch_paths` so a freshly
        // installed tool with no skills yet still registers a root.
        let descriptors = registry_slice();
        let mut trusted: Vec<PathBuf> = Vec::new();
        let mut detected_tool_ids = Vec::new();

        for tool in &detected {
            if !tool.detected {
                continue;
            }
            detected_tool_ids.push(tool.id);
            let descriptor = descriptors.iter().find(|d| d.id == tool.id);
            let Some(descriptor) = descriptor else {
                continue;
            };
            for raw in &descriptor.watch_paths {
                let resolved = crate::registry::detect::expand_home(raw, home.as_deref());
                if resolved.exists() {
                    trusted.push(resolved);
                }
            }
        }

        // De-duplicate while preserving order; the watcher handles
        // duplicates idempotently anyway, but cleaner trusted-roots
        // lists make tracing easier to read.
        trusted.sort();
        trusted.dedup();

        // Watcher refuses to start with no trusted roots if we then
        // try to canonicalise nothing - which is the desired behaviour
        // when the host has none of the supported tools installed.
        // We special-case that by returning a pipeline that emits no
        // events but still answers `subscribe_events()`.
        let mut watcher = Watcher::start(&trusted)?;
        for root in &trusted {
            // Ignore individual escape errors: a stray path that
            // canonicalises outside the union shouldn't take down the
            // whole pipeline. Saturation, however, must surface.
            match watcher.watch_root(root) {
                Ok(()) => {}
                Err(crate::watcher::WatcherError::WatchLimitExceeded { .. }) => {
                    return Err(PipelineError::Watcher(
                        crate::watcher::WatcherError::WatchLimitExceeded {
                            recommended_value: 524_288,
                        },
                    ));
                }
                Err(other) => {
                    tracing::warn!(?root, error = ?other, "skipping root: watch_root failed");
                }
            }
        }

        let (events_tx, _) = broadcast::channel(BROADCAST_CAPACITY);
        let dispatcher_tx = events_tx.clone();
        let dispatcher_index = Arc::clone(&index);
        let dispatcher_home = home.clone();
        let mut watch_rx = watcher.subscribe();

        // Spawn the dispatch task. It runs until the watcher's
        // broadcaster is dropped (i.e. until the `Watcher` we hold is
        // dropped on `Pipeline::drop`).
        tokio::spawn(async move {
            loop {
                match watch_rx.recv().await {
                    Ok(event) => {
                        for emitted in
                            dispatch_event(&dispatcher_index, dispatcher_home.as_deref(), &event)
                        {
                            let _ = dispatcher_tx.send(emitted);
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(skipped = n, "pipeline watcher subscriber lagged");
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        tracing::debug!("pipeline dispatcher exiting; watcher closed");
                        break;
                    }
                }
            }
        });

        Ok(Self {
            index,
            _watcher: watcher,
            events_tx,
            detected_tool_ids,
            home,
        })
    }

    /// Subscribe to the pipeline's outbound event stream.
    #[must_use]
    pub fn subscribe_events(&self) -> broadcast::Receiver<PipelineEvent> {
        self.events_tx.subscribe()
    }

    /// Walk every detected tool's `component_roots` patterns and upsert
    /// every file we find. Synchronous - intended to be called from the
    /// IPC layer's `start_full_scan` command (which wraps it in a
    /// `spawn_blocking`).
    pub fn full_scan(&self) -> Result<ScanReport, PipelineError> {
        full_scan_inner(
            &self.index,
            &self.detected_tool_ids,
            self.home.as_deref(),
            &self.events_tx,
        )
    }

    /// Read-only handle to the underlying index. Exposed so the IPC
    /// layer can route read-only commands (`list_components`, `search`)
    /// through the same handle the pipeline writes through.
    #[must_use]
    pub fn index(&self) -> Arc<IndexHandle> {
        Arc::clone(&self.index)
    }

    /// Cheap handle the IPC layer can `app.manage(...)` for the
    /// `start_full_scan` command without taking on the (non-`Sync`)
    /// `Pipeline` itself.
    #[must_use]
    pub fn scan_context(&self) -> ScanContext {
        ScanContext {
            index: Arc::clone(&self.index),
            detected_tool_ids: self.detected_tool_ids.clone(),
            home: self.home.clone(),
            events_tx: self.events_tx.clone(),
        }
    }

    /// Sender for the pipeline's event channel, exposed so the IPC
    /// bridge can subscribe without taking ownership of the pipeline.
    #[must_use]
    pub fn events_sender(&self) -> broadcast::Sender<PipelineEvent> {
        self.events_tx.clone()
    }
}

/// Translate one `WatchEvent` into zero or more `PipelineEvent`s.
fn dispatch_event(
    index: &IndexHandle,
    home: Option<&Path>,
    event: &WatchEvent,
) -> Vec<PipelineEvent> {
    match event {
        WatchEvent::Created { path } | WatchEvent::Modified { path } => {
            classify_and_upsert(index, home, path)
        }
        WatchEvent::Deleted { path } => match lookup_component_id_by_path(index, path) {
            Ok(Some(id)) => match delete_component(index, &id) {
                Ok(_) => vec![PipelineEvent::ComponentDeleted { id }],
                Err(err) => {
                    tracing::warn!(?path, ?err, "failed to delete component row");
                    Vec::new()
                }
            },
            Ok(None) => Vec::new(),
            Err(err) => {
                tracing::warn!(?path, ?err, "lookup failed during delete dispatch");
                Vec::new()
            }
        },
        // Treat a rename as delete-of-from + upsert-of-to. The watcher
        // already paired the two halves; we only fan them out into the
        // index here.
        WatchEvent::Renamed { from, to } => {
            let mut out = Vec::new();
            if let Ok(Some(id)) = lookup_component_id_by_path(index, from) {
                if delete_component(index, &id).is_ok() {
                    out.push(PipelineEvent::ComponentDeleted { id });
                }
            }
            out.extend(classify_and_upsert(index, home, to));
            out
        }
    }
}

fn classify_and_upsert(
    index: &IndexHandle,
    home: Option<&Path>,
    path: &Path,
) -> Vec<PipelineEvent> {
    let descriptors = registry_slice();
    let Some(classification) = classify_path(path, descriptors, home) else {
        return Vec::new();
    };

    let Some(descriptor) = descriptors.iter().find(|d| d.id == classification.tool) else {
        return Vec::new();
    };

    match upsert_component(
        index,
        descriptor,
        &classification.component_root,
        path,
        &classification.component_name,
    ) {
        Ok(outcome) => {
            if outcome.had_parse_error {
                vec![PipelineEvent::ParseError {
                    id: outcome.id,
                    path: path.to_string_lossy().into_owned(),
                }]
            } else {
                vec![PipelineEvent::ComponentUpserted {
                    id: outcome.id,
                    kind: outcome.kind,
                }]
            }
        }
        Err(err) => {
            tracing::warn!(?path, ?err, "upsert failed during dispatch");
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::UpsertKind;
    use std::fs;
    use std::time::Duration;
    use tempfile::tempdir;
    use tokio::time::timeout;

    /// Lay out a minimal claude-code home tree so detection passes.
    /// Returns a tempdir whose path acts as `$HOME` for the scan.
    fn fake_home() -> tempfile::TempDir {
        let home = tempdir().expect("tempdir");
        let claude_dir = home.path().join(".claude");
        fs::create_dir_all(claude_dir.join("skills").join("foo")).expect("mkdir skill");
        fs::write(
            claude_dir.join("skills").join("foo").join("SKILL.md"),
            b"---\nname: foo\ndescription: foo skill\n---\nhello\n",
        )
        .unwrap();
        // Settings + .claude.json so detection registers the tool.
        fs::write(claude_dir.join("settings.json"), b"{}").unwrap();
        fs::write(home.path().join(".claude.json"), br#"{"mcpServers": {}}"#).unwrap();
        home
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn pipeline_full_scan_finds_components() {
        let home = fake_home();
        let index = Arc::new(IndexHandle::open_in_memory().expect("open"));
        let pipeline =
            Pipeline::start_with_home(Arc::clone(&index), Some(home.path().to_path_buf()))
                .expect("start");

        let report = pipeline.full_scan().expect("scan");
        assert!(
            report.components_inserted >= 2,
            "expected at least skill + settings rows, got report = {report:?}",
        );
        // Skill + settings.json + (mcp embedded inside .claude.json) must appear.
        let count: i64 = index
            .read(
                |c| Ok(c.query_row("SELECT COUNT(*) FROM component", [], |r| r.get::<_, i64>(0))?),
            )
            .unwrap();
        assert!(count >= 2, "expected at least 2 components, got {count}");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn pipeline_watcher_picks_up_new_files() {
        let home = fake_home();
        let index = Arc::new(IndexHandle::open_in_memory().expect("open"));
        let pipeline =
            Pipeline::start_with_home(Arc::clone(&index), Some(home.path().to_path_buf()))
                .expect("start");
        let mut rx = pipeline.subscribe_events();

        // Yield so the watcher has time to register. macOS FSEvents
        // takes ~300 ms to spool up after `watch_root`; Linux inotify
        // is faster but a single sleep covers both.
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Drop a brand-new skill into the watched tree.
        let new_skill = home.path().join(".claude").join("skills").join("bar");
        fs::create_dir_all(&new_skill).expect("mkdir");
        fs::write(
            new_skill.join("SKILL.md"),
            b"---\nname: bar\ndescription: bar skill\n---\nbody\n",
        )
        .unwrap();

        // Wait up to 5s for an upsert event for `bar`.
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            assert!(
                std::time::Instant::now() < deadline,
                "did not receive ComponentUpserted for new skill",
            );
            if let Ok(Ok(PipelineEvent::ComponentUpserted { id, kind })) =
                timeout(Duration::from_millis(500), rx.recv()).await
            {
                if id.contains("/bar") {
                    assert!(matches!(kind, UpsertKind::Inserted | UpsertKind::Updated));
                    return;
                }
            }
            // Any other branch (Deleted / ParseError / ScanCompleted /
            // recv error / timeout) just keeps the loop going until the
            // deadline asserts above.
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn full_scan_reports_counts() {
        let home = fake_home();
        let index = Arc::new(IndexHandle::open_in_memory().expect("open"));
        let pipeline =
            Pipeline::start_with_home(Arc::clone(&index), Some(home.path().to_path_buf()))
                .expect("start");

        let report = pipeline.full_scan().expect("scan");
        assert!(report.tools_scanned >= 1);
        assert!(report.components_seen >= 2);
        // Second scan with no changes should be all-unchanged.
        let report2 = pipeline.full_scan().expect("rescan");
        assert!(report2.components_unchanged >= 2);
        assert_eq!(report2.components_inserted, 0);
    }
}
