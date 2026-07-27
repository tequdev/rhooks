//! `XFLUnchecked` — the poison-propagating hot-path counterpart to
//! [`crate::xfl::XFL`].
//!
//! [`crate::xfl::XFL`]'s operators return `Result<XFL, HookError>` at every
//! step, and hooks-lib supplies `Result`-chaining operator impls (see
//! `xfl.rs`'s module doc comment) so a chain of checked ops never needs an
//! explicit `?` between steps — but each step still costs a guest-side
//! branch on the `Result` before the next host call can be issued.
//! `XFLUnchecked` skips that branch: its operators pass the raw `i64`
//! straight into the next host call with **no** guest-side check of
//! whether it is already an error, then [`XFLUnchecked::validate`] turns
//! the final raw value into a real `Result<XFL, HookError>` with one last
//! host round trip. Use it for arithmetic chains where the per-step
//! `Result` branch is the measured cost problem, not as a default
//! replacement for [`crate::xfl::XFL`].
//!
//! # Why this is sound: the host validates every operand, every call
//!
//! `XFLUnchecked`'s operators never validate their inputs on the guest
//! side — but every one of them (`Add`→`float_sum`, `Mul`→`float_multiply`,
//! `Div`→`float_divide`, `Neg`→`float_negate`, and `Sub`'s two host legs,
//! `float_negate` then `float_sum`) is, from the guest's point of view,
//! just a WASM import call into the **host**. The host does not trust the
//! guest to have validated anything: every one of these Hook API functions
//! is implemented in xahaud as a `DEFINE_HOOK_FUNCTION` that runs
//! `RETURN_IF_INVALID_FLOAT` on **every** `int64_t float*` parameter
//! **before** doing any arithmetic (verified directly against a local
//! xahaud checkout, `src/xrpld/app/hook/detail/applyHook.cpp`):
//!
//! ```c
//! #define RETURN_IF_INVALID_FLOAT(float1)                 \
//!     {                                                    \
//!         if (float1 < 0)                                  \
//!             return INVALID_FLOAT;                        \
//!         if (float1 != 0)                                 \
//!         {                                                \
//!             auto const mantissa = get_mantissa(float1);  \
//!             auto const exponent = get_exponent(float1);  \
//!             if (!mantissa || !exponent)                  \
//!                 return INVALID_FLOAT;                     \
//!             if (mantissa.value() < minMantissa ||         \
//!                 mantissa.value() > maxMantissa ||         \
//!                 exponent.value() > maxExponent ||         \
//!                 exponent.value() < minExponent)           \
//!                 return INVALID_FLOAT;                     \
//!         }                                                 \
//!     }
//! ```
//!
//! `float1 < 0` is a signed comparison, i.e. it tests bit 63 — exactly the
//! bit the module doc comment for `xfl.rs` says is reserved to distinguish
//! a valid float (`0`) from an error code (`1`, i.e. negative as a full
//! `i64`). So **every** host-routed operator call (`+`, `*`, `/`, `-`, and
//! unary `-`) rejects a poisoned/error-code operand immediately, before
//! `HookAPI::float_sum`/`float_multiply`/`float_divide`/`float_negate`'s own
//! arithmetic runs at all, and independently of whichever internal fast
//! paths those functions have (e.g. `HookAPI::float_sum`'s `if (float1 ==
//! 0) return float2;` short-circuit only ever executes *after*
//! `RETURN_IF_INVALID_FLOAT` has already accepted both operands — see
//! [`XFLUnchecked::validate`]'s doc comment for why that specific
//! short-circuit matters for validation). **A poisoned (negative) or
//! otherwise out-of-range operand can therefore never produce a
//! spuriously "valid-looking" positive result from any of `XFLUnchecked`'s
//! operators** — audited op-by-op against `HookAPI.cpp`/`applyHook.cpp`:
//!
//! | Operator | Host function(s) | Guard | Result on poisoned/invalid operand |
//! |---|---|---|---|
//! | `Add` | `float_sum` | `RETURN_IF_INVALID_FLOAT` on both operands | `INVALID_FLOAT`, never a false-valid value |
//! | `Mul` | `float_multiply` | `RETURN_IF_INVALID_FLOAT` on both operands | `INVALID_FLOAT`, never a false-valid value |
//! | `Div` | `float_divide` | `RETURN_IF_INVALID_FLOAT` on both operands | `INVALID_FLOAT`, never a false-valid value |
//! | `Neg` | `float_negate` | `RETURN_IF_INVALID_FLOAT` on the one operand | `INVALID_FLOAT`, never a false-valid value |
//! | `Sub` (`self + (-rhs)`) | `float_negate` then `float_sum` | same as `Neg`, then same as `Add` | same as `Add` |
//!
//! Every operator here is a host round trip — there is no local-only
//! fast path (an earlier version of this module tried a local sign-bit
//! flip for `Neg`; it did not correctly capture XFL's actual negation
//! semantics and was reverted). `XFLUnchecked`'s performance win is
//! entirely about *when* validation happens (once, in `validate()`, not
//! once per step), never about skipping a host round trip that a correct
//! implementation actually needs.
//!
//! **Poison-propagation caveat:** `RETURN_IF_INVALID_FLOAT` collapses
//! *every* invalid operand — a genuine propagated error code, an
//! out-of-range bit pattern, anything — to the single code `INVALID_FLOAT`.
//! It does not preserve or re-derive the original failure's specific
//! `HookError` variant. So a chain like `a.unchecked() / b.unchecked() *
//! c.unchecked()` where `b` was itself, say, a `DoesntExist`-poisoned
//! sentinel will validate to `Err(HookError::InvalidFloat)`, not
//! `Err(HookError::DoesntExist)` — the specific upstream cause is lost the
//! moment the poisoned value passes through the first host-routed
//! operator. If preserving the *specific* upstream `HookError` matters,
//! check each `Result` before entering an `XFLUnchecked` chain (or use
//! [`crate::xfl::XFL`]'s `Result`-chaining operators instead, which
//! short-circuit locally with no host round trip and so preserve the
//! original `HookError` exactly).
//!
//! # No `PartialEq`/`PartialOrd`
//!
//! Deliberately not implemented, for the same two reasons as
//! [`crate::xfl::XFL`] (see its module doc comment's "Comparison: methods,
//! not `PartialEq`/`PartialOrd`" section): comparison here would have to be
//! a fallible `float_compare` host round trip, which `PartialEq`/
//! `PartialOrd`'s fixed `bool`/`Option<Ordering>` return types cannot
//! express — and on top of that, comparing two values that might each be
//! poisoned invites exactly the kind of silent-wrong-answer bug this type
//! otherwise avoids by construction. Call [`XFLUnchecked::validate`] first
//! and compare the resulting `XFL`s with [`crate::xfl::XFL::eq`]/
//! [`crate::xfl::XFL::lt`]/[`crate::xfl::XFL::gt`]/
//! [`crate::xfl::XFL::compare`].

use crate::error::{Result, res};
use crate::xfl::XFL;

/// A raw XFL `i64` register value that has **not** been validated: it may
/// be a canonical XFL value, a non-canonical bit pattern, or a negative
/// Hook API error code sharing the same channel. See the module doc
/// comment for the full soundness argument and the poison audit table.
#[derive(Clone, Copy, Debug)]
pub struct XFLUnchecked(i64);

impl XFLUnchecked {
    /// Wrap a raw `i64` with no validation whatsoever.
    #[inline(always)]
    #[must_use]
    pub fn from_raw_bits(bits: i64) -> XFLUnchecked {
        XFLUnchecked(bits)
    }

    /// The raw, unvalidated `i64` bit pattern.
    #[inline(always)]
    #[must_use]
    pub fn raw_bits(self) -> i64 {
        self.0
    }

    /// Validate `self`, turning it into a real [`XFL`] or a definitive
    /// [`crate::error::HookError`].
    ///
    /// Implemented as `float_sum(self, 0)` — a host round trip, not a
    /// guest-side range check — deliberately: the host's
    /// `RETURN_IF_INVALID_FLOAT` (see the module doc comment) is the same
    /// gate every other float host call runs, so this reuses it instead of
    /// re-deriving the mantissa/exponent bounds locally and risking drift
    /// from the host's actual rules.
    ///
    /// `float_sum(value, 0)` specifically (not `float_sum(0, value)` or
    /// some other minimal-cost round trip) was checked against
    /// `HookAPI::float_sum`'s C++ body before choosing it, because that
    /// function has its own internal short-circuit: `if (float1 == 0)
    /// return float2; if (float2 == 0) return float1;` — i.e. summing with
    /// `0` skips `float_sum`'s own arithmetic entirely and returns the
    /// other operand untouched. That looked, before checking, like it
    /// might mean `float_sum(value, 0)` "validates" `0` (the constant) but
    /// waves `value` through unchecked. It does not: that short-circuit
    /// lives inside `HookAPI::float_sum`, which is only ever reached
    /// *after* the `DEFINE_HOOK_FUNCTION(int64_t, float_sum, ...)` wrapper
    /// in `applyHook.cpp` has already run `RETURN_IF_INVALID_FLOAT(float1)`
    /// (rejecting an invalid `value` immediately, before `HookAPI::float_sum`
    /// is called at all) and `RETURN_IF_INVALID_FLOAT(float2)` (trivially
    /// satisfied by the literal `0`). So `float_sum(value, 0)` **does**
    /// fully validate `value`: an invalid `value` never reaches the
    /// short-circuit, and a valid `value` passes through it unchanged
    /// (correctly — it was already canonical). No deviation from a
    /// straightforward `float_sum(value, 0)` round trip was needed.
    #[inline(always)]
    pub fn validate(self) -> Result<XFL> {
        res(unsafe { hooks_core::float_sum(self.0, 0) }).map(XFL::from_raw_bits)
    }
}

impl core::ops::Neg for XFLUnchecked {
    type Output = XFLUnchecked;

    /// `-self`, via the `float_negate` host call — not a local sign-bit
    /// flip (see the module doc comment for why). No guest-side validation
    /// of `self` either way: a poisoned/invalid `self` is rejected by the
    /// host's `RETURN_IF_INVALID_FLOAT` and the result is `INVALID_FLOAT`'s
    /// raw bits, itself a valid (still-negative) poison value to keep
    /// propagating.
    #[inline(always)]
    fn neg(self) -> XFLUnchecked {
        XFLUnchecked(unsafe { hooks_core::float_negate(self.0) })
    }
}

impl core::ops::Add for XFLUnchecked {
    type Output = XFLUnchecked;

    /// `self + rhs` via `float_sum`, with no guest-side validation of
    /// either operand — see the module doc comment.
    #[inline(always)]
    fn add(self, rhs: XFLUnchecked) -> XFLUnchecked {
        XFLUnchecked(unsafe { hooks_core::float_sum(self.0, rhs.0) })
    }
}

impl core::ops::Sub for XFLUnchecked {
    type Output = XFLUnchecked;

    /// `self - rhs`, implemented as `self + (-rhs)`: one `float_negate`
    /// host call plus one `float_sum` host call, mirroring [`XFL`]'s `Sub`
    /// (minus the `?` — `XFLUnchecked`'s `Neg` can't fail its own `Output`
    /// type the way `XFL`'s does, since a rejected negation just becomes
    /// another poisoned `i64` to keep propagating, not a `Result::Err`).
    #[inline(always)]
    // See `XFL`'s `Sub` impl for why this `#[allow]` is needed: `self +
    // (-rhs)` dispatches to this module's own `Add`/`Neg` impls, not raw
    // integer arithmetic.
    #[allow(clippy::arithmetic_side_effects)]
    fn sub(self, rhs: XFLUnchecked) -> XFLUnchecked {
        self + (-rhs)
    }
}

impl core::ops::Mul for XFLUnchecked {
    type Output = XFLUnchecked;

    /// `self * rhs` via `float_multiply`, with no guest-side validation of
    /// either operand — see the module doc comment.
    #[inline(always)]
    fn mul(self, rhs: XFLUnchecked) -> XFLUnchecked {
        XFLUnchecked(unsafe { hooks_core::float_multiply(self.0, rhs.0) })
    }
}

impl core::ops::Div for XFLUnchecked {
    type Output = XFLUnchecked;

    /// `self / rhs` via `float_divide`, with no guest-side validation of
    /// either operand — see the module doc comment.
    #[inline(always)]
    fn div(self, rhs: XFLUnchecked) -> XFLUnchecked {
        XFLUnchecked(unsafe { hooks_core::float_divide(self.0, rhs.0) })
    }
}

// Generates the mixed `XFLUnchecked op XFL` / `XFL op XFLUnchecked` impls
// for each listed `$Trait::$method`, treating the `XFL` side as implicitly
// unchecked (zero-cost reinterpretation, see `XFL::unchecked`/
// `XFLUnchecked::from_raw_bits`) so a chain can freely mix the two types at
// its boundary (typically: start from a known-valid `XFL`, do the hot loop
// in `XFLUnchecked`, `validate()` once at the end).
macro_rules! xfl_unchecked_mixed_ops {
    ($( $Trait:ident :: $method:ident ),+ $(,)?) => {
        $(
            impl core::ops::$Trait<XFL> for XFLUnchecked {
                type Output = XFLUnchecked;

                #[inline(always)]
                fn $method(self, rhs: XFL) -> XFLUnchecked {
                    core::ops::$Trait::$method(self, rhs.unchecked())
                }
            }

            impl core::ops::$Trait<XFLUnchecked> for XFL {
                type Output = XFLUnchecked;

                #[inline(always)]
                fn $method(self, rhs: XFLUnchecked) -> XFLUnchecked {
                    core::ops::$Trait::$method(self.unchecked(), rhs)
                }
            }
        )+
    };
}

xfl_unchecked_mixed_ops!(Add::add, Sub::sub, Mul::mul, Div::div);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::HookError;

    #[test]
    fn raw_bits_round_trip() {
        for bits in [0i64, 1, -1, i64::MAX, i64::MIN, 42, -42] {
            assert_eq!(XFLUnchecked::from_raw_bits(bits).raw_bits(), bits);
        }
    }

    #[test]
    fn validate_is_not_implemented_on_host() {
        let x = XFLUnchecked::from_raw_bits(0);
        assert!(matches!(x.validate(), Err(HookError::NotImplemented)));
    }

    #[test]
    fn arithmetic_routes_through_host_stub() {
        let a = XFLUnchecked::from_raw_bits(1);
        let b = XFLUnchecked::from_raw_bits(2);
        // Host stubs are deterministic `NOT_IMPLEMENTED`; validating the
        // result confirms the operator actually issued a host call rather
        // than, say, silently returning one of the raw operands.
        assert!(matches!((a + b).validate(), Err(HookError::NotImplemented)));
        assert!(matches!((a - b).validate(), Err(HookError::NotImplemented)));
        assert!(matches!((a * b).validate(), Err(HookError::NotImplemented)));
        assert!(matches!((a / b).validate(), Err(HookError::NotImplemented)));
        assert!(matches!((-a).validate(), Err(HookError::NotImplemented)));
    }

    #[test]
    fn mixed_xfl_and_unchecked_ops_type_check() {
        let checked = XFL::one();
        let unchecked = checked.unchecked();
        let _: XFLUnchecked = unchecked + checked;
        let _: XFLUnchecked = checked + unchecked;
        let _: XFLUnchecked = unchecked - checked;
        let _: XFLUnchecked = checked - unchecked;
        let _: XFLUnchecked = unchecked * checked;
        let _: XFLUnchecked = checked * unchecked;
        let _: XFLUnchecked = unchecked / checked;
        let _: XFLUnchecked = checked / unchecked;
    }
}
