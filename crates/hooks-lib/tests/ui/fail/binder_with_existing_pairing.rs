//! An instance binder combined with the two-type pairing form: nothing is
//! declared, so there is no instance for the binder to be one of.

use hooks_lib::{HookData, HookKey, hook_state};

#[derive(HookKey, Clone, Copy)]
struct MyKey {
    tag: u8,
}

#[derive(HookData, Clone, Copy)]
struct MyValue {
    count: u32,
}

fn main() {
    hook_state!(my_key, MyKey => MyValue);
}
