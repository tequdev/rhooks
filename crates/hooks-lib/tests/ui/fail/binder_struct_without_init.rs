//! An instance binder on a struct form with no initializer: a bound key
//! whose fields were never given values would silently address an all-zero
//! ledger key.

use hooks_lib::hook_state;

fn main() {
    hook_state!(deposit, DepositKey {tag: u8, owner: u64} => DepositValue {amount: u64});
}
