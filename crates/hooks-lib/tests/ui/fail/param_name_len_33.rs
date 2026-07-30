//! A caller-authored `ToBytes` name encoding to **33** bytes — one past the
//! Hook API's parameter-name upper bound — paired through the two-type form.
//! See `param_name_len_zero.rs` for why the concrete override needs its own
//! assertion.

use hooks_lib::convert::ToBytes;
use hooks_lib::{ParamValue, hook_parameter};

struct TooLongName;

impl ToBytes for TooLongName {
    const MAX_LEN: usize = 33;

    fn write(&self, _buf: &mut [u8]) -> usize {
        0
    }
}

#[derive(ParamValue)]
struct Value {
    v: u8,
}

hook_parameter!(TooLongName => Value);

fn main() {}
