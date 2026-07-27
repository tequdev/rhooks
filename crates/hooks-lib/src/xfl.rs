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
//! - The all-zero bit pattern (`0i64`) is a special case: canonical zero,
//!   treated as neither positive nor negative by the host — confirmed
//!   against `HookAPI::float_sign` and `HookAPI::float_negate` in vendored
//!   `xrpld/app/hook/detail/HookAPI.cpp`/`HookAPI.h`, both of which
//!   special-case `float1 == 0` before consulting bit 62 at all.
//!
//! This crate does not expose a `float_exponent` host call — none exists in
//! `extern.h` — so [`XFL::exponent`] decodes the bias-97 exponent field
//! locally from the raw bit pattern instead of making a host call. It cannot
//! practically fail, but returns `Result<i64>` for signature uniformity with
//! [`XFL::mantissa`] (both describe components of the same value).
//!
//! Wraps 12 of the 74 Hook API functions privately (never exposed as
//! separate `api::*` wrappers, only as `XFL`/`XFLUnchecked` methods and
//! operators, here and in [`crate::xfl_unchecked`]): `float_set`,
//! `float_multiply`, `float_mulratio`, `float_sum`, `float_invert`,
//! `float_divide`, `float_one`, `float_mantissa`, `float_sign`, `float_int`,
//! `float_log`, `float_root`. Two more XFL-shaped Hook API functions exist —
//! `float_negate` and `float_compare` — but this crate **deliberately never
//! calls either of them**: negation is a pure local sign-bit flip (see the
//! `Neg` impl below) and comparison is a pure local bit/order comparison
//! (see the `PartialEq`/`PartialOrd` impls below); both are fully
//! computable from the bit pattern alone, so routing them through a host
//! call would just be a slower way to reach the same answer. The remaining
//! 3 buffer-shaped float functions (`float_sto`, `float_sto_set`,
//! `slot_float`) live in `api::float` and are forwarded to by [`XFL::sto`],
//! [`XFL::sto_set`], and [`XFL::from_slot`].
//!
//! # Operators, not methods
//!
//! `XFL` implements `Add`/`Sub`/`Mul`/`Div` with `Output =
//! Result<XFL, HookError>` (every XFL arithmetic op is a fallible host
//! call) and `Neg` with `Output = XFL` (always succeeds — pure local
//! sign-bit flip, no host call at all). `Sub` is built from the two:
//! `self - rhs` is implemented as `self + (-rhs)`, i.e. one local negate
//! plus one `float_sum` host call, not a dedicated `float_subtract` (no
//! such host function exists).
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
/// Bit offset of the sign field (`0` = negative, `1` = positive; see the
/// module doc comment).
const SIGN_SHIFT: u32 = 62;

/// A Xahau XFL value: an opaque wrapper over the raw `i64` bit pattern the
/// Hook API's `float_*` functions operate on.
///
/// The inner field is deliberately private: XFL host calls return negative
/// values as error codes sharing the same `i64` channel as valid floats, so
/// a public field would let a caller smuggle a raw error code in as if it
/// were a value. [`XFL::from_raw_bits`] / [`XFL::raw_bits`] are the explicit,
/// documented escape hatches for unchecked representation access.
///
/// # `PartialEq`/`PartialOrd` are host-value-only
///
/// This type's `PartialEq`/`PartialOrd` impls are pure local bit
/// comparisons (see their doc comments below for exactly what they compute
/// and why that is sound) — they are **only guaranteed correct for
/// canonical values that came from a host `float_*` call** (`XFL::new`,
/// arithmetic results, `XFL::from_slot`, `XFL::sto_set`, ...). A value
/// assembled by hand via [`XFL::from_raw_bits`] with an out-of-range
/// mantissa/exponent, or any other non-canonical bit pattern, has **no**
/// correctness guarantee under these impls: comparisons involving it may
/// give an answer that does not correspond to any real numeric ordering.
/// Validate an untrusted raw bit pattern (e.g. via
/// [`crate::xfl_unchecked::XFLUnchecked::validate`]) before comparing it.
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
    ///
    /// The result is **not** guaranteed canonical: nothing checks that
    /// `bits` decodes to an in-range mantissa/exponent, or that it isn't
    /// actually a negative Hook API error code smuggled in as a "value".
    /// Every arithmetic/comparison impl on `XFL` documents that it is only
    /// correct for host-produced canonical values — a value constructed
    /// here bypasses that guarantee entirely until it is validated (e.g.
    /// via [`crate::xfl_unchecked::XFLUnchecked::validate`]).
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

/// Flip the sign bit (bit 62) of a raw XFL bit pattern, leaving canonical
/// zero (`0i64`) unchanged — the pure-local equivalent of
/// `HookAPI::float_negate` (verified against vendored
/// `xrpld/app/hook/HookAPI.h`'s `invert_sign`/`is_negative`: `float_negate`
/// special-cases `float1 == 0` to `0`, otherwise XORs bit 62). No host
/// call, no validation: mantissa/exponent bits (0..=61, excluding bit 62)
/// are left untouched, so this can neither create nor destroy validity —
/// applying it to an out-of-range/non-canonical bit pattern yields another
/// bit pattern with exactly the same mantissa/exponent bits and thus the
/// same validity, just a flipped sign.
#[inline(always)]
fn flip_sign_bit(bits: i64) -> i64 {
    if bits == 0 {
        0
    } else {
        bits ^ (1i64 << SIGN_SHIFT)
    }
}

/// Map a canonical XFL bit pattern to an `i64` "order key" such that
/// ordinary `i64` comparison of the keys matches XFL's numeric (sign
/// aware, not raw-bit) ordering. Only sound for canonical, host-produced
/// bit patterns (see the `XFL` type doc comment's `PartialEq`/`PartialOrd`
/// section).
///
/// Why raw bit comparison alone is *not* enough: bits 0..=61
/// (exponent+mantissa) encode magnitude the same way regardless of sign,
/// so among two negative values the raw bit pattern grows *with*
/// magnitude — exactly backwards from numeric order, where a
/// larger-magnitude negative number is numerically smaller. This function
/// corrects that by negating the magnitude bits for negative inputs (sign
/// bit clear), which also reverses their relative order to match. Zero is
/// handled as its own case (bit 62 doesn't reliably indicate sign for the
/// zero bit pattern — see the module doc comment). Positive values need no
/// correction: their magnitude bits already sort the same way the raw `i64`
/// does, and bit 62 being set on every positive value keeps them all
/// numerically above zero and above every negative value's key (whose
/// negated magnitude keys are always `<= 0`).
#[inline(always)]
fn order_key(bits: i64) -> i64 {
    if bits == 0 {
        0
    } else if (bits >> SIGN_SHIFT) & 1 == 1 {
        // Positive: bit 62 set, so `bits` (as a plain i64) already sorts
        // the same way the represented value does.
        bits
    } else {
        // Negative: bit 62 clear, so `bits` holds only the magnitude
        // (exponent+mantissa) bits, which is always in `0..2^62` — small
        // enough that negating it can never overflow i64.
        bits.wrapping_neg()
    }
}

impl core::ops::Neg for XFL {
    /// Always succeeds — see the impl doc comment.
    type Output = XFL;

    /// `-self`: a pure local sign-bit flip, no host call. See
    /// [`flip_sign_bit`]'s doc comment for exactly what this computes and
    /// why it is sound even for a non-canonical `self`.
    #[inline(always)]
    fn neg(self) -> XFL {
        XFL(flip_sign_bit(self.0))
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

    /// `self - rhs`, implemented as `self + (-rhs)`: one local sign flip
    /// (no host call) plus one `float_sum` host call. There is no
    /// dedicated `float_subtract` host function.
    #[inline(always)]
    // `self + (-rhs)` dispatches to this module's own `Add`/`Neg` impls
    // above (a fallible host round trip and a pure local bit flip,
    // respectively), not raw integer arithmetic — `clippy::
    // arithmetic_side_effects` can't see past the operator syntax to tell
    // the difference, so it flags this unconditionally; there is no
    // overflow/panic risk here to warn about.
    #[allow(clippy::arithmetic_side_effects)]
    fn sub(self, rhs: XFL) -> Result<XFL> {
        self + (-rhs)
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
    /// Bitwise equality — sound because a canonical (host-produced) XFL
    /// representation is unique per value (fixed-precision normalized
    /// mantissa, no `-0`/`+0` distinction beyond the single all-zero
    /// pattern). See the type doc comment for why this is **not**
    /// guaranteed correct for non-canonical values.
    #[inline(always)]
    fn eq(&self, other: &XFL) -> bool {
        self.0 == other.0
    }
}

impl PartialOrd for XFL {
    /// Sign-magnitude-aware ordering via [`order_key`] — never returns
    /// `None` for canonical values (XFL has no NaN-equivalent). See the
    /// type doc comment for why this is **not** guaranteed correct for
    /// non-canonical values.
    #[inline(always)]
    fn partial_cmp(&self, other: &XFL) -> Option<core::cmp::Ordering> {
        Some(order_key(self.0).cmp(&order_key(other.0)))
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

    /// Hand-crafted canonical bit patterns for `1.0` and `-1.0`
    /// (mantissa `1_000_000_000_000_000` = `1e15`, stored exponent field
    /// `97` = unbiased `0`), used by the local (no-host-call) operator
    /// tests below — same construction style as
    /// `exponent_decodes_bias_97_field`.
    const POSITIVE_ONE_BITS: i64 = (1i64 << 62) | (97i64 << 54) | 1_000_000_000_000_000i64;
    const NEGATIVE_ONE_BITS: i64 = (97i64 << 54) | 1_000_000_000_000_000i64;
    /// Same magnitude construction, stored exponent field `98` (unbiased
    /// `1`), i.e. `10.0` / `-10.0` — one order of magnitude above the
    /// `_ONE` constants, for ordering tests.
    const POSITIVE_TEN_BITS: i64 = (1i64 << 62) | (98i64 << 54) | 1_000_000_000_000_000i64;
    const NEGATIVE_TEN_BITS: i64 = (98i64 << 54) | 1_000_000_000_000_000i64;

    #[test]
    fn raw_bits_round_trip() {
        for bits in [0i64, 1, -1, i64::MAX, i64::MIN, 42, -42] {
            assert_eq!(XFL::from_raw_bits(bits).raw_bits(), bits);
        }
    }

    #[test]
    fn smoke_not_implemented_on_host() {
        // `XFL::one()` on a host build is `hooks_core::float_one()`'s
        // deterministic `NOT_IMPLEMENTED` stub — not a real `1.0` bit
        // pattern. That's fine for the operators exercised here: `+`/`-`/
        // `*`/`/` all route through a host call that the stub answers with
        // `NOT_IMPLEMENTED` regardless of the operands' actual bits.
        let one = XFL::one();
        assert!(matches!(XFL::new(0, 1), Err(HookError::NotImplemented)));
        assert!(matches!(one + one, Err(HookError::NotImplemented)));
        assert!(matches!(one - one, Err(HookError::NotImplemented)));
        assert!(matches!(one * one, Err(HookError::NotImplemented)));
        assert!(matches!(one / one, Err(HookError::NotImplemented)));
        assert!(matches!(one.invert(), Err(HookError::NotImplemented)));
        assert!(matches!(
            one.mulratio(false, 1, 2),
            Err(HookError::NotImplemented)
        ));
        assert_eq!(one.mantissa(), Err(HookError::NotImplemented));
        assert_eq!(one.sign(), Err(HookError::NotImplemented));
        assert_eq!(one.to_int(0, false), Err(HookError::NotImplemented));
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
        let one = XFL::one();
        let err: Result<XFL> = Err(HookError::DoesntExist);
        assert_eq!(err + one, Err(HookError::DoesntExist));
        assert_eq!(one + err, Err(HookError::DoesntExist));
        assert_eq!(err - one, Err(HookError::DoesntExist));
        assert_eq!(one - err, Err(HookError::DoesntExist));
        assert_eq!(err * one, Err(HookError::DoesntExist));
        assert_eq!(one * err, Err(HookError::DoesntExist));
        assert_eq!(err / one, Err(HookError::DoesntExist));
        assert_eq!(one / err, Err(HookError::DoesntExist));
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

    #[test]
    fn neg_is_a_pure_local_sign_flip() {
        let positive_one = XFL::from_raw_bits(POSITIVE_ONE_BITS);
        let negative_one = XFL::from_raw_bits(NEGATIVE_ONE_BITS);
        assert_eq!((-positive_one).raw_bits(), NEGATIVE_ONE_BITS);
        assert_eq!((-negative_one).raw_bits(), POSITIVE_ONE_BITS);
        // Canonical zero stays zero (not flipped to some nonzero pattern).
        assert_eq!((-XFL::from_raw_bits(0)).raw_bits(), 0);
    }

    #[test]
    fn eq_is_bitwise_and_local() {
        let a = XFL::from_raw_bits(POSITIVE_ONE_BITS);
        let b = XFL::from_raw_bits(POSITIVE_ONE_BITS);
        let c = XFL::from_raw_bits(NEGATIVE_ONE_BITS);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn ord_handles_sign_magnitude_correctly() {
        let zero = XFL::from_raw_bits(0);
        let positive_one = XFL::from_raw_bits(POSITIVE_ONE_BITS);
        let positive_ten = XFL::from_raw_bits(POSITIVE_TEN_BITS);
        let negative_one = XFL::from_raw_bits(NEGATIVE_ONE_BITS);
        let negative_ten = XFL::from_raw_bits(NEGATIVE_TEN_BITS);

        // Naive raw-bit comparison would get this backwards: -10's
        // magnitude bits are numerically larger than -1's.
        assert!(negative_ten < negative_one);
        assert!(negative_one < zero);
        assert!(zero < positive_one);
        assert!(positive_one < positive_ten);
        assert!(negative_ten < positive_one);
        assert_eq!(
            negative_one.partial_cmp(&negative_one),
            Some(core::cmp::Ordering::Equal)
        );
    }
}
