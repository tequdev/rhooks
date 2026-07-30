//! `existing` followed by anything other than `Name = bytes => Ty` gets its
//! own targeted diagnostic — never the binder or removed-comma-form ones.

use hooks_lib::{ParamValue, hook_parameter};

#[derive(ParamValue)]
struct Config {
    min_amount: u64,
}

struct CfgName;
hook_parameter!(existing CfgName => Config);

fn main() {}
