//! An instance binder whose initializer forgets a field: this is *not* a
//! macro diagnostic — the initializer is re-emitted verbatim inside
//! `DepositKey { .. }`, so rustc's own missing-field error lands on the
//! caller's own tokens. Definite initialization is preserved.

use hooks_lib::hook_state;

fn main() {
    hook_state!(deposit, DepositKey {tag: u8, owner: u64} = {tag: 1}
                => DepositValue {amount: u64});
}
