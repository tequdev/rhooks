//! `account-id-macro` — cross-checks `hooks_lib::account_id!`'s
//! **compile-time** r-address decode against three independent **runtime**
//! Hook API sources of truth: `hook_account`, `util_accid`, and
//! `util_raddr`.
//!
//! Build: `hooks-build build --manifest-path examples/14_account-id-macro/Cargo.toml`
//!
//! ## What this demonstrates
//!
//! `account_id!("rHb9CJAWyB4rj91VRWn96DkukG4bwdtyTh")` decodes that
//! r-address into an `AccountId` entirely inside the proc-macro, at compile
//! time — no base58/checksum logic ships in this hook's wasm binary at all,
//! and the constant below is exactly as cheap as hand-writing the 20-byte
//! array literal (see this crate's e2e test, which asserts the built wasm
//! is byte-identical either way). This hook proves that compile-time result
//! is actually correct by checking it three ways at runtime:
//!
//! 1. `hook_account()` — the account this hook is installed on — must equal
//!    the constant (true only when installed on the address's own account).
//! 2. `util_accid()` — the *host's own* runtime r-address -> AccountID
//!    conversion, given the exact same string — must agree with the
//!    macro's compile-time result.
//! 3. `util_raddr()` — converting the constant back to text — must
//!    round-trip to the original r-address string byte-for-byte.
//!
//! `rHb9CJAWyB4rj91VRWn96DkukG4bwdtyTh` is the Xahau/XRPL standalone-network
//! genesis/master account (seed `"masterpassphrase"`) — the same constant
//! `examples/80_reward`/`examples/81_govern` hand-hardcode as
//! `GENESIS_ACCOUNT`. The e2e test installs this hook on that exact account,
//! so check 1 above is meaningful (not vacuously true).

#![no_std]

use hooks_lib::prelude::*;
use hooks_lib::{accept, account_id, hook, hook_errors, rollback};

/// The r-address `OWNER` is decoded from — kept as a byte-string constant
/// so `util_raddr`'s round-tripped output can be compared against it
/// directly (see check 3 in [`my_hook`]).
const OWNER_RADDR: &[u8; 34] = b"rHb9CJAWyB4rj91VRWn96DkukG4bwdtyTh";

/// `account_id!`'s compile-time decode of [`OWNER_RADDR`] — zero runtime
/// cost: this expands to a plain `AccountId([u8; 20])` literal, so no
/// base58/checksum decode logic exists in the compiled wasm at all.
const OWNER: AccountId = account_id!("rHb9CJAWyB4rj91VRWn96DkukG4bwdtyTh");

/// Scratch buffer for `util_raddr`'s text output (check 3 in [`my_hook`]).
/// A `static`, not a stack local — see `examples/README.md`'s "Statics for
/// templates and large buffers": a 34-byte `[0u8; 34]` stack local
/// compiles to a compiler-generated zero-init loop at this optimization
/// level, while a zero-initialized `static` lands in linear-memory BSS
/// (zero bytes of data segment, zero code).
static RADDR_BUF: HookStatic<[u8; 34]> = HookStatic::new([0u8; 34]);

hook_errors! {
    /// `account-id-macro` rollback codes.
    pub enum AccountIdMacroError {
        /// `hook_account()` failed (host error).
        HookAccountFailed = 1,
        /// `hook_account()` succeeded, but the account this hook is
        /// installed on does not match `OWNER` — this hook was not
        /// installed on the address `OWNER_RADDR` names.
        HookAccountMismatch = 2,
        /// `util_accid` failed to convert `OWNER_RADDR` at runtime.
        UtilAccidFailed = 3,
        /// `util_accid`'s runtime conversion of `OWNER_RADDR` disagrees
        /// with `account_id!`'s compile-time decode of the same string.
        UtilAccidMismatch = 4,
        /// `util_raddr` failed to convert `OWNER` back to text at runtime.
        UtilRaddrFailed = 5,
        /// `util_raddr` wrote a different number of bytes than
        /// `OWNER_RADDR`'s length.
        UtilRaddrLenMismatch = 6,
        /// `util_raddr`'s output does not round-trip to `OWNER_RADDR`.
        UtilRaddrMismatch = 7,
        /// [`RADDR_BUF`] had already been `take()`n (never happens in this
        /// hook, which only takes it once per invocation).
        RaddrBufAlreadyTaken = 8,
    }
}

/// Hook entry point: see the crate doc comment for the three checks this
/// performs. Accepts only if all three agree with the `account_id!`
/// compile-time constant `OWNER`.
#[hook]
fn my_hook() -> i64 {
    // (1) hook_account(): must equal OWNER (true only when this hook is
    // installed on OWNER_RADDR's own account).
    let installed_on = match hook_account_buf() {
        Ok(a) => a,
        Err(_) => rollback!(
            b"account-id-macro: hook_account failed",
            AccountIdMacroError::HookAccountFailed
        ),
    };
    if !buf_eq_20(&installed_on, &OWNER) {
        rollback!(
            b"account-id-macro: hook_account does not match the account_id! constant",
            AccountIdMacroError::HookAccountMismatch
        );
    }

    // (2) util_accid(): the host's own runtime r-address -> AccountID
    // conversion of the exact same string must agree with account_id!'s
    // compile-time result.
    let runtime_accid = match util_accid_buf(OWNER_RADDR) {
        Ok(a) => a,
        Err(_) => rollback!(
            b"account-id-macro: util_accid failed",
            AccountIdMacroError::UtilAccidFailed
        ),
    };
    if !buf_eq_20(&runtime_accid, &OWNER) {
        rollback!(
            b"account-id-macro: util_accid does not match the account_id! constant",
            AccountIdMacroError::UtilAccidMismatch
        );
    }

    // (3) util_raddr(): converting OWNER back to text must round-trip to
    // the exact original r-address string.
    let Some(raddr_buf) = RADDR_BUF.take() else {
        rollback!(
            b"account-id-macro: RADDR_BUF already taken",
            AccountIdMacroError::RaddrBufAlreadyTaken
        );
    };
    let n = match util_raddr(raddr_buf, OWNER.as_ref()) {
        Ok(n) => n,
        Err(_) => rollback!(
            b"account-id-macro: util_raddr failed",
            AccountIdMacroError::UtilRaddrFailed
        ),
    };
    if n != OWNER_RADDR.len() {
        rollback!(
            b"account-id-macro: util_raddr wrote an unexpected length",
            AccountIdMacroError::UtilRaddrLenMismatch
        );
    }
    if !buf_eq_34(raddr_buf, OWNER_RADDR) {
        rollback!(
            b"account-id-macro: util_raddr does not round-trip to OWNER_RADDR",
            AccountIdMacroError::UtilRaddrMismatch
        );
    }

    accept!(b"account-id-macro: all three checks passed", 0)
}
