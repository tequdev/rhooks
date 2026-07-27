//! Raw `float_*`/`slot_float` calls — the *only* Hook API surface this
//! crate bypasses `hooks_lib`'s ordinary `Result`-based wrappers for.
//!
//! Every other Hook API call in this crate goes through `hooks_lib::api`'s
//! (including its `_exact`/`_buf` convenience) wrappers directly (see
//! `lib.rs`). XFL is different on purpose: reward.c's own reward-rate math
//! treats every `float_*` result as a raw `i64` and folds host-level
//! failure into the *same* validity checks it already needs for
//! legitimately out-of-range values (`xfl_rr <= 0`, `required_delay < 0`,
//! ...) — it never asks "did this specific call fail," only "is the
//! resulting number usable." `hooks_lib::xfl::XFL` intentionally does the
//! opposite: every method returns `Result<XFL, HookError>` precisely so a
//! caller can't accidentally treat a raw error code as a value. That's the
//! right default for hook code in general, but it is extra validation this
//! particular call chain neither wants nor uses — reproducing reward.c's
//! math exactly means working with the same raw bit patterns it does at
//! every step, not converting to `XFL` and back for each intermediate
//! `float_divide`/`float_multiply`/`float_int` result.
//!
//! (This also sidesteps a real but narrower toolchain cost:
//! `hooks_lib::error::res`'s `HookError::from(i64)` decode, invoked on
//! every negative return `XFL`'s methods produce, does add measurable wasm
//! size/nesting per call — see `examples/81_govern/src/lib.rs`'s module
//! doc comment for the fuller writeup of where that did and didn't end up
//! mattering for these two crates. It is not, on its own, why this module
//! exists; the validation semantics above are.)

use hooks_lib::raw as hooks_core;

/// `float_set(exponent, mantissa)`, raw.
pub fn float_set(exponent: i32, mantissa: i64) -> i64 {
    unsafe { hooks_core::float_set(exponent, mantissa) }
}

/// `float_divide(a, b)`, raw.
pub fn float_divide(a: i64, b: i64) -> i64 {
    unsafe { hooks_core::float_divide(a, b) }
}

/// `float_multiply(a, b)`, raw.
pub fn float_multiply(a: i64, b: i64) -> i64 {
    unsafe { hooks_core::float_multiply(a, b) }
}

/// `float_int(x, decimal_places, abs)`, raw.
pub fn float_int(x: i64, decimal_places: u32, abs: u32) -> i64 {
    unsafe { hooks_core::float_int(x, decimal_places, abs) }
}

/// `float_sign(x)`, raw.
pub fn float_sign(x: i64) -> i64 {
    unsafe { hooks_core::float_sign(x) }
}

/// `float_one()`, raw.
pub fn float_one() -> i64 {
    unsafe { hooks_core::float_one() }
}

/// `float_compare(a, b, mode)`, raw.
pub fn float_compare(a: i64, b: i64, mode: u32) -> i64 {
    unsafe { hooks_core::float_compare(a, b, mode) }
}

/// `slot_float(slot_no)`, raw — reward.c's `int64_t xfl_fee =
/// slot_float(11);`, used unchecked exactly like every other `float_*`
/// result here (see the module doc comment).
pub fn slot_float(slot_no: u32) -> i64 {
    unsafe { hooks_core::slot_float(slot_no) }
}
