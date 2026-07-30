//! The removed `$Name, $bytes => $Ty` comma-form, with a byte-string
//! literal after the comma — the exact shape callers used to write.

use hooks_lib::{ParamValue, hook_parameter};

#[derive(ParamValue)]
struct Config {
    min_amount: u64,
}

struct CfgName;
hook_parameter!(CfgName, b"CFG" => Config);

fn main() {}
