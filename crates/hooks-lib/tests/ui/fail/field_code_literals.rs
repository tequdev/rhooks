//! Field-code parameters take `impl Into<u32>`, so a bare integer literal no
//! longer infers — it needs a `u32` suffix. A documented migration break; the
//! pass fixture beside this one shows the fixed form.

use hooks_lib::prelude::*;

fn main() {
    let mut buf = [0u8; 20];
    let _ = otxn_field(&mut buf, 0);
    let _ = otxn_field_u64(0);
    let _ = otxn_field_exact::<[u8; 20]>(0);
}
