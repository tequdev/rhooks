//! A caller marker type spelled in snake_case, attached with the `existing`
//! keyword form: the capability the removed comma-form provided (it never
//! naming-checked the caller's own type), preserved 1:1.

#![allow(non_camel_case_types)]

use hooks_lib::prelude::*;
use hooks_lib::{ParamValue, hook_parameter};

#[derive(ParamValue)]
struct Config {
    min_amount: u64,
}

struct cfg_name;
hook_parameter!(existing cfg_name = b"CFG" => Config);

fn main() {
    assert_eq!(cfg_name.get_name(), b"CFG".as_slice());
    assert_eq!(
        hook_param_typed(&cfg_name).err(),
        Some(HookError::NotImplemented)
    );
}
