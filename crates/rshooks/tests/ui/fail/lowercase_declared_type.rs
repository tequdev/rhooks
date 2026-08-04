//! A key type this macro itself declares must be UpperCamelCase. The
//! entity name is checked by the same rule (see `entity_names.rs`); this
//! pins the key side, which is reached only after the entity parses.

use rshooks::hook_state;

hook_state!(MyState, my_key = b"MK" => u64);

fn main() {}
