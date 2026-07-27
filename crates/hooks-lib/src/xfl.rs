//! XFL — the Xahau 64-bit decimal floating-point representation used
//! throughout the Hook API for amounts and other fractional values.
//!
//! Bit layout (little detail, big consequence — see below), per the
//! `hook-api` skill's Float API reference:
//! - bits 0..=53 (54 bits): mantissa (magnitude `10^15` to `10^16 - 1`)
//! - bits 54..=61 (8 bits): biased exponent (unbiased range `-96` to `+80`,
//!   bias `97`, so the stored field ranges `1..=177`)
//! - bit 62: sign (`0` = negative, `1` = positive, per the skill's Float
//!   API doc)
//! - bit 63: unused/reserved — always `0` for a valid XFL, because the Hook
//!   API multiplexes error codes onto the same `i64` return channel as
//!   negative values, and a valid float must never look like an error code.
//!
//! This crate does not expose a `float_exponent` host call — none exists in
//! `extern.h` — so [`XFL::exponent`] decodes the bias-97 exponent field
//! locally from the raw bit pattern instead of making a host call. It cannot
//! practically fail, but returns `Result<i64>` for signature uniformity with
//! [`XFL::mantissa`] (both describe components of the same value).
//!
//! Wraps 14 of the 74 Hook API functions privately (never exposed as
//! separate `api::*` wrappers, only as `XFL`/`XFLUnchecked` methods and
//! operators, here and in [`crate::xfl_unchecked`]): `float_set`,
//! `float_multiply`, `float_mulratio`, `float_negate`, `float_compare`,
//! `float_sum`, `float_invert`, `float_divide`, `float_one`,
//! `float_mantissa`, `float_sign`, `float_int`, `float_log`, `float_root`.
//! **`Neg` and comparisons are host round trips, not local bit
//! manipulation** — see those impls' doc comments below for why a
//! guest-side sign-bit flip or a guest-side bit/order comparison is *not*
//! a sound substitute for `float_negate`/`float_compare` (this module
//! originally tried the local-only route; it does not correctly capture
//! XFL's actual comparison/negation semantics, so it was reverted in favor
//! of the host round trip). The remaining 3 buffer-shaped float functions
//! (`float_sto`, `float_sto_set`, `slot_float`) live in `api::float` and
//! are forwarded to by [`XFL::sto`], [`XFL::sto_set`], and
//! [`XFL::from_slot`].
//!
//! # Operators, not methods (for arithmetic)
//!
//! `XFL` implements `Add`/`Sub`/`Mul`/`Div`/`Neg`, all with `Output =
//! Result<XFL, HookError>` (every one of these is a fallible host round
//! trip — `float_sum`/`float_multiply`/`float_divide`/`float_negate`).
//! `Sub` is built from `Add` and `Neg`: `self - rhs` is `self + (-rhs)?`,
//! i.e. one `float_negate` host call plus one `float_sum` host call — there
//! is no dedicated `float_subtract` host function.
//!
//! To keep `?`-free chains ergonomic despite the fallible `Output`, this
//! module also implements `Add`/`Sub`/`Mul`/`Div` for `Result<XFL,
//! HookError>` on either side of a plain `XFL` (see the
//! `xfl_result_chain_ops!` macro below), so `a + b + c` type-checks and
//! short-circuits on the first error exactly like `(a + b)? + c` would, as
//! long as the chain alternates a bare `XFL` in at each step. This is
//! possible without violating Rust's orphan rules only because it happens
//! inside hooks-lib itself: `impl Add<XFL> for Result<XFL, HookError>` is a
//! concrete (no impl-level type parameters), non-generic impl of a foreign
//! trait, permitted because the orphan check finds a locally-headed type
//! (`XFL` itself) among the impl's `Self` type and the trait's own generic
//! argument. A downstream hook crate cannot replicate this pattern for its
//! own types layered over `XFL`/`Result` — that is the normal, expected
//! orphan-rule boundary; this crate is the one place in the dependency
//! graph where `XFL` is local, so it is the one place this trick is legal.
//!
//! **Not** implemented, and not legal to implement here either: `Add`/
//! `Sub`/`Mul`/`Div` for `Result<XFL, HookError>` on *both* sides at once
//! (e.g. combining two independently-fallible values directly, such as the
//! sum of two separate `mulratio` results). Both sides would then be
//! headed by the foreign, non-fundamental `core::result::Result`
//! constructor with no locally-headed type anywhere in the sequence the
//! orphan check examines — `XFL` only appears nested inside `Result`'s own
//! generic argument, which the check does not look inside for
//! non-fundamental foreign types (unlike `Box`/`&`/`&mut`, which are
//! "fundamental" and are looked into). Confirmed by attempting exactly
//! that impl and reading rustc's own E0117 diagnostic, not just reasoned
//! about in the abstract — see `xfl_result_chain_ops!`'s doc comment for
//! the full explanation. Combining two already-`Result` values needs one
//! explicit `?` on either side (`a? + b`, or `a + b?`), no worse
//! ergonomically than the method-call API this module replaces.
//!
//! `XFL` deliberately has **no panicking arithmetic**: every fallible op
//! returns `Result` rather than panicking, so a hook author must handle (or
//! explicitly `unwrap`/`expect` — both `deny`d workspace-wide — or
//! propagate with `?`) every failure instead of it silently rolling back
//! the whole hook via an unhandled panic. See [`crate::xfl_unchecked`] for
//! `XFLUnchecked`, the deliberately-poisonable hot-path counterpart, for
//! chains where per-step `Result` handling is itself the measured cost
//! problem.
//!
//! # Comparison: both methods and operators, both via `float_compare`
//!
//! [`XFL::eq`]/[`XFL::lt`]/[`XFL::gt`]/[`XFL::compare`] all return
//! `Result<bool>` — comparison is a fallible `float_compare` host round
//! trip (an invalid operand is a real, reachable failure mode, e.g.
//! `INVALID_FLOAT`), and `core::cmp::PartialEq`/`PartialOrd` cannot express
//! that: their methods return a bare `bool`/`Option<Ordering>`, with no room
//! for an `Err` case. `XFL` implements both anyway: `PartialEq`/
//! `PartialOrd` are thin forwarding wrappers over the very same
//! `float_compare`-backed methods (so `a == b`/`a < b`/... work, still
//! backed by the real host comparison, not a local bit trick), and on a
//! `float_compare` failure they fall back to `false`/`None` — the same
//! convention `f64`'s own `PartialEq`/`PartialOrd` use for `NaN`:
//! "couldn't establish equality/order" is represented as "not equal"/"not
//! comparable," not a panic, and not a hidden `rollback!` from inside what
//! looks like an ordinary boolean expression (an early draft of this tried
//! exactly that — call `rollback!` on a `float_compare` failure from
//! inside `PartialEq`/`PartialOrd` — and rejected it: on `not(target_arch =
//! "wasm32")`, `crate::api::control::rollback` loops forever rather than
//! returning, since there is no host to actually terminate the process, so
//! that design would hang any host-target test/doctest that ever hit a
//! `float_compare` failure — trivially reachable, since **every**
//! `float_compare` call fails deterministically on a host build, per the
//! `NOT_IMPLEMENTED` host stub). Use [`XFL::eq`]/[`XFL::lt`]/[`XFL::gt`]/
//! [`XFL::compare`] directly — not `==`/`<`/`>` — anywhere a
//! `float_compare` failure needs to be distinguished from a genuine
//! inequality/incomparability (which, unlike `f64`, never actually happens
//! between two valid XFLs — every pair of valid XFLs is totally ordered, so
//! `None`/`false` from the operators is, in practice, always a signal that
//! one operand was invalid). `core::ops::Neg` does not have this
//! false/None fallback story at all: unlike `PartialEq`/`PartialOrd`, its
//! `Output` type is not fixed by the trait — `Result<XFL, HookError>` is a
//! perfectly valid `Neg::Output`, so a `float_negate` failure propagates as
//! a real `Err`, the same as every other arithmetic operator here.

use crate::api;
use crate::error::{Result, res};
use crate::types::{AccountId, CurrencyCode};

/// Bias applied to the stored 8-bit exponent field (bits 54..=61): unbiased
/// exponent = stored field - 97.
const EXPONENT_BIAS: i64 = 97;
/// Bit offset of the exponent field.
const EXPONENT_SHIFT: u32 = 54;
/// Mask for the 8-bit exponent field once shifted into place.
const EXPONENT_MASK: i64 = 0xFF;

/// A Xahau XFL value: an opaque wrapper over the raw `i64` bit pattern the
/// Hook API's `float_*` functions operate on.
///
/// The inner field is deliberately private: XFL host calls return negative
/// values as error codes sharing the same `i64` channel as valid floats, so
/// a public field would let a caller smuggle a raw error code in as if it
/// were a value. [`XFL::from_raw_bits`] / [`XFL::raw_bits`] are the explicit,
/// documented escape hatches for unchecked representation access.
///
/// `PartialEq`/`PartialOrd` are implemented, backed by the fallible
/// `float_compare` host call, with a `false`/`None` fallback on failure
/// (see the module doc comment's "Comparison: both methods and operators,
/// both via `float_compare`" section for why, and for when to use
/// [`XFL::eq`]/[`XFL::lt`]/[`XFL::gt`]/[`XFL::compare`] — all of which
/// return `Result<bool>` — instead of `==`/`<`/`>`).
///
/// # Examples
///
/// ```
/// use hooks_lib::xfl::XFL;
///
/// let one = XFL::one();
/// assert_eq!(one.raw_bits(), XFL::from_raw_bits(one.raw_bits()).raw_bits());
/// ```
#[derive(Clone, Copy, Debug)]
pub struct XFL(i64);

impl XFL {
    /// Wrap a raw XFL bit pattern with no validation. Escape hatch for
    /// interop with values obtained outside the typed API (e.g. persisted
    /// state).
    #[inline(always)]
    #[must_use]
    pub fn from_raw_bits(bits: i64) -> XFL {
        XFL(bits)
    }

    /// The raw XFL bit pattern. Escape hatch for interop; does not validate
    /// that `self` is actually a valid (non-error-code) XFL.
    #[inline(always)]
    #[must_use]
    pub fn raw_bits(self) -> i64 {
        self.0
    }

    /// Reinterpret `self` as an [`crate::xfl_unchecked::XFLUnchecked`] for a
    /// hot-path arithmetic chain. Zero-cost: just moves the raw `i64` into
    /// the other newtype, no host call and no validation either way.
    #[inline(always)]
    #[must_use]
    pub fn unchecked(self) -> crate::xfl_unchecked::XFLUnchecked {
        crate::xfl_unchecked::XFLUnchecked::from_raw_bits(self.0)
    }

    /// Construct a normalized XFL from `exponent` and `mantissa`.
    #[inline(always)]
    pub fn new(exponent: i32, mantissa: i64) -> Result<XFL> {
        res(unsafe { hooks_core::float_set(exponent, mantissa) }).map(XFL::from_raw_bits)
    }

    /// The XFL representation of `1.0`. Cannot practically fail, so this is
    /// a bare `XFL`, not a `Result`.
    #[inline(always)]
    #[must_use]
    pub fn one() -> XFL {
        XFL::from_raw_bits(unsafe { hooks_core::float_one() })
    }

    /// `1 / self`.
    #[inline(always)]
    pub fn invert(self) -> Result<XFL> {
        res(unsafe { hooks_core::float_invert(self.0) }).map(XFL::from_raw_bits)
    }

    /// `self * (num / den)`, rounding up when `round_up` is set, down
    /// otherwise.
    #[inline(always)]
    pub fn mulratio(self, round_up: bool, num: u32, den: u32) -> Result<XFL> {
        res(unsafe { hooks_core::float_mulratio(self.0, round_up as u32, num, den) })
            .map(XFL::from_raw_bits)
    }

    /// The mantissa component of `self` (`0` to `9_999_999_999_999_999`).
    #[inline(always)]
    pub fn mantissa(self) -> Result<i64> {
        res(unsafe { hooks_core::float_mantissa(self.0) })
    }

    /// The unbiased exponent component of `self` (`-96` to `+80`).
    ///
    /// Decoded locally from the raw bit pattern (bits 54..=61, bias 97) —
    /// there is no `float_exponent` host call. See the module doc comment
    /// for the bit layout. Wrapped in `Ok` for signature uniformity with
    /// [`XFL::mantissa`]; this computation cannot practically fail.
    #[inline(always)]
    pub fn exponent(self) -> Result<i64> {
        let field = (self.0 >> EXPONENT_SHIFT) & EXPONENT_MASK;
        // `field` is masked to 0..=0xFF and `EXPONENT_BIAS` is the fixed
        // constant 97, so this never overflows i64; `wrapping_sub` (rather
        // than a plain `-`) also sidesteps `clippy::arithmetic_side_effects`
        // without needing a blanket `#[allow]`.
        Ok(field.wrapping_sub(EXPONENT_BIAS))
    }

    /// Whether `self` is negative (per `float_sign`: `0` = positive or
    /// zero, `1` = negative — so `true` here means "negative").
    #[inline(always)]
    pub fn sign(self) -> Result<bool> {
        res(unsafe { hooks_core::float_sign(self.0) }).map(|v| v != 0)
    }

    /// Convert `self` to an integer, keeping `decimal_places` fractional
    /// digits; `absolute` requests the magnitude (dropping the sign)
    /// instead of erroring on a negative result.
    #[inline(always)]
    pub fn to_int(self, decimal_places: u32, absolute: bool) -> Result<i64> {
        res(unsafe { hooks_core::float_int(self.0, decimal_places, absolute as u32) })
    }

    /// Compare `self` to `rhs` under the bitmask `mode` (see
    /// `hooks_core::{COMPARE_EQUAL, COMPARE_LESS, COMPARE_GREATER}`, freely
    /// combinable, e.g. `COMPARE_LESS | COMPARE_EQUAL` for `<=`), via the
    /// `float_compare` host call.
    #[inline(always)]
    pub fn compare(self, rhs: XFL, mode: u32) -> Result<bool> {
        res(unsafe { hooks_core::float_compare(self.0, rhs.0, mode) }).map(|v| v != 0)
    }

    /// `self == rhs`.
    #[inline(always)]
    pub fn eq(self, rhs: XFL) -> Result<bool> {
        self.compare(rhs, hooks_core::COMPARE_EQUAL)
    }

    /// `self < rhs`.
    #[inline(always)]
    pub fn lt(self, rhs: XFL) -> Result<bool> {
        self.compare(rhs, hooks_core::COMPARE_LESS)
    }

    /// `self > rhs`.
    #[inline(always)]
    pub fn gt(self, rhs: XFL) -> Result<bool> {
        self.compare(rhs, hooks_core::COMPARE_GREATER)
    }

    /// `log10(self)`.
    #[inline(always)]
    pub fn log(self) -> Result<XFL> {
        res(unsafe { hooks_core::float_log(self.0) }).map(XFL::from_raw_bits)
    }

    /// `self ^ (1/n)`.
    #[inline(always)]
    pub fn root(self, n: u32) -> Result<XFL> {
        res(unsafe { hooks_core::float_root(self.0, n) }).map(XFL::from_raw_bits)
    }

    /// Encode `self` as a serialized Amount into `out`. Thin forwarding call
    /// to [`api::float::float_sto`] — reuses its pointer-direction and
    /// `Option` handling rather than duplicating it here.
    #[inline(always)]
    pub fn sto(
        self,
        out: &mut [u8],
        currency: Option<&CurrencyCode>,
        issuer: Option<&AccountId>,
        field_code: u32,
    ) -> Result<usize> {
        api::float::float_sto(out, currency, issuer, self, field_code)
    }

    /// Decode a serialized Amount (`buf`) into an XFL. Forwards to
    /// [`api::float::float_sto_set`].
    #[inline(always)]
    pub fn sto_set(buf: &[u8]) -> Result<XFL> {
        api::float::float_sto_set(buf)
    }

    /// Read the amount held in slot `slot_no` as an XFL. Forwards to
    /// [`api::float::slot_float`].
    #[inline(always)]
    pub fn from_slot(slot_no: u32) -> Result<XFL> {
        api::float::slot_float(slot_no)
    }
}

impl core::ops::Neg for XFL {
    type Output = Result<XFL>;

    /// `-self`, via the `float_negate` host call.
    ///
    /// This is **not** a local sign-bit flip. An earlier version of this
    /// impl tried exactly that (flip bit 62, leave canonical zero alone) on
    /// the theory that XFL's sign-magnitude layout makes negation "just"
    /// flipping the sign bit — that theory does not hold up against XFL's
    /// actual negation semantics, so `Neg` routes through the host
    /// `float_negate` call instead, the same way every other arithmetic
    /// operator here does. `Output` is `Result<XFL, HookError>` (not a bare
    /// `XFL`) to make room for that call failing, e.g. on an already-invalid
    /// `self`.
    #[inline(always)]
    fn neg(self) -> Result<XFL> {
        res(unsafe { hooks_core::float_negate(self.0) }).map(XFL::from_raw_bits)
    }
}

impl core::ops::Add for XFL {
    type Output = Result<XFL>;

    /// `self + rhs`, via the `float_sum` host call.
    #[inline(always)]
    fn add(self, rhs: XFL) -> Result<XFL> {
        res(unsafe { hooks_core::float_sum(self.0, rhs.0) }).map(XFL::from_raw_bits)
    }
}

impl core::ops::Sub for XFL {
    type Output = Result<XFL>;

    /// `self - rhs`, implemented as `self + (-rhs)?`: one `float_negate`
    /// host call plus one `float_sum` host call. There is no dedicated
    /// `float_subtract` host function. The `?` on `-rhs` propagates a
    /// negation failure (e.g. `rhs` already invalid) as this call's own
    /// error, rather than feeding a poisoned value into `float_sum`.
    #[inline(always)]
    // `self + (-rhs)?` dispatches to this module's own `Add`/`Neg` impls
    // above (both fallible host round trips), not raw integer arithmetic —
    // `clippy::arithmetic_side_effects` can't see past the operator syntax
    // to tell the difference, so it flags this unconditionally; there is no
    // overflow/panic risk here to warn about.
    #[allow(clippy::arithmetic_side_effects)]
    fn sub(self, rhs: XFL) -> Result<XFL> {
        self + (-rhs)?
    }
}

impl core::ops::Mul for XFL {
    type Output = Result<XFL>;

    /// `self * rhs`, via the `float_multiply` host call.
    #[inline(always)]
    fn mul(self, rhs: XFL) -> Result<XFL> {
        res(unsafe { hooks_core::float_multiply(self.0, rhs.0) }).map(XFL::from_raw_bits)
    }
}

impl core::ops::Div for XFL {
    type Output = Result<XFL>;

    /// `self / rhs`, via the `float_divide` host call.
    #[inline(always)]
    fn div(self, rhs: XFL) -> Result<XFL> {
        res(unsafe { hooks_core::float_divide(self.0, rhs.0) }).map(XFL::from_raw_bits)
    }
}

impl PartialEq for XFL {
    /// `self == other`, forwarding to [`XFL::eq`] (`float_compare` under
    /// `COMPARE_EQUAL`). Falls back to `false` on a `float_compare`
    /// failure — see the module doc comment's "Comparison: both methods
    /// and operators, both via `float_compare`" section for why, and for
    /// when to call [`XFL::eq`] directly instead.
    #[inline(always)]
    fn eq(&self, other: &XFL) -> bool {
        XFL::eq(*self, *other).unwrap_or(false)
    }
}

impl PartialOrd for XFL {
    /// `self.partial_cmp(other)`, via up to two `float_compare` host calls
    /// (`COMPARE_LESS`, then — only if that came back `false` — `COMPARE_
    /// GREATER`; a `false` result for both means `Ordering::Equal`).
    /// Falls back to `None` on a `float_compare` failure at either step —
    /// see the module doc comment's "Comparison: both methods and
    /// operators, both via `float_compare`" section for why, and for when
    /// to call [`XFL::lt`]/[`XFL::gt`]/[`XFL::compare`] directly instead.
    #[inline(always)]
    fn partial_cmp(&self, other: &XFL) -> Option<core::cmp::Ordering> {
        match XFL::lt(*self, *other) {
            Ok(true) => Some(core::cmp::Ordering::Less),
            Ok(false) => match XFL::gt(*self, *other) {
                Ok(true) => Some(core::cmp::Ordering::Greater),
                Ok(false) => Some(core::cmp::Ordering::Equal),
                Err(_) => None,
            },
            Err(_) => None,
        }
    }
}

// Generates `impl $Trait<XFL> for Result<XFL, HookError>` and
// `impl $Trait<Result<XFL, HookError>> for XFL` for each listed
// `$Trait::$method`, so a chain of `+`/`-`/`*`/`/` that alternates a plain
// `XFL` in on one side at each step (`((a + b) + c) + d`, ...) short-
// circuits on the first error without an explicit `?` between steps.
//
// Deliberately does NOT generate `impl $Trait<Result<XFL, HookError>> for
// Result<XFL, HookError>` (combining two *already-`Result`* values
// directly, e.g. `mulratio_result_a + mulratio_result_b`): Rust's orphan
// rule considers, in order, the impl's `Self` type and then the trait's own
// generic argument, and permits the impl once it finds *some* type in that
// sequence whose own outer type constructor is local — `Result<XFL,
// HookError>`'s outer constructor is `core::result::Result`, which is
// foreign and (unlike `Box`/`&`/`&mut`) not "fundamental", so the orphan
// check does not look inside it for the locally-defined `XFL` nested in its
// generic argument. `Result<XFL, HookError>` op `XFL` (or the reverse)
// finds `XFL` itself sitting directly in that sequence and is fine; `Result
// <XFL, HookError>` op `Result<XFL, HookError>` never puts a
// locally-headed type in the sequence at all and is therefore rejected
// with E0117 — confirmed by attempting exactly that impl here and reading
// the compiler's own diagnostic, not just reasoned about in the abstract.
// A genuine two-independently-fallible-values combination needs one
// explicit `?` on either side (`a? + b`/`a + b?`, both still O(1) and still
// short-circuiting) — no worse, ergonomically, than the pre-operator
// method-call API this replaces.
macro_rules! xfl_result_chain_ops {
    ($( $Trait:ident :: $method:ident ),+ $(,)?) => {
        $(
            impl core::ops::$Trait<XFL> for Result<XFL> {
                type Output = Result<XFL>;

                #[inline(always)]
                fn $method(self, rhs: XFL) -> Result<XFL> {
                    core::ops::$Trait::$method(self?, rhs)
                }
            }

            impl core::ops::$Trait<Result<XFL>> for XFL {
                type Output = Result<XFL>;

                #[inline(always)]
                fn $method(self, rhs: Result<XFL>) -> Result<XFL> {
                    core::ops::$Trait::$method(self, rhs?)
                }
            }
        )+
    };
}

xfl_result_chain_ops!(Add::add, Sub::sub, Mul::mul, Div::div);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::HookError;

    #[test]
    fn raw_bits_round_trip() {
        for bits in [0i64, 1, -1, i64::MAX, i64::MIN, 42, -42] {
            assert_eq!(XFL::from_raw_bits(bits).raw_bits(), bits);
        }
    }

    #[test]
    fn smoke_not_implemented_on_host() {
        // `matches!`, not `assert_eq!`, for every `Result<XFL, _>` here —
        // every assertion below only ever needs to distinguish `Err(...)`
        // from `Ok(...)` (never compares two `Ok(XFL)`s against each
        // other), so plain `assert_eq!` against an `Err(...)` pattern
        // would actually work today (`Result`'s derived `PartialEq`
        // short-circuits on the `Ok`/`Err` discriminant before ever
        // calling `XFL::eq`) — but `matches!` is used consistently
        // throughout this file regardless, so no assertion here
        // accidentally depends on `XFL`'s `PartialEq` impl (which forwards
        // to the fallible `float_compare`-backed `XFL::eq` and falls back
        // to `false` on failure — see the module doc comment — a real
        // trap for an `assert_eq!` that *does* compare two `Ok(XFL)`s).
        let one = XFL::one();
        assert!(matches!(XFL::new(0, 1), Err(HookError::NotImplemented)));
        assert!(matches!(one + one, Err(HookError::NotImplemented)));
        assert!(matches!(one - one, Err(HookError::NotImplemented)));
        assert!(matches!(one * one, Err(HookError::NotImplemented)));
        assert!(matches!(one / one, Err(HookError::NotImplemented)));
        assert!(matches!(-one, Err(HookError::NotImplemented)));
        assert!(matches!(one.invert(), Err(HookError::NotImplemented)));
        assert!(matches!(
            one.mulratio(false, 1, 2),
            Err(HookError::NotImplemented)
        ));
        assert_eq!(one.mantissa(), Err(HookError::NotImplemented));
        assert_eq!(one.sign(), Err(HookError::NotImplemented));
        assert_eq!(one.to_int(0, false), Err(HookError::NotImplemented));
        assert_eq!(one.compare(one, 1), Err(HookError::NotImplemented));
        assert_eq!(one.eq(one), Err(HookError::NotImplemented));
        assert_eq!(one.lt(one), Err(HookError::NotImplemented));
        assert_eq!(one.gt(one), Err(HookError::NotImplemented));
        assert!(matches!(one.log(), Err(HookError::NotImplemented)));
        assert!(matches!(one.root(2), Err(HookError::NotImplemented)));
        assert!(matches!(
            XFL::sto_set(&[0u8; 8]),
            Err(HookError::NotImplemented)
        ));
        assert!(matches!(XFL::from_slot(1), Err(HookError::NotImplemented)));
    }

    #[test]
    fn result_chain_short_circuits_on_first_error() {
        // `Result<XFL> op XFL` and `XFL op Result<XFL>` both type-check
        // and, given an `Err` input, never reach the host stub (host builds
        // have no way to observe that directly, but a mismatched-code
        // assertion here would fail if the wrong error propagated).
        // `matches!`, not `assert_eq!`, for the same reason as
        // `smoke_not_implemented_on_host` above.
        let one = XFL::one();
        let err: Result<XFL> = Err(HookError::DoesntExist);
        assert!(matches!(err + one, Err(HookError::DoesntExist)));
        assert!(matches!(one + err, Err(HookError::DoesntExist)));
        assert!(matches!(err - one, Err(HookError::DoesntExist)));
        assert!(matches!(one - err, Err(HookError::DoesntExist)));
        assert!(matches!(err * one, Err(HookError::DoesntExist)));
        assert!(matches!(one * err, Err(HookError::DoesntExist)));
        assert!(matches!(err / one, Err(HookError::DoesntExist)));
        assert!(matches!(one / err, Err(HookError::DoesntExist)));
    }

    #[test]
    // `one == one`/`one < one`/`one > one` below are all `false` given the
    // host stub (checked via a bound variable + `assert!`, not
    // `assert_eq!(..., false)` — `clippy::bool_assert_comparison` wants
    // `assert!(!(...))`  for that, but `clippy::neg_cmp_op_on_partial_ord`
    // simultaneously objects to negating `<`/`>` directly, since the
    // operands could genuinely be incomparable; binding first sidesteps
    // both).
    fn comparison_operators_fall_back_like_f64_nan_on_host() {
        // `float_compare`'s host stub is deterministic `NOT_IMPLEMENTED`
        // (an `Err`) regardless of operands, so every `PartialEq`/
        // `PartialOrd` call here exercises the `false`/`None` fallback —
        // and, crucially, returns promptly rather than hanging (an
        // earlier design that rolled the hook back on a `float_compare`
        // failure from inside these trait impls would have looped forever
        // right here, since `rollback` never returns on a host target).
        let one = XFL::one();
        let is_eq = one == one;
        let is_lt = one < one;
        let is_gt = one > one;
        assert!(!is_eq);
        assert_eq!(one.partial_cmp(&one), None);
        assert!(!is_lt);
        assert!(!is_gt);
        // The `Result<bool>`-returning inherent methods these forward to
        // still report the real failure explicitly.
        assert_eq!(one.eq(one), Err(HookError::NotImplemented));
    }

    #[test]
    fn exponent_decodes_bias_97_field() {
        // Stored field 97 (bits 54..=61) decodes to unbiased exponent 0.
        let bits = 97i64 << EXPONENT_SHIFT;
        assert_eq!(XFL::from_raw_bits(bits).exponent(), Ok(0));
        // Stored field 1 (minimum) decodes to -96.
        let bits_min = 1i64 << EXPONENT_SHIFT;
        assert_eq!(XFL::from_raw_bits(bits_min).exponent(), Ok(-96));
        // Stored field 177 (maximum) decodes to +80.
        let bits_max = 177i64 << EXPONENT_SHIFT;
        assert_eq!(XFL::from_raw_bits(bits_max).exponent(), Ok(80));
    }
}
