//! The removed comma-form with a *constant expression* rather than a
//! literal after the comma: the diagnostic must not depend on what follows
//! the comma, only on the leading UpperCamelCase identifier.

use hooks_lib::{ParamValue, hook_parameter};

const CFG_NAME: &[u8] = b"CFG";

#[derive(ParamValue)]
struct Config {
    min_amount: u64,
}

struct CfgName;
hook_parameter!(CfgName, CFG_NAME => Config);

fn main() {}
