//! The `existing` keyword form, on all three macros and with the caller
//! marker types it has to tolerate.
//!
//! `existing` attaches the fixed-bytes impls and the role pairing to a
//! key/name type the **caller** declared. The inherent accessors go on the
//! **entity**, as they do for every form — never on the caller's type.
//! Because the macro did not declare that type, its spelling is the
//! caller's business and is deliberately not naming-checked: both a
//! `snake_case` marker and a raw identifier must keep working.

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
hook_state!(RewardRate, existing RewardRateKey = b"RR" => u64);

/// A snake_case marker type.
struct cfg_name;
hook_parameter!(Cfg, existing cfg_name = b"CFG" => Config);

/// A raw-identifier marker type. Raw identifiers are rejected in *binder*
/// position, but are ordinary type names here.
struct r#type;
otxn_parameter!(Ins, existing r#type = b"INS" => Instruction);

fn main() {
    assert_eq!(RewardRate.get_state().err(), Some(HookError::NotImplemented));
    assert_eq!(RewardRate.delete_state(), Err(HookError::NotImplemented));
    // The caller's own type got the key-side impls it was named for.
    assert_eq!(
        state_get_typed(&RewardRateKey).err(),
        Some(HookError::NotImplemented)
    );

    assert_eq!(Cfg.get_name(), b"CFG".as_slice());
    assert_eq!(
        hook_param_typed(&cfg_name).err(),
        Some(HookError::NotImplemented)
    );

    assert_eq!(Ins.get_name(), b"INS".as_slice());
    assert_eq!(
        otxn_param_typed(&r#type).err(),
        Some(HookError::NotImplemented)
    );
}
