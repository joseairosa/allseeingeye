//! 200 ms per-path debouncing coalescer.
//!
//! The coalescer sits between `notify`'s raw event stream and the rest of
//! the crate. It exists because:
//!
//! 1. Editors emit "atomic save" bursts (write temp -> rename) which arrive
//!    as `Delete + Create` on the same path within a few ms. Without
//!    coalescing, every save costs the parser two parses + two FTS upserts.
//! 2. A single Save in some editors emits 3-5 `Modify` events as the
//!    underlying writer flushes pages. We collapse them to one `Modified`.
//! 3. `notify` emits rename as two events (`Rename(From)` then
//!    `Rename(To)`). When both arrive within the window we re-pair them
//!    into a single `Renamed { from, to }`.
//!
//! Strategy: a single tokio task owns a `HashMap<PathBuf, PendingState>`.
//! Each entry has a deadline; when its deadline elapses, the entry is
//! flushed as the appropriate `WatchEvent`. New events for the same path
//! reset the deadline. Rename pairing operates across path keys: when a
//! `RenameFrom(A)` is followed by a `RenameTo(B)` within the window, we
//! emit `Renamed { from: A, to: B }` and clear both.
//!
//! Concurrency model:
//! * Raw events arrive on a `tokio::sync::mpsc::UnboundedSender`. Bounded
//!   queues are tempting but we never want to drop FS events silently;
//!   instead the OS watcher provides backpressure by dropping events itself
//!   when its kernel buffer fills, which `notify` reports separately.
//! * Coalesced events are fanned out via `tokio::sync::broadcast` so any
//!   number of subscribers (parser pool, IPC layer, UI debug panel) can
//!   listen without serial coupling.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use tokio::sync::{broadcast, mpsc};
use tokio::time::{sleep_until, Instant};

use super::event::WatchEvent;

/// Debounce window. The figure comes from `docs/05-data-architecture.md`
/// ("File watching"): "Coalescing: 200 ms debounce per path."
pub(crate) const DEBOUNCE_WINDOW: Duration = Duration::from_millis(200);

/// Capacity of the broadcast channel. Subscribers that fall behind by more
/// than this many events will lag; we accept lag rather than blocking the
/// watcher task.
const BROADCAST_CAPACITY: usize = 1024;

/// One raw event the coalescer ingests. This is the surface produced by the
/// engine's `notify` callback after lightweight classification - we don't
/// expose `notify::Event` directly so the coalescer is fully testable
/// without touching the file system.
#[derive(Debug, Clone)]
pub(crate) enum RawEvent {
    Created(PathBuf),
    Modified(PathBuf),
    Deleted(PathBuf),
    /// Source side of a rename. Held in pending state until the matching
    /// `RenameTo` arrives within the debounce window.
    RenameFrom(PathBuf),
    /// Destination side of a rename. Pairs with the most recent unpaired
    /// `RenameFrom` whose deadline has not yet elapsed.
    RenameTo(PathBuf),
}

/// Per-path pending state. A path is in exactly one of these "shapes" at any
/// time; new events transition it according to the rules in
/// `transition_for_event`.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum Pending {
    Created,
    Modified,
    Deleted,
    /// Paired-pending source of a rename - we keep the path key here and
    /// wait for a `RenameTo` to claim it.
    RenameFrom,
}

/// Public handle for sending raw events into the coalescer. The receiver
/// lives in the spawned task; dropping the last clone of this handle will
/// close the input channel and (after any pending flushes) shut the task
/// down cleanly.
#[derive(Debug, Clone)]
pub(crate) struct CoalescerInput {
    tx: mpsc::UnboundedSender<RawEvent>,
}

impl CoalescerInput {
    pub(crate) fn send(&self, event: RawEvent) {
        // We deliberately ignore send errors. They only happen when the
        // coalescer task has exited, in which case there is no recipient
        // and the engine is in the middle of being torn down anyway.
        let _ = self.tx.send(event);
    }
}

/// The coalescer. Owns the broadcast sender; spawn the task with
/// `Coalescer::start` and use `subscribe()` to obtain receivers.
#[derive(Debug)]
pub(crate) struct Coalescer {
    input: CoalescerInput,
    output: broadcast::Sender<WatchEvent>,
}

impl Coalescer {
    /// Start the coalescer task on the current Tokio runtime. Returns the
    /// public input handle (clone + send raw events on it) and the
    /// broadcast sender the engine clones for subscribers.
    pub(crate) fn start() -> Self {
        Self::start_with_window(DEBOUNCE_WINDOW)
    }

    /// Start with a custom debounce window. Tests override the window so
    /// they don't need to wait 200 ms each.
    pub(crate) fn start_with_window(window: Duration) -> Self {
        let (raw_tx, raw_rx) = mpsc::unbounded_channel::<RawEvent>();
        let (out_tx, _) = broadcast::channel::<WatchEvent>(BROADCAST_CAPACITY);

        let out_for_task = out_tx.clone();
        tokio::spawn(async move {
            run(raw_rx, out_for_task, window).await;
        });

        Self {
            input: CoalescerInput { tx: raw_tx },
            output: out_tx,
        }
    }

    pub(crate) fn input(&self) -> CoalescerInput {
        self.input.clone()
    }

    pub(crate) fn subscribe(&self) -> broadcast::Receiver<WatchEvent> {
        self.output.subscribe()
    }
}

/// Driver loop. Owned by the spawned task; never returns until the input
/// channel closes (i.e. all `CoalescerInput`s have been dropped).
async fn run(
    mut rx: mpsc::UnboundedReceiver<RawEvent>,
    out: broadcast::Sender<WatchEvent>,
    window: Duration,
) {
    // Per-path pending state + deadline.
    let mut pending: HashMap<PathBuf, (Pending, Instant)> = HashMap::new();

    loop {
        // Compute the next deadline. If `pending` is empty, we wait
        // indefinitely on the input channel; otherwise we race the channel
        // against the earliest deadline.
        let next_deadline = pending.values().map(|(_, d)| *d).min();

        tokio::select! {
            // Bias toward processing input first so a flood of events
            // doesn't starve. With `biased`, the select polls branches in
            // declaration order.
            biased;

            maybe_event = rx.recv() => {
                if let Some(event) = maybe_event {
                    apply_event(&mut pending, &out, event, window);
                } else {
                    // Channel closed - drain any pending paths so we
                    // don't lose events and exit cleanly.
                    drain_all(&mut pending, &out);
                    return;
                }
            }

            // Only poll the timer branch when we actually have a deadline.
            // Without this gate, `select!` would pick a `Future::pending()`
            // and never wake on timer expiry.
            () = wait_until(next_deadline), if next_deadline.is_some() => {
                flush_expired(&mut pending, &out);
            }
        }
    }
}

/// Wait until `deadline`. Returns immediately if `deadline` is `None` -
/// callers gate this with `is_some()` so we never actually hit the `None`
/// branch in practice; the `unreachable!` documents the invariant.
async fn wait_until(deadline: Option<Instant>) {
    match deadline {
        Some(d) => sleep_until(d).await,
        // Defensive: if the gate ever lets us through with `None`, yield
        // for a short period rather than busy-loop.
        None => sleep_until(Instant::now() + Duration::from_millis(1)).await,
    }
}

/// Apply one raw event to the pending map. Encodes the state machine:
///
/// | current pending | incoming         | new pending      | emit?            |
/// |-----------------|------------------|------------------|------------------|
/// | (none)          | `Created`        | `Created`        | -                |
/// | (none)          | `Modified`       | `Modified`       | -                |
/// | (none)          | `Deleted`        | `Deleted`        | -                |
/// | (none)          | `RenameFrom`     | `RenameFrom`     | -                |
/// | (none)          | `RenameTo`       | `Created`        | -  (orphan To)   |
/// | `Created`       | `Modified`       | `Created`        | -                |
/// | `Created`       | `Deleted`        | (clear)          | -  (no-op net)   |
/// | `Modified`      | `Modified`       | `Modified` (reset)| -               |
/// | `Modified`      | `Deleted`        | `Deleted`        | -                |
/// | `Deleted`       | `Created`        | `Modified`       | -  (atomic save) |
/// | `Deleted`       | `Modified`       | `Deleted`        | -                |
/// | `RenameFrom(A)` | `RenameTo(B!=A)` | (clear A,B)      | `Renamed{A,B}`   |
///
/// Anything not listed degrades to "replace and reset deadline" so we never
/// drop a real event.
fn apply_event(
    pending: &mut HashMap<PathBuf, (Pending, Instant)>,
    out: &broadcast::Sender<WatchEvent>,
    event: RawEvent,
    window: Duration,
) {
    let now = Instant::now();
    let new_deadline = now + window;

    match event {
        RawEvent::Created(path) => {
            match pending.get(&path).map(|(p, _)| *p) {
                Some(Pending::Deleted) => {
                    // Atomic save: a delete+create on the same path within
                    // the window is a single Modified.
                    pending.insert(path, (Pending::Modified, new_deadline));
                }
                Some(Pending::Created | Pending::Modified) => {
                    // Already in a "file present and changed" state; just
                    // refresh deadline.
                    pending.insert(path, (Pending::Created, new_deadline));
                }
                Some(Pending::RenameFrom) => {
                    // We had an unpaired RenameFrom on this exact path and
                    // got a Create back on it. Treat as the file came back -
                    // emit Created, clear RenameFrom semantics.
                    pending.insert(path, (Pending::Created, new_deadline));
                }
                None => {
                    pending.insert(path, (Pending::Created, new_deadline));
                }
            }
        }
        RawEvent::Modified(path) => {
            match pending.get(&path).map(|(p, _)| *p) {
                Some(Pending::Created) => {
                    // Stay Created; refresh window.
                    pending.insert(path, (Pending::Created, new_deadline));
                }
                Some(Pending::Deleted) => {
                    // Spurious Modify after Delete - keep Deleted, refresh
                    // window so the eventual flush is correctly Deleted.
                    pending.insert(path, (Pending::Deleted, new_deadline));
                }
                _ => {
                    pending.insert(path, (Pending::Modified, new_deadline));
                }
            }
        }
        RawEvent::Deleted(path) => {
            if matches!(pending.get(&path).map(|(p, _)| *p), Some(Pending::Created)) {
                // Created then Deleted within the window - net no-op. Drop
                // the entry without emitting.
                pending.remove(&path);
                return;
            }
            pending.insert(path, (Pending::Deleted, new_deadline));
        }
        RawEvent::RenameFrom(path) => {
            pending.insert(path, (Pending::RenameFrom, new_deadline));
        }
        RawEvent::RenameTo(to) => {
            // Find the most recent un-expired RenameFrom. We can't index
            // by deadline so we scan; the map is small in practice (one
            // entry per file currently in flight).
            let mut from_match: Option<PathBuf> = None;
            let mut latest: Option<Instant> = None;
            for (key, (state, deadline)) in pending.iter() {
                if matches!(state, Pending::RenameFrom)
                    && *deadline >= now
                    && key != &to
                    && latest.is_none_or(|l| *deadline > l)
                {
                    latest = Some(*deadline);
                    from_match = Some(key.clone());
                }
            }
            if let Some(from) = from_match {
                pending.remove(&from);
                pending.remove(&to);
                emit(out, WatchEvent::Renamed { from, to });
            } else {
                // Orphan RenameTo: no pending RenameFrom in window.
                // Treat as a Created on `to`.
                pending.insert(to, (Pending::Created, new_deadline));
            }
        }
    }
}

/// Find every entry whose deadline has elapsed and emit it.
fn flush_expired(
    pending: &mut HashMap<PathBuf, (Pending, Instant)>,
    out: &broadcast::Sender<WatchEvent>,
) {
    let now = Instant::now();
    let expired_keys: Vec<PathBuf> = pending
        .iter()
        .filter_map(|(k, (_, d))| if *d <= now { Some(k.clone()) } else { None })
        .collect();
    for key in expired_keys {
        if let Some((state, _)) = pending.remove(&key) {
            emit_for_state(out, key, state);
        }
    }
}

/// Drain all pending state at shutdown.
fn drain_all(
    pending: &mut HashMap<PathBuf, (Pending, Instant)>,
    out: &broadcast::Sender<WatchEvent>,
) {
    let drained: Vec<(PathBuf, Pending)> = pending
        .drain()
        .map(|(path, (state, _))| (path, state))
        .collect();
    for (path, state) in drained {
        emit_for_state(out, path, state);
    }
}

fn emit_for_state(out: &broadcast::Sender<WatchEvent>, path: PathBuf, state: Pending) {
    let event = match state {
        Pending::Created => WatchEvent::Created { path },
        Pending::Modified => WatchEvent::Modified { path },
        // An unpaired `RenameFrom` collapses to `Deleted`: the file is gone
        // from this path and nothing claimed the destination within the
        // debounce window.
        Pending::Deleted | Pending::RenameFrom => WatchEvent::Deleted { path },
    };
    emit(out, event);
}

fn emit(out: &broadcast::Sender<WatchEvent>, event: WatchEvent) {
    // `broadcast::send` errors only when there are no live receivers. That
    // is a normal state during startup/shutdown, not a fault.
    let _ = out.send(event);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::time::timeout;

    /// Short window so tests don't have to wait 200 ms each.
    const TEST_WINDOW: Duration = Duration::from_millis(40);

    /// Helper: collect events from a receiver until `timeout_after` elapses
    /// without any new event arriving.
    async fn collect_until_idle(
        rx: &mut broadcast::Receiver<WatchEvent>,
        idle: Duration,
    ) -> Vec<WatchEvent> {
        let mut out = Vec::new();
        loop {
            match timeout(idle, rx.recv()).await {
                Ok(Ok(event)) => out.push(event),
                _ => return out,
            }
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn coalescer_collapses_modify_burst() {
        let coalescer = Coalescer::start_with_window(TEST_WINDOW);
        let input = coalescer.input();
        let mut rx = coalescer.subscribe();

        let path = PathBuf::from("/tmp/aseye-test/burst.json");
        for _ in 0..5 {
            input.send(RawEvent::Modified(path.clone()));
            tokio::time::sleep(Duration::from_millis(5)).await;
        }

        let events = collect_until_idle(&mut rx, TEST_WINDOW * 4).await;
        assert_eq!(events.len(), 1, "burst should collapse to one event");
        assert_eq!(events[0], WatchEvent::Modified { path });
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn coalescer_collapses_atomic_save_to_modified() {
        let coalescer = Coalescer::start_with_window(TEST_WINDOW);
        let input = coalescer.input();
        let mut rx = coalescer.subscribe();

        let path = PathBuf::from("/tmp/aseye-test/atomic.yaml");
        input.send(RawEvent::Deleted(path.clone()));
        // Editor's atomic save: delete then create within window.
        tokio::time::sleep(Duration::from_millis(5)).await;
        input.send(RawEvent::Created(path.clone()));

        let events = collect_until_idle(&mut rx, TEST_WINDOW * 4).await;
        assert_eq!(events.len(), 1, "atomic save should collapse to Modified");
        assert_eq!(events[0], WatchEvent::Modified { path });
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn coalescer_emits_separate_events_after_window() {
        let coalescer = Coalescer::start_with_window(TEST_WINDOW);
        let input = coalescer.input();
        let mut rx = coalescer.subscribe();

        let path = PathBuf::from("/tmp/aseye-test/spaced.md");
        input.send(RawEvent::Modified(path.clone()));
        // Wait clearly past the window so the first event flushes alone.
        tokio::time::sleep(TEST_WINDOW * 4).await;
        input.send(RawEvent::Modified(path.clone()));

        let events = collect_until_idle(&mut rx, TEST_WINDOW * 4).await;
        assert_eq!(events.len(), 2);
        assert_eq!(events[0], WatchEvent::Modified { path: path.clone() });
        assert_eq!(events[1], WatchEvent::Modified { path });
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn coalescer_pairs_rename_from_and_to() {
        let coalescer = Coalescer::start_with_window(TEST_WINDOW);
        let input = coalescer.input();
        let mut rx = coalescer.subscribe();

        let from = PathBuf::from("/tmp/aseye-test/old.md");
        let to = PathBuf::from("/tmp/aseye-test/new.md");
        input.send(RawEvent::RenameFrom(from.clone()));
        tokio::time::sleep(Duration::from_millis(5)).await;
        input.send(RawEvent::RenameTo(to.clone()));

        let events = collect_until_idle(&mut rx, TEST_WINDOW * 4).await;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0], WatchEvent::Renamed { from, to });
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn coalescer_preserves_unrelated_paths() {
        let coalescer = Coalescer::start_with_window(TEST_WINDOW);
        let input = coalescer.input();
        let mut rx = coalescer.subscribe();

        let a = PathBuf::from("/tmp/aseye-test/a.txt");
        let b = PathBuf::from("/tmp/aseye-test/b.txt");
        input.send(RawEvent::Created(a.clone()));
        input.send(RawEvent::Modified(b.clone()));

        let mut events = collect_until_idle(&mut rx, TEST_WINDOW * 4).await;
        // Sort by path string so the test is order-independent.
        events.sort_by_key(|e| e.primary_path().clone());
        assert_eq!(events.len(), 2);
        assert_eq!(events[0], WatchEvent::Created { path: a });
        assert_eq!(events[1], WatchEvent::Modified { path: b });
    }
}
