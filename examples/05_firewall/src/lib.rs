//! `firewall` — rolls the transaction back if its sender matches a
//! blacklisted account supplied via the `BL` Hook parameter; accepts
//! otherwise. Straight-line code: no loops, so no `guard!` is needed — and
//! the account comparison uses `hooks_lib::buf_eq_20` rather than `==`, so
//! no compiler-generated loop appears in the compiled output either (see
//! `hooks_lib::buf_eq` docs).
//!
//! Build: `hooks-build build --manifest-path examples/05_firewall/Cargo.toml`

#![no_std]

use hooks_lib::prelude::*;
use hooks_lib::{accept, hook, hook_errors, hook_parameter, rollback};

// The Hook parameter carrying the 20-byte blocked `AccountId` (name
// `"BL"`). `hook_parameter!`'s Form 1 (a fixed-byte-string name) ties the
// name to `AccountId` at the type level — `hook_param_typed` below
// replaces the previous buffer-mode `hook_param(&mut blocked, BL_PARAM)`
// with an identical outcome: that call's own `Ok(n) if n == ACC_ID_LEN`
// guard already required an *exact* 20-byte read (govern.c-style
// buffer-mode reads and `hook_param_exact`'s `FixedRead`-backed exact-
// length check agree in every reachable case — see
// `examples/81_govern`'s README, "Parameter read semantics" section, for
// the general argument), so this migration changes nothing observable:
// missing, too-short, or too-long all still fall through to `accept!()`
// exactly as before.
hook_parameter!(BlockedParam, BlockedParamName = b"BL" => AccountId);

hook_errors! {
    /// `firewall` rollback codes.
    pub enum FirewallError {
        /// `otxn_field(sfAccount)` did not return a 20-byte `AccountId`.
        CouldNotReadSender = 1,
        /// The originating transaction's sender matched the blacklisted
        /// account configured via the `BL` Hook parameter.
        BlockedAccount = 2,
    }
}

/// Hook entry point. Rolls back if the originating transaction's sender is
/// the blacklisted account; accepts otherwise.
#[hook]
fn my_hook() -> i64 {
    let mut sender = AccountId::default();
    match otxn_field(&mut sender, sfAccount) {
        Ok(n) if n == ACC_ID_LEN => {}
        _ => rollback!(
            b"firewall: could not read otxn sender",
            FirewallError::CouldNotReadSender
        ),
    }

    // No (valid) blacklist parameter configured: nothing to block.
    let blocked: AccountId = match BlockedParam.get_value() {
        Ok(v) => v,
        Err(_) => accept!(),
    };

    // `buf_eq_20` (not `sender == blocked`): array `==` compiles to a
    // compiler-generated, unguarded `bcmp`-style loop on `wasm32v1-none` at
    // `opt-level = "z"` — see `hooks_lib::buf_eq` docs and
    // `docs/DESIGN.md` §6.3.
    if buf_eq_20(&sender, &blocked) {
        rollback!(b"firewall: blocked account", FirewallError::BlockedAccount);
    }

    accept!()
}
