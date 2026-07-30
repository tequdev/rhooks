//! The four declaration forms that reject an instance binder, each with a
//! diagnostic naming the way out.
//!
//! Only Form 1 and a struct form carrying an explicit initializer can name a
//! complete instance; every other form either has nothing to bind or would
//! have to emit module-owned impls from inside a function body.

use hooks_lib::{HookData, HookKey, hook_state};

/// A key type the module owns, for the `existing` case below.
struct OwnKey;

#[derive(HookKey, Clone, Copy)]
struct PairedKey {
    tag: u8,
}

#[derive(HookData, Clone, Copy)]
struct PairedValue {
    count: u32,
}

fn main() {
    // A struct form with no initializer: a bound key whose fields were never
    // given values would silently address an all-zero ledger key.
    hook_state!(deposit, DepositKey {tag: u8, owner: [u8; 20]} => DepositValue {amount: u64});

    // `existing` emits impls for a type the *module* owns — non-local impls
    // from a function body, and a collision between any two functions that
    // did it.
    hook_state!(own, existing OwnKey = b"OK" => u64);

    // A newtype's instance needs the inner value, which this grammar has
    // nowhere to spell.
    hook_state!(account, AccountKey [u8; 20] => u64);

    // The two-type pairing form declares nothing, so there is no instance
    // for the binder to be one of.
    hook_state!(paired, PairedKey => PairedValue);
}
