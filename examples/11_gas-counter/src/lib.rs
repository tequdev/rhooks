//! `gas-counter` — a HookApiVersion 1 ("Gas"-type) counterpart to
//! `state-counter`: the same persistent-counter behavior, but built with
//! `--api-version 1` instead of the toolchain's default (0).
//!
//! Guard-type (v0) hooks must prove every loop is bounded before
//! `hooks-build build` will accept the binary at all: a hand-written loop
//! needs `guard!`/`guard_m!`, and a compiler-generated one (as LLVM can
//! produce for byte-array comparisons even with no loop in the Rust source
//! — see `firewall`'s README) needs `--auto-guard`. Gas-type (v1) hooks have
//! no such static analysis (`docs/DESIGN.md` §6.3): execution is bounded at
//! runtime by a pre-allocated gas pool (`sfHookGas`) instead, so this hook
//! uses an ordinary Rust `for` loop *and* a plain fixed-size array equality
//! check with no guard annotation anywhere, and needs no `--auto-guard`
//! step either. The same source, if it were built with `--api-version 0`,
//! would fail `hooks-build build` outright (missing guard on the `for`
//! loop) — that is the whole point of the comparison this example exists to
//! make.
//!
//! Build: `hooks-build build --manifest-path examples/11_gas-counter/Cargo.toml --api-version 1`

#![no_std]

use hooks_lib::prelude::*;
use hooks_lib::{accept, hook, hook_errors, pad, rollback};

/// The 32-byte state key: the entry name, zero-padded at compile time.
/// `pad!` expands in an inline `const` block, so no copy loop exists at
/// runtime for this part. `StateKey` is a `#[repr(transparent)]` newtype
/// over `[u8; 32]` (see `hooks_lib::types`), so its public tuple field wraps
/// `pad!`'s plain-array output at zero cost.
const STATE_KEY: StateKey = StateKey(pad!(b"gascount"));

hook_errors! {
    /// `gas-counter` rollback codes.
    pub enum GasCounterError {
        /// `state_set` failed to persist the incremented counter.
        StateSetFailed = 1,
    }
}

/// A fixed step table, summed with an ordinary `for` loop in [`my_hook`] below.
/// Under a Guard-type (v0) build, that loop would need an explicit
/// `guard!(STEPS.len() as u64)` (or `--auto-guard`) before `hooks-build
/// build` would accept the binary; a Gas-type (v1) build needs neither —
/// the loop pass is skipped entirely for API version 1.
const STEPS: [u8; 4] = [1, 1, 1, 1];

/// A sentinel state value: writing exactly these 8 bytes into the
/// `gascount` state entry (via a separate transaction/tool, not by this
/// hook) resets the counter back to zero on the next invocation. Detected
/// with a plain `[u8; 8]` equality check — the kind of comparison LLVM
/// lowers to an unguarded byte-compare loop at `opt-level = "z"` for larger
/// arrays (see `firewall`'s `AccountId` comparison), needing
/// `--auto-guard` under v0. Here it needs nothing extra.
const RESET_MARKER: [u8; 8] = [0xFF; 8];

/// Hook entry point. Reads the current counter (defaulting to zero if
/// absent, of unexpected size, or equal to [`RESET_MARKER`]), increments it
/// by the sum of [`STEPS`] (computed with a plain, unguarded loop), writes
/// it back, and accepts with the new count as the return-code payload.
#[hook]
fn my_hook() -> i64 {
    let mut raw = [0u8; 8];
    let previous = match state(&mut raw, &STATE_KEY) {
        // Ordinary fixed-size array equality — no guard needed under
        // Gas-type hooks.
        Ok(n) if n == raw.len() && raw != RESET_MARKER => u64::from_le_bytes(raw),
        // No existing entry, a value of unexpected size, or the reset
        // marker: start (back) at zero.
        _ => 0,
    };

    // Ordinary Rust loop, no `guard!`/`guard_m!` call — legal only because
    // this is a Gas-type (v1) hook; the identical loop in a Guard-type (v0)
    // hook is a hard `hooks-build build` error.
    let mut increment: u64 = 0;
    for step in STEPS {
        increment = increment.wrapping_add(step as u64);
    }

    let next = previous.wrapping_add(increment);
    if state_set(&next.to_le_bytes(), &STATE_KEY).is_err() {
        rollback!(
            b"gas-counter: state_set failed",
            GasCounterError::StateSetFailed
        );
    }

    accept!(b"gas-counter: incremented", next as i64)
}
