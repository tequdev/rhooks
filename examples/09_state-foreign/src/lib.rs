//! `state-foreign` — gates acceptance on a single configuration flag stored
//! in *another* account's Hook state, read via `state_foreign`. The
//! account to read from is itself configurable via a Hook parameter (`ACCT`),
//! following the same "config via hook_param" idiom as `hook-params`.
//!
//! Build: `hooks-build build --manifest-path examples/09_state-foreign/Cargo.toml`

#![no_std]

use hooks_lib::prelude::*;
use hooks_lib::{accept, hook, hook_errors, hook_parameter, pad, rollback};

// The Hook parameter carrying the 20-byte `AccountId` this hook reads its
// gate flag from (name `"ACCT"`). Migrated to `hook_parameter!`'s Form 1
// — `hook_param_exact(ACCT_PARAM)` this replaces was already an exact-
// length read, so this changes nothing observable (same argument as
// `examples/05_firewall`'s `BlockedParamName`).
hook_parameter!(AcctParamName = b"ACCT" => AccountId);

/// The state key this hook looks for on the foreign account: the name
/// `b"enabled"`, zero-padded at compile time to the full 32-byte state-key
/// width (see `state-counter`'s use of `pad!` for the same trick — no
/// runtime copy loop, hence no loop guard, is needed for it).
///
/// Deliberately **not** migrated to `hook_state!` — see `my_hook`'s doc
/// comment for the full argument (in short: a `hook_state!` Form 1 short
/// key would send the real 7 bytes and rely on the *host's* left-padding,
/// which is byte-incompatible with this constant's own right-padding via
/// `pad!`, and the `state_foreign` read below has its own reason to stay
/// raw independent of the key).
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
///
/// The `state_foreign` read below is deliberately **not** migrated to
/// `state_foreign_get_typed`, for two independent reasons:
///
/// - **Semantic mismatch.** `state_foreign_get_typed::<u8>`'s decode goes
///   through `u8::read`, which reads a *prefix* (`buf.get(..1)`) of
///   whatever the state entry's actual byte length turns out to be — it
///   would silently accept and decode just the first byte of an
///   `enabled` entry that happened to be longer than 1 byte. The raw
///   buffer-mode read here instead requires an *exact* 1-byte length
///   (`Ok(n) if n == flag.len()`), rejecting anything else via the same
///   `ReadFailed` catch-all a genuine host error hits. Since this hook
///   has no control over what the *foreign* account's state actually
///   contains (see the e2e suite's own comment: no example hook writes
///   this key), silently tolerating an oversized value would be a real,
///   observable behavior change for a state layout this crate doesn't
///   own — state's typed layer is built for lenient prefix decoding
///   (see `hooks_lib::convert::FromBytes`), not the exact-length
///   contract `hook_param_exact`'s `FixedRead` gives params.
/// - **Key byte-compatibility.** Even setting the above aside,
///   `ENABLED_KEY` itself can't move to `hook_state!`'s short-key Form 1
///   (`= b"enabled" => u8`) without changing the actual wire bytes: that
///   form sends the real 7 bytes and relies on the *host's* own left-
///   padding (zeros leading, real bytes at the end — see
///   `hooks_lib::state`'s "Key length and padding" doc comment), which is
///   the *opposite* byte layout from `pad!`'s right-padding (real bytes
///   at the start, zeros trailing — see `pad!`'s own doc comment). Using
///   the short-key form here would silently read/write a completely
///   different 32-byte state slot than this crate's README documents.
#[hook]
fn my_hook() -> i64 {
    let target: AccountId = match hook_param_typed(&AcctParamName) {
        Ok(t) => t,
        Err(_) => rollback!(
            b"state-foreign: ACCT parameter not configured",
            StateForeignError::AcctNotConfigured
        ),
    };

    let mut flag = [0u8; 1];
    match state_foreign(&mut flag, &ENABLED_KEY, None, &target) {
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
