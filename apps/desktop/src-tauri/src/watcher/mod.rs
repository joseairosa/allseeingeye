//! File watcher.
//!
//! Phase 1.3 wraps `notify` v6 with a 200 ms per-path debouncing coalescer
//! and a `tokio::sync::broadcast` fan-out so any number of in-process
//! consumers (parser pool, IPC layer, debug panel) can subscribe without
//! blocking each other.
//!
//! Public surface (everything else is module-private):
//! * [`Watcher`]      - lifecycle owner over `notify::RecommendedWatcher`.
//! * [`WatchEvent`]   - the post-coalescer event type.
//! * [`WatcherError`] - typed errors for init / watch / saturation / escape.
//!
//! Threading model (cross-reference `docs/08-tech-architecture.md`):
//!
//! ```text
//! notify thread (background)        Tokio multi-threaded runtime
//!     +----------------+                 +-----------------+
//!     | RecommendedW.  |  RawEvent  =>   |   Coalescer     |
//!     |  callback      |    mpsc         |   (1 task)      |
//!     +----------------+                 +--------+--------+
//!                                                 | broadcast
//!                                                 v
//!                                        +-----------------+
//!                                        |   Subscribers   |
//!                                        | (parser, IPC)   |
//!                                        +-----------------+
//! ```
//!
//! See `docs/05-data-architecture.md` "File watching" + "Concurrency model"
//! and `docs/11-risks.md` TR-3 for inotify saturation handling.

mod coalescer;
mod engine;
mod error;
mod event;

pub use engine::Watcher;
pub use error::WatcherError;
pub use event::WatchEvent;
