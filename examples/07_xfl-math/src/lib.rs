//! `xfl-math` — reads the originating transaction's `Amount` (native or
//! IOU — XFL handles both uniformly) via a slot, computes a percentage of
//! it with `mulratio`, and rolls back if that computed share is below a
//! fixed minimum XFL value. Demonstrates the `Result`-based XFL API: every
//! `float_*` operation can fail (division by zero, mantissa/exponent
//! overflow, ...), and this hook handles each failure explicitly instead of
//! assuming success.
//!
//! Build: `hooks-build build --manifest-path examples/07_xfl-math/Cargo.toml`

#![no_std]

use hooks_lib::prelude::*;
use hooks_lib::{accept, hook, hook_errors, rollback};

/// The percentage this hook computes from the transaction `Amount`: 1%,
/// expressed as `mulratio`'s `(num, den)` ratio.
const PERCENT_NUM: u32 = 1;
const PERCENT_DEN: u32 = 100;

hook_errors! {
    /// `xfl-math` rollback codes.
    pub enum XflMathError {
        /// `otxn_slot` failed to load the originating transaction into a
        /// slot.
        OtxnSlotFailed = 1,
        /// `slot_subfield` found no `Amount` field on the originating
        /// transaction.
        NoAmountField = 2,
        /// `XFL::from_slot` could not decode the `Amount` slot as a valid
        /// XFL amount.
        InvalidAmount = 3,
        /// `mulratio` failed (e.g. `XflOverflow`) computing the percentage
        /// share.
        MulratioFailed = 4,
        /// `XFL::new` failed to construct the fixed minimum-share
        /// constant.
        MinShareConstructFailed = 5,
        /// The `.lt()` comparison between the computed share and the
        /// minimum failed.
        ComparisonFailed = 6,
        /// The computed share fell below the fixed minimum.
        BelowMinimum = 7,
    }
}

/// Hook entry point.
#[hook]
fn my_hook() -> i64 {
    // Load the originating transaction into a slot, then navigate to its
    // `Amount` field's own slot. `slot_subfield`'s `new_slot = 0` means
    // "auto-assign a slot number" — same convention as `otxn_slot`.
    let txn_slot = match otxn_slot(0) {
        Ok(s) => s,
        Err(_) => rollback!(b"xfl-math: otxn_slot failed", XflMathError::OtxnSlotFailed),
    };
    let amount_slot = match slot_subfield(txn_slot, sfAmount, 0) {
        Ok(s) => s,
        Err(_) => rollback!(
            b"xfl-math: no Amount field on otxn",
            XflMathError::NoAmountField
        ),
    };

    // `slot_float` (`XFL::from_slot`) decodes the object in a slot as an
    // XFL — it works the same way whether the underlying Amount is a
    // 48-byte IOU amount or an 8-byte native XRP/XAH amount, unlike reading
    // the raw bytes and interpreting the native-amount bit layout by hand
    // (see `hook-params`/`errors`, which only handle the native case).
    //
    // An equivalent, buffer-based route to the same XFL (shown here only
    // in comment form — pick one, not both, in real code) is
    // `otxn_field(&mut buf, sfAmount)` followed by `XFL::sto_set(&buf[..n])`;
    // `slot_float` is used below because this example is about slot
    // navigation as well as XFL.
    let amount = match XFL::from_slot(amount_slot) {
        Ok(x) => x,
        // `NotAnAmount` if the field somehow isn't an Amount at all;
        // anything else is an unexpected host-level failure. Either way,
        // there's nothing sensible left to compute, so roll back.
        Err(_) => rollback!(
            b"xfl-math: Amount is not a valid XFL amount",
            XflMathError::InvalidAmount
        ),
    };

    // `self * (num / den)`: 1% of the transaction amount, rounding down.
    let share = match amount.mulratio(false, PERCENT_NUM, PERCENT_DEN) {
        Ok(x) => x,
        // E.g. `XflOverflow` if the amount is large enough that the scaled
        // result can't be represented; `DivisionByZero` can't happen here
        // (PERCENT_DEN is a nonzero compile-time constant) but is still a
        // documented `float_mulratio` failure mode in general.
        Err(_) => rollback!(
            b"xfl-math: mulratio failed (overflow?)",
            XflMathError::MulratioFailed
        ),
    };

    // A fixed minimum share, `0.000001` (1e-6), below which this hook
    // considers the transaction not worth processing further. XFL's
    // mantissa is normalized to 16 significant digits (`10^15..=10^16-1`,
    // see `hooks_lib::xfl`'s module doc comment), so `1e-6` is
    // `1_000_000_000_000_000 * 10^-21` (mantissa `1e15`, exponent `-21`),
    // not `mantissa 1e15, exponent -6` (that would be `1e9`).
    let min_share = match XFL::new(-21, 1_000_000_000_000_000) {
        Ok(x) => x,
        Err(_) => rollback!(
            b"xfl-math: could not construct min_share",
            XflMathError::MinShareConstructFailed
        ),
    };

    // `XFL` has no `PartialOrd` (comparison is a fallible host call, see
    // `hooks_lib::xfl`'s module doc comment) — `.lt()` returns `Result<bool>`
    // and is handled explicitly, same as every other XFL operation here.
    match share.lt(min_share) {
        Ok(true) => rollback!(
            b"xfl-math: computed share below minimum",
            XflMathError::BelowMinimum
        ),
        Ok(false) => {}
        Err(_) => rollback!(
            b"xfl-math: comparison failed",
            XflMathError::ComparisonFailed
        ),
    }

    // Slots are a limited resource (see `docs/DESIGN.md` and the Slot API
    // reference); free them once this hook is done with them. Cleanup
    // failure isn't itself a reason to reject the transaction, so its
    // `Result` is deliberately discarded (not ignored silently — `let _ =`
    // makes that a visible, reviewed choice) rather than rolled back on.
    let _ = slot_clear(amount_slot);
    let _ = slot_clear(txn_slot);

    accept!()
}
