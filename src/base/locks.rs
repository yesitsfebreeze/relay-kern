//! Poison-tolerant `RwLock` helpers.
//!
//! `std::sync::RwLock::{read, write}` return `Err(PoisonError)` when a thread
//! panicked while holding the write guard. Most kern call sites historically
//! `unwrap()` that result, which converts a worker-thread panic into a daemon
//! crash. The helpers in this module instead recover the inner guard via
//! `PoisonError::into_inner()`, log a warning through `tracing`, and hand the
//! caller a usable guard.
//!
//! # When to use
//!
//! Reach for these helpers from any kern code path where a single panicked
//! writer should not bring down the whole daemon — e.g. background tick
//! workers, gossip handlers, retrieval, MCP tool handlers. Prefer the helpers
//! over `lock.read().unwrap()` / `lock.write().unwrap()`.
//!
//! # What poison means
//!
//! A `RwLock` is poisoned when a thread panics while holding the write guard.
//! The lock is still memory-safe to access — Rust's borrow checker and the
//! lock's invariants are intact — but the protected value may have been left
//! in a *logically* inconsistent intermediate state by the aborted operation.
//!
//! # Why recovery is safe (caveats)
//!
//! - The thread that poisoned the lock is gone; it cannot continue mutating.
//! - The data is fully initialised (no `MaybeUninit`/uninit memory exposed).
//! - The remaining state is whatever the panicked operation had committed up
//!   to the panic point. Treat it as **best-effort**: invariants that span
//!   multiple fields may be temporarily broken until the next successful
//!   write restores them.
//!
//! Callers that require strict transactional consistency should not use these
//! helpers; they should propagate the error or rebuild from a known-good
//! snapshot. For the kern graph we accept best-effort recovery: a stale or
//! mid-update `GraphGnn` is preferable to a dead daemon.

use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};

/// Acquire a read guard, recovering from poison.
///
/// On poison, the inner guard is extracted via `PoisonError::into_inner()` and
/// a `warn!` is emitted via `tracing`. See module docs for safety reasoning.
pub fn read_recovered<T: ?Sized>(lock: &RwLock<T>) -> RwLockReadGuard<'_, T> {
	match lock.read() {
		Ok(g) => g,
		Err(poisoned) => {
			tracing::warn!(
				target: "kern::locks",
				"RwLock poisoned on read; recovering inner guard (best-effort, state may be partially mutated)"
			);
			poisoned.into_inner()
		}
	}
}

/// Acquire a write guard, recovering from poison.
///
/// On poison, the inner guard is extracted via `PoisonError::into_inner()` and
/// a `warn!` is emitted via `tracing`. See module docs for safety reasoning.
pub fn write_recovered<T: ?Sized>(lock: &RwLock<T>) -> RwLockWriteGuard<'_, T> {
	match lock.write() {
		Ok(g) => g,
		Err(poisoned) => {
			tracing::warn!(
				target: "kern::locks",
				"RwLock poisoned on write; recovering inner guard (best-effort, state may be partially mutated)"
			);
			poisoned.into_inner()
		}
	}
}
