//! Process-global backend registry for native (non-`wasm32`) test hosts.
//!
//! Compiled only off `wasm32` and behind the `testenv` feature. It lets a
//! native test harness (see the `hooks-testenv` crate) supply real
//! semantics for a curated subset of the Hook API, instead of the
//! deterministic `NOT_IMPLEMENTED` stubs `api.rs` provides for every
//! function on non-`wasm32` targets. With no backend installed,
//! [`with_backend`] returns `None` and callers fall back to their existing
//! `NOT_IMPLEMENTED`-equivalent behavior — this module is purely additive
//! and never changes behavior on its own.
//!
//! **This module does not change `api.rs`'s host stubs.** Those mirror
//! `extern.h`'s `u32` `read_ptr`/`write_ptr` ABI byte-for-byte (matching
//! `wasm32`, where a pointer *is* a `u32` linear-memory offset) — on a
//! 64-bit host, casting a real pointer down to `u32` (as every hooks-lib
//! wrapper does before calling into `api.rs`) can silently truncate it, so
//! `api.rs`'s stubs cannot soundly reconstruct a real buffer from those
//! arguments. Routing therefore happens one layer up, in the hooks-lib
//! wrapper functions (see e.g. `hooks_lib::api::state::state`): they still
//! hold real `&[u8]`/`&mut [u8]` slices at the point they'd otherwise
//! truncate a pointer for the `wasm32`-mirroring FFI call, and hand those
//! slices to [`HostBackend`] directly instead.
//!
//! This emulates Hook API *semantics only* (see [`HostBackend`]'s doc) — it
//! is not a faithful re-implementation of xahaud. Guard checks, instruction
//! counting, and fees are entirely out of scope; a real `xahaud` node
//! remains the source of truth.
//!
//! ## Relationship to `HookHost`
//!
//! [`crate::host::HookHost`] is a *different*, independently-designed
//! substitution point: a generated, crate-root-level trait mirroring every
//! `api.rs` function 1:1 (same names/signatures as `extern.h`), with `Guest`
//! as its only implementor today, and no wiring into `hooks-lib` or this
//! backend registry. [`HostBackend`] here is hand-written, lives one layer
//! up (curated by semantics, not by raw FFI signature — `state`,
//! `otxn_field`, `accept`, ... taking/returning real `&[u8]`/`Vec<u8>`
//! rather than `u32` pointers), and *is* wired into `hooks-lib`'s wrapper
//! functions (see this module's doc above). The two traits were built for
//! the same broad purpose — pluggable non-`wasm32` Hook API semantics — by
//! separate efforts that have not yet been reconciled; unifying them (e.g.
//! `HostBackend` implemented in terms of `HookHost`, or `hooks-testenv`
//! implementing `HookHost` directly) is a deliberately out-of-scope future
//! refactor, not attempted here.

extern crate std;

use std::boxed::Box;
use std::sync::{PoisonError, RwLock};
use std::vec::Vec;

/// A pluggable source of Hook API semantics for native (non-`wasm32`) host
/// builds, installed process-globally via [`set_backend`].
///
/// Implemented by native test harnesses (see the `hooks-testenv` crate).
/// Each method mirrors one function from Phase 1's API coverage list
/// (`state`, `state_set`, `state_foreign`, `otxn_field`, `otxn_type`,
/// `otxn_id`, `hook_param`, `hook_account`, `hook_hash`, `ledger_seq`,
/// `ledger_last_time`, `accept`, `rollback`, `trace`, `trace_num`) — every
/// other Hook API function has no backend hook at all and keeps returning
/// `NOT_IMPLEMENTED` from the plain stub in `api.rs`, backend installed or
/// not.
pub trait HostBackend: Send + Sync {
    /// Read this hook's own state entry for `key`. `Err(code)` carries a
    /// raw Hook API error code (e.g. [`crate::error::DOESNT_EXIST`]).
    fn state(&self, key: &[u8]) -> Result<Vec<u8>, i64>;

    /// Write this hook's own state entry for `key`; an empty `data` deletes
    /// it. `Ok(n)` is the number of bytes written (`0` on delete).
    fn state_set(&self, key: &[u8], data: &[u8]) -> Result<i64, i64>;

    /// Read a state entry belonging to `account`/`namespace` (`None` for
    /// either means "this hook's own", matching the Hook API's own
    /// defaulting).
    fn state_foreign(
        &self,
        key: &[u8],
        namespace: Option<&[u8]>,
        account: Option<&[u8]>,
    ) -> Result<Vec<u8>, i64>;

    /// Read field `field_id` from the originating transaction.
    fn otxn_field(&self, field_id: u32) -> Result<Vec<u8>, i64>;

    /// The originating transaction's `tt*` type code. Never fails on the
    /// real Hook API, so this returns a plain value like [`Self::ledger_seq`].
    fn otxn_type(&self) -> i64;

    /// The ID (hash) of the originating transaction.
    fn otxn_id(&self, flags: u32) -> Result<Vec<u8>, i64>;

    /// Read this hook's own parameter `key`.
    fn hook_param(&self, key: &[u8]) -> Result<Vec<u8>, i64>;

    /// The AccountID this hook is installed on.
    fn hook_account(&self) -> Result<Vec<u8>, i64>;

    /// The hash of the hook definition at chain position `hook_no`.
    fn hook_hash(&self, hook_no: i32) -> Result<Vec<u8>, i64>;

    /// The sequence number of the current ledger.
    fn ledger_seq(&self) -> i64;

    /// The close time of the previous ledger.
    fn ledger_last_time(&self) -> i64;

    /// Terminate the hook invocation successfully. Like the real Hook API's
    /// `accept`, this never returns — implementations are expected to
    /// unwind out of the call (see `hooks-testenv::TestEnv::invoke_hook`),
    /// not loop or abort.
    fn accept(&self, msg: &[u8], code: i64) -> !;

    /// Terminate the hook invocation with a rollback. See [`Self::accept`].
    fn rollback(&self, msg: &[u8], code: i64) -> !;

    /// Emit a trace message (with optional accompanying `data`, hex-encoded
    /// when `as_hex` is set). Returns the raw Hook API success payload.
    fn trace(&self, msg: &[u8], data: &[u8], as_hex: bool) -> i64;

    /// Emit a trace message followed by an integer.
    fn trace_num(&self, msg: &[u8], number: i64) -> i64;
}

/// The process-global backend slot. `None` until a harness calls
/// [`set_backend`].
static BACKEND: RwLock<Option<Box<dyn HostBackend>>> = RwLock::new(None);

/// Install `backend` as the process-global testenv backend, returning
/// whatever backend was previously installed (if any).
///
/// Callers that install a backend are expected to serialize access (see
/// `hooks-testenv`'s global test lock) — this registry does not itself
/// arbitrate concurrent installs.
pub fn set_backend(backend: Box<dyn HostBackend>) -> Option<Box<dyn HostBackend>> {
    let mut guard = BACKEND.write().unwrap_or_else(PoisonError::into_inner);
    guard.replace(backend)
}

/// Remove and return the currently-installed backend, if any.
pub fn clear_backend() -> Option<Box<dyn HostBackend>> {
    let mut guard = BACKEND.write().unwrap_or_else(PoisonError::into_inner);
    guard.take()
}

/// Run `f` against the currently-installed backend, if any. Returns `None`
/// (without calling `f`) if no backend is installed — the caller should
/// then fall back to its own `NOT_IMPLEMENTED`-equivalent behavior.
pub fn with_backend<R>(f: impl FnOnce(&dyn HostBackend) -> R) -> Option<R> {
    let guard = BACKEND.read().unwrap_or_else(PoisonError::into_inner);
    guard.as_deref().map(f)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::panic)] // the dummy backend's accept/rollback need to unwind; see docs/DESIGN.md §8
    use super::*;

    struct Dummy;

    impl HostBackend for Dummy {
        fn state(&self, _key: &[u8]) -> Result<Vec<u8>, i64> {
            Ok(Vec::from(*b"dummy"))
        }
        fn state_set(&self, _key: &[u8], _data: &[u8]) -> Result<i64, i64> {
            Ok(0)
        }
        fn state_foreign(
            &self,
            key: &[u8],
            _namespace: Option<&[u8]>,
            _account: Option<&[u8]>,
        ) -> Result<Vec<u8>, i64> {
            self.state(key)
        }
        fn otxn_field(&self, _field_id: u32) -> Result<Vec<u8>, i64> {
            Ok(Vec::new())
        }
        fn otxn_type(&self) -> i64 {
            0
        }
        fn otxn_id(&self, _flags: u32) -> Result<Vec<u8>, i64> {
            Ok(Vec::new())
        }
        fn hook_param(&self, _key: &[u8]) -> Result<Vec<u8>, i64> {
            Ok(Vec::new())
        }
        fn hook_account(&self) -> Result<Vec<u8>, i64> {
            Ok(Vec::new())
        }
        fn hook_hash(&self, _hook_no: i32) -> Result<Vec<u8>, i64> {
            Ok(Vec::new())
        }
        fn ledger_seq(&self) -> i64 {
            42
        }
        fn ledger_last_time(&self) -> i64 {
            7
        }
        fn accept(&self, _msg: &[u8], _code: i64) -> ! {
            panic!("test-only backend: accept")
        }
        fn rollback(&self, _msg: &[u8], _code: i64) -> ! {
            panic!("test-only backend: rollback")
        }
        fn trace(&self, _msg: &[u8], _data: &[u8], _as_hex: bool) -> i64 {
            0
        }
        fn trace_num(&self, _msg: &[u8], _number: i64) -> i64 {
            0
        }
    }

    // A single test function: `BACKEND` is one process-global static, and
    // `cargo test` runs test functions concurrently by default, so every
    // assertion that depends on the slot being empty/full has to live in
    // one test to avoid racing a sibling test's `set_backend`/`clear_backend`.
    #[test]
    fn set_query_clear_round_trips() {
        assert!(with_backend(|b| b.ledger_seq()).is_none());

        let previous = set_backend(Box::new(Dummy));
        assert!(previous.is_none());
        assert_eq!(with_backend(|b| b.ledger_seq()), Some(42));
        assert_eq!(
            with_backend(|b| b.state(b"k")),
            Some(Ok(Vec::from(*b"dummy")))
        );

        let cleared = clear_backend();
        assert!(cleared.is_some());
        assert!(with_backend(|b| b.ledger_seq()).is_none());
    }
}
