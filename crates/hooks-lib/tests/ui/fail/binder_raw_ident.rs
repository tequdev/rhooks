//! A raw-identifier binder: detected by its `r#` prefix before any other
//! check, so it is rejected as a raw identifier rather than sailing past the
//! keyword list by spelling.

use hooks_lib::hook_state;

fn main() {
    hook_state!(r#fn, MyKey = b"MK" => u64);
}
