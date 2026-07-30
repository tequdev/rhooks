//! `existing` and the pairing form emit impls for a type the surrounding
//! *module* owns, so they are supported at module scope only. Inside a
//! function body those are non-local definitions.
//!
//! This is a **policy**, not a parser rule — a proc macro cannot see its own
//! position. `#![deny(non_local_definitions)]` is what turns rustc's own
//! lint into the error this fixture pins, which is exactly how a caller who
//! cares would catch it.

#![deny(non_local_definitions)]

use hooks_lib::{HookData, HookKey, hook_state};

/// A key type the module owns.
struct OwnKey;

#[derive(HookKey, Clone, Copy)]
struct PairedKey {
    tag: u8,
}

#[derive(HookData, Clone, Copy)]
struct PairedValue {
    count: u32,
}

fn main() {
    hook_state!(OwnState, existing OwnKey = b"OK" => u64);
    hook_state!(PairedState, PairedKey => PairedValue);
}
