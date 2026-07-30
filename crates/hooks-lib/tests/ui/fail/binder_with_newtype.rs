//! An instance binder combined with Form 4: a newtype's instance needs the
//! inner value, which this grammar has nowhere to spell.

use hooks_lib::hook_state;

fn main() {
    hook_state!(account, AccountKey [u8; 20] => u64);
}
