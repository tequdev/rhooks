//! The removed comma-form with a *lowercase* marker type — historically
//! legal, since the comma-form never naming-checked the caller's own type.
//! It now parses as an instance binder, and is rejected by the rule that
//! catches every binder without a declaring form.

#![allow(non_camel_case_types)]

use hooks_lib::{ParamValue, hook_parameter};

#[derive(ParamValue)]
struct Config {
    min_amount: u64,
}

struct cfg_name;
hook_parameter!(cfg_name, b"CFG" => Config);

fn main() {}
