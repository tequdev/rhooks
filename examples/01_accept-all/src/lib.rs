#![no_std]

use rshooks::*;

metadata! {
    name: "accept-all",
    description: "Accepts every transaction selected by HookOn.",
    HookOn: [Invoke],
    HookName: "accept",
}

#[hook]
fn my_hook() -> i64 {
    trace!(b"accept-all: accepting transaction");
    accept!()
}
