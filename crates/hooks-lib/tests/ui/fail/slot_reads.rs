//! The read table is per-marker: containers have no `value()`, and a typed
//! scalar has no `as_xfl()`. Reaching for the wrong one is a compile error,
//! not a runtime decode failure.

use hooks_lib::prelude::*;
use hooks_lib::slot_obj::Opaque;

fn main() {
    fn container(root: SlotObject<STObject>) -> Result<()> {
        // An STObject is read through its fields, or through the raw escapes.
        let _ = root.value()?;
        Ok(())
    }

    fn opaque(blob: SlotObject<Opaque>) -> Result<()> {
        // Opaque means "this layer does not model a typed read for it".
        let _ = blob.value()?;
        Ok(())
    }

    fn scalar(seq: SlotObject<u32>) -> Result<()> {
        // `as_xfl` is an Amount operation.
        let _ = seq.as_xfl()?;
        Ok(())
    }

    let _ = (container, opaque, scalar);
}
