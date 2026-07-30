//! A type name this macro itself declares must be UpperCamelCase — the
//! check that predates the instance binder, and that a lowercase leading
//! identifier still reaches when no comma follows it.

use hooks_lib::hook_state;

hook_state!(my_key = b"MK" => u64);

fn main() {}
