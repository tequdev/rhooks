//! A lone `_` binder: `let _ = Key;` would bind nothing, so it is rejected
//! in favor of dropping the binder entirely.

use hooks_lib::hook_state;

fn main() {
    hook_state!(_, MyKey = b"MK" => u64);
}
