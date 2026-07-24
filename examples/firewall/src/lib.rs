//! `firewall` — rolls the transaction back if its sender matches a
//! blacklisted account supplied via the `BL` Hook parameter; accepts
//! otherwise. Straight-line code: no loops, so no `guard!` is needed — and
//! the account comparison uses `hooks_lib::buf_eq_20` rather than `==`, so
//! no compiler-generated loop appears in the compiled output either (see
//! `hooks_lib::buf_eq` docs).
//!
//! Build: `hooks-build build --manifest-path examples/firewall/Cargo.toml`

#![no_std]

use hooks_lib::prelude::*;
use hooks_lib::{accept, rollback};

/// Name of the Hook parameter carrying the 20-byte blocked `AccountId`.
const BL_PARAM: &[u8] = b"BL";

/// Hook entry point. Rolls back if the originating transaction's sender is
/// the blacklisted account; accepts otherwise.
#[unsafe(no_mangle)]
pub extern "C" fn hook(_reserved: u32) -> i64 {
    let mut sender: AccountId = [0u8; ACC_ID_LEN];
    match otxn_field(&mut sender, sfAccount) {
        Ok(n) if n == ACC_ID_LEN => {}
        _ => rollback!(b"firewall: could not read otxn sender", -1),
    }

    let mut blocked: AccountId = [0u8; ACC_ID_LEN];
    match hook_param(&mut blocked, BL_PARAM) {
        // No (valid) blacklist parameter configured: nothing to block.
        Ok(n) if n == ACC_ID_LEN => {}
        _ => accept!(),
    }

    // `buf_eq_20` (not `sender == blocked`): array `==` compiles to a
    // compiler-generated, unguarded `bcmp`-style loop on `wasm32v1-none` at
    // `opt-level = "z"` — see `hooks_lib::buf_eq` docs and
    // `docs/DESIGN.md` §6.3.
    if buf_eq_20(&sender, &blocked) {
        rollback!(b"firewall: blocked account", -1);
    }

    accept!()
}
