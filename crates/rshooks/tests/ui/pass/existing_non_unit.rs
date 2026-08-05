//! The `existing` form over a caller-owned key type that **cannot be
//! constructed** from the invocation site: a private field, no `Default`,
//! no `Copy`, no public constructor.
//!
//! This compiles only because generated code never builds the key. The
//! entity encodes the declared literal itself; the caller's type receives
//! the encoding impls and is never instantiated.

use rshooks::prelude::*;
use rshooks::hook_state;

mod owned {
    /// A key type with no way to build one from outside this module.
    pub struct Locked {
        _private: (),
    }
}

hook_state!(LockedState, existing owned::Locked = b"LK" => u64);

fn main() {
    assert_eq!(
        LockedState.get_state().err(),
        Some(HookError::NotImplemented)
    );
    assert_eq!(LockedState.delete_state(), Err(HookError::NotImplemented));
}
