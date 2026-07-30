//! `state-counter` — maintains a persistent counter in Hook state,
//! incrementing it by one on every invocation.
//!
//! The minimal typed-storage tutorial: `hook_state!`'s Form 2 (a struct key
//! with a fixed instance) declares the key struct, its `HookKey`-equivalent
//! codegen, a `const` holding the one fixed instance, and the
//! `TypedStateKey` pairing — all from one declaration — then
//! `state_get_typed`/`state_set_typed` read/write it with no hand-rolled
//! `[0u8; 8]` buffer, no manual `from_le_bytes`/`to_le_bytes`, no length
//! check.
//!
//! Build: `hooks-build build --manifest-path examples/02_state-counter/Cargo.toml`

#![no_std]

use hooks_lib::prelude::*;
use hooks_lib::{accept, hook, hook_errors, hook_state, rollback};

// The state key: `name` is a plain `[u8; 7]`, encoded at its own real
// length — 7 bytes, sent to the host exactly as written, with no local
// padding up to the fixed 32-byte key space. This is the same idiom as the
// C hook `state(&v, 8, "counter", 7)`: the Hook API itself accepts any key
// from 1 to 32 bytes and left-pads a shorter one internally (see
// `hooks_lib::state`'s module doc comment, "Key length and padding") — so
// `CounterKey { name: *b"counter" }` lands on the exact same 32-byte
// storage slot a bare `*b"counter"` key would.
//
// `hook_state!`'s Form 2 (`Name { fields } = { inits } => Value`) declares
// the `CounterKey` struct, its `HookKey`-equivalent `ToBytes`/
// `StateKeyEncode` codegen, a `const CounterKey: CounterKey = CounterKey {
// .. };` holding the one fixed instance (legal because a type name and a
// value name live in separate namespaces), and the `TypedStateKey` pairing
// with `u64` — all in one declaration. See `hooks_lib::hook_state!`'s doc
// comment for the full grammar staircase this is one step of, and why a
// struct (rather than a bare `[u8; 7]`) is required at all: Rust's orphan
// rule needs a type this crate itself defines to make the generated
// `impl TypedStateKey for CounterKey` legal from outside `hooks_lib`.
hook_state!(CounterKey {name: [u8; 7]} = {name: *b"counter"} => u64);

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
    // `&CounterKey` refers to the `const` `hook_state!`'s Form 2 declared.
    let count = state_get_typed(&CounterKey).unwrap_or(None).unwrap_or(0);

    let next = count.wrapping_add(1);
    if state_set_typed(&CounterKey, &next).is_err() {
        rollback!(
            b"state-counter: state_set failed",
            StateCounterError::StateSetFailed
        );
    }

    accept!(b"state-counter: incremented", next as i64)
}
