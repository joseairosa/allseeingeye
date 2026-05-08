//! Pipeline error type.
//!
//! Wraps the error types from each module the pipeline composes
//! (watcher, index) so callers can match on the originating layer
//! without manually unwrapping `Box<dyn Error>`-style values.

use thiserror::Error;

use crate::index::IndexError;
use crate::watcher::WatcherError;

/// Errors raised from the pipeline orchestrator.
#[derive(Debug, Error)]
pub enum PipelineError {
    #[error("watcher error: {0}")]
    Watcher(#[from] WatcherError),

    #[error("index error: {0}")]
    Index(#[from] IndexError),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}
