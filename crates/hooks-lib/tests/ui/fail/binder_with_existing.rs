//! An instance binder combined with the `existing` keyword form: the
//! `existing` form emits impls for a module-owned type, which cannot come
//! from inside a function body.

use hooks_lib::hook_state;

struct OwnKey;

fn main() {
    hook_state!(own, existing OwnKey = b"OK" => u64);
}
