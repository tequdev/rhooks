//! A pairing form's key must not already be paired: the second declaration
//! emits a second `impl TypedStateKey for MyKey`, which is rustc's `E0119`.
//!
//! Not macro-enforced — the macro sees one invocation at a time and has no
//! way to know what another one did. Pinned so the failure mode stays a
//! plain conflicting-implementations error rather than something stranger.

use rshooks::{HookData, HookKey, hook_state};

#[derive(HookKey, Clone, Copy)]
struct MyKey {
    tag: u8,
}

#[derive(HookData, Clone, Copy)]
struct MyValue {
    count: u32,
}

hook_state!(FirstState, MyKey => MyValue);
hook_state!(SecondState, MyKey => MyValue);

fn main() {}
