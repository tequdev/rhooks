//! The `existing` keyword form, on all three macros and with the caller
//! marker types it has to tolerate.
//!
//! `existing` attaches the fixed-bytes impls, the pairing, and the inherent
//! accessors to a key/name type the **caller** declared — so unlike a name
//! the macro declares itself, its spelling is the caller's business and is
//! deliberately not naming-checked. Both a `snake_case` marker and a raw
//! identifier must keep working.

#![allow(non_camel_case_types)]

use hooks_lib::prelude::*;
use hooks_lib::{ParamValue, hook_parameter, hook_state, otxn_parameter};

#[derive(ParamValue)]
struct Config {
    min_amount: u64,
}

#[derive(ParamValue)]
struct Instruction {
    action: u8,
}

/// A state key the caller owns outright — carrying a visibility and a doc
/// comment of its own, which is the whole reason to declare it yourself.
/// The state side has this capability only through `existing`.
pub struct RewardRateKey;
hook_state!(existing RewardRateKey = b"RR" => u64);

/// A snake_case marker type.
struct cfg_name;
hook_parameter!(existing cfg_name = b"CFG" => Config);

/// A raw-identifier marker type. Raw identifiers are rejected in *binder*
/// position, but are ordinary type names here.
struct r#type;
otxn_parameter!(existing r#type = b"INS" => Instruction);

fn main() {
    assert_eq!(
        RewardRateKey.get_state().err(),
        Some(HookError::NotImplemented)
    );
    assert_eq!(RewardRateKey.delete_state(), Err(HookError::NotImplemented));

    assert_eq!(cfg_name.get_name(), b"CFG".as_slice());
    assert_eq!(
        hook_param_typed(&cfg_name).err(),
        Some(HookError::NotImplemented)
    );

    assert_eq!(r#type.get_name(), b"INS".as_slice());
    assert_eq!(
        otxn_param_typed(&r#type).err(),
        Some(HookError::NotImplemented)
    );
}
