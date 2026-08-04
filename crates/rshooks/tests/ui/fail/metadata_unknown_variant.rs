//! Bare identifiers are resolved against the generated `TxType` enum rather
//! than accepted as unchecked strings.

use rshooks::metadata;

metadata! {
    name: "typo",
    HookOn: [Paymant],
}

fn main() {}
