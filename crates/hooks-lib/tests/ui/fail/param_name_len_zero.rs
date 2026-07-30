//! A caller-authored `ToBytes` name encoding to **0** bytes, paired through
//! the two-type form. The concrete `with_name_bytes` override this generates
//! replaces the trait default's own `1..=PARAM_NAME_MAX_LEN` assertion, so
//! the override carries a monomorphized copy of it — without which a name
//! the Hook API rejects at runtime would compile silently.

use hooks_lib::convert::ToBytes;
use hooks_lib::{ParamValue, hook_parameter};

struct ZeroLenName;

impl ToBytes for ZeroLenName {
    const MAX_LEN: usize = 0;

    fn write(&self, _buf: &mut [u8]) -> usize {
        0
    }
}

#[derive(ParamValue)]
struct Value {
    v: u8,
}

hook_parameter!(ZeroLenName => Value);

fn main() {}
