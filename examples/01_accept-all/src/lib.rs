#![no_std]

use hooks_lib::*;

#[hook]
fn my_hook() -> i64 {
    trace!(b"accept-all: accepting transaction");
    accept!()
}
