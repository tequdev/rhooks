#![no_std]

use hooks_lib::{accept, hook, trace};

#[hook]
fn my_hook() -> i64 {
    trace!(b"accept-all: accepting transaction");
    accept!()
}
