//! `accept-all` — the minimal starter Hook: traces a short message, then
//! unconditionally accepts the originating transaction.
//!
//! Build: `hooks-build build --manifest-path examples/accept-all/Cargo.toml`

#![no_std]

use hooks_lib::{accept, hook, trace};

/// Hook entry point. Accepts every transaction it is invoked for.
#[hook]
fn my_hook() -> i64 {
    trace!(b"accept-all: accepting transaction");
    accept!()
}
