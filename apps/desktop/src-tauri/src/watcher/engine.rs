//! `notify` v6 wrapper + lifecycle for the file watcher.
//!
//! Owns the underlying `notify::RecommendedWatcher` and the `Coalescer`. The
//! engine is the only place that touches `notify` types; the rest of the
//! crate consumes the post-coalescer `WatchEvent` stream.
//!
//! Symlink-escape protection: every `watch_root` call canonicalises the
//! requested path and refuses it unless the canonical path lies within at
//! least one of the `trusted_roots` declared at `start()` time. This
//! mirrors the SR-3 mitigation in `docs/11-risks.md` (path traversal via
//! tool config) and the same containment rule used by
//! `fs::safety::assert_within_root`.
//!
//! Linux inotify saturation (TR-3): when `notify::Watcher::watch` returns
//! `ErrorKind::MaxFilesWatch`, we surface it as a typed
//! `WatcherError::WatchLimitExceeded { recommended_value: 524288 }`. The
//! IPC layer (Phase 1.6) turns that into a UI banner suggesting:
//!
//! ```text
//! sudo sysctl -w fs.inotify.max_user_watches=524288
//! ```

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use notify::event::{ModifyKind, RenameMode};
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher as NotifyWatcher};
use tokio::sync::broadcast;

use super::coalescer::{Coalescer, CoalescerInput, RawEvent};
use super::error::WatcherError;
use super::event::WatchEvent;
use crate::fs::safety::assert_within_root;
use crate::fs::FsError;

/// Public file watcher.
///
/// Lifecycle:
/// 1. `Watcher::start(trusted_roots)` spawns the coalescer task and
///    constructs the underlying `notify` watcher.
/// 2. Call `watch_root(p)` for each tool root you want live updates for.
/// 3. Call `subscribe()` to obtain a `broadcast::Receiver<WatchEvent>`.
/// 4. Drop the `Watcher` to tear everything down.
///
/// Each `Watcher` instance owns one OS-level watcher; for parallel watch
/// trees create separate `Watcher`s. In practice MVP runs exactly one.
pub struct Watcher {
    inner: RecommendedWatcher,
    coalescer: Coalescer,
    trusted_roots: Vec<PathBuf>,
    watched: HashSet<PathBuf>,
}

impl std::fmt::Debug for Watcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `RecommendedWatcher` is not `Debug` on all platforms; surface the
        // bookkeeping fields instead.
        f.debug_struct("Watcher")
            .field("trusted_roots", &self.trusted_roots)
            .field("watched", &self.watched)
            .finish_non_exhaustive()
    }
}

impl Watcher {
    /// Construct and start the watcher.
    ///
    /// `trusted_roots` is the set of roots a watch path must canonicalise
    /// inside. In production this comes from `ToolDescriptor::watch_paths`
    /// expanded against the user's HOME; tests pass a single tmpdir. The
    /// argument is taken by slice and copied so the caller does not need to
    /// hold the buffer.
    ///
    /// Roots are canonicalised eagerly so containment checks are O(roots)
    /// path-prefix comparisons rather than re-canonicalising per check.
    ///
    /// # Errors
    /// * `WatcherError::Init` - underlying `notify::Watcher::new` failed.
    /// * `WatcherError::Canonicalize` - any of the trusted roots failed to
    ///   canonicalise (does not exist, permission denied, ...).
    pub fn start(trusted_roots: &[PathBuf]) -> Result<Self, WatcherError> {
        let canonical_roots: Vec<PathBuf> = trusted_roots
            .iter()
            .map(|p| {
                std::fs::canonicalize(p).map_err(|e| WatcherError::Canonicalize {
                    path: p.clone(),
                    source: e,
                })
            })
            .collect::<Result<_, _>>()?;

        let coalescer = Coalescer::start();
        let input = coalescer.input();

        let inner = RecommendedWatcher::new(
            move |result: notify::Result<notify::Event>| {
                handle_notify_event(&input, result);
            },
            notify::Config::default(),
        )
        .map_err(|source| WatcherError::Init { source })?;

        Ok(Self {
            inner,
            coalescer,
            trusted_roots: canonical_roots,
            watched: HashSet::new(),
        })
    }

    /// Subscribe to coalesced events. Each subscriber receives a fresh
    /// receiver; broadcast lag policy is "drop oldest" (a slow subscriber
    /// will see `Lagged(n)` errors but the watcher does not block).
    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<WatchEvent> {
        self.coalescer.subscribe()
    }

    /// The canonical paths currently being watched. Useful for diagnostics
    /// and for tests asserting idempotency of `watch_root`.
    #[must_use]
    pub fn watched_roots(&self) -> Vec<PathBuf> {
        let mut out: Vec<PathBuf> = self.watched.iter().cloned().collect();
        out.sort();
        out
    }

    /// Begin watching `path` recursively. Idempotent: a second call with
    /// the same canonical path is a no-op.
    ///
    /// # Errors
    /// * `WatcherError::Canonicalize` - `path` cannot be canonicalised.
    /// * `WatcherError::PathEscape` - canonical(path) is not inside any
    ///   trusted root.
    /// * `WatcherError::WatchLimitExceeded` - inotify (or platform
    ///   equivalent) reports `MaxFilesWatch`.
    /// * `WatcherError::Watch` - any other underlying `notify` failure.
    pub fn watch_root(&mut self, path: &Path) -> Result<(), WatcherError> {
        let canonical = self.canonicalise_and_check(path)?;

        if self.watched.contains(&canonical) {
            // Idempotent: already watching. Don't ask `notify` to watch
            // again (it would fail on some platforms).
            return Ok(());
        }

        self.inner
            .watch(&canonical, RecursiveMode::Recursive)
            .map_err(|e| WatcherError::from_watch_error(canonical.clone(), e))?;

        self.watched.insert(canonical);
        Ok(())
    }

    /// Stop watching `path`. Idempotent: unwatching a path that is not
    /// currently watched is a no-op.
    ///
    /// # Errors
    /// * `WatcherError::Canonicalize` - `path` cannot be canonicalised.
    /// * `WatcherError::Unwatch` - underlying `notify::Watcher::unwatch`
    ///   failed for a reason other than "not watched".
    pub fn unwatch_root(&mut self, path: &Path) -> Result<(), WatcherError> {
        let canonical =
            std::fs::canonicalize(path).map_err(|source| WatcherError::Canonicalize {
                path: path.to_path_buf(),
                source,
            })?;

        if !self.watched.contains(&canonical) {
            return Ok(());
        }

        self.inner
            .unwatch(&canonical)
            .map_err(|source| WatcherError::Unwatch {
                path: canonical.clone(),
                source,
            })?;

        self.watched.remove(&canonical);
        Ok(())
    }

    fn canonicalise_and_check(&self, path: &Path) -> Result<PathBuf, WatcherError> {
        let canonical =
            std::fs::canonicalize(path).map_err(|source| WatcherError::Canonicalize {
                path: path.to_path_buf(),
                source,
            })?;

        // The trusted-roots set was canonicalised at `start()` so we can
        // compare prefixes directly. Use `assert_within_root` for the
        // single-root case so the check rule stays bit-identical to the
        // safe-write path; for the multi-root case we accept any match.
        for root in &self.trusted_roots {
            match assert_within_root(&canonical, root) {
                Ok(_) => return Ok(canonical),
                // Try the next root; only a non-escape error is fatal.
                Err(FsError::EscapeDetected { .. }) => {}
                Err(other) => return Err(WatcherError::Fs { source: other }),
            }
        }

        Err(WatcherError::PathEscape { path: canonical })
    }
}

/// `notify` callbacks run on a background thread inside `notify`. We do
/// the smallest amount of work possible here: classify into our `RawEvent`
/// shape and forward to the coalescer's mpsc. All buffering, debouncing,
/// and rename pairing happens inside the coalescer task.
fn handle_notify_event(input: &CoalescerInput, result: notify::Result<notify::Event>) {
    let event = match result {
        Ok(e) => e,
        Err(err) => {
            // We can't surface errors back to the engine from the callback
            // synchronously. Log them; if it's a saturation error the
            // operator will already see the typed error from the `watch`
            // call that originally hit the limit.
            tracing::warn!(error = ?err, "notify error in callback");
            return;
        }
    };

    for path in event.paths.iter().cloned() {
        match event.kind {
            EventKind::Create(_) => input.send(RawEvent::Created(path)),
            EventKind::Modify(ModifyKind::Name(RenameMode::From)) => {
                input.send(RawEvent::RenameFrom(path));
            }
            EventKind::Modify(ModifyKind::Name(RenameMode::To)) => {
                input.send(RawEvent::RenameTo(path));
            }
            // `RenameMode::Both` is emitted on some platforms when the
            // source and dest are reported in a single event with two
            // entries in `paths`. We can't pair them inside this loop
            // without more bookkeeping; downgrade to a Modify on the dest
            // path which will round-trip correctly through the indexer.
            EventKind::Modify(ModifyKind::Name(RenameMode::Both | RenameMode::Other)) => {
                input.send(RawEvent::Modified(path));
            }
            EventKind::Modify(_) => input.send(RawEvent::Modified(path)),
            EventKind::Remove(_) => input.send(RawEvent::Deleted(path)),
            EventKind::Access(_) | EventKind::Other | EventKind::Any => {
                // Access events are noise (read-only opens, attribute reads).
                // Other / Any are rare platform-specific events with no
                // structural meaning - skip rather than guess.
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs as stdfs;
    use std::time::Duration;
    use tempfile::tempdir;
    use tokio::time::timeout;

    /// Drain events from `rx` until either we get a match or `total`
    /// elapses. Returns the matched event, or `None` if none arrived.
    async fn wait_for<F>(
        rx: &mut broadcast::Receiver<WatchEvent>,
        total: Duration,
        predicate: F,
    ) -> Option<WatchEvent>
    where
        F: Fn(&WatchEvent) -> bool,
    {
        let deadline = tokio::time::Instant::now() + total;
        loop {
            let now = tokio::time::Instant::now();
            if now >= deadline {
                return None;
            }
            match timeout(deadline - now, rx.recv()).await {
                Ok(Ok(event)) => {
                    if predicate(&event) {
                        return Some(event);
                    }
                }
                Ok(Err(_)) | Err(_) => return None,
            }
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn engine_watch_creates_event() {
        let dir = tempdir().expect("tempdir");
        let canonical_root = stdfs::canonicalize(dir.path()).unwrap();

        let mut watcher = Watcher::start(std::slice::from_ref(&canonical_root)).expect("start");
        let mut rx = watcher.subscribe();
        watcher.watch_root(dir.path()).expect("watch");

        // Yield to let the watcher actually register before we write.
        tokio::time::sleep(Duration::from_millis(50)).await;

        let target = canonical_root.join("hello.txt");
        stdfs::write(&target, b"hi").expect("write");

        let observed = wait_for(&mut rx, Duration::from_secs(2), |e| {
            // FSEvents on macOS sometimes reports the new file as Modified;
            // accept either. The classifier guarantees the path matches.
            e.primary_path() == &target
        })
        .await;

        assert!(observed.is_some(), "expected event for {target:?}");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn engine_modify_creates_event() {
        let dir = tempdir().expect("tempdir");
        let canonical_root = stdfs::canonicalize(dir.path()).unwrap();
        let target = canonical_root.join("existing.txt");
        stdfs::write(&target, b"initial").expect("seed");

        let mut watcher = Watcher::start(std::slice::from_ref(&canonical_root)).expect("start");
        let mut rx = watcher.subscribe();
        watcher.watch_root(dir.path()).expect("watch");

        tokio::time::sleep(Duration::from_millis(50)).await;
        stdfs::write(&target, b"changed").expect("rewrite");

        let observed = wait_for(&mut rx, Duration::from_secs(2), |e| {
            matches!(e, WatchEvent::Modified { path } | WatchEvent::Created { path } if path == &target)
        })
        .await;

        assert!(observed.is_some(), "expected modify event for {target:?}");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn engine_unwatch_silences_events() {
        let dir = tempdir().expect("tempdir");
        let canonical_root = stdfs::canonicalize(dir.path()).unwrap();

        let mut watcher = Watcher::start(std::slice::from_ref(&canonical_root)).expect("start");
        let mut rx = watcher.subscribe();
        watcher.watch_root(dir.path()).expect("watch");

        tokio::time::sleep(Duration::from_millis(50)).await;
        watcher.unwatch_root(dir.path()).expect("unwatch");
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Drain any pre-existing events from setup.
        while rx.try_recv().is_ok() {}

        let target = canonical_root.join("after-unwatch.txt");
        stdfs::write(&target, b"x").expect("write");

        // Allow plenty of time for an event to arrive if it were going to.
        let observed = wait_for(&mut rx, Duration::from_millis(500), |e| {
            e.primary_path() == &target
        })
        .await;

        assert!(observed.is_none(), "expected silence after unwatch");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn engine_idempotent_watch() {
        let dir = tempdir().expect("tempdir");
        let canonical_root = stdfs::canonicalize(dir.path()).unwrap();

        let mut watcher = Watcher::start(std::slice::from_ref(&canonical_root)).expect("start");
        watcher.watch_root(dir.path()).expect("first watch");
        // Second watch on the same canonical path is a no-op.
        watcher.watch_root(dir.path()).expect("idempotent watch");

        let roots = watcher.watched_roots();
        assert_eq!(roots.len(), 1, "expected exactly one watched root");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn path_escape_blocked() {
        let trusted = tempdir().expect("trusted");
        let other = tempdir().expect("other");
        let canonical_trusted = stdfs::canonicalize(trusted.path()).unwrap();
        let canonical_other = stdfs::canonicalize(other.path()).unwrap();

        let mut watcher = Watcher::start(&[canonical_trusted]).expect("start");
        let err = watcher
            .watch_root(&canonical_other)
            .expect_err("watching outside trusted roots must fail");
        match err {
            WatcherError::PathEscape { .. } => {}
            other => panic!("expected PathEscape, got {other:?}"),
        }
    }
}
