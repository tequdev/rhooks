//! `Deposit` and `r#Deposit` are the same identifier — the escape exists
//! only to get a name past the lexer, and is not part of the name.
//!
//! The collision checks canonicalize before comparing, so these are caught
//! at the caller's own tokens rather than later, as an
//! infinitely-sized-type or duplicate-definition error inside generated
//! code.

#![allow(non_camel_case_types)]

use hooks_lib::{HookData, HookKey, hook_state};

#[derive(HookKey, Clone, Copy)]
struct r#Deposit {
    tag: u8,
}

#[derive(HookData, Clone, Copy)]
struct Amount {
    count: u32,
}

fn main() {
    // Entity == pairing key, spelled raw.
    hook_state!(Deposit, r#Deposit => Amount);

    // Entity == already-declared value, spelled raw.
    hook_state!(Amount, AmountKey {tag: u8} => r#Amount);
}
