//! `existing Name = => Ty` — the `=` is there but the name bytes are not.
//! The collector would otherwise report a generic "expected … before `=>`";
//! since the caller wrote `existing`, they get that form's own diagnostic.

use hooks_lib::{ParamValue, hook_parameter};

#[derive(ParamValue)]
struct Config {
    min_amount: u64,
}

struct CfgName;
hook_parameter!(existing CfgName = => Config);

fn main() {}
