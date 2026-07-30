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

// No `use hooks_lib::prelude::*;`: this hook reaches hook state entirely
// through the entity's own methods (`Counter.get_state()`), so it needs
// none of the prelude's free functions. Reach for the prelude when a hook
// calls the Hook API directly — most of the other examples do.
use hooks_lib::{accept, hook, hook_errors, hook_state, rollback};

// The state key: `name` is a plain `[u8; 7]`, encoded at its own real
// length — 7 bytes, sent to the host exactly as written, with no local
// padding up to the fixed 32-byte key space. This is the same idiom as the
// C hook `state(&v, 8, "counter", 7)`: the Hook API itself accepts any key
// from 1 to 32 bytes and left-pads a shorter one internally (see
// `hooks_lib::state`'s module doc comment, "Key length and padding") — so
// `Counter { name: *b"counter" }` lands on the exact same 32-byte
// storage slot a bare `*b"counter"` key would.
//
// `hook_state!`'s Form 2 (`Entity, Key { fields } = { inits } => Value`)
// declares the `Counter` **entity** (what this hook operates on) and the
// `CounterKey` key component that addresses it, gives both the
// `HookKey`-equivalent `ToBytes`/`StateKeyEncode` codegen and the
// `TypedStateKey` pairing with `u64`, and adds a
// `const Counter: Counter = Counter { .. };` holding the one fixed instance
// (legal because a type name and a value name live in separate namespaces).
// The accessors hang off the entity — `Counter.get_state()` below. See
// `hooks_lib::hook_state!`'s doc comment for the full grammar staircase
// this is one step of, and why a struct (rather than a bare `[u8; 7]`) is
// required at all: Rust's orphan rule needs a type this crate itself
// defines to make the generated `impl TypedStateKey for Counter` legal from
// outside `hooks_lib`.
hook_state!(Counter, CounterKey {name: [u8; 7]} = {name: *b"counter"} => u64);

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
    // `Counter` here is the `const` `hook_state!`'s Form 2 declared — the
    // entity, which is what carries `get_state`/`set_state`.
    let count = Counter.get_state().unwrap_or(None).unwrap_or(0);

    let next = count.wrapping_add(1);
    if Counter.set_state(&next).is_err() {
        rollback!(
            b"state-counter: state_set failed",
            StateCounterError::StateSetFailed
        );
    }

    accept!(b"state-counter: incremented", next as i64)
}
