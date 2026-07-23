//! Hook API error model.
//!
//! Every Hook API function returns an `i64`: non-negative values are success
//! payloads (often "bytes written" or a slot/field-pointer value), negative
//! values are one of the 45 error codes from `hook/error.h`. [`HookError`] is
//! a typed, exhaustive-by-construction mirror of those codes (plus
//! [`HookError::Unknown`] for forward-compatibility with codes this crate
//! does not yet know about).

/// A Hook API error, decoded from the negative `i64` return of a raw
/// `hooks-core` call.
///
/// # Examples
///
/// ```
/// use hooks_lib::error::HookError;
///
/// let err = HookError::from(-5);
/// assert_eq!(err, HookError::DoesntExist);
/// assert_eq!(err.code(), -5);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookError {
    /// `OUT_OF_BOUNDS` (-1): memory access out of bounds.
    OutOfBounds,
    /// `INTERNAL_ERROR` (-2): unexpected internal error.
    InternalError,
    /// `TOO_BIG` (-3): data is too large.
    TooBig,
    /// `TOO_SMALL` (-4): buffer is too small.
    TooSmall,
    /// `DOESNT_EXIST` (-5): requested item does not exist.
    DoesntExist,
    /// `NO_FREE_SLOTS` (-6): no available slots.
    NoFreeSlots,
    /// `INVALID_ARGUMENT` (-7): argument is invalid.
    InvalidArgument,
    /// `ALREADY_SET` (-8): already configured.
    AlreadySet,
    /// `PREREQUISITE_NOT_MET` (-9): prerequisite condition not satisfied.
    PrerequisiteNotMet,
    /// `FEE_TOO_LARGE` (-10): fee is too large.
    FeeTooLarge,
    /// `EMISSION_FAILURE` (-11): transaction emission failed.
    EmissionFailure,
    /// `TOO_MANY_NONCES` (-12): nonce generation limit exceeded.
    TooManyNonces,
    /// `TOO_MANY_EMITTED_TXN` (-13): emitted transaction limit exceeded.
    TooManyEmittedTxn,
    /// `NOT_IMPLEMENTED` (-14): feature not implemented (also what every
    /// host-build stub returns).
    NotImplemented,
    /// `INVALID_ACCOUNT` (-15): account ID is invalid.
    InvalidAccount,
    /// `GUARD_VIOLATION` (-16): infinite-loop guard violation.
    GuardViolation,
    /// `INVALID_FIELD` (-17): field ID is invalid.
    InvalidField,
    /// `PARSE_ERROR` (-18): failed to parse data.
    ParseError,
    /// `RC_ROLLBACK` (-19): hook execution terminated via rollback.
    RcRollback,
    /// `RC_ACCEPT` (-20): hook execution terminated via accept.
    RcAccept,
    /// `NO_SUCH_KEYLET` (-21): keylet not found.
    NoSuchKeylet,
    /// `NOT_AN_ARRAY` (-22): object is not an array.
    NotAnArray,
    /// `NOT_AN_OBJECT` (-23): object is not an object.
    NotAnObject,
    /// `INVALID_FLOAT` (-10024, verbatim from the header — not -24): XFL
    /// value is invalid.
    InvalidFloat,
    /// `DIVISION_BY_ZERO` (-25): division by zero.
    DivisionByZero,
    /// `MANTISSA_OVERSIZED` (-26): XFL mantissa too large.
    MantissaOversized,
    /// `MANTISSA_UNDERSIZED` (-27): XFL mantissa too small.
    MantissaUndersized,
    /// `EXPONENT_OVERSIZED` (-28): XFL exponent too large.
    ExponentOversized,
    /// `EXPONENT_UNDERSIZED` (-29): XFL exponent too small.
    ExponentUndersized,
    /// `XFL_OVERFLOW` (-30): XFL arithmetic overflow.
    XflOverflow,
    /// `NOT_IOU_AMOUNT` (-31): not an IOU amount.
    NotIouAmount,
    /// `NOT_AN_AMOUNT` (-32): not an amount type.
    NotAnAmount,
    /// `CANT_RETURN_NEGATIVE` (-33): cannot return a negative value.
    CantReturnNegative,
    /// `NOT_AUTHORIZED` (-34): no access permission.
    NotAuthorized,
    /// `PREVIOUS_FAILURE_PREVENTS_RETRY` (-35): a previous failure prevents
    /// retrying this operation.
    PreviousFailurePreventsRetry,
    /// `TOO_MANY_PARAMS` (-36): parameter limit exceeded.
    TooManyParams,
    /// `INVALID_TXN` (-37): transaction is invalid.
    InvalidTxn,
    /// `RESERVE_INSUFFICIENT` (-38): reserve insufficient for the operation.
    ReserveInsufficient,
    /// `COMPLEX_NOT_SUPPORTED` (-39): complex-domain result not supported.
    ComplexNotSupported,
    /// `DOES_NOT_MATCH` (-40): values do not match.
    DoesNotMatch,
    /// `INVALID_KEY` (-41): key is invalid.
    InvalidKey,
    /// `NOT_A_STRING` (-42): value is not a string.
    NotAString,
    /// `MEM_OVERLAP` (-43): memory regions overlap.
    MemOverlap,
    /// `TOO_MANY_STATE_MODIFICATIONS` (-44): state modification limit
    /// exceeded.
    TooManyStateModifications,
    /// `TOO_MANY_NAMESPACES` (-45): namespace limit exceeded.
    TooManyNamespaces,
    /// A negative code this version of hooks-lib does not recognize yet.
    /// Carries the raw code for forward-compatibility.
    Unknown(i64),
}

impl From<i64> for HookError {
    fn from(code: i64) -> Self {
        match code {
            hooks_core::OUT_OF_BOUNDS => HookError::OutOfBounds,
            hooks_core::INTERNAL_ERROR => HookError::InternalError,
            hooks_core::TOO_BIG => HookError::TooBig,
            hooks_core::TOO_SMALL => HookError::TooSmall,
            hooks_core::DOESNT_EXIST => HookError::DoesntExist,
            hooks_core::NO_FREE_SLOTS => HookError::NoFreeSlots,
            hooks_core::INVALID_ARGUMENT => HookError::InvalidArgument,
            hooks_core::ALREADY_SET => HookError::AlreadySet,
            hooks_core::PREREQUISITE_NOT_MET => HookError::PrerequisiteNotMet,
            hooks_core::FEE_TOO_LARGE => HookError::FeeTooLarge,
            hooks_core::EMISSION_FAILURE => HookError::EmissionFailure,
            hooks_core::TOO_MANY_NONCES => HookError::TooManyNonces,
            hooks_core::TOO_MANY_EMITTED_TXN => HookError::TooManyEmittedTxn,
            hooks_core::NOT_IMPLEMENTED => HookError::NotImplemented,
            hooks_core::INVALID_ACCOUNT => HookError::InvalidAccount,
            hooks_core::GUARD_VIOLATION => HookError::GuardViolation,
            hooks_core::INVALID_FIELD => HookError::InvalidField,
            hooks_core::PARSE_ERROR => HookError::ParseError,
            hooks_core::RC_ROLLBACK => HookError::RcRollback,
            hooks_core::RC_ACCEPT => HookError::RcAccept,
            hooks_core::NO_SUCH_KEYLET => HookError::NoSuchKeylet,
            hooks_core::NOT_AN_ARRAY => HookError::NotAnArray,
            hooks_core::NOT_AN_OBJECT => HookError::NotAnObject,
            hooks_core::INVALID_FLOAT => HookError::InvalidFloat,
            hooks_core::DIVISION_BY_ZERO => HookError::DivisionByZero,
            hooks_core::MANTISSA_OVERSIZED => HookError::MantissaOversized,
            hooks_core::MANTISSA_UNDERSIZED => HookError::MantissaUndersized,
            hooks_core::EXPONENT_OVERSIZED => HookError::ExponentOversized,
            hooks_core::EXPONENT_UNDERSIZED => HookError::ExponentUndersized,
            hooks_core::XFL_OVERFLOW => HookError::XflOverflow,
            hooks_core::NOT_IOU_AMOUNT => HookError::NotIouAmount,
            hooks_core::NOT_AN_AMOUNT => HookError::NotAnAmount,
            hooks_core::CANT_RETURN_NEGATIVE => HookError::CantReturnNegative,
            hooks_core::NOT_AUTHORIZED => HookError::NotAuthorized,
            hooks_core::PREVIOUS_FAILURE_PREVENTS_RETRY => HookError::PreviousFailurePreventsRetry,
            hooks_core::TOO_MANY_PARAMS => HookError::TooManyParams,
            hooks_core::INVALID_TXN => HookError::InvalidTxn,
            hooks_core::RESERVE_INSUFFICIENT => HookError::ReserveInsufficient,
            hooks_core::COMPLEX_NOT_SUPPORTED => HookError::ComplexNotSupported,
            hooks_core::DOES_NOT_MATCH => HookError::DoesNotMatch,
            hooks_core::INVALID_KEY => HookError::InvalidKey,
            hooks_core::NOT_A_STRING => HookError::NotAString,
            hooks_core::MEM_OVERLAP => HookError::MemOverlap,
            hooks_core::TOO_MANY_STATE_MODIFICATIONS => HookError::TooManyStateModifications,
            hooks_core::TOO_MANY_NAMESPACES => HookError::TooManyNamespaces,
            other => HookError::Unknown(other),
        }
    }
}

impl HookError {
    /// The raw negative `i64` error code this variant corresponds to. Exact
    /// inverse of [`HookError::from`]: `HookError::from(c).code() == c` for
    /// every code, known or unknown.
    #[must_use]
    pub fn code(&self) -> i64 {
        match *self {
            HookError::OutOfBounds => hooks_core::OUT_OF_BOUNDS,
            HookError::InternalError => hooks_core::INTERNAL_ERROR,
            HookError::TooBig => hooks_core::TOO_BIG,
            HookError::TooSmall => hooks_core::TOO_SMALL,
            HookError::DoesntExist => hooks_core::DOESNT_EXIST,
            HookError::NoFreeSlots => hooks_core::NO_FREE_SLOTS,
            HookError::InvalidArgument => hooks_core::INVALID_ARGUMENT,
            HookError::AlreadySet => hooks_core::ALREADY_SET,
            HookError::PrerequisiteNotMet => hooks_core::PREREQUISITE_NOT_MET,
            HookError::FeeTooLarge => hooks_core::FEE_TOO_LARGE,
            HookError::EmissionFailure => hooks_core::EMISSION_FAILURE,
            HookError::TooManyNonces => hooks_core::TOO_MANY_NONCES,
            HookError::TooManyEmittedTxn => hooks_core::TOO_MANY_EMITTED_TXN,
            HookError::NotImplemented => hooks_core::NOT_IMPLEMENTED,
            HookError::InvalidAccount => hooks_core::INVALID_ACCOUNT,
            HookError::GuardViolation => hooks_core::GUARD_VIOLATION,
            HookError::InvalidField => hooks_core::INVALID_FIELD,
            HookError::ParseError => hooks_core::PARSE_ERROR,
            HookError::RcRollback => hooks_core::RC_ROLLBACK,
            HookError::RcAccept => hooks_core::RC_ACCEPT,
            HookError::NoSuchKeylet => hooks_core::NO_SUCH_KEYLET,
            HookError::NotAnArray => hooks_core::NOT_AN_ARRAY,
            HookError::NotAnObject => hooks_core::NOT_AN_OBJECT,
            HookError::InvalidFloat => hooks_core::INVALID_FLOAT,
            HookError::DivisionByZero => hooks_core::DIVISION_BY_ZERO,
            HookError::MantissaOversized => hooks_core::MANTISSA_OVERSIZED,
            HookError::MantissaUndersized => hooks_core::MANTISSA_UNDERSIZED,
            HookError::ExponentOversized => hooks_core::EXPONENT_OVERSIZED,
            HookError::ExponentUndersized => hooks_core::EXPONENT_UNDERSIZED,
            HookError::XflOverflow => hooks_core::XFL_OVERFLOW,
            HookError::NotIouAmount => hooks_core::NOT_IOU_AMOUNT,
            HookError::NotAnAmount => hooks_core::NOT_AN_AMOUNT,
            HookError::CantReturnNegative => hooks_core::CANT_RETURN_NEGATIVE,
            HookError::NotAuthorized => hooks_core::NOT_AUTHORIZED,
            HookError::PreviousFailurePreventsRetry => hooks_core::PREVIOUS_FAILURE_PREVENTS_RETRY,
            HookError::TooManyParams => hooks_core::TOO_MANY_PARAMS,
            HookError::InvalidTxn => hooks_core::INVALID_TXN,
            HookError::ReserveInsufficient => hooks_core::RESERVE_INSUFFICIENT,
            HookError::ComplexNotSupported => hooks_core::COMPLEX_NOT_SUPPORTED,
            HookError::DoesNotMatch => hooks_core::DOES_NOT_MATCH,
            HookError::InvalidKey => hooks_core::INVALID_KEY,
            HookError::NotAString => hooks_core::NOT_A_STRING,
            HookError::MemOverlap => hooks_core::MEM_OVERLAP,
            HookError::TooManyStateModifications => hooks_core::TOO_MANY_STATE_MODIFICATIONS,
            HookError::TooManyNamespaces => hooks_core::TOO_MANY_NAMESPACES,
            HookError::Unknown(code) => code,
        }
    }
}

/// The result type every hooks-lib wrapper returns: `Ok(payload)` for a
/// non-negative Hook API return, `Err(HookError)` for a negative one.
pub type Result<T> = core::result::Result<T, HookError>;

/// Funnel point every wrapper in `api/*.rs` and `xfl.rs` calls through:
/// convert a raw Hook API `i64` return into a [`Result<i64>`] by sign.
#[inline(always)]
pub(crate) fn res(code: i64) -> Result<i64> {
    if code < 0 {
        Err(HookError::from(code))
    } else {
        Ok(code)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_known_codes() {
        let known: &[i64] = &[
            -1, -2, -3, -4, -5, -6, -7, -8, -9, -10, -11, -12, -13, -14, -15, -16, -17, -18, -19,
            -20, -21, -22, -23, -10024, -25, -26, -27, -28, -29, -30, -31, -32, -33, -34, -35, -36,
            -37, -38, -39, -40, -41, -42, -43, -44, -45,
        ];
        for &code in known {
            assert_eq!(
                HookError::from(code).code(),
                code,
                "round-trip failed for {code}"
            );
        }
    }

    #[test]
    fn invalid_float_is_irregular() {
        assert_eq!(HookError::from(-10024), HookError::InvalidFloat);
        assert_eq!(HookError::InvalidFloat.code(), -10024);
    }

    #[test]
    fn unknown_code_round_trips() {
        let err = HookError::from(-9999);
        assert_eq!(err, HookError::Unknown(-9999));
        assert_eq!(err.code(), -9999);
    }

    #[test]
    fn res_splits_on_sign() {
        assert_eq!(res(0), Ok(0));
        assert_eq!(res(32), Ok(32));
        assert_eq!(res(-5), Err(HookError::DoesntExist));
    }
}
