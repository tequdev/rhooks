//! Navigation is gated by the parent's type: a field code addresses a field
//! of an object, an index addresses an element of an array, and neither
//! works on the other. The user's original strictness, on a single method.

use hooks_lib::prelude::*;

fn main() {
    fn object(root: SlotObject<STObject>) -> Result<()> {
        // An object has no element 0.
        let _ = root.get(0u32)?;
        Ok(())
    }

    fn array(entries: SlotObject<STArray>) -> Result<()> {
        // An array has no `sfAccount` field.
        let _ = entries.get(sfAccount)?;
        Ok(())
    }

    fn counted(root: SlotObject<STObject>) -> Result<()> {
        // `count()` is an array (or Opaque) operation.
        let _ = root.count()?;
        Ok(())
    }

    let _ = (object, array, counted);
}
