//! File-system safety + atomic writer.
//!
//! Phase 1.5. Implements the contract from `docs/05-data-architecture.md`
//! ("Atomic writes" and "Failure modes and recovery") and the safety rules
//! in `docs/08-tech-architecture.md` ("File-system safety") and
//! `docs/11-risks.md` (TR-2, SR-3).
//!
//! Public API surface (everything else is private):
//! * [`atomic_write`] — temp + fsync + rename + parent fsync.
//! * [`write_sidecar_backup`] — copy `<path>` to `<path>.aseye-backup`.
//! * [`assert_within_root`] — canonicalise + containment check.
//! * [`assert_safe_target`] — forbidden-segment + home-dir check.
//! * [`safe_atomic_write`] — combined gate over `atomic_write`.
//! * [`FsError`] — typed errors for all of the above.

pub mod atomic;
pub mod error;
pub mod safety;

pub use atomic::{atomic_write, write_sidecar_backup};
pub use error::FsError;
pub use safety::{
    assert_safe_target, assert_safe_target_with_override, assert_within_root,
    safe_atomic_write, safe_atomic_write_with_options,
};
