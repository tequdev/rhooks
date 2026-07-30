//! A caller marker type spelled as a raw identifier, attached with the
//! `existing` keyword form. Raw identifiers are rejected in *binder*
//! position but are perfectly ordinary type names here.

#![allow(non_camel_case_types)]

use hooks_lib::prelude::*;
use hooks_lib::{ParamValue, otxn_parameter};

#[derive(ParamValue)]
struct Instruction {
    action: u8,
}

struct r#type;
otxn_parameter!(existing r#type = b"INS" => Instruction);

fn main() {
    assert_eq!(r#type.get_name(), b"INS".as_slice());
    assert_eq!(
        otxn_param_typed(&r#type).err(),
        Some(HookError::NotImplemented)
    );
}
