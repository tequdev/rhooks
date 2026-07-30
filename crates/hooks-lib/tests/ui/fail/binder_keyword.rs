//! A binder spelled as a keyword reserved for a future edition (`gen`,
//! edition 2024): rejected by name, rather than left to rustc's parse error
//! inside the expansion.

use hooks_lib::hook_state;

fn main() {
    hook_state!(gen, GenKey = b"GK" => u64);
}
