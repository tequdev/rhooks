//! `existing` is reserved as a binder name: `existing, Name = ..` would
//! otherwise parse as a valid binder literally named `existing`, silently
//! dropping the caller's intended `existing` keyword form.

use hooks_lib::{ParamValue, hook_parameter};

#[derive(ParamValue)]
struct Config {
    min_amount: u64,
}

struct CfgName;
hook_parameter!(existing, CfgName = b"CFG" => Config);

fn main() {}
