//! `xfl-math` — reads the originating transaction's `Amount` (native or
//! IOU — XFL handles both uniformly) via a slot, computes a percentage of
//! it with `mulratio`, and rolls back if that computed share is below a
//! fixed minimum XFL value. Also demonstrates hooks-lib's XFL operator API
//! end to end: the checked `Sub`/`Neg` operators, the `.compare()`-family
//! methods, the `==`/`<`/`>` (`PartialEq`/`PartialOrd`) operators, and
//! `XFLUnchecked`'s poison-propagating chain. Every fallible step's
//! `Result` is still handled explicitly instead of assuming success —
//! comparison and `Sub`/`Neg` are all fallible host round trips here, same
//! as `Add`/`Mul`/`Div`/`mulratio`; the one exception is the `==`/`<`/`>`
//! demonstration near the end, which is deliberately used only where a
//! `float_compare` failure is not practically reachable (see the in-source
//! comment there, and this crate's README).
//!
//! Build: `hooks-build build --manifest-path examples/07_xfl-math/Cargo.toml`

#![no_std]

use hooks_lib::prelude::*;
// The numbered slot functions left the prelude when the typed `SlotObject`
// layer arrived (they address the same 255 registers, and mixing the two
// silently corrupts handles). This hook uses them directly and by number, so
// it names the module explicitly — see `hooks_lib::slot_obj`'s module doc.
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
        /// `amount - share` (the checked `Sub` operator) failed.
        RemainingComputeFailed = 8,
        /// The `remaining <= 0` comparison (`.compare()`) failed.
        RemainingComparisonFailed = 9,
        /// `amount - share` computed but was not strictly positive — would
        /// mean `share >= amount`, which `mulratio(1, 100)` should never
        /// produce for a positive `amount`.
        NotEnoughRemaining = 10,
        /// `XFL::new` failed to construct the fixed growth-factor constant
        /// used by the `XFLUnchecked` compounding demonstration below.
        GrowthConstructFailed = 11,
        /// The `XFLUnchecked` compounding chain's final `validate()` call
        /// failed.
        CompoundValidationFailed = 12,
        /// The `compounded <= share` comparison (`.compare()`) failed.
        CompoundComparisonFailed = 13,
        /// The compounded value (computed at a `>1` growth factor) did not
        /// come out strictly greater than the starting `share` — would
        /// mean something is wrong with the compounding chain above.
        CompoundNotIncreasing = 14,
        /// `compounded > remaining` (the `>` operator, `PartialOrd`) — would
        /// mean the compounded projection somehow exceeds the transaction
        /// amount minus its own share, which shouldn't happen for any
        /// realistic `Amount`.
        CompoundExceedsRemaining = 15,
    }
}

/// Hook entry point.
#[hook]
// Every `+`/`-`/`*` below dispatches to `hooks_lib::xfl`'s/
// `hooks_lib::xfl_unchecked`'s `XFL`/`XFLUnchecked` operator impls (all
// fallible or infallible host round trips), never raw integer arithmetic
// that could silently wrap. `clippy::arithmetic_side_effects` can't see
// past the operator syntax to tell the difference, so it flags every use
// of `+`/`-`/`*` in this function unconditionally; there is nothing here
// for it to actually warn about.
#[allow(clippy::arithmetic_side_effects)]
fn my_hook() -> i64 {
    // Load the originating transaction into a slot, then navigate to its
    // `Amount` field. No slot numbers: the host auto-assigns them and the
    // handles carry them. `sfAmount` is an `SField<Amount>`, so `.get()`
    // hands back a `SlotObject<Amount>` — which is the type `as_xfl()`
    // lives on.
    let txn = match SlotObject::from_otxn() {
        Ok(s) => s,
        Err(_) => rollback!(b"xfl-math: otxn_slot failed", XflMathError::OtxnSlotFailed),
    };
    let amount_slot = match txn.get(sfAmount) {
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
    let amount = match amount_slot.as_xfl() {
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
    // No operator equivalent — `mulratio` takes two extra scale
    // parameters beyond `self`/`rhs`, so it stays a named method.
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

    // `XFL` has no `PartialOrd` (comparison is a fallible `float_compare`
    // host round trip that `PartialOrd`'s fixed `Option<Ordering>` return
    // can't express — see `hooks_lib::xfl`'s module doc comment) — `.lt()`
    // returns `Result<bool>` and is handled explicitly, same as every
    // other XFL operation here.
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

    // --- Checked operators: `Sub` ----------------------------------------
    // `amount - share`: what would remain of the transaction amount after
    // setting the computed share aside. `Sub`'s `Output` is
    // `Result<XFL, HookError>` (implemented as `amount + (-share)?`: one
    // `float_negate` host call plus one `float_sum` host call — see
    // `hooks_lib::xfl`'s module doc comment), matched explicitly like
    // every other fallible step here.
    let remaining = match amount - share {
        Ok(x) => x,
        Err(_) => rollback!(
            b"xfl-math: amount - share failed",
            XflMathError::RemainingComputeFailed
        ),
    };
    // Canonical zero needs no host call to construct — the all-zero bit
    // pattern is always valid (see `hooks_lib::xfl`'s module doc comment).
    // `COMPARE_LESS | COMPARE_EQUAL` is `.compare()`'s bitmask spelling of
    // `<=` (no dedicated `le`/`ge` convenience methods — `eq`/`lt`/`gt`
    // cover the common cases, `.compare()` covers the rest).
    match remaining.compare(XFL::from_raw_bits(0), COMPARE_LESS | COMPARE_EQUAL) {
        Ok(true) => rollback!(
            b"xfl-math: amount - share was not positive",
            XflMathError::NotEnoughRemaining
        ),
        Ok(false) => {}
        Err(_) => rollback!(
            b"xfl-math: remaining comparison failed",
            XflMathError::RemainingComparisonFailed
        ),
    }

    // --- XFLUnchecked: a hot-path compounding chain -----------------------
    // Purely illustrative (a real 3-multiply chain is nowhere near where
    // per-step `Result` handling would be the measured bottleneck worth
    // optimizing away — see `hooks_lib::xfl_unchecked`'s module doc
    // comment) — compounds `share` at a fixed 1.01x growth factor across 3
    // periods with **no** intermediate `Result` handling, then validates
    // once at the end.
    let growth = match XFL::new(-15, 1_010_000_000_000_000) {
        Ok(x) => x, // 1.01
        Err(_) => rollback!(
            b"xfl-math: could not construct growth factor",
            XflMathError::GrowthConstructFailed
        ),
    };
    let compounded_raw =
        share.unchecked() * growth.unchecked() * growth.unchecked() * growth.unchecked();
    let compounded = match compounded_raw.validate() {
        Ok(x) => x,
        Err(_) => rollback!(
            b"xfl-math: compounded share failed to validate",
            XflMathError::CompoundValidationFailed
        ),
    };
    // Another `.compare()` sanity check: compounding at a `>1` growth
    // factor must strictly increase the value.
    match compounded.compare(share, COMPARE_LESS | COMPARE_EQUAL) {
        Ok(true) => rollback!(
            b"xfl-math: compounded share did not increase",
            XflMathError::CompoundNotIncreasing
        ),
        Ok(false) => {}
        Err(_) => rollback!(
            b"xfl-math: compound comparison failed",
            XflMathError::CompoundComparisonFailed
        ),
    }

    // --- Operator-based comparison: `==`/`<`/`>` (`PartialEq`/`PartialOrd`) --
    // `compounded` and `remaining` are both already-validated `XFL` values
    // at this point — each only exists because the fallible step that
    // produced it (`compounded_raw.validate()`, `amount - share`) already
    // returned `Ok`. `XFL`'s `PartialEq`/`PartialOrd` fall back to
    // `false`/`None` on a `float_compare` failure rather than propagating
    // one (see `hooks_lib::xfl`'s module doc comment) — a real risk for a
    // comparison whose operands might be unvalidated, but not here: there
    // is no path from two already-`Ok`, host-validated XFLs to a
    // `float_compare` failure in practice. That's exactly the situation
    // this crate's README calls out as reasonable for `==`/`<`/`>` instead
    // of the `Result`-returning methods used everywhere else in this hook
    // (compare the `.lt()`/`.compare()` call sites above, all of which
    // compare a value that has *not* yet been separately validated).
    // `compounded` (~1.0303% of `amount`) should always be far smaller
    // than `remaining` (~99% of `amount`) — `>` here is a sanity check
    // that would only trip on a logic bug above, same spirit as the
    // `CompoundNotIncreasing` check.
    if compounded > remaining {
        rollback!(
            b"xfl-math: compounded share unexpectedly exceeds remaining amount",
            XflMathError::CompoundExceedsRemaining
        );
    }

    // `as_xfl()` above consumed the `Amount` handle without clearing its
    // slot — the C cost model, and the right default for a hook that reads
    // once and exits, since the host frees every slot at the end anyway
    // (`take_xfl()` is the clearing form, for loops). The transaction
    // handle is the one left; releasing it is optional for the same reason,
    // and is kept here only because this example previously did.
    //
    // Cleanup failure isn't a reason to reject the transaction, so the
    // `Result` is deliberately discarded — `let _ =` makes that a visible,
    // reviewed choice rather than a silent one.
    let _ = txn.clear();

    accept!()
}
