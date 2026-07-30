//! A binder invocation expands to items *plus a `let`*, so it is statement
//! position only. At module scope rustc rejects the `let` with its own
//! "expected item" error.

use hooks_lib::hook_state;

hook_state!(counter, CounterKey = b"CTR" => u64);

fn main() {}
