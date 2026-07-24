//! `hooks-testenv` — a native, in-process Hook API emulator for testing
//! Rust Hook logic with plain `cargo test`, no `xahaud` node required.
//!
//! # What this is (and is not)
//!
//! This crate is a **semantics-only reference implementation** of a curated
//! subset of the Hook API (see [`TestEnv`]'s methods and
//! `hooks_core::backend::HostBackend` for the exact coverage: `state`,
//! `state_set`, `state_foreign`, `otxn_field`, `otxn_type`, `otxn_id`,
//! `hook_param`, `hook_account`, `hook_hash`, `ledger_seq`,
//! `ledger_last_time`, `accept`, `rollback`, `trace`, `trace_num`).
//! **`xahaud` remains the source of truth.** This emulator does not model
//! guard checks, instruction counting, fees, ledger-wide invariants, or any
//! Hook API function outside that list — those keep returning
//! `NOT_IMPLEMENTED`, exactly as they do without `hooks-testenv` at all. Use
//! this crate to test a hook's *logic* quickly and in-process; use the
//! `e2e/` suite (a real standalone `xahaud` node) to validate the real
//! thing before shipping.
//!
//! # How it works
//!
//! `hooks-lib`'s wrapper functions (`state`, `otxn_field`, `accept!`, ...),
//! when built with the `testenv` feature on a non-`wasm32` host, consult a
//! process-global backend slot in `hooks-core`
//! (`hooks_core::backend::with_backend`) instead of returning
//! `NOT_IMPLEMENTED` unconditionally. [`TestEnv::invoke_hook`] installs
//! itself into that slot for the duration of one hook call, then removes
//! it again — see that method's doc for the accept/rollback unwind
//! mechanics.
//!
//! # Example
//!
//! ```
//! use hooks_lib::prelude::*;
//! use hooks_lib::{accept, rollback};
//! use hooks_testenv::{ExitType, TestEnv};
//!
//! fn firewall_hook(_reserved: u32) -> i64 {
//!     let mut sender: AccountId = [0u8; ACC_ID_LEN];
//!     if otxn_field(&mut sender, sfAccount).is_err() {
//!         rollback!(b"no sender", -1);
//!     }
//!     let mut blocked: AccountId = [0u8; ACC_ID_LEN];
//!     match hook_param(&mut blocked, b"BL") {
//!         Ok(n) if n == ACC_ID_LEN => {}
//!         _ => accept!(),
//!     }
//!     if sender == blocked {
//!         rollback!(b"blocked", -1);
//!     }
//!     accept!()
//! }
//!
//! let env = TestEnv::new();
//! env.set_otxn_field(hooks_core::sfcodes::sfAccount, &[0xAA; 20]);
//! env.set_hook_param(b"BL", &[0xAA; 20]);
//!
//! let exit = env.invoke_hook(firewall_hook);
//! assert_eq!(exit.exit, ExitType::Rollback);
//! ```

use std::collections::BTreeMap;
use std::panic::{self, AssertUnwindSafe};
use std::sync::{Arc, Mutex, PoisonError};

use hooks_core::backend::HostBackend;

/// Serializes every [`TestEnv::invoke_hook`] call process-wide.
///
/// `hooks_core::backend`'s registry is one static slot shared by the whole
/// process; `cargo test` runs test functions concurrently by default, so
/// without this lock two tests installing backends at the same time could
/// clobber each other's registration mid-hook.
static GLOBAL_LOCK: Mutex<()> = Mutex::new(());

/// How a hook invocation ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitType {
    /// The hook called `accept!`.
    Accept,
    /// The hook called `rollback!`.
    Rollback,
}

/// The outcome of a [`TestEnv::invoke_hook`] call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookExit {
    /// Whether the hook accepted or rolled back.
    pub exit: ExitType,
    /// The application-defined return code passed to `accept!`/`rollback!`.
    pub code: i64,
    /// The message bytes passed to `accept!`/`rollback!` (empty for the
    /// no-argument `accept!()` form).
    pub msg: Vec<u8>,
}

/// Panic payload used to unwind out of a hook's `accept!`/`rollback!` call
/// and back to [`TestEnv::invoke_hook`]. Never observed by hook logic
/// itself — only downcast by `invoke_hook`'s `catch_unwind`.
struct HookExitSignal(HookExit);

/// In-memory state backing one [`TestEnv`].
#[derive(Default)]
struct Inner {
    hook_account: Option<Vec<u8>>,
    hook_hash: Option<Vec<u8>>,
    hook_params: BTreeMap<Vec<u8>, Vec<u8>>,
    state: BTreeMap<Vec<u8>, Vec<u8>>,
    otxn_fields: BTreeMap<u32, Vec<u8>>,
    otxn_type: Option<u32>,
    otxn_id: Option<Vec<u8>>,
    ledger_seq: u32,
    ledger_last_time: u64,
}

/// A native, in-process Hook API test environment.
///
/// Configure it with the `set_*` methods, run a hook function with
/// [`invoke_hook`](TestEnv::invoke_hook), then inspect resulting state with
/// [`state`](TestEnv::state). See the crate doc for a full example.
#[derive(Clone)]
pub struct TestEnv {
    inner: Arc<Mutex<Inner>>,
}

impl Default for TestEnv {
    fn default() -> Self {
        Self::new()
    }
}

impl TestEnv {
    /// Create an empty test environment: no state, no params, no
    /// originating-transaction fields, `ledger_seq`/`ledger_last_time` both
    /// zero.
    #[must_use]
    pub fn new() -> Self {
        TestEnv {
            inner: Arc::new(Mutex::new(Inner::default())),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Set the AccountID this hook is considered installed on
    /// (`hook_account`).
    pub fn set_hook_account(&self, account: &[u8]) {
        self.lock().hook_account = Some(account.to_vec());
    }

    /// Set the hash of the hook definition (`hook_hash`).
    pub fn set_hook_hash(&self, hash: &[u8]) {
        self.lock().hook_hash = Some(hash.to_vec());
    }

    /// Set a hook parameter (`hook_param`).
    pub fn set_hook_param(&self, name: &[u8], value: &[u8]) {
        self.lock()
            .hook_params
            .insert(name.to_vec(), value.to_vec());
    }

    /// Set this hook's own state entry for `key` (as if written by a prior
    /// invocation's `state_set`). Useful for seeding state before
    /// [`invoke_hook`](Self::invoke_hook); use [`state`](Self::state)
    /// afterward to read back what the hook wrote.
    pub fn set_state(&self, key: &[u8], value: &[u8]) {
        self.lock().state.insert(key.to_vec(), value.to_vec());
    }

    /// Set a field of the originating transaction (`otxn_field`), keyed by
    /// its raw `sf*` field code.
    pub fn set_otxn_field(&self, field_id: u32, value: &[u8]) {
        self.lock().otxn_fields.insert(field_id, value.to_vec());
    }

    /// Set the originating transaction's `tt*` type code (`otxn_type`).
    pub fn set_otxn_type(&self, tt: u32) {
        self.lock().otxn_type = Some(tt);
    }

    /// Set the originating transaction's ID/hash (`otxn_id`).
    pub fn set_otxn_id(&self, id: &[u8]) {
        self.lock().otxn_id = Some(id.to_vec());
    }

    /// Set the current ledger sequence (`ledger_seq`).
    pub fn set_ledger_seq(&self, seq: u32) {
        self.lock().ledger_seq = seq;
    }

    /// Set the previous ledger's close time (`ledger_last_time`).
    pub fn set_ledger_last_time(&self, time: u64) {
        self.lock().ledger_last_time = time;
    }

    /// Read back this hook's own state entry for `key`, or `None` if unset.
    /// Use after [`invoke_hook`](Self::invoke_hook) to assert on state a
    /// hook wrote via `state_set`.
    #[must_use]
    pub fn state(&self, key: &[u8]) -> Option<Vec<u8>> {
        self.lock().state.get(key).cloned()
    }

    /// Run `hook_fn` against this environment's configuration.
    ///
    /// Installs this [`TestEnv`] as the process-global `hooks-core` testenv
    /// backend (serialized against every other concurrent `invoke_hook`
    /// call via a global lock), calls `hook_fn(0)`, then uninstalls it
    /// again.
    ///
    /// A well-formed hook always ends by calling `accept!`/`rollback!`,
    /// which — with this backend installed — unwind out of the call via a
    /// panic carrying a [`HookExit`], caught here and returned normally (no
    /// panic escapes this function for that case). Note that Rust's default
    /// panic hook still prints a `thread panicked at ...` line to stderr for
    /// this expected control-flow unwind; that is intentionally left alone
    /// here rather than swapped out process-wide, since the panic hook is a
    /// single global resource and this call only holds [`GLOBAL_LOCK`]
    /// against *other* `invoke_hook` calls, not every panic in the process —
    /// overriding it here could swallow an unrelated, genuine panic from a
    /// concurrently running test. If `hook_fn` returns normally instead
    /// (never calling either), that is treated as an implicit
    /// `accept!(&[], code)` with `code` set to the returned value. Any
    /// *other* panic (a real bug in the hook logic under test) propagates
    /// to the caller unchanged.
    pub fn invoke_hook(&self, hook_fn: fn(u32) -> i64) -> HookExit {
        let _serialize = GLOBAL_LOCK.lock().unwrap_or_else(PoisonError::into_inner);

        let backend: Box<dyn HostBackend> = Box::new(BackendHandle(Arc::clone(&self.inner)));
        let previous = hooks_core::backend::set_backend(backend);

        let outcome = panic::catch_unwind(AssertUnwindSafe(|| hook_fn(0)));

        hooks_core::backend::clear_backend();
        if let Some(prev) = previous {
            // Defensive: restore whatever was installed before us. In
            // practice this is `None`, since `invoke_hook` is meant to run
            // one at a time under `GLOBAL_LOCK`.
            hooks_core::backend::set_backend(prev);
        }

        match outcome {
            Ok(code) => HookExit {
                exit: ExitType::Accept,
                code,
                msg: Vec::new(),
            },
            Err(payload) => match payload.downcast::<HookExitSignal>() {
                Ok(signal) => signal.0,
                Err(other) => panic::resume_unwind(other),
            },
        }
    }
}

/// Adapts one [`TestEnv`]'s shared state to `hooks_core::backend::HostBackend`.
struct BackendHandle(Arc<Mutex<Inner>>);

impl BackendHandle {
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl HostBackend for BackendHandle {
    fn state(&self, key: &[u8]) -> Result<Vec<u8>, i64> {
        self.lock()
            .state
            .get(key)
            .cloned()
            .ok_or(hooks_core::DOESNT_EXIST)
    }

    fn state_set(&self, key: &[u8], data: &[u8]) -> Result<i64, i64> {
        let mut inner = self.lock();
        if data.is_empty() {
            inner.state.remove(key);
            Ok(0)
        } else {
            inner.state.insert(key.to_vec(), data.to_vec());
            Ok(data.len() as i64)
        }
    }

    fn state_foreign(
        &self,
        key: &[u8],
        namespace: Option<&[u8]>,
        account: Option<&[u8]>,
    ) -> Result<Vec<u8>, i64> {
        // Phase 1 only models "this hook's own" namespace/account (`None`
        // for both, matching the Hook API's own defaulting); anything else
        // is out of scope, same as any other not-yet-covered function.
        match (namespace, account) {
            (None, None) => self.state(key),
            _ => Err(hooks_core::NOT_IMPLEMENTED),
        }
    }

    fn otxn_field(&self, field_id: u32) -> Result<Vec<u8>, i64> {
        self.lock()
            .otxn_fields
            .get(&field_id)
            .cloned()
            .ok_or(hooks_core::DOESNT_EXIST)
    }

    fn otxn_type(&self) -> i64 {
        i64::from(self.lock().otxn_type.unwrap_or(0))
    }

    fn otxn_id(&self, _flags: u32) -> Result<Vec<u8>, i64> {
        self.lock().otxn_id.clone().ok_or(hooks_core::DOESNT_EXIST)
    }

    fn hook_param(&self, key: &[u8]) -> Result<Vec<u8>, i64> {
        self.lock()
            .hook_params
            .get(key)
            .cloned()
            .ok_or(hooks_core::DOESNT_EXIST)
    }

    fn hook_account(&self) -> Result<Vec<u8>, i64> {
        self.lock()
            .hook_account
            .clone()
            .ok_or(hooks_core::DOESNT_EXIST)
    }

    fn hook_hash(&self, _hook_no: i32) -> Result<Vec<u8>, i64> {
        self.lock()
            .hook_hash
            .clone()
            .ok_or(hooks_core::DOESNT_EXIST)
    }

    fn ledger_seq(&self) -> i64 {
        i64::from(self.lock().ledger_seq)
    }

    fn ledger_last_time(&self) -> i64 {
        self.lock().ledger_last_time as i64
    }

    // `panic_any` is this crate's deliberate control-flow mechanism for
    // unwinding out of `accept!`/`rollback!` back to `invoke_hook`'s
    // `catch_unwind` (see that method's doc) — not an error condition, so
    // `clippy::panic` is allowed right here rather than crate-wide.
    #[allow(clippy::panic)]
    fn accept(&self, msg: &[u8], code: i64) -> ! {
        panic::panic_any(HookExitSignal(HookExit {
            exit: ExitType::Accept,
            code,
            msg: msg.to_vec(),
        }))
    }

    #[allow(clippy::panic)]
    fn rollback(&self, msg: &[u8], code: i64) -> ! {
        panic::panic_any(HookExitSignal(HookExit {
            exit: ExitType::Rollback,
            code,
            msg: msg.to_vec(),
        }))
    }

    fn trace(&self, msg: &[u8], data: &[u8], _as_hex: bool) -> i64 {
        msg.len().wrapping_add(data.len()) as i64
    }

    fn trace_num(&self, msg: &[u8], _number: i64) -> i64 {
        msg.len() as i64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hooks_lib::prelude::*;
    use hooks_lib::{accept, pad, rollback};

    /// `firewall`-equivalent hook logic (see `examples/firewall`), defined
    /// inline: rolls back if the originating transaction's sender matches
    /// the `BL` hook parameter, accepts otherwise.
    fn firewall_hook(_reserved: u32) -> i64 {
        let mut sender: AccountId = [0u8; ACC_ID_LEN];
        match otxn_field(&mut sender, hooks_core::sfcodes::sfAccount) {
            Ok(n) if n == ACC_ID_LEN => {}
            _ => rollback!(b"firewall: could not read otxn sender", -1),
        }

        let mut blocked: AccountId = [0u8; ACC_ID_LEN];
        match hook_param(&mut blocked, b"BL") {
            Ok(n) if n == ACC_ID_LEN => {}
            _ => accept!(),
        }

        if sender == blocked {
            rollback!(b"firewall: blocked account", -1);
        }

        accept!()
    }

    /// `state-counter`-equivalent hook logic (see `examples/state-counter`),
    /// defined inline: reads an 8-byte little-endian counter from state
    /// (defaulting to zero), increments it, and writes it back.
    fn counter_hook(_reserved: u32) -> i64 {
        let key: StateKey = pad!(b"counter");

        let mut raw = [0u8; 8];
        let count = match state(&mut raw, &key) {
            Ok(n) if n == raw.len() => u64::from_le_bytes(raw),
            _ => 0,
        };

        let next = count.wrapping_add(1);
        if state_set(&next.to_le_bytes(), &key).is_err() {
            rollback!(b"state-counter: state_set failed", -1);
        }

        accept!(b"state-counter: incremented", next as i64)
    }

    #[test]
    fn firewall_rolls_back_blocked_sender() {
        let env = TestEnv::new();
        let blocked_account = [0xAAu8; ACC_ID_LEN];
        env.set_otxn_field(hooks_core::sfcodes::sfAccount, &blocked_account);
        env.set_hook_param(b"BL", &blocked_account);

        let exit = env.invoke_hook(firewall_hook);

        assert_eq!(exit.exit, ExitType::Rollback);
        assert_eq!(exit.code, -1);
        assert_eq!(exit.msg, b"firewall: blocked account");
    }

    #[test]
    fn firewall_accepts_other_sender() {
        let env = TestEnv::new();
        env.set_otxn_field(hooks_core::sfcodes::sfAccount, &[0x11u8; ACC_ID_LEN]);
        env.set_hook_param(b"BL", &[0xAAu8; ACC_ID_LEN]);

        let exit = env.invoke_hook(firewall_hook);

        assert_eq!(exit.exit, ExitType::Accept);
    }

    #[test]
    fn firewall_accepts_when_no_blocklist_configured() {
        let env = TestEnv::new();
        env.set_otxn_field(hooks_core::sfcodes::sfAccount, &[0x11u8; ACC_ID_LEN]);

        let exit = env.invoke_hook(firewall_hook);

        assert_eq!(exit.exit, ExitType::Accept);
    }

    #[test]
    fn counter_increments_and_persists_in_state() {
        let key: StateKey = pad!(b"counter");
        let env = TestEnv::new();

        let first = env.invoke_hook(counter_hook);
        assert_eq!(first.exit, ExitType::Accept);
        assert_eq!(first.code, 1);
        assert_eq!(env.state(&key), Some(1u64.to_le_bytes().to_vec()));

        let second = env.invoke_hook(counter_hook);
        assert_eq!(second.exit, ExitType::Accept);
        assert_eq!(second.code, 2);
        assert_eq!(env.state(&key), Some(2u64.to_le_bytes().to_vec()));
    }

    #[test]
    fn ledger_and_hook_context_round_trip() {
        let env = TestEnv::new();
        env.set_ledger_seq(123);
        env.set_ledger_last_time(456);
        env.set_hook_account(&[0x22u8; ACC_ID_LEN]);
        env.set_hook_hash(&[0x33u8; 32]);
        env.set_otxn_id(&[0x44u8; 32]);
        env.set_otxn_type(hooks_core::tts::ttPAYMENT);

        fn context_hook(_reserved: u32) -> i64 {
            let seq = ledger_seq();
            let time = ledger_last_time();
            let acct = hook_account_buf().unwrap_or([0u8; ACC_ID_LEN]);
            let hash = hook_hash_buf(0).unwrap_or([0u8; HASH_LEN]);
            let mut id = [0u8; HASH_LEN];
            let _ = otxn_id(&mut id, 0);
            let tt = otxn_type();

            // A bitmask (not a sum) so this stays free of
            // `clippy::arithmetic_side_effects`-flagged runtime arithmetic.
            let mut mask = 0u32;
            if seq == 123 {
                mask |= 0b1;
            }
            if time == 456 {
                mask |= 0b10;
            }
            if acct == [0x22u8; ACC_ID_LEN] {
                mask |= 0b100;
            }
            if hash == [0x33u8; HASH_LEN] {
                mask |= 0b1000;
            }
            if id == [0x44u8; HASH_LEN] {
                mask |= 0b1_0000;
            }
            if tt == 0 {
                mask |= 0b10_0000;
            }
            accept!(b"context", i64::from(mask))
        }

        let exit = env.invoke_hook(context_hook);
        assert_eq!(exit.exit, ExitType::Accept);
        assert_eq!(exit.code, 0b11_1111);
    }
}
