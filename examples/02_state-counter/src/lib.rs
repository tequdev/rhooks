//! `state-counter` — maintains a persistent counter in Hook state,
//! incrementing it by one on every invocation.
//!
//! Build: `hooks-build build --manifest-path examples/02_state-counter/Cargo.toml`

#![no_std]

use hooks_lib::prelude::*;
use hooks_lib::{accept, hook_errors, pad, rollback};

/// The 32-byte state key: the entry name, zero-padded at compile time.
/// `pad!` expands in an inline `const` block, so no copy loop (and hence no
/// loop guard) exists at runtime.
const STATE_KEY: StateKey = pad!(b"counter");

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
#[unsafe(no_mangle)]
pub extern "C" fn hook(_reserved: u32) -> i64 {
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
