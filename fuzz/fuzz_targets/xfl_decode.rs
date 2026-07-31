//! Fuzz target for `hooks_lib::xfl::XFL`'s non-host-dependent decoding path
//! (`docs/DESIGN.md` §5.3): `from_raw_bits` -> `mantissa()`/`exponent()` ->
//! reconstruction, for every possible raw `i64` bit pattern, including ones
//! that don't correspond to a valid XFL (bit 63 set, out-of-range exponent
//! field, etc.) — `XFL` never validates on construction (`from_raw_bits` is
//! documented as an unchecked escape hatch), so decoding must never panic no
//! matter what garbage bits it is given.
//!
//! `XFL::mantissa()`/`sign()`/`compare()`/`to_int()` go through
//! `hooks-core`'s host-call stubs, which on a non-`wasm32` host
//! deterministically return `NOT_IMPLEMENTED` (see
//! `hooks-core/src/api.rs`) regardless of input — so they cannot
//! meaningfully fail differently per input here, but calling them is still
//! useful panic-freedom coverage for the `Result`-wrapping plumbing
//! (`hooks_lib::error::res`) itself.
//!
//! `XFL::exponent()` is the one member that *is* computed locally from the
//! raw bits (no host call, per the module doc comment on
//! `crates/hooks-lib/src/xfl.rs`), so this is the property that actually
//! varies with the fuzzer's input and is checked against an independent
//! from-scratch recomputation below.
#![no_main]

use hooks_lib::xfl::XFL;
use libfuzzer_sys::fuzz_target;

/// Bit offset of the exponent field (mirrors `xfl.rs`'s private
/// `EXPONENT_SHIFT`, duplicated here since it's not part of the public API).
const EXPONENT_SHIFT: u32 = 54;
/// Mask for the 8-bit exponent field once shifted into place (mirrors
/// `xfl.rs`'s private `EXPONENT_MASK`).
const EXPONENT_MASK: i64 = 0xFF;
/// Bias applied to the stored exponent field (mirrors `xfl.rs`'s private
/// `EXPONENT_BIAS`).
const EXPONENT_BIAS: i64 = 97;

fuzz_target!(|bits: i64| {
    let xfl = XFL::from_raw_bits(bits);

    // Round trip: the raw bit pattern must survive verbatim through the
    // unchecked constructor/accessor pair, for every possible i64.
    assert_eq!(xfl.raw_bits(), bits);

    // `exponent()` must never panic, and must match an independent
    // recomputation of the same bias-97, 8-bit-field decode.
    let want = ((bits >> EXPONENT_SHIFT) & EXPONENT_MASK) - EXPONENT_BIAS;
    match xfl.exponent() {
        Ok(got) => assert_eq!(got, want, "exponent() decode mismatch for bits {bits:#x}"),
        Err(e) => panic!("exponent() must never fail (computed locally, no host call): {e:?}"),
    }

    // The remaining accessors all route through host-call stubs that are
    // `NOT_IMPLEMENTED` on this (non-wasm32) target for every input; the
    // property under test here is purely "does not panic".
    let _ = xfl.mantissa();
    let _ = xfl.sign();
    let _ = xfl.to_int(0, false);
    let _ = xfl.to_int(15, true);
    let _ = xfl.compare(xfl, 1);
    let _ = xfl.eq(xfl);
    let _ = xfl.lt(xfl);
    let _ = xfl.gt(xfl);
});
