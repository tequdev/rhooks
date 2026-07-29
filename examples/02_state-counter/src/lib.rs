//! `state-counter` — maintains a persistent counter in Hook state,
//! incrementing it by one on every invocation.
//!
//! Build: `hooks-build build --manifest-path examples/02_state-counter/Cargo.toml`

#![no_std]

use hooks_lib::prelude::*;
use hooks_lib::{accept, hook, hook_errors, rollback};

/// The state key: a short, literal byte string, sent to the host exactly
/// as-is (7 bytes) — no local padding up to the fixed 32-byte key space.
/// This is the same idiom as the C hook `state(&v, 8, "counter", 7)`: the
/// Hook API itself accepts any key from 1 to 32 bytes and left-pads a
/// shorter one internally (see `hooks_lib::state`'s module doc comment,
/// "Key length and padding"), so there is no need to build a full 32-byte
/// `StateKey` by hand here.
const STATE_KEY: [u8; 7] = *b"counter";

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
    let mut raw = [0u8; 8];
    let count = match state(&mut raw, &STATE_KEY) {
        Ok(n) if n == raw.len() => u64::from_le_bytes(raw),
        // No existing entry (or a value of unexpected size): start at zero.
        _ => 0,
    };

    let next = count.wrapping_add(1);
    if state_set(&next.to_le_bytes(), &STATE_KEY).is_err() {
        rollback!(
            b"state-counter: state_set failed",
            StateCounterError::StateSetFailed
        );
    }

    accept!(b"state-counter: incremented", next as i64)
}
