//! The two-type pairing form declares nothing, and deliberately grows no
//! inherent methods: it must not silently claim six method names on a type
//! this macro does not own.

use hooks_lib::{HookData, HookKey, hook_state};

#[derive(HookKey, Clone, Copy)]
struct MyKey {
    tag: u8,
}

#[derive(HookData, Clone, Copy)]
struct MyValue {
    count: u32,
}

hook_state!(MyKey => MyValue);

fn main() {
    let _ = MyKey { tag: 0 }.get_state();
}
