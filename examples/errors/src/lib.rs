//! `errors` — demonstrates a meaningful, hook-defined rollback error-code
//! system: a small `enum` of rejection reasons, each with its own negative
//! `i64` code, matched to a distinct `rollback!` call. See the README for
//! the full code table and how it shows up as `HookExecution.HookReturnCode`
//! in transaction metadata.
//!
//! Build: `hooks-build build --manifest-path examples/errors/Cargo.toml`

#![no_std]

use hooks_lib::prelude::*;
use hooks_lib::{accept, rollback};

/// Reject a transaction whose `SourceTag` is this value — a stand-in for
/// "known-bad" tag used by some other system integrating with this hook.
const BLOCKED_SOURCE_TAG: u32 = 13;

/// Reject a native-amount transaction moving more than this many drops (100
/// XAH) — a stand-in for a simple spend-limit policy.
const MAX_DROPS: u64 = 100_000_000;

/// Bits reserved by the native-Amount serialization format (see
/// `hook-params`' README for the full explanation); masking them off
/// recovers the plain drops magnitude.
const NATIVE_AMOUNT_FLAG_BITS: u64 = 0xC000_0000_0000_0000;

/// This hook's rejection reasons, each carrying its own stable negative
/// code. Deliberately chosen well outside the documented Hook API error
/// range (`-1..=-45`, plus `-10024`) so a `HookReturnCode` of, say, `-101`
/// is unambiguously *this hook's* policy, not a Hook API failure — see the
/// README's code table.
#[derive(Clone, Copy)]
enum RejectReason {
    /// Couldn't read the originating transaction's `Account` field at all
    /// (should not happen for a well-formed transaction; defensive).
    BadAccountField,
    /// `SourceTag` matches [`BLOCKED_SOURCE_TAG`].
    BlockedSourceTag,
    /// `Amount` isn't a native (XRP/XAH) amount — this hook's policy only
    /// understands native amounts (see `examples/xfl-math` for handling IOU
    /// amounts too).
    NotNativeAmount,
    /// Native `Amount` exceeds [`MAX_DROPS`].
    AmountTooLarge,
}

impl RejectReason {
    /// The stable negative code reported as this transaction's
    /// `HookReturnCode`.
    fn code(self) -> i64 {
        match self {
            RejectReason::BadAccountField => -101,
            RejectReason::BlockedSourceTag => -102,
            RejectReason::NotNativeAmount => -103,
            RejectReason::AmountTooLarge => -104,
        }
    }

    /// A short, fixed diagnostic message for this reason (the `rollback`
    /// message, not the code — see the Hook API's `rollback(msg, code)`).
    fn message(self) -> &'static [u8] {
        match self {
            RejectReason::BadAccountField => b"errors: could not read otxn Account",
            RejectReason::BlockedSourceTag => b"errors: blocked SourceTag",
            RejectReason::NotNativeAmount => b"errors: unsupported (non-native) Amount",
            RejectReason::AmountTooLarge => b"errors: amount exceeds policy limit",
        }
    }

    /// Roll the hook back with this reason's message and code. Never
    /// returns (see [`hooks_lib::api::control::rollback`]).
    fn rollback(self) -> ! {
        rollback!(self.message(), self.code())
    }
}

/// Hook entry point. Runs a small chain of policy checks, rolling back with
/// a distinct, documented code the first time one fails; accepts if every
/// check passes.
#[unsafe(no_mangle)]
pub extern "C" fn hook(_reserved: u32) -> i64 {
    let mut sender: AccountId = [0u8; ACC_ID_LEN];
    match otxn_field(&mut sender, sfAccount) {
        Ok(n) if n == ACC_ID_LEN => {}
        _ => RejectReason::BadAccountField.rollback(),
    }

    // `SourceTag` is an optional transaction field: `DOESNT_EXIST` means
    // "no tag was set", which is not a policy violation, so only an exact
    // match against the blocked value rolls back.
    match otxn_field_u64(sfSourceTag) {
        Ok(tag) if tag == u64::from(BLOCKED_SOURCE_TAG) => {
            RejectReason::BlockedSourceTag.rollback()
        }
        _ => {}
    }

    let mut amount_raw = [0u8; 8];
    let drops = match otxn_field(&mut amount_raw, sfAmount) {
        Ok(n) if n == amount_raw.len() => {
            u64::from_be_bytes(amount_raw) & !NATIVE_AMOUNT_FLAG_BITS
        }
        _ => RejectReason::NotNativeAmount.rollback(),
    };

    if drops > MAX_DROPS {
        RejectReason::AmountTooLarge.rollback();
    }

    accept!()
}
