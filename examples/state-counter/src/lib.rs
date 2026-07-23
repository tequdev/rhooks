//! `state-counter` — maintains a persistent counter in Hook state,
//! incrementing it by one on every invocation.
//!
//! Build: `hooks-build build --manifest-path examples/state-counter/Cargo.toml`

#![no_std]

use hooks_lib::prelude::*;
use hooks_lib::{accept, guard, rollback};

/// Name of the state entry (zero-padded to a 32-byte state key below).
const KEY_NAME: &[u8] = b"counter";

/// Builds the 32-byte, zero-padded state key for [`KEY_NAME`].
///
/// The padding loop is bounded by `KEY_NAME.len()` (a small, fixed
/// constant) and carries a `guard!` per the Hook API's static loop-guard
/// requirement — this is the one place in this example a loop is written
/// by hand rather than expressed as a fixed-size copy.
fn state_key() -> StateKey {
    let mut key: StateKey = [0u8; STATE_KEY_LEN];
    let mut i: usize = 0;
    loop {
        guard!(KEY_NAME.len() as u32);
        if i >= KEY_NAME.len() {
            break;
        }
        if let Some(slot) = key.get_mut(i)
            && let Some(&b) = KEY_NAME.get(i)
        {
            *slot = b;
        }
        i = i.wrapping_add(1);
    }
    key
}

/// Hook entry point. Reads the current counter (defaulting to zero if
/// absent or of unexpected size), increments it, writes it back, and
/// accepts with the new count as the return-code payload.
#[unsafe(no_mangle)]
pub extern "C" fn hook(_reserved: u32) -> i64 {
    let key = state_key();

    let mut raw = [0u8; 8];
    let count = match state(&mut raw, &key) {
        Ok(n) if n == raw.len() => u64::from_le_bytes(raw),
        // No existing entry (or a value of unexpected size): start at zero.
        _ => 0,
    };

    let next = count.wrapping_add(1);
    if state_set(&next.to_le_bytes(), &key).is_err() {
        rollback!(b"state-counter: state_set failed", -1);
    }

    accept!(b"state-counter: incremented", next as i64)
}
