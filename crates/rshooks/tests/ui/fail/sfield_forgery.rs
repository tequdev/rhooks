//! `SField::new` is `pub(crate)`: the typed field table is generated from
//! the header, and a public constructor would let safe downstream code forge
//! any code/type pair it liked — spelling a 20-byte currency field as an
//! `SField<AccountId>` so that `.value()` hands back an `AccountId` built
//! from currency bytes.
//!
//! Reading a field as something it is not stays possible — that is what
//! `SlotObject::assume_type` is for — but only by saying so at the call
//! site, not by silently mislabelling the field.

use rshooks::prelude::*;

fn main() {
    // Forging a field code with a chosen value type.
    let forged: SField<AccountId> = SField::new(rshooks::raw::sfcodes::sfTakerPaysCurrency);
    let _ = forged;

    // The same via the fully-qualified path.
    let _ = rshooks::slot_obj::SField::<u64>::new(0);
}
