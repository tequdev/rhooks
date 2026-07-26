//! `state-foreign` — gates acceptance on a single configuration flag stored
//! in *another* account's Hook state, read via `state_foreign`. The
//! account to read from is itself configurable via a Hook parameter (`ACCT`),
//! following the same "config via hook_param" idiom as `hook-params`.
//!
//! Build: `hooks-build build --manifest-path examples/09_state-foreign/Cargo.toml`

#![no_std]

use hooks_lib::prelude::*;
use hooks_lib::{accept, hook, hook_errors, pad, rollback};

/// Name of the Hook parameter carrying the 20-byte `AccountId` this hook
/// reads its gate flag from.
const ACCT_PARAM: &[u8] = b"ACCT";

/// The state key this hook looks for on the foreign account: the name
/// `b"enabled"`, zero-padded at compile time to the full 32-byte state-key
/// width (see `state-counter`'s use of `pad!` for the same trick — no
/// runtime copy loop, hence no loop guard, is needed for it).
const ENABLED_KEY: StateKey = StateKey(pad!(b"enabled"));

hook_errors! {
    /// `state-foreign` rollback codes.
    pub enum StateForeignError {
        /// The `ACCT` Hook parameter isn't configured (or isn't a 20-byte
        /// `AccountId`).
        AcctNotConfigured = 1,
        /// The target account has no `enabled` entry in this hook's
        /// namespace.
        NotConfiguredOnTarget = 2,
        /// `state_foreign` failed for a reason other than "no entry"
        /// (e.g. a malformed target account).
        ReadFailed = 3,
        /// The target account's `enabled` entry exists but its first byte
        /// is zero.
        FlagOff = 4,
    }
}

/// Hook entry point. Reads the `enabled` state entry on the account named
/// by the `ACCT` Hook parameter (in *this* hook's own namespace on that
/// foreign account — `state_foreign`'s `namespace = None` means "this
/// hook's own", same as for a local `state` call) and accepts only if that
/// entry exists and its first byte is nonzero.
#[hook]
fn my_hook() -> i64 {
    let target: AccountId = match hook_param_exact::<ACC_ID_LEN>(ACCT_PARAM) {
        Ok(t) => t.into(),
        Err(_) => rollback!(
            b"state-foreign: ACCT parameter not configured",
            StateForeignError::AcctNotConfigured
        ),
    };

    let mut flag = [0u8; 1];
    match state_foreign(&mut flag, &ENABLED_KEY, None, Some(target.as_ref())) {
        Ok(n) if n == flag.len() => {}
        // `DOESNT_EXIST`: the foreign account has no `enabled` entry in
        // this hook's namespace — treated as "not enabled", not a crash.
        // Any other error (e.g. a malformed `target` that isn't a real
        // account) is reported with its own message instead of being
        // lumped in with "not enabled".
        Err(HookError::DoesntExist) => rollback!(
            b"state-foreign: not configured on target account",
            StateForeignError::NotConfiguredOnTarget
        ),
        _ => rollback!(
            b"state-foreign: state_foreign read failed",
            StateForeignError::ReadFailed
        ),
    }

    if flag[0] == 0 {
        rollback!(
            b"state-foreign: target account's flag is off",
            StateForeignError::FlagOff
        );
    }

    accept!()
}
