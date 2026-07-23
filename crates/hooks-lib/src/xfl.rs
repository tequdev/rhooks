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
//! separate `api::*` wrappers, only as `XFL` methods below): `float_set`,
//! `float_multiply`, `float_mulratio`, `float_negate`, `float_compare`,
//! `float_sum`, `float_invert`, `float_divide`, `float_one`,
//! `float_mantissa`, `float_sign`, `float_int`, `float_log`, `float_root`.
//! The remaining 3 buffer-shaped float functions (`float_sto`,
//! `float_sto_set`, `slot_float`) live in `api::float` and are forwarded to
//! by [`XFL::sto`], [`XFL::sto_set`], and [`XFL::from_slot`].

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
/// No `PartialEq`/`PartialOrd`/`core::ops::*` impls: comparison and
/// arithmetic are fallible host calls, and silently panicking or silently
/// returning `false` on error is unacceptable for financial logic. Use
/// [`XFL::eq`]/[`XFL::lt`]/[`XFL::gt`]/[`XFL::compare`] instead, all of
/// which return `Result<bool>`.
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

// `mul`/`add`/`div`/`neg` intentionally share names with the
// `core::ops::{Mul, Add, Div, Neg}` trait methods for readability, but this
// type deliberately does NOT implement those traits (see the type doc
// comment: arithmetic here is a fallible host call, and the operator traits
// require an infallible result, which would force a panic on error).
#[allow(clippy::should_implement_trait)]
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

    /// `self * rhs`.
    #[inline(always)]
    pub fn mul(self, rhs: XFL) -> Result<XFL> {
        res(unsafe { hooks_core::float_multiply(self.0, rhs.0) }).map(XFL::from_raw_bits)
    }

    /// `self + rhs`.
    #[inline(always)]
    pub fn add(self, rhs: XFL) -> Result<XFL> {
        res(unsafe { hooks_core::float_sum(self.0, rhs.0) }).map(XFL::from_raw_bits)
    }

    /// `self / rhs`.
    #[inline(always)]
    pub fn div(self, rhs: XFL) -> Result<XFL> {
        res(unsafe { hooks_core::float_divide(self.0, rhs.0) }).map(XFL::from_raw_bits)
    }

    /// `-self`.
    #[inline(always)]
    pub fn neg(self) -> Result<XFL> {
        res(unsafe { hooks_core::float_negate(self.0) }).map(XFL::from_raw_bits)
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
    /// `hooks_core::{COMPARE_EQUAL, COMPARE_LESS, COMPARE_GREATER}`).
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
        // `XFL` deliberately has no `PartialEq` (see the type doc comment),
        // so `Result<XFL, HookError>` can't be compared with `assert_eq!`.
        // `matches!` sidesteps that without needing `unwrap`/`expect`.
        let one = XFL::one();
        assert!(matches!(XFL::new(0, 1), Err(HookError::NotImplemented)));
        assert!(matches!(one.mul(one), Err(HookError::NotImplemented)));
        assert!(matches!(one.add(one), Err(HookError::NotImplemented)));
        assert!(matches!(one.div(one), Err(HookError::NotImplemented)));
        assert!(matches!(one.neg(), Err(HookError::NotImplemented)));
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
