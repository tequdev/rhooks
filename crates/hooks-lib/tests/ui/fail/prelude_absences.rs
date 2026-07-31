//! Two things deliberately left the prelude, and this pins that they stay
//! gone: the raw `sfcodes` glob (the typed `SField` constants took those
//! names) and the numbered slot functions (they address the same registers
//! `SlotObject` manages — mixing the two silently corrupts handles).
//!
//! Both are still reachable by explicit path; the pass fixture beside this
//! one compiles those paths.

use hooks_lib::prelude::*;

fn main() {
    // Numbered slot functions: not in the prelude.
    let _ = slot_clear(1);
    let _ = slot_set(&[0u8; 34], 1);
    let _ = otxn_slot(0);
    let _ = meta_slot(0);
    let _ = slot_subarray(1, 0, 0);

    // `sfSequence` *is* in scope, but as the typed constant — so it is not a
    // `u32` and cannot be used where one is required by type.
    let _: u32 = sfSequence;
}
