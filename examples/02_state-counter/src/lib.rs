//! `state-counter` — maintains a persistent counter in Hook state,
//! incrementing it by one on every invocation.
//!
//! The minimal typed-storage tutorial: a one-field `#[derive(HookKey)]`
//! struct wrapping a short, literal byte string, paired with `u64` via
//! `hook_state!`, read/written through `state_get_typed`/`state_set_typed`
//! — no hand-rolled `[0u8; 8]` buffer, no manual `from_le_bytes`/
//! `to_le_bytes`, no length check.
//!
//! Build: `hooks-build build --manifest-path examples/02_state-counter/Cargo.toml`

#![no_std]

use hooks_lib::prelude::*;
use hooks_lib::{HookKey, accept, hook, hook_errors, hook_state, rollback};

/// The state key. `name` is a plain `[u8; 7]`, encoded (via `HookKey`'s
/// derive) at its own real length — 7 bytes, sent to the host exactly as
/// written, with no local padding up to the fixed 32-byte key space. This
/// is the same idiom as the C hook `state(&v, 8, "counter", 7)`: the Hook
/// API itself accepts any key from 1 to 32 bytes and left-pads a shorter
/// one internally (see `hooks_lib::state`'s module doc comment, "Key
/// length and padding") — so `CounterKey { name: *b"counter" }` lands on
/// the exact same 32-byte storage slot a bare `*b"counter"` key would.
///
/// A one-field struct (rather than a bare `[u8; 7]`) is required here, not
/// just stylistic: `hook_state!` below expands to `impl TypedStateKey for
/// CounterKey`, and Rust's orphan rule needs `CounterKey` — a type this
/// crate defines — to make that `impl` legal from outside `hooks_lib`
/// itself (see [`hook_state!`](hooks_lib::hook_state)'s doc comment for
/// the full explanation, including what happens if you try a bare
/// `[u8; N]` instead).
#[derive(HookKey, Clone, Copy)]
struct CounterKey {
    name: [u8; 7],
}

// Pairs `CounterKey` with its one value type (`u64`) at the type level —
// `state_get_typed`/`state_set_typed` below always resolve `u64` from the
// key argument itself, so there is no independently-chosen value type
// left for a call site to get wrong.
hook_state!(CounterKey => u64);

/// The one state entry this hook maintains.
const STATE_KEY: CounterKey = CounterKey { name: *b"counter" };

hook_errors! {
    /// `state-counter` rollback codes.
    pub enum StateCounterError {
        /// `state_set` failed to persist the incremented counter.
        StateSetFailed = 1,
    }
}

/// Hook entry point. Reads the current counter (defaulting to zero if
/// absent or of unexpected size), increments it, writes it back, and
/// accepts with the new count as the return-code payload.
#[hook]
fn my_hook() -> i64 {
    // `Ok(None)` (no entry yet) and `Err(_)` (a real error — e.g. a
    // present-but-wrong-sized entry) both default to zero, the same
    // "start from scratch" behavior a hand-rolled buffer read would give.
    let count = state_get_typed(&STATE_KEY).unwrap_or(None).unwrap_or(0);

    let next = count.wrapping_add(1);
    if state_set_typed(&STATE_KEY, &next).is_err() {
        rollback!(
            b"state-counter: state_set failed",
            StateCounterError::StateSetFailed
        );
    }

    accept!(b"state-counter: incremented", next as i64)
}
