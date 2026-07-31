//! `SlotObject` is affine: it is neither `Copy` nor `Clone`, and every
//! operation that ends the slot's life takes `self`. A stale duplicate could
//! read or clear a slot the host has since handed to something else.

use hooks_lib::prelude::*;

fn main() {
    fn no_copy(root: SlotObject<STObject>) -> Result<()> {
        let moved = root;
        // `root` was moved, not copied.
        let _ = root.field_code()?;
        let _ = moved.clear();
        Ok(())
    }

    fn moved_after_clear(root: SlotObject<STObject>) -> Result<()> {
        root.clear()?;
        // `clear` consumed it.
        let _ = root.field_code()?;
        Ok(())
    }

    fn use_after_value(seq: SlotObject<u32>) -> Result<()> {
        let _first = seq.value()?;
        // A terminal read consumed the handle.
        let _second = seq.value()?;
        Ok(())
    }

    let _ = (no_copy, moved_after_clear, use_after_value);
}
