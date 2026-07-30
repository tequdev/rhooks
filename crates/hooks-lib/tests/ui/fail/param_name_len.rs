//! Caller-authored `ToBytes` name types whose encoded length falls outside
//! the Hook API's `1..=32` bound, paired through the two-type form.
//!
//! The concrete `with_name_bytes` override these declarations generate for
//! the name *replaces* the trait default's body — and with it the default's
//! own length assertion — so each override carries a monomorphized copy. Without
//! that copy a 0-byte name (which the host rejects at runtime) or a
//! multi-kilobyte stack buffer would compile with no complaint at all.

use hooks_lib::convert::ToBytes;
use hooks_lib::{ParamValue, hook_parameter};

#[derive(ParamValue)]
struct Value {
    v: u8,
}

/// Encodes to nothing: below the Hook API's 1-byte lower bound.
struct ZeroLenName;

impl ToBytes for ZeroLenName {
    const MAX_LEN: usize = 0;

    fn write(&self, _buf: &mut [u8]) -> usize {
        0
    }
}

/// One byte past the Hook API's 32-byte upper bound.
struct TooLongName;

impl ToBytes for TooLongName {
    const MAX_LEN: usize = 33;

    fn write(&self, _buf: &mut [u8]) -> usize {
        0
    }
}

hook_parameter!(ZeroLen, ZeroLenName => Value);
hook_parameter!(TooLong, TooLongName => Value);

fn main() {}
